//! RT-side of a hosted plugin: the `Send` audio processor plus preallocated
//! de-interleave scratch. Lives on the DSP worker thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clack_host::events::event_types::ParamValueEvent;
use clack_host::events::io::EventBuffer;
use clack_host::events::Pckn;
use clack_host::prelude::*;
use clack_host::utils::Cookie;

use crate::audio::effects::Effect;
use crate::audio::plugins::host::SplitwaveHost;
use crate::audio::plugins::ParamRing;

/// Upper bound on parameter changes applied per block; the UI knob rate is far
/// below this, so the `EventBuffer` never grows past its preallocated capacity.
const MAX_PARAM_EVENTS_PER_BLOCK: usize = 64;

/// The pipeline carries interleaved stereo. Audio flows only through the
/// plugin's main (index 0) input/output ports; other ports the plugin declares
/// (e.g. a sidechain input) are still allocated and fed silence, because a
/// missing port buffer makes the plugin read a null pointer and crash.
pub struct PluginNode {
    processor: StartedPluginAudioProcessor<SplitwaveHost>,
    in_ports: AudioPorts,
    out_ports: AudioPorts,
    in_bufs: Vec<Vec<Vec<f32>>>,
    out_bufs: Vec<Vec<Vec<f32>>>,
    steady: u64,
    max_frames: usize,
    // UI parameter writes drained into `events` and fed to `process` each block.
    params: Arc<ParamRing>,
    events: EventBuffer,
    // Cleared on drop so the host's main thread can reclaim the matching
    // main-thread instance once its processor is gone from the DAG.
    alive: Arc<AtomicBool>,
}

impl Drop for PluginNode {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
    }
}

fn alloc(port_channels: &[u32], max_frames: usize) -> Vec<Vec<Vec<f32>>> {
    port_channels
        .iter()
        .map(|&ch| vec![vec![0.0; max_frames]; ch.max(1) as usize])
        .collect()
}

impl PluginNode {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        processor: StartedPluginAudioProcessor<SplitwaveHost>,
        input_channels: &[u32],
        output_channels: &[u32],
        max_frames: usize,
        params: Arc<ParamRing>,
        alive: Arc<AtomicBool>,
    ) -> Self {
        // Discard any queue backlog from a previously loaded plugin on this node
        // so a freshly instantiated plugin never receives another's param ids.
        while params.pop().is_some() {}
        Self {
            processor,
            in_ports: AudioPorts::with_capacity(
                input_channels.iter().map(|c| *c as usize).sum(),
                input_channels.len(),
            ),
            out_ports: AudioPorts::with_capacity(
                output_channels.iter().map(|c| *c as usize).sum(),
                output_channels.len(),
            ),
            in_bufs: alloc(input_channels, max_frames),
            out_bufs: alloc(output_channels, max_frames),
            steady: 0,
            max_frames,
            params,
            events: EventBuffer::with_capacity(MAX_PARAM_EVENTS_PER_BLOCK),
            alive,
        }
    }
}

impl Effect for PluginNode {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        if frames == 0 || frames > self.max_frames || samples.len() < frames * 2 {
            return;
        }
        let Self {
            processor,
            in_ports,
            out_ports,
            in_bufs,
            out_bufs,
            steady,
            params,
            events,
            ..
        } = self;

        // Drain UI parameter writes into events fed to the plugin this block.
        // Global target (all ports/channels/keys), null cookie: the plugin
        // resolves the parameter by its stable id.
        events.clear();
        let mut drained = 0;
        while drained < MAX_PARAM_EVENTS_PER_BLOCK {
            let Some((id, value)) = params.pop() else {
                break;
            };
            if let Some(param_id) = ClapId::from_raw(id) {
                events.push(&ParamValueEvent::new(
                    0,
                    param_id,
                    Pckn::match_all(),
                    value,
                    Cookie::empty(),
                ));
            }
            drained += 1;
        }

        // Main input port, first two channels; silence stays in every other
        // port/channel from allocation.
        if let Some(main) = in_bufs.first_mut() {
            for i in 0..frames {
                main[0][i] = samples[2 * i];
                if main.len() > 1 {
                    main[1][i] = samples[2 * i + 1];
                }
            }
        }

        let inputs = in_ports.with_input_buffers(in_bufs.iter_mut().map(|port| AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_input_only(port.iter_mut().map(|ch| InputChannel {
                buffer: &mut ch[..frames],
                is_constant: false,
            })),
        }));
        let mut outputs = out_ports.with_output_buffers(out_bufs.iter_mut().map(|port| {
            AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_output_only(
                    port.iter_mut().map(|ch| &mut ch[..frames]),
                ),
            }
        }));

        let processed = processor
            .process(
                &inputs,
                &mut outputs,
                &events.as_input(),
                &mut OutputEvents::void(),
                Some(*steady),
                None,
            )
            .is_ok();
        drop(inputs);
        drop(outputs);

        if processed {
            if let Some(main) = out_bufs.first() {
                let right = if main.len() > 1 { 1 } else { 0 };
                for i in 0..frames {
                    samples[2 * i] = main[0][i];
                    samples[2 * i + 1] = main[right][i];
                }
            }
        }
        *steady += frames as u64;
    }
}
