pub(super) mod builder;
pub(super) mod graph;
pub(super) mod nodes;
pub(super) mod staging;

#[allow(unused_imports)]
pub(super) use builder::{
    build_output_graph, inputs_feeding_output, plan_cuts, reachable_backward, CutPlan,
};
#[allow(unused_imports)]
pub(super) use graph::{BuiltOutputGraph, OutputGraph};
#[allow(unused_imports)]
pub(super) use nodes::{OutputMeta, SourceMeta, SourceStats};

/// One second of frames at the ring's own clock rate.
pub fn ring_capacity_frames(sample_rate: u32) -> usize {
    sample_rate.max(1) as usize
}

/// Block size used by the resampler. 256 frames @ 48 kHz ~ 5.3 ms.
pub const RESAMPLE_CHUNK: usize = 256;

pub const DSP_BLOCK_FRAMES: usize = 1024;

pub(super) const MAX_NET_CH: u32 = crate::audio::netaudio::MAX_CHANNELS as u32;
