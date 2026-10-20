use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};
use std::f32::consts::PI;
use std::sync::Arc;

pub const FFT_SIZE: usize = 4096;
pub const BINS: usize = FFT_SIZE / 2;
pub const BARS: usize = 80;
pub const F_MIN: f32 = 20.0;
pub const DB_FLOOR: f32 = -96.0;
pub const MAX_CH: usize = 8;

pub struct SpectrumCalculator {
    r2c: Arc<dyn RealToComplex<f32>>,
    hann: Vec<f32>,
    in_buf: Vec<f32>,
    spectrum_buf: Vec<Complex<f32>>,
    last_sample_rate: u32,
    band_bins: Vec<(usize, usize)>,
}

impl SpectrumCalculator {
    pub fn new() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(FFT_SIZE);
        let in_buf = r2c.make_input_vec();
        let spectrum_buf = r2c.make_output_vec();

        let mut hann = vec![0.0_f32; FFT_SIZE];
        for (i, h) in hann.iter_mut().enumerate() {
            *h = 0.5 * (1.0 - (2.0 * PI * i as f32 / (FFT_SIZE - 1) as f32).cos());
        }

        let mut s = Self {
            r2c,
            hann,
            in_buf,
            spectrum_buf,
            last_sample_rate: 0,
            band_bins: Vec::with_capacity(BARS),
        };
        s.update_bands(48000);
        s
    }

    fn update_bands(&mut self, sample_rate: u32) {
        if sample_rate == self.last_sample_rate && !self.band_bins.is_empty() {
            return;
        }
        self.last_sample_rate = sample_rate;
        self.band_bins.clear();
        let f_max = (sample_rate as f32) / 2.0;
        let bins_f = BINS as f32;

        for b in 0..BARS {
            let f0 = F_MIN * (f_max / F_MIN).powf(b as f32 / BARS as f32);
            let f1 = F_MIN * (f_max / F_MIN).powf((b + 1) as f32 / BARS as f32);
            let lo = (1usize).max(((f0 / f_max) * bins_f).floor() as usize);
            let hi = (lo + 1).max(((f1 / f_max) * bins_f).ceil() as usize);
            self.band_bins.push((lo.min(BINS), hi.min(BINS)));
        }
    }

    /// Takes interleaved audio with `frames` frames and `channels` channels.
    /// Runs FFT over up to `MAX_CH` channels, takes max magnitude per bin,
    /// and returns 80 log-spaced bar levels in decibels [DB_FLOOR..0 dB].
    pub fn calculate_bars(
        &mut self,
        interleaved: &[f32],
        channels: usize,
        sample_rate: u32,
    ) -> Vec<f32> {
        self.update_bands(sample_rate);
        let ch = channels.max(1);
        let cap_ch = ch.min(MAX_CH);
        let frames = interleaved.len() / ch;
        let n = FFT_SIZE.min(frames);

        let mut band_peaks = vec![0.0_f32; BARS];

        for c in 0..cap_ch {
            // Fill input with Hann-windowed samples for this channel
            for i in 0..n {
                self.in_buf[i] = interleaved[i * ch + c] * self.hann[i];
            }
            for i in n..FFT_SIZE {
                self.in_buf[i] = 0.0;
            }

            if self
                .r2c
                .process(&mut self.in_buf, &mut self.spectrum_buf)
                .is_ok()
            {
                for b in 0..BARS {
                    let (lo, hi) = self.band_bins[b];
                    let mut peak = band_peaks[b];
                    for k in lo..hi {
                        let c = self.spectrum_buf[k];
                        let mag = (c.re * c.re + c.im * c.im).sqrt();
                        if mag > peak {
                            peak = mag;
                        }
                    }
                    band_peaks[b] = peak;
                }
            }
        }

        // Convert to normalized dB
        let bins_norm = BINS as f32;
        let mut bars_db = vec![DB_FLOOR; BARS];
        for b in 0..BARS {
            let norm = band_peaks[b] / bins_norm;
            if norm > 1e-7 {
                bars_db[b] = (20.0 * norm.log10()).max(DB_FLOOR);
            }
        }

        bars_db
    }
}
