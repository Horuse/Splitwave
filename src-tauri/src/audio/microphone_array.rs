//! Allocation-free N-channel spatial primitives used by Microphone Array.
//!
//! Capture owns the clock-domain boundary; this module only receives channels
//! already placed on a common timeline. Keeping that boundary explicit prevents
//! a synchronizer from treating acoustic time-of-flight as clock drift.

use crate::error::{AppError, AppResult};

const SPEED_OF_SOUND_MPS: f32 = 343.0;
const FRACTIONAL_DELAY_ORDER: usize = 3;
const HISTORY_FRAMES: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Point3 {
    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }

    fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    fn length(self) -> f32 {
        self.dot(self).sqrt()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SteeringTarget {
    Direction {
        azimuth_degrees: f32,
        elevation_degrees: f32,
    },
    Point(Point3),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    DelayAndSum,
    Gsc,
    /// MVDR is represented separately so callers can report a deterministic
    /// capability error rather than silently running a different algorithm.
    Mvdr,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemberConfig {
    pub position: Point3,
    pub enabled: bool,
    pub weight: f32,
    pub gain_db: f32,
    pub polarity_inverted: bool,
    pub fixed_delay_samples: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessorConfig<'a> {
    pub sample_rate: u32,
    pub target: SteeringTarget,
    pub algorithm: Algorithm,
    pub strength: f32,
    pub max_attenuation_db: f32,
    pub gsc_filter_length: usize,
    pub gsc_adaptation_rate: f32,
    pub members: &'a [MemberConfig],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveAlgorithm {
    DelayAndSum,
    Gsc,
}

/// A bounded time-domain processor. The input is planar: `N` adjacent planes
/// of `frames` samples. Its buffers are fixed at construction and `process`
/// never allocates or blocks.
pub struct Processor {
    active: Vec<usize>,
    gains: Vec<f32>,
    delays: Vec<f32>,
    weights: Vec<f32>,
    history: Vec<f32>,
    history_head: usize,
    active_algorithm: ActiveAlgorithm,
    strength: f32,
    gsc: Option<Gsc>,
}

struct Gsc {
    filter_len: usize,
    adaptation_rate: f32,
    max_correction: f32,
    coeffs: Vec<f32>,
    history: Vec<f32>,
    head: usize,
}

impl Processor {
    pub fn new(config: ProcessorConfig<'_>) -> AppResult<Self> {
        if config.sample_rate == 0 {
            return Err(AppError::Validation(
                "Microphone Array processing rate must be positive".into(),
            ));
        }
        let active: Vec<usize> = config
            .members
            .iter()
            .enumerate()
            .filter_map(|(index, member)| member.enabled.then_some(index))
            .collect();
        if active.is_empty() {
            return Err(AppError::Validation(
                "Microphone Array needs at least one enabled member".into(),
            ));
        }

        let steering = steering_delays(config.sample_rate, config.target, config.members)?;
        let mut gains = Vec::with_capacity(active.len());
        let mut delays = Vec::with_capacity(active.len());
        let mut weights = Vec::with_capacity(active.len());
        for &index in &active {
            let member = config.members[index];
            let gain = 10.0_f32.powf(member.gain_db.clamp(-24.0, 24.0) / 20.0);
            gains.push(if member.polarity_inverted {
                -gain
            } else {
                gain
            });
            delays.push((steering[index] + member.fixed_delay_samples).clamp(0.0, 1024.0));
            weights.push(member.weight.clamp(0.0, 4.0));
        }
        let weight_sum: f32 = weights.iter().sum();
        if weight_sum <= f32::EPSILON {
            return Err(AppError::Validation(
                "Microphone Array has no positive member weight".into(),
            ));
        }
        for weight in &mut weights {
            *weight /= weight_sum;
        }

        let use_gsc =
            matches!(config.algorithm, Algorithm::Gsc | Algorithm::Auto) && active.len() >= 2;
        let gsc = use_gsc.then(|| {
            Gsc::new(
                active.len() - 1,
                config.gsc_filter_length.clamp(1, 64),
                config.gsc_adaptation_rate.clamp(0.0, 1.0),
                db_to_linear(-config.max_attenuation_db.clamp(0.0, 36.0)),
            )
        });
        let active_algorithm = if gsc.is_some() {
            ActiveAlgorithm::Gsc
        } else {
            ActiveAlgorithm::DelayAndSum
        };

        Ok(Self {
            history: vec![0.0; active.len() * HISTORY_FRAMES],
            history_head: 0,
            active,
            gains,
            delays,
            weights,
            active_algorithm,
            strength: config.strength.clamp(0.0, 1.0),
            gsc,
        })
    }

    pub fn active_algorithm(&self) -> ActiveAlgorithm {
        self.active_algorithm
    }

    pub const fn fractional_delay_order() -> usize {
        FRACTIONAL_DELAY_ORDER
    }

    pub const fn latency_frames() -> usize {
        0
    }

    /// Processes `frames` planar samples into one mono block. `input` must have
    /// one plane for every configured member, including disabled members.
    pub fn process(&mut self, input: &[f32], frames: usize, output: &mut [f32]) -> AppResult<()> {
        if output.len() < frames {
            return Err(AppError::Validation(
                "Microphone Array output block is too small".into(),
            ));
        }
        let configured = input.len() / frames.max(1);
        if input.len() != configured * frames
            || self.active.iter().any(|&index| index >= configured)
        {
            return Err(AppError::Validation(
                "Microphone Array planar input shape is invalid".into(),
            ));
        }

        for frame in 0..frames {
            let mut fixed = 0.0;
            for (slot, &configured_index) in self.active.iter().enumerate() {
                let sample = input[configured_index * frames + frame] * self.gains[slot];
                self.history[slot * HISTORY_FRAMES + self.history_head] = sample;
                fixed += self.delayed(slot, self.delays[slot]) * self.weights[slot];
            }
            let output_sample = match &mut self.gsc {
                Some(gsc) => {
                    let cancelled = gsc.process(
                        fixed,
                        &self.active,
                        &self.history,
                        self.history_head,
                        &self.delays,
                    );
                    fixed + (cancelled - fixed) * self.strength
                }
                None => fixed,
            };
            output[frame] = if output_sample.is_finite() {
                output_sample
            } else {
                0.0
            };
            self.history_head = (self.history_head + 1) % HISTORY_FRAMES;
        }
        Ok(())
    }

    fn delayed(&self, slot: usize, delay: f32) -> f32 {
        let whole = delay.floor() as usize;
        let fraction = delay - whole as f32;
        let p = |back: usize| {
            let offset = (whole + back) % HISTORY_FRAMES;
            let index = (self.history_head + HISTORY_FRAMES - offset) % HISTORY_FRAMES;
            self.history[slot * HISTORY_FRAMES + index]
        };
        // Third-order Lagrange interpolation, evaluated on samples at
        // n-whole through n-whole-3. It has a fixed three-frame latency.
        let f = fraction;
        let c0 = -((f - 1.0) * (f - 2.0) * (f - 3.0)) / 6.0;
        let c1 = (f * (f - 2.0) * (f - 3.0)) / 2.0;
        let c2 = -(f * (f - 1.0) * (f - 3.0)) / 2.0;
        let c3 = (f * (f - 1.0) * (f - 2.0)) / 6.0;
        c0 * p(0) + c1 * p(1) + c2 * p(2) + c3 * p(3)
    }
}

impl Gsc {
    fn new(
        blocking_channels: usize,
        filter_len: usize,
        adaptation_rate: f32,
        max_correction: f32,
    ) -> Self {
        Self {
            filter_len,
            adaptation_rate,
            max_correction,
            coeffs: vec![0.0; blocking_channels * filter_len],
            history: vec![0.0; blocking_channels * filter_len],
            head: 0,
        }
    }

    fn process(
        &mut self,
        fixed: f32,
        active: &[usize],
        samples: &[f32],
        head: usize,
        delays: &[f32],
    ) -> f32 {
        let channels = active.len();
        let reference = delayed_from_history(samples, channels - 1, head, delays[channels - 1]);
        let mut predicted = 0.0;
        let mut norm = 1.0e-6;
        for channel in 0..channels - 1 {
            let aligned = delayed_from_history(samples, channel, head, delays[channel]);
            let blocking = aligned - reference;
            let history_index = channel * self.filter_len + self.head;
            self.history[history_index] = blocking;
            for tap in 0..self.filter_len {
                let index = (self.head + self.filter_len - tap) % self.filter_len;
                let value = self.history[channel * self.filter_len + index];
                predicted += self.coeffs[channel * self.filter_len + tap] * value;
                norm += value * value;
            }
        }
        let bounded = predicted.clamp(
            -self.max_correction.max(0.01),
            self.max_correction.max(0.01),
        );
        let output = fixed - bounded * self.max_correction;
        let residual = output;
        let slowdown = if fixed.abs() > 1.5 * (norm.sqrt() / channels as f32) {
            0.1
        } else {
            1.0
        };
        let step = self.adaptation_rate * slowdown * residual / norm;
        for channel in 0..channels - 1 {
            for tap in 0..self.filter_len {
                let index = (self.head + self.filter_len - tap) % self.filter_len;
                let value = self.history[channel * self.filter_len + index];
                let coefficient = &mut self.coeffs[channel * self.filter_len + tap];
                *coefficient = (*coefficient + step * value).clamp(-4.0, 4.0);
            }
        }
        self.head = (self.head + 1) % self.filter_len;
        output
    }
}

fn delayed_from_history(history: &[f32], slot: usize, head: usize, delay: f32) -> f32 {
    let whole = delay.floor() as usize;
    let fraction = delay - whole as f32;
    let sample = |back: usize| {
        let offset = (whole + back) % HISTORY_FRAMES;
        history[slot * HISTORY_FRAMES + (head + HISTORY_FRAMES - offset) % HISTORY_FRAMES]
    };
    let f = fraction;
    let c0 = -((f - 1.0) * (f - 2.0) * (f - 3.0)) / 6.0;
    let c1 = (f * (f - 2.0) * (f - 3.0)) / 2.0;
    let c2 = -(f * (f - 1.0) * (f - 3.0)) / 2.0;
    let c3 = (f * (f - 1.0) * (f - 2.0)) / 6.0;
    c0 * sample(0) + c1 * sample(1) + c2 * sample(2) + c3 * sample(3)
}

pub fn steering_delays(
    sample_rate: u32,
    target: SteeringTarget,
    members: &[MemberConfig],
) -> AppResult<Vec<f32>> {
    if sample_rate == 0 || members.is_empty() {
        return Err(AppError::Validation(
            "Microphone Array needs a rate and members".into(),
        ));
    }
    let arrivals: Vec<f32> = members
        .iter()
        .map(|member| match target {
            SteeringTarget::Direction {
                azimuth_degrees,
                elevation_degrees,
            } => {
                let azimuth = azimuth_degrees.to_radians();
                let elevation = elevation_degrees.to_radians();
                let direction = Point3 {
                    x: elevation.cos() * azimuth.cos(),
                    y: elevation.cos() * azimuth.sin(),
                    z: elevation.sin(),
                };
                -member.position.dot(direction) / SPEED_OF_SOUND_MPS
            }
            SteeringTarget::Point(point) => {
                member.position.sub(point).length() / SPEED_OF_SOUND_MPS
            }
        })
        .collect();
    let latest = arrivals.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    Ok(arrivals
        .into_iter()
        .map(|arrival| ((latest - arrival) * sample_rate as f32).max(0.0))
        .collect())
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(x: f32) -> MemberConfig {
        MemberConfig {
            position: Point3 { x, y: 0.0, z: 0.0 },
            enabled: true,
            weight: 1.0,
            gain_db: 0.0,
            polarity_inverted: false,
            fixed_delay_samples: 0.0,
        }
    }

    #[test]
    fn broadside_has_no_relative_delay_for_any_n() {
        for count in [2, 4, 8, 16] {
            let members: Vec<_> = (0..count).map(|i| member(i as f32 * 0.04)).collect();
            let delays = steering_delays(
                48_000,
                SteeringTarget::Direction {
                    azimuth_degrees: 90.0,
                    elevation_degrees: 0.0,
                },
                &members,
            )
            .unwrap();
            assert!(delays.iter().all(|delay| delay.abs() < 1.0e-4));
        }
    }

    #[test]
    fn delay_and_sum_preserves_constructive_target_for_n_2_4_8() {
        for channels in [2, 4, 8] {
            let members: Vec<_> = (0..channels).map(|i| member(i as f32 * 0.04)).collect();
            let mut processor = Processor::new(ProcessorConfig {
                sample_rate: 48_000,
                target: SteeringTarget::Direction {
                    azimuth_degrees: 90.0,
                    elevation_degrees: 0.0,
                },
                algorithm: Algorithm::DelayAndSum,
                strength: 1.0,
                max_attenuation_db: 24.0,
                gsc_filter_length: 8,
                gsc_adaptation_rate: 0.05,
                members: &members,
            })
            .unwrap();
            let frames = 512;
            let input = vec![0.5; channels * frames];
            let mut out = vec![0.0; frames];
            processor.process(&input, frames, &mut out).unwrap();
            let settled = &out[Processor::latency_frames() + 4..];
            let mean = settled.iter().sum::<f32>() / settled.len() as f32;
            assert!((mean - 0.5).abs() < 0.001, "N={channels}, mean={mean}");
        }
    }

    #[test]
    fn fractional_delay_is_finite_and_bounded() {
        let mut members = vec![member(0.0), member(0.04)];
        members[1].fixed_delay_samples = 5.25;
        let mut processor = Processor::new(ProcessorConfig {
            sample_rate: 48_000,
            target: SteeringTarget::Direction {
                azimuth_degrees: 90.0,
                elevation_degrees: 0.0,
            },
            algorithm: Algorithm::DelayAndSum,
            strength: 1.0,
            max_attenuation_db: 24.0,
            gsc_filter_length: 8,
            gsc_adaptation_rate: 0.05,
            members: &members,
        })
        .unwrap();
        let frames = 256;
        let mut input = vec![0.0; 2 * frames];
        input[0] = 1.0;
        input[frames + 5] = 1.0;
        let mut out = vec![0.0; frames];
        processor.process(&input, frames, &mut out).unwrap();
        assert!(out
            .iter()
            .all(|sample| sample.is_finite() && sample.abs() <= 1.01));
    }

    #[test]
    fn gsc_uses_the_general_path_for_n_2_4_8() {
        for channels in [2, 4, 8] {
            let members: Vec<_> = (0..channels).map(|i| member(i as f32 * 0.04)).collect();
            let processor = Processor::new(ProcessorConfig {
                sample_rate: 48_000,
                target: SteeringTarget::Direction {
                    azimuth_degrees: 90.0,
                    elevation_degrees: 0.0,
                },
                algorithm: Algorithm::Gsc,
                strength: 1.0,
                max_attenuation_db: 18.0,
                gsc_filter_length: 8,
                gsc_adaptation_rate: 0.02,
                members: &members,
            })
            .unwrap();
            assert_eq!(processor.active_algorithm(), ActiveAlgorithm::Gsc);
        }
    }
}
