//! Real-time DSP effects. All effects operate on interleaved stereo f32 frames.
//!
//! Parameters live in `Arc<Atomic*>` cells shared with the UI side of the
//! engine. The audio callback reads them lock-free on every block, so slider
//! moves and mute toggles take effect within a couple of milliseconds without
//! restarting the pipeline.

use crate::audio::plugins::host_api::HostedEffect;

/// Fixed DSP block size; hosted-plugin scratch buffers are sized to it. Must
/// stay >= the pipeline's `DSP_BLOCK_FRAMES`, or a block would overrun them.
pub(crate) const PLUGIN_MAX_BLOCK: usize = 1024;

pub mod biquad;
pub mod channel_balance;
pub mod compressor;
pub mod controls;
pub mod de_esser;
pub mod declick;
pub mod delay;
pub mod eq;
pub mod gain;
pub mod instantiate;
pub mod level_meter;
pub mod limiter;
pub mod lufs_meter;
pub mod mute;
pub mod noise_gate;
pub mod noise_suppressor;
pub(crate) mod offload;
pub mod registry;
pub mod reverb;
pub mod saturator;
pub(crate) mod util;
pub mod waveform;

use channel_balance::ChannelBalanceEffect;
use compressor::CompressorEffect;
use de_esser::DeEsserEffect;
use declick::DeclickEffect;
use delay::DelayEffect;
use eq::EqEffect;
use gain::GainEffect;
pub use level_meter::{update_meter, LevelMeterEffect, MeterHandle};
use limiter::LimiterEffect;
pub use lufs_meter::{LufsHandle, LufsMeterEffect};
use mute::MuteEffect;
use noise_gate::NoiseGateEffect;
use noise_suppressor::NoiseSuppressorEffect;
use reverb::ReverbEffect;
use saturator::SaturatorEffect;
pub use waveform::{WaveformEffect, WaveformHandle};

pub use controls::EffectControl;
pub use instantiate::instantiate_effect;
pub use registry::{EffectBuild, EffectRegistry, GrHandle};

pub trait Effect: Send {
    fn process(&mut self, samples: &mut [f32], frames: usize);
    /// Frames (not stereo samples) of delay between input and output. Pipeline
    /// pads parallel paths to align at mixing points.
    fn latency_frames(&self) -> usize {
        0
    }
}

/// Enum dispatch wrapper so the RT thread doesn't pay a vtable indirection per
/// process call. The closed set of effects is known at compile time; LLVM can
/// inline the inner loop for each variant.
pub enum RuntimeEffect {
    Gain(GainEffect),
    Mute(MuteEffect),
    ChannelBalance(ChannelBalanceEffect),
    Saturator(SaturatorEffect),
    Eq(EqEffect),
    LevelMeter(LevelMeterEffect),
    LufsMeter(LufsMeterEffect),
    Waveform(WaveformEffect),
    Limiter(LimiterEffect),
    Compressor(CompressorEffect),
    NoiseGate(NoiseGateEffect),
    Delay(DelayEffect),
    Reverb(ReverbEffect),
    NoiseSuppressor(NoiseSuppressorEffect),
    Declick(DeclickEffect),
    DeEsser(DeEsserEffect),
    HostedPlugin(HostedEffect),
}

impl RuntimeEffect {
    #[inline]
    pub fn latency_frames(&self) -> usize {
        match self {
            RuntimeEffect::Gain(e) => e.latency_frames(),
            RuntimeEffect::Mute(e) => e.latency_frames(),
            RuntimeEffect::ChannelBalance(e) => e.latency_frames(),
            RuntimeEffect::Saturator(e) => e.latency_frames(),
            RuntimeEffect::Eq(e) => e.latency_frames(),
            RuntimeEffect::LevelMeter(e) => e.latency_frames(),
            RuntimeEffect::LufsMeter(e) => e.latency_frames(),
            RuntimeEffect::Waveform(e) => e.latency_frames(),
            RuntimeEffect::Limiter(e) => e.latency_frames(),
            RuntimeEffect::Compressor(e) => e.latency_frames(),
            RuntimeEffect::NoiseGate(e) => e.latency_frames(),
            RuntimeEffect::Delay(e) => e.latency_frames(),
            RuntimeEffect::Reverb(e) => e.latency_frames(),
            RuntimeEffect::NoiseSuppressor(e) => e.latency_frames(),
            RuntimeEffect::Declick(e) => e.latency_frames(),
            RuntimeEffect::DeEsser(e) => e.latency_frames(),
            RuntimeEffect::HostedPlugin(e) => e.latency_frames(),
        }
    }

    #[inline]
    pub fn process_with_sidechain(
        &mut self,
        main: &mut [f32],
        sidechain: Option<&[f32]>,
        frames: usize,
    ) {
        match self {
            RuntimeEffect::Compressor(e) => e.process_with_sidechain(main, sidechain, frames),
            RuntimeEffect::NoiseGate(e) => e.process_with_sidechain(main, sidechain, frames),
            RuntimeEffect::Gain(e) => e.process(main, frames),
            RuntimeEffect::Mute(e) => e.process(main, frames),
            RuntimeEffect::ChannelBalance(e) => e.process(main, frames),
            RuntimeEffect::Saturator(e) => e.process(main, frames),
            RuntimeEffect::Eq(e) => e.process(main, frames),
            RuntimeEffect::LevelMeter(e) => e.process(main, frames),
            RuntimeEffect::LufsMeter(e) => e.process(main, frames),
            RuntimeEffect::Waveform(e) => e.process(main, frames),
            RuntimeEffect::Limiter(e) => e.process(main, frames),
            RuntimeEffect::Delay(e) => e.process(main, frames),
            RuntimeEffect::Reverb(e) => e.process(main, frames),
            RuntimeEffect::NoiseSuppressor(e) => e.process(main, frames),
            RuntimeEffect::Declick(e) => e.process(main, frames),
            RuntimeEffect::DeEsser(e) => e.process(main, frames),
            RuntimeEffect::HostedPlugin(e) => e.process(main, frames),
        }
    }
}
