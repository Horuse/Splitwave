//! RT side of a hosted VST3 plugin: the processor, its bus buffers, and the
//! parameter queues the plugin reads inside `process`.
//!
//! Everything here is allocated when the node is built. `process` runs on the
//! DSP worker, where an allocation or a lock is a dropout.

#![allow(non_snake_case)]

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use vst3::Steinberg::Vst::{
    AudioBusBuffers, AudioBusBuffers__type0, IAudioProcessor, IAudioProcessorTrait, IComponent,
    IComponentTrait, IParamValueQueue, IParamValueQueueTrait, IParameterChanges,
    IParameterChangesTrait, ParamID, ParamValue, ProcessData, ProcessModes_, SymbolicSampleSizes_,
};
use vst3::Steinberg::{int32, kResultOk, kResultTrue, tresult};
use vst3::{Class, ComPtr, ComWrapper};

use crate::audio::effects::Effect;
use crate::audio::plugins::ParamRing;

/// Upper bound on parameter changes handed to the plugin per block. The UI knob
/// rate is far below this; the queues are allocated once at this size.
const MAX_PARAM_CHANGES_PER_BLOCK: usize = 64;

/// One parameter's changes within a block. The host writes at most one point
/// per block, so the storage is a single value rather than a list.
#[derive(Default)]
pub struct ParamQueue {
    id: AtomicU32,
    value: AtomicU64,
    live: AtomicBool,
}

impl ParamQueue {
    fn set(&self, id: ParamID, value: ParamValue) {
        self.id.store(id, Ordering::Relaxed);
        self.value.store(value.to_bits(), Ordering::Relaxed);
        self.live.store(true, Ordering::Relaxed);
    }
}

impl Class for ParamQueue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for ParamQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id.load(Ordering::Relaxed)
    }

    unsafe fn getPointCount(&self) -> int32 {
        self.live.load(Ordering::Relaxed) as int32
    }

    unsafe fn getPoint(
        &self,
        index: int32,
        sampleOffset: *mut int32,
        value: *mut ParamValue,
    ) -> tresult {
        if index != 0 || !self.live.load(Ordering::Relaxed) {
            return vst3::Steinberg::kInvalidArgument;
        }
        // Block-rate automation: the change lands on the first frame.
        *sampleOffset = 0;
        *value = f64::from_bits(self.value.load(Ordering::Relaxed));
        kResultOk
    }

    unsafe fn addPoint(
        &self,
        _sampleOffset: int32,
        _value: ParamValue,
        _index: *mut int32,
    ) -> tresult {
        // Only the plugin would call this, to write automation back to the
        // host. We record no automation.
        vst3::Steinberg::kNotImplemented
    }
}

/// The `inputParameterChanges` the plugin reads each block. Holds its queues
/// for the life of the node so a block costs no allocation.
pub struct ParamChanges {
    queues: Vec<ComWrapper<ParamQueue>>,
    used: AtomicUsize,
}

impl ParamChanges {
    fn new() -> Self {
        Self {
            queues: (0..MAX_PARAM_CHANGES_PER_BLOCK)
                .map(|_| ComWrapper::new(ParamQueue::default()))
                .collect(),
            used: AtomicUsize::new(0),
        }
    }

    fn clear(&self) {
        for queue in &self.queues {
            queue.live.store(false, Ordering::Relaxed);
        }
        self.used.store(0, Ordering::Relaxed);
    }

    /// Returns false once the block's queues are spent; the ring keeps the
    /// unread writes for the next block.
    fn push(&self, id: ParamID, value: ParamValue) -> bool {
        let n = self.used.load(Ordering::Relaxed);
        let Some(queue) = self.queues.get(n) else {
            return false;
        };
        queue.set(id, value);
        self.used.store(n + 1, Ordering::Relaxed);
        true
    }
}

impl Class for ParamChanges {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for ParamChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        self.used.load(Ordering::Relaxed) as int32
    }

    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        let used = self.used.load(Ordering::Relaxed);
        if index < 0 || index as usize >= used {
            return std::ptr::null_mut();
        }
        self.queues[index as usize]
            .as_com_ref::<IParamValueQueue>()
            .map(|r| r.as_ptr())
            .unwrap_or(std::ptr::null_mut())
    }

    unsafe fn addParameterData(
        &self,
        _id: *const ParamID,
        _index: *mut int32,
    ) -> *mut IParamValueQueue {
        std::ptr::null_mut()
    }
}

/// Channel buffers for one bus, plus the pointer array the plugin is handed.
struct Bus {
    channels: Vec<Vec<f32>>,
    ptrs: Vec<*mut f32>,
}

impl Bus {
    fn new(channel_count: usize, max_frames: usize) -> Self {
        let mut channels = vec![vec![0.0; max_frames]; channel_count.max(1)];
        let ptrs = channels.iter_mut().map(|c| c.as_mut_ptr()).collect();
        Self { channels, ptrs }
    }
}

/// The pipeline carries interleaved stereo. Audio flows through bus 0 in each
/// direction; every other bus the plugin declares is still allocated and fed
/// silence, because a missing buffer makes the plugin read a null pointer.
pub struct Vst3Node {
    processor: ComPtr<IAudioProcessor>,
    component: ComPtr<IComponent>,
    inputs: Vec<Bus>,
    outputs: Vec<Bus>,
    input_buses: Vec<AudioBusBuffers>,
    output_buses: Vec<AudioBusBuffers>,
    changes: ComWrapper<ParamChanges>,
    changes_ptr: *mut IParameterChanges,
    params: Arc<ParamRing>,
    param_cursor: usize,
    max_frames: usize,
    latency: usize,
    alive: Arc<AtomicBool>,
}

// SAFETY: the processor and component interfaces are declared thread-safe by
// the bindings, and everything else is owned by this node. The node is built on
// the main thread and then belongs to one DSP worker.
unsafe impl Send for Vst3Node {}

impl Drop for Vst3Node {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
    }
}

impl Vst3Node {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        processor: ComPtr<IAudioProcessor>,
        component: ComPtr<IComponent>,
        input_channels: &[usize],
        output_channels: &[usize],
        max_frames: usize,
        params: Arc<ParamRing>,
        alive: Arc<AtomicBool>,
    ) -> Self {
        let mut inputs: Vec<Bus> = input_channels
            .iter()
            .map(|c| Bus::new(*c, max_frames))
            .collect();
        let mut outputs: Vec<Bus> = output_channels
            .iter()
            .map(|c| Bus::new(*c, max_frames))
            .collect();

        let input_buses = inputs.iter_mut().map(bus_buffers).collect();
        let output_buses = outputs.iter_mut().map(bus_buffers).collect();

        let changes = ComWrapper::new(ParamChanges::new());
        let changes_ptr = changes
            .as_com_ref::<IParameterChanges>()
            .map(|r| r.as_ptr())
            .expect("ParamChanges implements IParameterChanges");

        // Start at the ring's current end so a freshly built node never replays
        // writes issued for the plugin it replaced.
        let param_cursor = params.cursor();
        let latency = unsafe { processor.getLatencySamples() } as usize;

        Self {
            processor,
            component,
            inputs,
            outputs,
            input_buses,
            output_buses,
            changes,
            changes_ptr,
            params,
            param_cursor,
            max_frames,
            latency,
            alive,
        }
    }
}

fn bus_buffers(bus: &mut Bus) -> AudioBusBuffers {
    AudioBusBuffers {
        numChannels: bus.channels.len() as int32,
        silenceFlags: 0,
        __field0: AudioBusBuffers__type0 {
            channelBuffers32: bus.ptrs.as_mut_ptr(),
        },
    }
}

impl Effect for Vst3Node {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        if frames == 0 || frames > self.max_frames || samples.len() < frames * 2 {
            return;
        }

        self.changes.clear();
        while self.changes.used.load(Ordering::Relaxed) < MAX_PARAM_CHANGES_PER_BLOCK {
            let Some((id, value)) = self.params.read(&mut self.param_cursor) else {
                break;
            };
            if !self.changes.push(id, value) {
                break;
            }
        }

        // Main input bus, first two channels; silence stays everywhere else.
        if let Some(main) = self.inputs.first_mut() {
            let stereo = main.channels.len() > 1;
            for i in 0..frames {
                main.channels[0][i] = samples[2 * i];
                if stereo {
                    main.channels[1][i] = samples[2 * i + 1];
                }
            }
        }

        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            numSamples: frames as int32,
            numInputs: self.input_buses.len() as int32,
            numOutputs: self.output_buses.len() as int32,
            inputs: self.input_buses.as_mut_ptr(),
            outputs: self.output_buses.as_mut_ptr(),
            inputParameterChanges: self.changes_ptr,
            outputParameterChanges: std::ptr::null_mut(),
            inputEvents: std::ptr::null_mut(),
            outputEvents: std::ptr::null_mut(),
            // No transport to report: Splitwave is a router, not a sequencer.
            processContext: std::ptr::null_mut(),
        };

        // SAFETY: every buffer named by `data` is owned by this node and sized
        // for at least `frames`.
        let processed = unsafe { self.processor.process(&mut data) } == kResultOk;

        if processed {
            if let Some(main) = self.outputs.first() {
                let right = if main.channels.len() > 1 { 1 } else { 0 };
                for i in 0..frames {
                    samples[2 * i] = main.channels[0][i];
                    samples[2 * i + 1] = main.channels[right][i];
                }
            }
        }
    }

    fn latency_frames(&self) -> usize {
        self.latency
    }
}

/// Stops the plugin cleanly. Must run before the instance is released, and off
/// the RT thread.
pub fn deactivate(node: &Vst3Node) {
    unsafe {
        node.processor.setProcessing(0);
        node.component.setActive(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vst3::ComRef;

    /// The plugin sees exactly the changes pushed this block, and a block that
    /// pushes fewer than the last must not expose the leftovers.
    #[test]
    fn param_changes_expose_only_this_blocks_queues() {
        let changes = ComWrapper::new(ParamChanges::new());
        let c = changes.to_com_ptr::<IParameterChanges>().unwrap();
        unsafe {
            assert_eq!(c.getParameterCount(), 0);

            changes.push(7, 0.25);
            changes.push(9, 0.5);
            assert_eq!(c.getParameterCount(), 2);

            let q = ComRef::from_raw(c.getParameterData(1)).unwrap();
            assert_eq!(q.getParameterId(), 9);
            assert_eq!(q.getPointCount(), 1);
            let (mut offset, mut value) = (-1, 0.0);
            assert_eq!(q.getPoint(0, &mut offset, &mut value), kResultOk);
            assert_eq!((offset, value), (0, 0.5));

            // Out of range asks are answered with null, not a stale queue.
            assert!(c.getParameterData(2).is_null());

            changes.clear();
            changes.push(3, 1.0);
            assert_eq!(c.getParameterCount(), 1);
            let q = ComRef::from_raw(c.getParameterData(0)).unwrap();
            assert_eq!(q.getParameterId(), 3);
        }
    }

    #[test]
    fn the_queue_pool_is_bounded_and_never_grows() {
        let changes = ParamChanges::new();
        for i in 0..MAX_PARAM_CHANGES_PER_BLOCK {
            assert!(changes.push(i as u32, 0.0), "queue {i} should fit");
        }
        assert!(
            !changes.push(999, 0.0),
            "pushing past capacity must fail rather than allocate"
        );
    }
}

/// Whether the plugin can run at all in the format the pipeline uses.
pub fn supports_f32(processor: &ComPtr<IAudioProcessor>) -> bool {
    let supported =
        unsafe { processor.canProcessSampleSize(SymbolicSampleSizes_::kSample32 as int32) };
    supported == kResultTrue
}
