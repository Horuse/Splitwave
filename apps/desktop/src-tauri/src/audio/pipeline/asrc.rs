//! The asynchronous resampler a live source reads through when its producer
//! runs on a clock other than the graph's: a fixed-output sinc resampler
//! whose ratio `DriftLoop` steers, fed the queue as a smooth function of time
//! (`ArrivalClock` over the producer's `WriteClock`). Drift is absorbed by a
//! ratio that moves a few ppm, never by cutting or repeating audio.

use std::sync::Arc;

use crate::audio::drift_loop::{ArrivalClock, DriftLoop};
use crate::audio::input_bridge::WriteClock;
use crate::audio::resample::MultiResamplerOut;
use crate::error::AppResult;

pub(super) struct Asrc {
    resampler: MultiResamplerOut,
    /// Output over input frames at nominal rates.
    base_ratio: f64,
    last_ratio: f64,
    drift: DriftLoop,
    arrival: ArrivalClock,
    clock: Option<Arc<WriteClock>>,
    channels: usize,
    /// Interleaved input for one call, sized for the largest.
    pub(super) input: Vec<f32>,
    output: Vec<f32>,
}

impl Asrc {
    /// `frames` output frames per call at `out_rate` from a producer at
    /// `in_rate`.
    pub(super) fn new(
        in_rate: u32,
        out_rate: u32,
        frames: usize,
        channels: usize,
    ) -> AppResult<Self> {
        let resampler = MultiResamplerOut::for_drift(in_rate, out_rate, frames, channels)?;
        let input = Vec::with_capacity(resampler.input_frames_max() * channels);
        // The loop counts the queue in the producer's frames: one block of
        // output takes `frames * in / out` of them.
        let in_per_block = (frames as u64 * in_rate as u64 / out_rate.max(1) as u64).max(1);
        let base_ratio = out_rate as f64 / in_rate.max(1) as f64;
        Ok(Self {
            resampler,
            base_ratio,
            last_ratio: base_ratio,
            drift: DriftLoop::new(in_rate, in_per_block as usize),
            arrival: ArrivalClock::new(in_rate),
            clock: None,
            channels,
            input,
            output: Vec::with_capacity(frames * channels),
        })
    }

    pub(super) fn set_clock(&mut self, clock: Arc<WriteClock>) {
        self.clock = Some(clock);
    }

    /// Frames of input the next block will read.
    pub(super) fn need(&self) -> usize {
        self.resampler.input_frames_next()
    }

    /// Frames the filter itself delays by, in the producer's rate like the
    /// queue in front of it.
    pub(super) fn delay_frames(&self) -> usize {
        (self.resampler.delay_frames() as f64 / self.base_ratio).round() as usize
    }

    /// After the queue was moved by something other than the loop (realigned
    /// under silence after a gap, or the startup correction): the loop starts
    /// settling again and the producer's timing is learned afresh. The drift
    /// already learned is kept; the clocks have not changed.
    pub(super) fn restart(&mut self) {
        self.drift.restart();
        self.arrival.reset();
    }

    /// Once per block before reading: steer the ratio so the queue settles
    /// where the cushion wants it. Seen through the producer's clock, the
    /// queue plus what the producer has made but not yet written is the queue
    /// as it stands right after a delivery, the top of its saw: that settles
    /// on `start`, which also bounds the unwritten part while the producer
    /// stalls. Without a clock the loop sees the sawing queue itself, whose
    /// mean settles on `target`.
    pub(super) fn steer(&mut self, queued: usize, target: usize, start: usize, now: f64) {
        let error = match &self.clock {
            Some(clock) => {
                let (samples, at) = clock.read();
                self.arrival.observe(samples / self.channels as u64, at);
                queued as f64 + self.arrival.pending(now, start as f64) - start as f64
            }
            None => queued as f64 - target as f64,
        };
        let u = self.drift.update(error);
        let ratio = self.base_ratio / (1.0 + u);
        if (ratio - self.last_ratio).abs() > 1e-12 {
            self.resampler.set_ratio(ratio);
            self.last_ratio = ratio;
        }
    }

    /// Resample `input` (exactly `need()` frames) into one output block.
    pub(super) fn process(&mut self) -> AppResult<&[f32]> {
        self.output.clear();
        self.resampler.process(&self.input, &mut self.output)?;
        Ok(&self.output)
    }
}
