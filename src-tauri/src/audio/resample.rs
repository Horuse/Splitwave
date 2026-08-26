//! High-quality sinc-based resampler for interleaved f32 streams of any channel count.

use rubato::{
    Resampler, SincFixedIn, SincFixedOut, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};

use crate::error::{AppError, AppResult};

fn sinc_params() -> SincInterpolationParameters {
    SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Cubic,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    }
}

/// Fixed-OUTPUT resampler: produces exactly `chunk_out` frames per call,
/// pulling `input_frames_next()` frames of input. This is the pull-driven ASRC
/// used on the playback side -- one block per audio-clock tick, with the ratio
/// nudged for clock-drift tracking, so there is no second timer/clock and no
/// silence/drop discontinuities from buffer beating. Interleaved I/O carries
/// `channels` samples per frame.
pub struct MultiResamplerOut {
    inner: SincFixedOut<f32>,
    channels: usize,
    in_planar: Vec<Vec<f32>>,
    out_planar: Vec<Vec<f32>>,
    chunk_out: usize,
}

impl MultiResamplerOut {
    pub fn new(from_rate: u32, to_rate: u32, chunk_out: usize, channels: usize) -> AppResult<Self> {
        let ratio = to_rate as f64 / from_rate as f64;
        let inner = SincFixedOut::<f32>::new(ratio, 1.05, sinc_params(), chunk_out, channels)
            .map_err(|e| AppError::Stream(format!("resampler init: {e}")))?;
        let in_max = inner.input_frames_max();
        Ok(Self {
            inner,
            channels,
            in_planar: vec![vec![0.0; in_max]; channels],
            out_planar: vec![vec![0.0; chunk_out]; channels],
            chunk_out,
        })
    }

    /// Frames of input the next `process` call will consume (varies with ratio).
    pub fn input_frames_next(&self) -> usize {
        self.inner.input_frames_next()
    }

    /// Nudge the output/input ratio for clock-drift tracking (ramped, within the
    /// 1.05 relative bound set at construction).
    pub fn set_ratio(&mut self, ratio: f64) {
        let _ = self.inner.set_resample_ratio(ratio, true);
    }

    /// Resample exactly `input_frames_next()` frames of interleaved input
    /// into `chunk_out` frames appended to `dst`.
    pub fn process(&mut self, interleaved_in: &[f32], dst: &mut Vec<f32>) -> AppResult<()> {
        let frames = self.inner.input_frames_next();
        debug_assert_eq!(interleaved_in.len(), frames * self.channels);
        for v in &mut self.in_planar {
            v.resize(frames, 0.0);
        }
        for (i, frame) in interleaved_in.chunks_exact(self.channels).enumerate() {
            for c in 0..self.channels {
                self.in_planar[c][i] = frame[c];
            }
        }
        for v in &mut self.out_planar {
            v.resize(self.chunk_out, 0.0);
        }
        let (_in_used, produced) = self
            .inner
            .process_into_buffer(&self.in_planar, &mut self.out_planar, None)
            .map_err(|e| AppError::Stream(format!("resampler process: {e}")))?;
        for i in 0..produced {
            for c in 0..self.channels {
                dst.push(self.out_planar[c][i]);
            }
        }
        Ok(())
    }
}

/// Fixed-INPUT resampler: consumes exactly `chunk_in` frames per call and
/// appends the produced frames to `dst`. Interleaved I/O carries `channels`
/// samples per frame.
pub struct MultiResampler {
    inner: SincFixedIn<f32>,
    channels: usize,
    in_planar: Vec<Vec<f32>>,
    out_planar: Vec<Vec<f32>>,
    chunk_in: usize,
    out_max: usize,
}

impl MultiResampler {
    pub fn new(
        from_rate: u32,
        to_rate: u32,
        chunk_size: usize,
        channels: usize,
    ) -> AppResult<Self> {
        let ratio = to_rate as f64 / from_rate as f64;
        let inner = SincFixedIn::<f32>::new(ratio, 1.05, sinc_params(), chunk_size, channels)
            .map_err(|e| AppError::Stream(format!("resampler init: {e}")))?;

        let out_max = inner.output_frames_max();
        let in_planar = vec![vec![0.0_f32; chunk_size]; channels];
        let out_planar = vec![vec![0.0_f32; out_max]; channels];

        Ok(Self {
            inner,
            channels,
            in_planar,
            out_planar,
            chunk_in: chunk_size,
            out_max,
        })
    }

    pub fn chunk_in(&self) -> usize {
        self.chunk_in
    }
    pub fn out_max(&self) -> usize {
        self.out_max
    }

    /// Resample one fixed input chunk into a caller-owned, preallocated
    /// interleaved buffer. Returns the number of written samples. This is the
    /// RT-safe form used by speaker workers: it never grows a `Vec` in the DSP
    /// path.
    pub fn process_chunk_into(
        &mut self,
        interleaved_in: &[f32],
        output: &mut [f32],
    ) -> AppResult<usize> {
        debug_assert_eq!(interleaved_in.len(), self.chunk_in * self.channels);
        debug_assert!(output.len() >= self.out_max * self.channels);

        for (i, frame) in interleaved_in.chunks_exact(self.channels).enumerate() {
            for c in 0..self.channels {
                self.in_planar[c][i] = frame[c];
            }
        }

        let (_in_used, produced) = self
            .inner
            .process_into_buffer(&self.in_planar, &mut self.out_planar, None)
            .map_err(|e| AppError::Stream(format!("resampler process: {e}")))?;
        for i in 0..produced {
            for c in 0..self.channels {
                output[i * self.channels + c] = self.out_planar[c][i];
            }
        }
        Ok(produced * self.channels)
    }

    pub fn process_chunk(&mut self, interleaved_in: &[f32], dst: &mut Vec<f32>) -> AppResult<()> {
        let start = dst.len();
        dst.resize(start + self.out_max * self.channels, 0.0);
        let written = self.process_chunk_into(interleaved_in, &mut dst[start..])?;
        dst.truncate(start + written);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::MultiResampler;

    fn produced_frames(from_rate: u32, to_rate: u32) -> usize {
        const CHANNELS: usize = 2;
        const CHUNKS: usize = 32;
        let mut resampler = MultiResampler::new(from_rate, to_rate, 256, CHANNELS).unwrap();
        let input = vec![0.25_f32; 256 * CHANNELS];
        let mut output = Vec::with_capacity(resampler.out_max() * CHANNELS);
        let mut frames = 0;
        for _ in 0..CHUNKS {
            output.clear();
            resampler.process_chunk(&input, &mut output).unwrap();
            assert_eq!(output.len() % CHANNELS, 0);
            frames += output.len() / CHANNELS;
        }
        frames
    }

    #[test]
    fn converts_engine_rate_to_44k1() {
        let frames = produced_frames(48_000, 44_100);
        let expected = 32 * 256 * 44_100 / 48_000;
        assert!((frames as isize - expected as isize).unsigned_abs() <= 256);
    }

    #[test]
    fn converts_engine_rate_to_88k2() {
        let frames = produced_frames(48_000, 88_200);
        let expected = 32 * 256 * 88_200 / 48_000;
        assert!((frames as isize - expected as isize).unsigned_abs() <= 256);
    }

    #[test]
    fn independent_speaker_resamplers_do_not_share_state() {
        let mut a = MultiResampler::new(48_000, 44_100, 256, 2).unwrap();
        let mut b = MultiResampler::new(48_000, 88_200, 256, 2).unwrap();
        let input = vec![0.25_f32; 256 * 2];
        let mut out_a = vec![0.0; a.out_max() * 2];
        let mut out_b = vec![0.0; b.out_max() * 2];

        let written_a = a.process_chunk_into(&input, &mut out_a).unwrap();
        let written_b = b.process_chunk_into(&input, &mut out_b).unwrap();

        assert_eq!(written_a % 2, 0);
        assert_eq!(written_b % 2, 0);
        assert!(written_a < written_b);
    }
}
