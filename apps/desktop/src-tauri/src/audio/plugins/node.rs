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
use crate::audio::plugins::clap_host::SplitwaveHost;
use crate::audio::plugins::param_ring::MAX_PARAM_CHANGES_PER_BLOCK;
use crate::audio::plugins::ParamRing;

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
    // Per-node read position into the shared broadcast ring; every stereo pair
    // of a wide plugin reads the same writes through its own cursor.
    param_cursor: usize,
    events: EventBuffer,
    // Cleared on drop so the host's main thread can reclaim the matching
    // main-thread instance once its processor is gone from the DAG.
    alive: Arc<AtomicBool>,
    /// Interleaved channels the pipeline hands this node, which is what the
    /// plugin's main port carries.
    channels: usize,
    /// Reported by the plugin at activation. The DAG pads shorter parallel
    /// paths by it, so a wrong value here is an audible phase error rather
    /// than a missing feature.
    latency: usize,
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
        channels: usize,
        latency: usize,
    ) -> Self {
        let param_cursor = params.reader();
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
            param_cursor,
            events: EventBuffer::with_capacity(MAX_PARAM_CHANGES_PER_BLOCK),
            alive,
            channels,
            latency,
        }
    }
}

impl Effect for PluginNode {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let width = self.channels;
        if frames == 0 || frames > self.max_frames || samples.len() < frames * width {
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
            param_cursor,
            events,
            ..
        } = self;

        // Drain UI parameter writes into events fed to the plugin this block.
        // Global target (all ports/channels/keys), null cookie: the plugin
        // resolves the parameter by its stable id.
        events.clear();
        let mut drained = 0;
        while drained < MAX_PARAM_CHANGES_PER_BLOCK {
            let Some((id, value)) = params.read(param_cursor) else {
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

        // Main input port; silence stays in every other port, and in any channel
        // the plugin declares beyond what the pipeline carries.
        if let Some(main) = in_bufs.first_mut() {
            for (c, channel) in main.iter_mut().take(width).enumerate() {
                for i in 0..frames {
                    channel[i] = samples[i * width + c];
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
        let mut outputs =
            out_ports.with_output_buffers(out_bufs.iter_mut().map(|port| AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_output_only(
                    port.iter_mut().map(|ch| &mut ch[..frames]),
                ),
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
                for c in 0..width {
                    // A plugin narrower than the pipeline (a mono unit driven as
                    // a pair) repeats its last channel rather than leaving the
                    // rest of the block at whatever it held.
                    let src = &main[c.min(main.len() - 1)];
                    for i in 0..frames {
                        samples[i * width + c] = src[i];
                    }
                }
            }
        }
        *steady += frames as u64;
    }

    fn latency_frames(&self) -> usize {
        self.latency
    }
}

impl PluginNode {
    pub fn channels(&self) -> usize {
        self.channels
    }
}
