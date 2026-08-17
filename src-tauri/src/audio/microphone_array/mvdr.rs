//! Frequency-domain MVDR on steering-aligned channels. The upstream
//! fractional-delay stage makes the target steering vector all ones, while
//! preserving off-target phase differences for covariance estimation.

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

use crate::audio::health;
use crate::error::{AppError, AppResult};

const FFT_SIZE: usize = 512;
pub(super) const HOP_SIZE: usize = 256;
const BINS: usize = FFT_SIZE / 2 + 1;

pub(super) struct Mvdr {
    channels: usize,
    strength: f32,
    minimum_gain: f32,
    postfilter_enabled: bool,
    update_interval: u64,
    frame_index: u64,
    window: Vec<f32>,
    analysis: Vec<f32>,
    spectra: Vec<Complex32>,
    covariance: Vec<Complex32>,
    weights: Vec<Complex32>,
    output_spectrum: Vec<Complex32>,
    overlap: Vec<f32>,
    postfilter_gain: Vec<f32>,
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    fft_scratch: Vec<Complex32>,
    matrix: Vec<Complex32>,
    lower: Vec<Complex32>,
    forward_solution: Vec<Complex32>,
    solution: Vec<Complex32>,
}

impl Mvdr {
    pub(super) fn new(
        channels: usize,
        strength: f32,
        max_attenuation_db: f32,
        postfilter_enabled: bool,
    ) -> AppResult<Self> {
        if channels < 2 {
            return Err(AppError::Validation(
                "Microphone Array MVDR needs at least two channels".into(),
            ));
        }
        let mut planner = FftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(FFT_SIZE);
        let inverse = planner.plan_fft_inverse(FFT_SIZE);
        let scratch_len = forward
            .get_inplace_scratch_len()
            .max(inverse.get_inplace_scratch_len());
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|index| {
                (0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / FFT_SIZE as f32).cos())
                    .sqrt()
            })
            .collect();
        let mut covariance = vec![Complex32::new(0.0, 0.0); BINS * channels * channels];
        let mut weights = vec![Complex32::new(0.0, 0.0); BINS * channels];
        for bin in 0..BINS {
            for channel in 0..channels {
                covariance[(bin * channels + channel) * channels + channel].re = 1.0e-3;
                weights[bin * channels + channel].re = 1.0 / channels as f32;
            }
        }
        Ok(Self {
            channels,
            strength: strength.clamp(0.0, 1.0),
            minimum_gain: 10.0_f32.powf(-max_attenuation_db.clamp(0.0, 36.0) / 20.0),
            postfilter_enabled,
            update_interval: match channels {
                2..=4 => 2,
                5..=8 => 4,
                _ => 16,
            },
            frame_index: 0,
            window,
            analysis: vec![0.0; channels * FFT_SIZE],
            spectra: vec![Complex32::new(0.0, 0.0); channels * FFT_SIZE],
            covariance,
            weights,
            output_spectrum: vec![Complex32::new(0.0, 0.0); FFT_SIZE],
            overlap: vec![0.0; FFT_SIZE],
            postfilter_gain: vec![1.0; BINS],
            forward,
            inverse,
            fft_scratch: vec![Complex32::new(0.0, 0.0); scratch_len],
            matrix: vec![Complex32::new(0.0, 0.0); channels * channels],
            lower: vec![Complex32::new(0.0, 0.0); channels * channels],
            forward_solution: vec![Complex32::new(0.0, 0.0); channels],
            solution: vec![Complex32::new(0.0, 0.0); channels],
        })
    }

    pub(super) fn process(
        &mut self,
        aligned_planar: &[f32],
        output: &mut [f32],
        adapt: bool,
    ) -> AppResult<()> {
        if aligned_planar.len() != self.channels * HOP_SIZE || output.len() < HOP_SIZE {
            return Err(AppError::Validation(
                "Microphone Array MVDR block shape is invalid".into(),
            ));
        }
        for channel in 0..self.channels {
            let history = &mut self.analysis[channel * FFT_SIZE..(channel + 1) * FFT_SIZE];
            history.copy_within(HOP_SIZE.., 0);
            history[FFT_SIZE - HOP_SIZE..]
                .copy_from_slice(&aligned_planar[channel * HOP_SIZE..(channel + 1) * HOP_SIZE]);
            let spectrum = &mut self.spectra[channel * FFT_SIZE..(channel + 1) * FFT_SIZE];
            for index in 0..FFT_SIZE {
                spectrum[index] = Complex32::new(history[index] * self.window[index], 0.0);
            }
            self.forward
                .process_with_scratch(spectrum, &mut self.fft_scratch);
        }

        for bin in 0..BINS {
            let mut fixed = Complex32::new(0.0, 0.0);
            let mut mean_power = 0.0f32;
            for channel in 0..self.channels {
                let value = self.spectra[channel * FFT_SIZE + bin];
                fixed += value;
                mean_power += value.norm_sqr();
            }
            fixed /= self.channels as f32;
            mean_power /= self.channels as f32;
            let coherence = (fixed.norm_sqr() / mean_power.max(1.0e-12)).clamp(0.0, 1.0);
            if adapt {
                let alpha = if coherence > 0.65 { 0.998 } else { 0.96 };
                self.update_covariance(bin, alpha);
            }
            if adapt && self.frame_index.is_multiple_of(self.update_interval) {
                self.update_weights(bin);
            }
            let mut adaptive = Complex32::new(0.0, 0.0);
            for channel in 0..self.channels {
                adaptive += self.weights[bin * self.channels + channel].conj()
                    * self.spectra[channel * FFT_SIZE + bin];
            }
            let mut value = fixed + (adaptive - fixed) * self.strength;
            if self.postfilter_enabled {
                let desired = self.minimum_gain
                    + (1.0 - self.minimum_gain) * coherence.sqrt().clamp(0.0, 1.0);
                let previous = self.postfilter_gain[bin];
                let mut smoothed = previous * 0.9 + desired * 0.1;
                if bin > 0 {
                    smoothed = smoothed * 0.8 + self.postfilter_gain[bin - 1] * 0.2;
                }
                self.postfilter_gain[bin] = smoothed.clamp(self.minimum_gain, 1.0);
                value *= self.postfilter_gain[bin];
            }
            self.output_spectrum[bin] = value;
        }
        for bin in 1..FFT_SIZE / 2 {
            self.output_spectrum[FFT_SIZE - bin] = self.output_spectrum[bin].conj();
        }
        self.output_spectrum[0].im = 0.0;
        self.output_spectrum[FFT_SIZE / 2].im = 0.0;
        self.inverse
            .process_with_scratch(&mut self.output_spectrum, &mut self.fft_scratch);
        let scale = 1.0 / FFT_SIZE as f32;
        for index in 0..FFT_SIZE {
            self.overlap[index] += self.output_spectrum[index].re * scale * self.window[index];
        }
        output[..HOP_SIZE].copy_from_slice(&self.overlap[..HOP_SIZE]);
        self.overlap.copy_within(HOP_SIZE.., 0);
        self.overlap[FFT_SIZE - HOP_SIZE..].fill(0.0);
        self.frame_index = self.frame_index.wrapping_add(1);
        Ok(())
    }

    pub(super) fn reset(&mut self) {
        self.analysis.fill(0.0);
        self.spectra.fill(Complex32::new(0.0, 0.0));
        self.covariance.fill(Complex32::new(0.0, 0.0));
        self.weights.fill(Complex32::new(0.0, 0.0));
        self.output_spectrum.fill(Complex32::new(0.0, 0.0));
        self.overlap.fill(0.0);
        self.postfilter_gain.fill(1.0);
        self.frame_index = 0;
        for bin in 0..BINS {
            for channel in 0..self.channels {
                self.covariance[(bin * self.channels + channel) * self.channels + channel].re =
                    1.0e-3;
                self.weights[bin * self.channels + channel].re = 1.0 / self.channels as f32;
            }
        }
    }

    fn update_covariance(&mut self, bin: usize, alpha: f32) {
        let base = bin * self.channels * self.channels;
        for row in 0..self.channels {
            let row_value = self.spectra[row * FFT_SIZE + bin];
            for column in 0..=row {
                let column_value = self.spectra[column * FFT_SIZE + bin];
                let estimate = row_value * column_value.conj();
                let index = base + row * self.channels + column;
                let updated = self.covariance[index] * alpha + estimate * (1.0 - alpha);
                self.covariance[index] = updated;
                self.covariance[base + column * self.channels + row] = updated.conj();
            }
        }
    }

    fn update_weights(&mut self, bin: usize) {
        let covariance_start = bin * self.channels * self.channels;
        let covariance =
            &self.covariance[covariance_start..covariance_start + self.channels * self.channels];
        let solved = solve_loaded(
            covariance,
            self.channels,
            &mut self.matrix,
            &mut self.lower,
            &mut self.forward_solution,
            &mut self.solution,
        );
        let weights = &mut self.weights[bin * self.channels..(bin + 1) * self.channels];
        if !solved {
            health::bump(&health::ARRAY_MVDR_FALLBACK_BINS, 1);
            let equal = 1.0 / self.channels as f32;
            for weight in weights {
                *weight = Complex32::new(equal, 0.0);
            }
            return;
        }
        let denominator: Complex32 = self.solution.iter().copied().sum();
        if !finite(denominator) || denominator.norm_sqr() < 1.0e-12 {
            health::bump(&health::ARRAY_MVDR_FALLBACK_BINS, 1);
            return;
        }
        let mut norm = 0.0f32;
        for value in &mut self.solution {
            *value /= denominator;
            norm += value.norm_sqr();
        }
        let maximum_norm = 4.0 / (self.channels as f32).sqrt();
        let scale = (maximum_norm / norm.sqrt().max(1.0e-6)).min(1.0);
        for (weight, &value) in weights.iter_mut().zip(&self.solution) {
            let candidate = value * scale;
            if finite(candidate) {
                *weight = *weight * 0.8 + candidate * 0.2;
            }
        }
    }
}

fn solve_loaded(
    covariance: &[Complex32],
    channels: usize,
    matrix: &mut [Complex32],
    lower: &mut [Complex32],
    forward: &mut [Complex32],
    solution: &mut [Complex32],
) -> bool {
    let trace = (0..channels)
        .map(|channel| covariance[channel * channels + channel].re.max(0.0))
        .sum::<f32>()
        / channels as f32;
    let base_loading = (trace * 1.0e-3).max(1.0e-6);
    for attempt in 0..4 {
        matrix.copy_from_slice(covariance);
        let loading = base_loading * 10.0_f32.powi(attempt);
        for channel in 0..channels {
            matrix[channel * channels + channel].re += loading;
            matrix[channel * channels + channel].im = 0.0;
        }
        if cholesky_solve_ones(matrix, channels, lower, forward, solution) {
            return true;
        }
    }
    false
}

fn cholesky_solve_ones(
    matrix: &[Complex32],
    channels: usize,
    lower: &mut [Complex32],
    forward: &mut [Complex32],
    solution: &mut [Complex32],
) -> bool {
    lower.fill(Complex32::new(0.0, 0.0));
    for row in 0..channels {
        for column in 0..=row {
            let mut value = matrix[row * channels + column];
            for inner in 0..column {
                value -= lower[row * channels + inner] * lower[column * channels + inner].conj();
            }
            if row == column {
                if !value.re.is_finite() || value.re <= 1.0e-12 {
                    return false;
                }
                lower[row * channels + column] = Complex32::new(value.re.sqrt(), 0.0);
            } else {
                let diagonal = lower[column * channels + column].re;
                if diagonal <= 1.0e-12 {
                    return false;
                }
                lower[row * channels + column] = value / diagonal;
            }
        }
    }
    for row in 0..channels {
        let mut value = Complex32::new(1.0, 0.0);
        for column in 0..row {
            value -= lower[row * channels + column] * forward[column];
        }
        forward[row] = value / lower[row * channels + row].re;
    }
    for row in (0..channels).rev() {
        let mut value = forward[row];
        for column in row + 1..channels {
            value -= lower[column * channels + row].conj() * solution[column];
        }
        solution[row] = value / lower[row * channels + row].re;
    }
    solution.iter().all(|&value| finite(value))
}

fn finite(value: Complex32) -> bool {
    value.re.is_finite() && value.im.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;
    use std::time::Instant;

    #[test]
    fn identical_channels_reconstruct_with_explicit_latency() {
        let mut mvdr = Mvdr::new(4, 1.0, 18.0, false).unwrap();
        let blocks = 32;
        let mut input = Vec::with_capacity(blocks * HOP_SIZE);
        let mut output = Vec::with_capacity(blocks * HOP_SIZE);
        for block in 0..blocks {
            let mono: Vec<f32> = (0..HOP_SIZE)
                .map(|frame| {
                    let index = block * HOP_SIZE + frame;
                    (2.0 * std::f32::consts::PI * 997.0 * index as f32 / 48_000.0).sin() * 0.25
                })
                .collect();
            input.extend_from_slice(&mono);
            let mut planar = Vec::with_capacity(4 * HOP_SIZE);
            for _ in 0..4 {
                planar.extend_from_slice(&mono);
            }
            let mut block_output = vec![0.0; HOP_SIZE];
            mvdr.process(&planar, &mut block_output, true).unwrap();
            output.extend(block_output);
        }
        let skip = HOP_SIZE;
        let compared = output.len() - skip;
        let error = output[skip..]
            .iter()
            .zip(&input[..compared])
            .map(|(actual, expected)| (actual - expected).powi(2))
            .sum::<f32>()
            / compared as f32;
        assert!(
            error.sqrt() < 0.002,
            "reconstruction RMS error={}",
            error.sqrt()
        );
    }

    #[test]
    fn arbitrary_channel_counts_stay_finite() {
        for channels in [2, 4, 8, 16] {
            let mut mvdr = Mvdr::new(channels, 1.0, 24.0, true).unwrap();
            let mut state = 0x1357_2468u32;
            for _ in 0..16 {
                let input: Vec<f32> = (0..channels * HOP_SIZE)
                    .map(|_| {
                        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        (state as f32 / u32::MAX as f32) * 0.1 - 0.05
                    })
                    .collect();
                let mut output = vec![0.0; HOP_SIZE];
                mvdr.process(&input, &mut output, true).unwrap();
                assert!(
                    output.iter().all(|sample| sample.is_finite()),
                    "N={channels}"
                );
            }
        }
    }

    #[test]
    fn coherent_target_is_preserved_while_off_axis_tone_is_reduced() {
        let channels = 4;
        let blocks = 96;
        let total_frames = blocks * HOP_SIZE;
        let mut mvdr = Mvdr::new(channels, 1.0, 24.0, false).unwrap();
        let mut output = Vec::with_capacity(total_frames);
        let mut target = Vec::with_capacity(total_frames);
        let mut interference = Vec::with_capacity(total_frames);
        for block in 0..blocks {
            let mut planar = vec![0.0; channels * HOP_SIZE];
            for frame in 0..HOP_SIZE {
                let index = block * HOP_SIZE + frame;
                let target_sample = tone(700.0, index) * 0.12;
                target.push(target_sample);
                interference.push(tone(1_800.0, index) * 0.28);
                for channel in 0..channels {
                    let off_axis = tone(1_800.0, index + channel * 5) * 0.28;
                    planar[channel * HOP_SIZE + frame] = target_sample + off_axis;
                }
            }
            let mut block_output = vec![0.0; HOP_SIZE];
            mvdr.process(&planar, &mut block_output, true).unwrap();
            output.extend(block_output);
        }

        let start = total_frames / 2;
        let output = &output[start + HOP_SIZE..];
        let target = &target[start..total_frames - HOP_SIZE];
        let interference = &interference[start..total_frames - HOP_SIZE];
        let target_gain = projection_gain(output, target);
        let interference_gain = projection_gain(output, interference);
        assert!(target_gain > 0.8, "target gain={target_gain}");
        assert!(
            interference_gain.abs() < 0.35,
            "interference gain={interference_gain}"
        );
    }

    #[test]
    fn reset_clears_overlap_and_adaptive_state() {
        let mut mvdr = Mvdr::new(2, 1.0, 18.0, true).unwrap();
        let input = vec![0.4; 2 * HOP_SIZE];
        let mut output = vec![0.0; HOP_SIZE];
        mvdr.process(&input, &mut output, true).unwrap();
        mvdr.process(&input, &mut output, true).unwrap();
        assert!(output.iter().any(|sample| sample.abs() > 0.1));

        mvdr.reset();
        let silence = vec![0.0; 2 * HOP_SIZE];
        mvdr.process(&silence, &mut output, false).unwrap();
        assert!(output.iter().all(|sample| sample.abs() < 1.0e-7));
    }

    #[test]
    #[ignore = "manual performance benchmark"]
    fn realtime_cost_by_channel_count() {
        let audio_blocks = 48_000 / HOP_SIZE * 5;
        for channels in [2, 4, 8, 16] {
            let mut mvdr = Mvdr::new(channels, 1.0, 24.0, true).unwrap();
            let input = vec![0.01; channels * HOP_SIZE];
            let mut output = vec![0.0; HOP_SIZE];
            for _ in 0..32 {
                mvdr.process(&input, &mut output, true).unwrap();
            }
            let started = Instant::now();
            for _ in 0..audio_blocks {
                mvdr.process(black_box(&input), black_box(&mut output), true)
                    .unwrap();
            }
            let realtime_percent = started.elapsed().as_secs_f64() / 5.0 * 100.0;
            println!("MVDR N={channels}: {realtime_percent:.2}% of one worker core");
            assert!(
                realtime_percent < 100.0,
                "MVDR N={channels} missed real time"
            );
        }
    }

    fn tone(frequency: f32, frame: usize) -> f32 {
        (2.0 * std::f32::consts::PI * frequency * frame as f32 / 48_000.0).sin()
    }

    fn projection_gain(signal: &[f32], reference: &[f32]) -> f32 {
        let numerator = signal
            .iter()
            .zip(reference)
            .map(|(sample, reference)| sample * reference)
            .sum::<f32>();
        let denominator = reference.iter().map(|sample| sample * sample).sum::<f32>();
        numerator / denominator.max(1.0e-12)
    }
}
