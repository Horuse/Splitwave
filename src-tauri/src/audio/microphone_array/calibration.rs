//! Offline N-channel calibration. All allocation and FFT work happens outside
//! capture callbacks; the resulting scalar corrections are applied by the
//! real-time processor at construction.

use std::collections::VecDeque;

use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

use crate::audio::graph::{
    MicrophoneArrayCalibrationState, MicrophoneArrayChannelQuality, MicrophoneArrayData,
    MicrophoneArrayTarget,
};
use crate::error::{AppError, AppResult};

use super::{steering_delays, MemberConfig, Point3, SteeringTarget};

const MAX_ANALYSIS_FRAMES: usize = 32_768;
const MIN_PAIR_CONFIDENCE: f32 = 0.15;
const CLIP_THRESHOLD: f32 = 0.999;

#[derive(Debug, Clone)]
pub struct CalibrationConfig {
    pub sample_rate: u32,
    pub positions: Vec<Point3>,
    pub target: SteeringTarget,
    pub enabled: Vec<bool>,
    pub independent_devices: bool,
}

#[derive(Debug, Clone)]
pub struct PairObservation {
    pub first: usize,
    pub second: usize,
    pub observed_delay_samples: f32,
    pub expected_delay_samples: f32,
    pub residual_delay_samples: f32,
    pub confidence: f32,
    pub peak_to_sidelobe: f32,
    pub polarity_inverted: bool,
}

#[derive(Debug, Clone)]
pub struct ChannelCalibration {
    pub delay_offset_samples: f32,
    pub gain_db: f32,
    pub polarity_inverted: bool,
    pub rms_dbfs: f32,
    pub clipped_fraction: f32,
    pub confidence: f32,
    pub quality: MicrophoneArrayChannelQuality,
    pub exclusion_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CalibrationResult {
    pub channels: Vec<ChannelCalibration>,
    pub pairs: Vec<PairObservation>,
    pub residual_rms_samples: f32,
    pub quality_score: u8,
}

pub fn analyze(
    config: &CalibrationConfig,
    planar: &[f32],
    frames: usize,
) -> AppResult<CalibrationResult> {
    let channels = config.positions.len();
    if channels < 2
        || config.enabled.len() != channels
        || frames < 256
        || planar.len() != channels * frames
        || config.sample_rate == 0
    {
        return Err(AppError::Validation(
            "Microphone Array calibration input shape is invalid".into(),
        ));
    }
    let active: Vec<usize> = config
        .enabled
        .iter()
        .enumerate()
        .filter_map(|(index, &enabled)| enabled.then_some(index))
        .collect();
    if active.len() < 2 {
        return Err(AppError::Validation(
            "Microphone Array calibration needs at least two enabled channels".into(),
        ));
    }

    let analysis_frames = frames.min(MAX_ANALYSIS_FRAMES);
    let start = frames - analysis_frames;
    let fft_len = (analysis_frames * 2).next_power_of_two();
    let mut planner = FftPlanner::<f32>::new();
    let forward = planner.plan_fft_forward(fft_len);
    let inverse = planner.plan_fft_inverse(fft_len);
    let mut spectra = Vec::with_capacity(channels);
    let mut rms = vec![0.0f32; channels];
    let mut clipped = vec![0.0f32; channels];
    for channel in 0..channels {
        let samples = &planar[channel * frames + start..(channel + 1) * frames];
        let (spectrum, level, clipped_fraction) =
            channel_spectrum(samples, config.sample_rate, fft_len, forward.as_ref());
        spectra.push(spectrum);
        rms[channel] = level;
        clipped[channel] = clipped_fraction;
    }

    let steering_members: Vec<MemberConfig> = config
        .positions
        .iter()
        .map(|&position| MemberConfig {
            position,
            enabled: true,
            weight: 1.0,
            gain_db: 0.0,
            polarity_inverted: false,
            fixed_delay_samples: 0.0,
        })
        .collect();
    let expected_steering = steering_delays(config.sample_rate, config.target, &steering_members)?;
    let physical_span = maximum_span(&config.positions);
    let physical_bound =
        (physical_span / super::SPEED_OF_SOUND_MPS * config.sample_rate as f32).ceil() as usize + 8;
    let max_lag = if config.independent_devices {
        physical_bound.max(config.sample_rate as usize / 50)
    } else {
        physical_bound
    }
    .min(fft_len / 4);

    let mut pairs = Vec::new();
    let mut scratch = vec![Complex32::new(0.0, 0.0); fft_len];
    for (active_slot, &first) in active.iter().enumerate() {
        for &second in &active[active_slot + 1..] {
            let peak = gcc_phat(
                &spectra[first],
                &spectra[second],
                max_lag,
                inverse.as_ref(),
                &mut scratch,
            );
            let expected = expected_steering[first] - expected_steering[second];
            pairs.push(PairObservation {
                first,
                second,
                observed_delay_samples: peak.delay_samples,
                expected_delay_samples: expected,
                residual_delay_samples: peak.delay_samples - expected,
                confidence: peak.confidence,
                peak_to_sidelobe: peak.peak_to_sidelobe,
                polarity_inverted: peak.polarity_inverted,
            });
        }
    }

    let anchor = active[0];
    let connected = connected_channels(channels, anchor, &pairs);
    let (offsets, residual_rms) = solve_offsets(channels, anchor, &pairs)?;
    let polarities = solve_polarities(channels, anchor, &pairs);
    let reference_level = median(
        active
            .iter()
            .filter_map(|&index| (rms[index] > 1.0e-6).then_some(rms[index]))
            .collect(),
    )
    .unwrap_or(1.0);

    let mut channel_results = Vec::with_capacity(channels);
    for channel in 0..channels {
        let pair_confidence = mean(
            pairs
                .iter()
                .filter(|pair| pair.first == channel || pair.second == channel)
                .map(|pair| pair.confidence),
        );
        let rms_dbfs = linear_to_db(rms[channel]);
        let mut quality = MicrophoneArrayChannelQuality::Good;
        let mut reason = None;
        if !config.enabled[channel] {
            quality = MicrophoneArrayChannelQuality::Excluded;
            reason = Some("disabled".into());
        } else if rms[channel] < 0.001 {
            quality = MicrophoneArrayChannelQuality::Excluded;
            reason = Some("no signal".into());
        } else if !connected[channel] {
            quality = MicrophoneArrayChannelQuality::Excluded;
            reason = Some("disconnected delay graph".into());
        } else if clipped[channel] > 0.01 {
            quality = MicrophoneArrayChannelQuality::Excluded;
            reason = Some("clipping".into());
        } else if clipped[channel] > 0.001 {
            quality = MicrophoneArrayChannelQuality::Marginal;
            reason = Some("occasional clipping".into());
        } else if pair_confidence < 0.35 {
            quality = MicrophoneArrayChannelQuality::Marginal;
            reason = Some("low coherence".into());
        }
        channel_results.push(ChannelCalibration {
            delay_offset_samples: offsets[channel],
            gain_db: if rms[channel] > 1.0e-6 {
                (20.0 * (reference_level / rms[channel]).log10()).clamp(-12.0, 12.0)
            } else {
                0.0
            },
            polarity_inverted: polarities[channel],
            rms_dbfs,
            clipped_fraction: clipped[channel],
            confidence: pair_confidence,
            quality,
            exclusion_reason: reason,
        });
    }

    let usable = channel_results
        .iter()
        .filter(|channel| channel.quality != MicrophoneArrayChannelQuality::Excluded)
        .count();
    let mean_confidence = mean(channel_results.iter().map(|channel| channel.confidence));
    let residual_factor = (1.0 - residual_rms / 2.0).clamp(0.0, 1.0);
    let score = 100.0
        * (usable as f32 / active.len() as f32)
        * (0.35 + 0.65 * mean_confidence)
        * residual_factor;
    Ok(CalibrationResult {
        channels: channel_results,
        pairs,
        residual_rms_samples: residual_rms,
        quality_score: score.round().clamp(0.0, 100.0) as u8,
    })
}

pub fn apply_result(
    data: &MicrophoneArrayData,
    result: &CalibrationResult,
    stream_formats: &[(String, u32, u16)],
) -> AppResult<MicrophoneArrayData> {
    if result.channels.len() != data.members.len() {
        return Err(AppError::Validation(
            "Microphone Array calibration result no longer matches its members".into(),
        ));
    }
    let mut calibrated = data.clone();
    let latest = result
        .channels
        .iter()
        .filter(|channel| channel.quality != MicrophoneArrayChannelQuality::Excluded)
        .map(|channel| channel.delay_offset_samples)
        .fold(f32::NEG_INFINITY, f32::max);
    let latest = if latest.is_finite() { latest } else { 0.0 };
    for (member, channel) in calibrated.members.iter_mut().zip(&result.channels) {
        member.gain_db = channel.gain_db;
        member.polarity_inverted = channel.polarity_inverted;
        member.fixed_delay_samples = (latest - channel.delay_offset_samples).max(0.0);
        member.quality = channel.quality;
        member.exclusion_reason = channel.exclusion_reason.clone();
    }
    calibrated.calibration.state = if result.quality_score >= 50 {
        MicrophoneArrayCalibrationState::Ready
    } else {
        MicrophoneArrayCalibrationState::NeedsReview
    };
    calibrated.calibration.fingerprint = Some(fingerprint(&calibrated, stream_formats));
    calibrated.calibration.residual_delay_samples = Some(result.residual_rms_samples);
    calibrated.calibration.quality_score = Some(result.quality_score);
    Ok(calibrated)
}

pub fn fingerprint(data: &MicrophoneArrayData, stream_formats: &[(String, u32, u16)]) -> String {
    let mut hash = Fnv64::new();
    hash.u32(data.processing_sample_rate);
    hash.str(data.master_source_id.as_deref().unwrap_or(""));
    for source in &data.sources {
        hash.str(&source.id);
        hash.str(source.device_id.as_deref().unwrap_or(""));
    }
    for member in &data.members {
        hash.str(&member.source_id);
        hash.u32(member.channel_index);
        hash.bool(member.enabled);
        hash.f32(member.position.x);
        hash.f32(member.position.y);
        hash.f32(member.position.z);
    }
    match data.target {
        MicrophoneArrayTarget::Direction {
            azimuth_degrees,
            elevation_degrees,
        } => {
            hash.u8(0);
            hash.f32(azimuth_degrees);
            hash.f32(elevation_degrees);
        }
        MicrophoneArrayTarget::Point { x, y, z } => {
            hash.u8(1);
            hash.f32(x);
            hash.f32(y);
            hash.f32(z);
        }
    }
    for (source_id, rate, channels) in stream_formats {
        hash.str(source_id);
        hash.u32(*rate);
        hash.u16(*channels);
    }
    format!("array-v1-{:016x}", hash.finish())
}

pub fn fingerprint_matches(
    data: &MicrophoneArrayData,
    stream_formats: &[(String, u32, u16)],
) -> bool {
    let current = fingerprint(data, stream_formats);
    data.calibration
        .fingerprint
        .as_deref()
        .is_some_and(|saved| saved == current)
}

struct CorrelationPeak {
    delay_samples: f32,
    confidence: f32,
    peak_to_sidelobe: f32,
    polarity_inverted: bool,
}

fn channel_spectrum(
    samples: &[f32],
    sample_rate: u32,
    fft_len: usize,
    forward: &dyn rustfft::Fft<f32>,
) -> (Vec<Complex32>, f32, f32) {
    let mut spectrum = vec![Complex32::new(0.0, 0.0); fft_len];
    let high_alpha = (-2.0 * std::f32::consts::PI * 80.0 / sample_rate as f32).exp();
    let low_alpha = 1.0 - (-2.0 * std::f32::consts::PI * 8_000.0 / sample_rate as f32).exp();
    let mut previous = 0.0;
    let mut high_state = 0.0;
    let mut low_state = 0.0;
    let mut sum_sq = 0.0f64;
    let mut clipped = 0usize;
    for (index, &sample) in samples.iter().enumerate() {
        sum_sq += sample as f64 * sample as f64;
        clipped += usize::from(sample.abs() >= CLIP_THRESHOLD);
        high_state = sample - previous + high_alpha * high_state;
        previous = sample;
        low_state += low_alpha * (high_state - low_state);
        let window = 0.5
            - 0.5
                * (2.0 * std::f32::consts::PI * index as f32 / (samples.len() - 1).max(1) as f32)
                    .cos();
        spectrum[index].re = low_state * window;
    }
    forward.process(&mut spectrum);
    (
        spectrum,
        (sum_sq / samples.len() as f64).sqrt() as f32,
        clipped as f32 / samples.len() as f32,
    )
}

fn gcc_phat(
    first: &[Complex32],
    second: &[Complex32],
    max_lag: usize,
    inverse: &dyn rustfft::Fft<f32>,
    scratch: &mut [Complex32],
) -> CorrelationPeak {
    for ((out, &left), &right) in scratch.iter_mut().zip(first).zip(second) {
        let cross = right * left.conj();
        let magnitude = cross.norm();
        *out = if magnitude > 1.0e-12 {
            cross / magnitude
        } else {
            Complex32::new(0.0, 0.0)
        };
    }
    inverse.process(scratch);
    let scale = 1.0 / scratch.len() as f32;
    let at = |lag: isize| {
        let index = if lag >= 0 {
            lag as usize
        } else {
            scratch.len() - (-lag) as usize
        };
        scratch[index].re * scale
    };
    let mut best_lag = 0isize;
    let mut best = 0.0f32;
    for lag in -(max_lag as isize)..=max_lag as isize {
        let value = at(lag).abs();
        if value > best {
            best = value;
            best_lag = lag;
        }
    }
    let before = at(best_lag - 1).abs();
    let center = at(best_lag).abs();
    let after = at(best_lag + 1).abs();
    let denominator = before - 2.0 * center + after;
    let fractional = if denominator.abs() > 1.0e-9 {
        (0.5 * (before - after) / denominator).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let mut side_sum = 0.0f64;
    let mut side_count = 0usize;
    let mut second_peak = 0.0f32;
    for lag in -(max_lag as isize)..=max_lag as isize {
        if (lag - best_lag).abs() <= 2 {
            continue;
        }
        let value = at(lag).abs();
        side_sum += value as f64 * value as f64;
        side_count += 1;
        second_peak = second_peak.max(value);
    }
    let side_rms = (side_sum / side_count.max(1) as f64).sqrt() as f32;
    let peak_to_sidelobe = best / side_rms.max(1.0e-6);
    let ambiguity = second_peak / best.max(1.0e-6);
    let confidence = (((peak_to_sidelobe - 2.0) / 8.0).clamp(0.0, 1.0)
        * (1.0 - ambiguity).clamp(0.0, 1.0))
    .sqrt();
    CorrelationPeak {
        delay_samples: best_lag as f32 + fractional,
        confidence,
        peak_to_sidelobe,
        polarity_inverted: at(best_lag) < 0.0,
    }
}

fn solve_offsets(
    channels: usize,
    anchor: usize,
    pairs: &[PairObservation],
) -> AppResult<(Vec<f32>, f32)> {
    let mut weights: Vec<f64> = pairs
        .iter()
        .map(|pair| pair.confidence.max(0.0).powi(2) as f64)
        .collect();
    let mut offsets = weighted_offsets(channels, anchor, pairs, &weights)?;
    let mut errors: Vec<f32> = pairs
        .iter()
        .map(|pair| offsets[pair.second] - offsets[pair.first] - pair.residual_delay_samples)
        .collect();
    let scale = median(errors.iter().map(|error| error.abs()).collect()).unwrap_or(0.0);
    let threshold = (3.0 * 1.4826 * scale).max(0.25);
    for (weight, error) in weights.iter_mut().zip(&errors) {
        if error.abs() > threshold {
            *weight *= threshold as f64 / error.abs() as f64;
        }
    }
    offsets = weighted_offsets(channels, anchor, pairs, &weights)?;
    errors = pairs
        .iter()
        .map(|pair| offsets[pair.second] - offsets[pair.first] - pair.residual_delay_samples)
        .collect();
    let weight_sum: f64 = weights.iter().sum();
    let residual = if weight_sum > 0.0 {
        (errors
            .iter()
            .zip(&weights)
            .map(|(error, weight)| *weight * *error as f64 * *error as f64)
            .sum::<f64>()
            / weight_sum)
            .sqrt() as f32
    } else {
        f32::INFINITY
    };
    Ok((offsets, residual))
}

fn weighted_offsets(
    channels: usize,
    anchor: usize,
    pairs: &[PairObservation],
    weights: &[f64],
) -> AppResult<Vec<f32>> {
    let unknowns: Vec<usize> = (0..channels).filter(|&index| index != anchor).collect();
    let mut matrix = vec![vec![0.0f64; unknowns.len()]; unknowns.len()];
    let mut rhs = vec![0.0f64; unknowns.len()];
    for (pair, &weight) in pairs.iter().zip(weights) {
        if pair.confidence < MIN_PAIR_CONFIDENCE || weight <= 0.0 {
            continue;
        }
        let first = unknowns.iter().position(|&index| index == pair.first);
        let second = unknowns.iter().position(|&index| index == pair.second);
        let value = pair.residual_delay_samples as f64;
        if let Some(first) = first {
            matrix[first][first] += weight;
            rhs[first] -= weight * value;
        }
        if let Some(second) = second {
            matrix[second][second] += weight;
            rhs[second] += weight * value;
        }
        if let (Some(first), Some(second)) = (first, second) {
            matrix[first][second] -= weight;
            matrix[second][first] -= weight;
        }
    }
    for diagonal in 0..unknowns.len() {
        if matrix[diagonal][diagonal] < 1.0e-10 {
            matrix[diagonal][diagonal] = 1.0;
        }
    }
    let solution = gaussian_solve(matrix, rhs).ok_or_else(|| {
        AppError::Validation("Microphone Array calibration delay graph is disconnected".into())
    })?;
    let mut offsets = vec![0.0f32; channels];
    for (&channel, value) in unknowns.iter().zip(solution) {
        offsets[channel] = value as f32;
    }
    Ok(offsets)
}

fn gaussian_solve(mut matrix: Vec<Vec<f64>>, mut rhs: Vec<f64>) -> Option<Vec<f64>> {
    for column in 0..rhs.len() {
        let pivot = (column..rhs.len()).max_by(|&left, &right| {
            matrix[left][column]
                .abs()
                .total_cmp(&matrix[right][column].abs())
        })?;
        if matrix[pivot][column].abs() < 1.0e-10 {
            return None;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let diagonal = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= diagonal;
        }
        rhs[column] /= diagonal;
        for row in 0..rhs.len() {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            for index in column..rhs.len() {
                matrix[row][index] -= factor * matrix[column][index];
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    Some(rhs)
}

fn connected_channels(channels: usize, anchor: usize, pairs: &[PairObservation]) -> Vec<bool> {
    let mut connected = vec![false; channels];
    let mut queue = VecDeque::from([anchor]);
    connected[anchor] = true;
    while let Some(channel) = queue.pop_front() {
        for pair in pairs
            .iter()
            .filter(|pair| pair.confidence >= MIN_PAIR_CONFIDENCE)
        {
            let other = if pair.first == channel {
                Some(pair.second)
            } else if pair.second == channel {
                Some(pair.first)
            } else {
                None
            };
            if let Some(other) = other {
                if !connected[other] {
                    connected[other] = true;
                    queue.push_back(other);
                }
            }
        }
    }
    connected
}

fn solve_polarities(channels: usize, anchor: usize, pairs: &[PairObservation]) -> Vec<bool> {
    let mut polarity = vec![false; channels];
    let mut known = vec![false; channels];
    let mut queue = VecDeque::from([anchor]);
    known[anchor] = true;
    while let Some(channel) = queue.pop_front() {
        let mut edges: Vec<&PairObservation> = pairs
            .iter()
            .filter(|pair| pair.first == channel || pair.second == channel)
            .collect();
        edges.sort_by(|left, right| right.confidence.total_cmp(&left.confidence));
        for pair in edges {
            let other = if pair.first == channel {
                pair.second
            } else {
                pair.first
            };
            if !known[other] && pair.confidence >= MIN_PAIR_CONFIDENCE {
                polarity[other] = polarity[channel] ^ pair.polarity_inverted;
                known[other] = true;
                queue.push_back(other);
            }
        }
    }
    polarity
}

fn maximum_span(points: &[Point3]) -> f32 {
    let mut maximum = 0.0f32;
    for (index, &first) in points.iter().enumerate() {
        for &second in &points[index + 1..] {
            maximum = maximum.max(first.sub(second).length());
        }
    }
    maximum
}

fn mean(values: impl Iterator<Item = f32>) -> f32 {
    let (sum, count) = values.fold((0.0, 0usize), |(sum, count), value| {
        (sum + value, count + 1)
    });
    if count == 0 {
        0.0
    } else {
        sum / count as f32
    }
}

fn median(mut values: Vec<f32>) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    })
}

fn linear_to_db(value: f32) -> f32 {
    20.0 * value.max(1.0e-9).log10()
}

struct Fnv64(u64);

impl Fnv64 {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= *byte as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        self.u32(value.to_bits());
    }

    fn bool(&mut self, value: bool) {
        self.u8(value as u8);
    }

    fn str(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.bytes(value.as_bytes());
    }

    fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::graph::{
        MicrophoneArrayCalibration, MicrophoneArrayGeometry, MicrophoneArrayMember,
        MicrophoneArrayPoint, MicrophoneArraySource,
    };

    fn noise(frames: usize) -> Vec<f32> {
        let mut state = 0x9876_5432u32;
        (0..frames)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state as f32 / u32::MAX as f32) * 0.5 - 0.25
            })
            .collect()
    }

    fn delayed(source: &[f32], delay: f32, gain: f32) -> Vec<f32> {
        (0..source.len())
            .map(|frame| {
                let position = frame as f32 - delay;
                if position < 0.0 {
                    return 0.0;
                }
                let base = position.floor() as usize;
                let fraction = position - base as f32;
                let first = source.get(base).copied().unwrap_or(0.0);
                let second = source.get(base + 1).copied().unwrap_or(first);
                (first + (second - first) * fraction) * gain
            })
            .collect()
    }

    #[test]
    fn recovers_n_channel_delay_gain_and_polarity() {
        let frames = 16_384;
        let source = noise(frames);
        let delays = [0.0, 3.0, 7.25, 1.0];
        let gains = [1.0, 0.5, 1.5, -1.0];
        let mut planar = Vec::new();
        for (&delay, &gain) in delays.iter().zip(&gains) {
            planar.extend(delayed(&source, delay, gain));
        }
        let result = analyze(
            &CalibrationConfig {
                sample_rate: 48_000,
                positions: vec![
                    Point3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0
                    };
                    4
                ],
                target: SteeringTarget::Direction {
                    azimuth_degrees: 90.0,
                    elevation_degrees: 0.0,
                },
                enabled: vec![true; 4],
                independent_devices: false,
            },
            &planar,
            frames,
        )
        .unwrap();
        let maximum_delay_error = result
            .channels
            .iter()
            .zip(&delays)
            .map(|(channel, delay)| (channel.delay_offset_samples - delay).abs())
            .fold(0.0f32, f32::max);
        println!(
            "Calibration N=4: residual RMS={:.3} samples, maximum delay error={maximum_delay_error:.3} samples, quality={}",
            result.residual_rms_samples, result.quality_score
        );
        for (channel, &delay) in result.channels.iter().zip(&delays) {
            assert!((channel.delay_offset_samples - delay).abs() < 0.35);
        }
        assert!((result.channels[1].gain_db - 6.02).abs() < 0.5);
        assert!(result.channels[3].polarity_inverted);
        assert!(result.quality_score >= 70);
    }

    #[test]
    fn fingerprint_tracks_structural_calibration_inputs() {
        let mut data = sample_data();
        let first = fingerprint(&data, &[("source".into(), 48_000, 2)]);
        data.members[1].position.x = 0.05;
        let second = fingerprint(&data, &[("source".into(), 48_000, 2)]);
        assert_ne!(first, second);
        data.members[1].position.x = 0.04;
        let third = fingerprint(&data, &[("source".into(), 44_100, 2)]);
        assert_ne!(first, third);
    }

    #[test]
    fn fingerprint_match_rejects_changed_stream_format_without_deleting_profile() {
        let mut data = sample_data();
        let formats = [("source".into(), 48_000, 2)];
        data.calibration.fingerprint = Some(fingerprint(&data, &formats));
        assert!(fingerprint_matches(&data, &formats));
        assert!(!fingerprint_matches(&data, &[("source".into(), 44_100, 2)]));
        assert!(data.calibration.fingerprint.is_some());
    }

    fn sample_data() -> MicrophoneArrayData {
        MicrophoneArrayData {
            sources: vec![MicrophoneArraySource {
                id: "source".into(),
                device_id: Some("device".into()),
                label: "Interface".into(),
            }],
            members: (0..2)
                .map(|channel| MicrophoneArrayMember {
                    source_id: "source".into(),
                    channel_index: channel,
                    label: format!("Mic {}", channel + 1),
                    position: MicrophoneArrayPoint {
                        x: channel as f32 * 0.04,
                        y: 0.0,
                        z: 0.0,
                    },
                    enabled: true,
                    weight: 1.0,
                    gain_db: 0.0,
                    polarity_inverted: false,
                    fixed_delay_samples: 0.0,
                    quality: MicrophoneArrayChannelQuality::Good,
                    exclusion_reason: None,
                })
                .collect(),
            master_source_id: None,
            processing_sample_rate: 48_000,
            geometry: MicrophoneArrayGeometry::Linear {
                spacing_m: 0.04,
                orientation_degrees: 0.0,
            },
            target: MicrophoneArrayTarget::Direction {
                azimuth_degrees: 90.0,
                elevation_degrees: 0.0,
            },
            algorithm: crate::audio::graph::MicrophoneArrayAlgorithm::DelayAndSum,
            strength: 1.0,
            max_attenuation_db: 18.0,
            gsc_filter_length: 8,
            gsc_adaptation_rate: 0.05,
            postfilter_enabled: false,
            limiter_enabled: false,
            bypassed: false,
            calibration: MicrophoneArrayCalibration::default(),
        }
    }
}
