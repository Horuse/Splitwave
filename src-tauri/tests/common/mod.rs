#![allow(dead_code)]

use std::f32::consts::PI;

/// Mathematical test signal generators for audio engine testing.
pub mod generators {
    use super::*;

    /// Generates a pure sine wave at the specified frequency, sample rate, and amplitude.
    pub fn sine(freq_hz: f32, sample_rate: u32, duration_secs: f32, amplitude: f32) -> Vec<f32> {
        let total_frames = (sample_rate as f32 * duration_secs) as usize;
        let mut out = Vec::with_capacity(total_frames);
        let phase_step = 2.0 * PI * freq_hz / sample_rate as f32;
        for i in 0..total_frames {
            let sample = (i as f32 * phase_step).sin() * amplitude;
            out.push(sample);
        }
        out
    }

    /// Generates an interleaved stereo sine wave with independent left and right frequencies.
    pub fn sine_stereo(
        freq_l: f32,
        freq_r: f32,
        sample_rate: u32,
        duration_secs: f32,
        amplitude: f32,
    ) -> Vec<f32> {
        let total_frames = (sample_rate as f32 * duration_secs) as usize;
        let mut out = Vec::with_capacity(total_frames * 2);
        let step_l = 2.0 * PI * freq_l / sample_rate as f32;
        let step_r = 2.0 * PI * freq_r / sample_rate as f32;
        for i in 0..total_frames {
            out.push((i as f32 * step_l).sin() * amplitude);
            out.push((i as f32 * step_r).sin() * amplitude);
        }
        out
    }

    /// Generates a logarithmic sine sweep across a given frequency range (Chirp).
    pub fn sweep(
        start_hz: f32,
        end_hz: f32,
        sample_rate: u32,
        duration_secs: f32,
        amplitude: f32,
    ) -> Vec<f32> {
        let total_frames = (sample_rate as f32 * duration_secs) as usize;
        let mut out = Vec::with_capacity(total_frames);
        let sr = sample_rate as f32;
        let t_total = duration_secs;
        for i in 0..total_frames {
            let t = i as f32 / sr;
            let f = start_hz + (end_hz - start_hz) * (t / (2.0 * t_total));
            let sample = (2.0 * PI * f * t).sin() * amplitude;
            out.push(sample);
        }
        out
    }

    /// Generates a multi-tone signal composed of multiple harmonic frequencies.
    pub fn multitone(
        freqs: &[f32],
        sample_rate: u32,
        duration_secs: f32,
        peak_amplitude: f32,
    ) -> Vec<f32> {
        let total_frames = (sample_rate as f32 * duration_secs) as usize;
        let mut out = vec![0.0f32; total_frames];
        let num_tones = freqs.len().max(1) as f32;
        let amp_per_tone = peak_amplitude / num_tones;

        for &freq in freqs {
            let phase_step = 2.0 * PI * freq / sample_rate as f32;
            for (i, slot) in out.iter_mut().enumerate() {
                *slot += (i as f32 * phase_step).sin() * amp_per_tone;
            }
        }
        out
    }

    /// Generates a periodic tone burst (active tone alternating with silence).
    pub fn tone_burst(
        freq_hz: f32,
        sample_rate: u32,
        active_ms: f32,
        silent_ms: f32,
        cycles: usize,
        amplitude: f32,
    ) -> Vec<f32> {
        let active_frames = (sample_rate as f32 * active_ms / 1000.0) as usize;
        let silent_frames = (sample_rate as f32 * silent_ms / 1000.0) as usize;
        let mut out = Vec::with_capacity((active_frames + silent_frames) * cycles);
        let phase_step = 2.0 * PI * freq_hz / sample_rate as f32;

        for _ in 0..cycles {
            for i in 0..active_frames {
                out.push((i as f32 * phase_step).sin() * amplitude);
            }
            out.resize(out.len() + silent_frames, 0.0);
        }
        out
    }

    /// Generates pure DC offset of specified amplitude.
    pub fn dc_offset(sample_rate: u32, duration_secs: f32, offset: f32) -> Vec<f32> {
        let total_frames = (sample_rate as f32 * duration_secs) as usize;
        vec![offset; total_frames]
    }
}

/// Quantitative audio signal metrics (RMS, Peak, THD+N, SNR, DC offset).
pub mod metrics {
    use super::*;

    /// Calculates root-mean-square (RMS) energy.
    pub fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
        (sum_sq / samples.len() as f64).sqrt() as f32
    }

    /// Calculates absolute peak amplitude.
    pub fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()))
    }

    /// Converts linear amplitude to decibels full scale (dBFS).
    pub fn to_dbfs(amplitude: f32) -> f32 {
        if amplitude <= 1e-6 {
            -120.0
        } else {
            20.0 * amplitude.log10()
        }
    }

    /// Calculates average DC offset (mean sample value).
    pub fn dc_offset(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = samples.iter().map(|&s| s as f64).sum();
        (sum / samples.len() as f64) as f32
    }

    /// Verifies bit-exactness between two audio buffers.
    /// Returns (is_exact, max_absolute_difference).
    pub fn verify_bit_exactness(original: &[f32], processed: &[f32]) -> (bool, f32) {
        if original.len() != processed.len() {
            return (false, f32::INFINITY);
        }
        let mut max_diff = 0.0f32;
        for (&a, &b) in original.iter().zip(processed.iter()) {
            let diff = (a - b).abs();
            if diff > max_diff {
                max_diff = diff;
            }
        }
        (max_diff == 0.0, max_diff)
    }

    /// Calculates Total Harmonic Distortion + Noise (THD+N) relative to fundamental.
    /// Approximated via notch removal of fundamental frequency using discrete Fourier correlation.
    pub fn thd_n(samples: &[f32], fundamental_hz: f32, sample_rate: u32) -> f32 {
        let n = samples.len();
        if n < 256 {
            return 0.0;
        }
        let omega = 2.0 * PI * fundamental_hz / sample_rate as f32;

        let mut cos_sum = 0.0f64;
        let mut sin_sum = 0.0f64;
        for (i, &s) in samples.iter().enumerate() {
            let phi = (i as f32) * omega;
            cos_sum += (s as f64) * (phi.cos() as f64);
            sin_sum += (s as f64) * (phi.sin() as f64);
        }
        let a = (2.0 / n as f64) * cos_sum;
        let b = (2.0 / n as f64) * sin_sum;
        let fundamental_rms = ((a * a + b * b) / 2.0).sqrt();

        if fundamental_rms < 1e-6 {
            return 0.0;
        }

        let mut residual_sq = 0.0f64;
        for (i, &s) in samples.iter().enumerate() {
            let phi = (i as f32) * omega;
            let fundamental_val = a * (phi.cos() as f64) + b * (phi.sin() as f64);
            let residual = (s as f64) - fundamental_val;
            residual_sq += residual * residual;
        }
        let residual_rms = (residual_sq / n as f64).sqrt();

        (residual_rms / fundamental_rms) as f32
    }

    /// Calculates Signal-to-Noise Ratio (SNR) in dB between a reference signal and noise.
    pub fn snr_db(signal_rms: f32, noise_rms: f32) -> f32 {
        if noise_rms <= 1e-9 {
            120.0
        } else if signal_rms <= 1e-9 {
            -120.0
        } else {
            20.0 * (signal_rms / noise_rms).log10()
        }
    }
}
