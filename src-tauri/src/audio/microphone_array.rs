//! Allocation-free N-channel spatial primitives used by Microphone Array.
//!
//! Capture owns the clock-domain boundary; this module only receives channels
//! already placed on a common timeline. Keeping that boundary explicit prevents
//! a synchronizer from treating acoustic time-of-flight as clock drift.

use crate::error::{AppError, AppResult};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::sync::Arc;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::thread::{self, JoinHandle};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::time::Duration;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use cpal::traits::StreamTrait;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use rtrb::{Consumer, RingBuffer};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::audio::effects::{update_meter, MeterHandle};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::audio::graph::{
    MicrophoneArrayAlgorithm, MicrophoneArrayData, MicrophoneArrayMember, MicrophoneArrayTarget,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::audio::health;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::audio::input_bridge::BroadcastRx;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::audio::pipeline::dag::DSP_BLOCK_FRAMES;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::audio::resample::MultiResamplerOut;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::audio::streams;

const SPEED_OF_SOUND_MPS: f32 = 343.0;
const FRACTIONAL_DELAY_ORDER: usize = 3;
const HISTORY_FRAMES: usize = 2048;
const MAX_CLOCK_CORRECTION_PPM: f64 = 500.0;

#[cfg(any(target_os = "macos", target_os = "windows"))]
const ARRAY_RING_CAPACITY_FRAMES: usize = 48_000;
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ARRAY_WAIT: Duration = Duration::from_millis(1);

/// Bounded occupancy PLL for one slave clock-domain. Its ratio is shared by
/// every channel of that physical stream, so resampling cannot change their
/// relative phase or erase acoustic TDOA.
pub struct DomainSynchronizer {
    base_ratio: f64,
    ratio: f64,
    target_frames: f64,
    integral: f64,
}

impl DomainSynchronizer {
    pub fn new(source_rate: u32, master_rate: u32, target_frames: usize) -> AppResult<Self> {
        if source_rate == 0 || master_rate == 0 || target_frames == 0 {
            return Err(AppError::Validation(
                "Microphone Array synchronizer needs positive rates and ring target".into(),
            ));
        }
        let base_ratio = master_rate as f64 / source_rate as f64;
        Ok(Self {
            base_ratio,
            ratio: base_ratio,
            target_frames: target_frames as f64,
            integral: 0.0,
        })
    }

    /// Updates from ring occupancy only. `ratio` is output/input for rubato's
    /// fixed-output resampler: a growing source ring lowers it and consumes
    /// slightly more slave frames on the next output block.
    pub fn update(&mut self, available_frames: usize) -> f64 {
        let error = (available_frames as f64 - self.target_frames) / self.target_frames;
        self.integral = (self.integral + error * 0.000_02).clamp(-0.0005, 0.0005);
        let correction = (-error * 0.000_5 - self.integral).clamp(
            -MAX_CLOCK_CORRECTION_PPM / 1_000_000.0,
            MAX_CLOCK_CORRECTION_PPM / 1_000_000.0,
        );
        self.ratio = self.base_ratio * (1.0 + correction);
        self.ratio
    }

    pub fn ratio(&self) -> f64 {
        self.ratio
    }
}

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
        if matches!(config.algorithm, Algorithm::Mvdr) {
            return Err(AppError::Validation(
                "Microphone Array MVDR is not available in the time-domain processor".into(),
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
        // Third-order Lagrange interpolation evaluated against the causal
        // history at n-whole through n-whole-3.
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

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct DeviceSource {
    pub id: String,
    pub device: cpal::Device,
    pub config: cpal::StreamConfig,
    pub sample_format: cpal::SampleFormat,
    pub channels: usize,
    pub sample_rate: u32,
}

/// Owns every physical device stream and the array worker that consumes them.
/// Dropping it pauses producers before joining the worker, so a device callback
/// cannot write into a ring after its consumer is gone.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct Capture {
    streams: Vec<cpal::Stream>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Drop for Capture {
    fn drop(&mut self) {
        for stream in &self.streams {
            let _ = stream.pause();
        }
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct Domain {
    channels: usize,
    consumer: Consumer<f32>,
    resampler: MultiResamplerOut,
    synchronizer: Option<DomainSynchronizer>,
    input: Vec<f32>,
    output: Vec<f32>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Domain {
    fn new(
        source: DeviceSource,
        consumer: Consumer<f32>,
        processing_rate: u32,
        is_master: bool,
    ) -> AppResult<Self> {
        let resampler = MultiResamplerOut::new(
            source.sample_rate,
            processing_rate,
            DSP_BLOCK_FRAMES,
            source.channels,
        )?;
        let input_capacity = resampler.input_frames_max() * source.channels;
        let synchronizer = (!is_master)
            .then(|| {
                DomainSynchronizer::new(
                    source.sample_rate,
                    processing_rate,
                    ARRAY_RING_CAPACITY_FRAMES / 4,
                )
            })
            .transpose()?;
        Ok(Self {
            channels: source.channels,
            consumer,
            resampler,
            synchronizer,
            input: Vec::with_capacity(input_capacity),
            output: Vec::with_capacity(DSP_BLOCK_FRAMES * source.channels),
        })
    }

    fn available_frames(&self) -> usize {
        self.consumer.slots() / self.channels
    }

    fn next_input_frames(&self) -> usize {
        self.resampler.input_frames_next()
    }

    /// Produces one common-rate block. The master domain is allowed to gate the
    /// worker; independent domains zero-fill only their missing tail so a
    /// transient device stall cannot change channel-to-channel alignment.
    fn fill(&mut self, require_full_input: bool) -> bool {
        let available_frames = self.available_frames();
        if let Some(synchronizer) = &mut self.synchronizer {
            let ratio = synchronizer.update(available_frames);
            self.resampler.set_ratio(ratio);
        }
        let input_frames = self.resampler.input_frames_next();
        if require_full_input && self.available_frames() < input_frames {
            return false;
        }
        let want = input_frames * self.channels;
        self.input.clear();
        self.input.resize(want, 0.0);
        let have = self.consumer.slots().min(want) / self.channels * self.channels;
        if have > 0 {
            if let Ok(chunk) = self.consumer.read_chunk(have) {
                let (first, second) = chunk.as_slices();
                let first_len = first.len();
                self.input[..first_len].copy_from_slice(first);
                if !second.is_empty() {
                    self.input[first_len..first_len + second.len()].copy_from_slice(second);
                }
                chunk.commit_all();
            }
        }
        if have < want {
            health::bump(&health::ARRAY_SOURCE_UNDERRUN_SAMPLES, (want - have) as u64);
        }
        self.output.clear();
        self.resampler
            .process(&self.input, &mut self.output)
            .is_ok()
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn member_config(member: &MicrophoneArrayMember) -> MemberConfig {
    MemberConfig {
        position: Point3 {
            x: member.position.x,
            y: member.position.y,
            z: member.position.z,
        },
        enabled: member.enabled
            && member.quality != crate::audio::graph::MicrophoneArrayChannelQuality::Excluded,
        weight: member.weight,
        gain_db: member.gain_db,
        polarity_inverted: member.polarity_inverted,
        fixed_delay_samples: member.fixed_delay_samples,
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn processor_config(data: &MicrophoneArrayData) -> ProcessorConfig<'_> {
    let target = match data.target {
        MicrophoneArrayTarget::Direction {
            azimuth_degrees,
            elevation_degrees,
        } => SteeringTarget::Direction {
            azimuth_degrees,
            elevation_degrees,
        },
        MicrophoneArrayTarget::Point { x, y, z } => SteeringTarget::Point(Point3 { x, y, z }),
    };
    let algorithm = match data.algorithm {
        MicrophoneArrayAlgorithm::DelayAndSum => Algorithm::DelayAndSum,
        MicrophoneArrayAlgorithm::Gsc => Algorithm::Gsc,
        MicrophoneArrayAlgorithm::Mvdr => Algorithm::Mvdr,
        MicrophoneArrayAlgorithm::Auto => Algorithm::Auto,
    };
    // The caller owns the member conversion so this helper only exists to
    // keep target/algorithm mapping together.
    ProcessorConfig {
        sample_rate: data.processing_sample_rate,
        target,
        algorithm,
        strength: data.strength,
        max_attenuation_db: data.max_attenuation_db,
        gsc_filter_length: data.gsc_filter_length as usize,
        gsc_adaptation_rate: data.gsc_adaptation_rate,
        members: &[],
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn start_capture(
    data: MicrophoneArrayData,
    sources: Vec<DeviceSource>,
    bridge: BroadcastRx,
    meter: Option<MeterHandle>,
    report_error: Arc<dyn Fn(cpal::StreamError) + Send + Sync>,
) -> AppResult<Capture> {
    if sources.len() != data.sources.len() {
        return Err(AppError::Validation(
            "Microphone Array source resolution did not preserve every source".into(),
        ));
    }
    let master_index = data
        .master_source_id
        .as_deref()
        .and_then(|id| data.sources.iter().position(|source| source.id == id))
        .unwrap_or(0);
    let mut member_domains = Vec::with_capacity(data.members.len());
    for member in &data.members {
        let domain = sources
            .iter()
            .position(|source| source.id == member.source_id)
            .ok_or_else(|| {
                AppError::Validation("Microphone Array member source disappeared".into())
            })?;
        if member.channel_index as usize >= sources[domain].channels {
            return Err(AppError::Validation(format!(
                "Microphone Array channel {} is unavailable on source {}",
                member.channel_index, sources[domain].id
            )));
        }
        member_domains.push(domain);
    }
    let members: Vec<MemberConfig> = data.members.iter().map(member_config).collect();
    let mut config = processor_config(&data);
    config.members = &members;
    let processor = Processor::new(config)?;

    let mut domains = Vec::with_capacity(sources.len());
    let mut streams = Vec::with_capacity(sources.len());
    for (index, source) in sources.into_iter().enumerate() {
        let (producer, consumer) =
            RingBuffer::<f32>::new(ARRAY_RING_CAPACITY_FRAMES * source.channels);
        let reporter = report_error.clone();
        let stream = streams::build_raw_input_stream(
            &source.device,
            &source.config,
            source.sample_format,
            source.channels,
            producer,
            move |error| reporter(error),
        )?;
        domains.push(Domain::new(
            source,
            consumer,
            data.processing_sample_rate,
            index == master_index,
        )?);
        streams.push(stream);
    }

    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let join = thread::Builder::new()
        .name("microphone-array".into())
        .spawn(move || {
            run_capture_worker(
                domains,
                master_index,
                member_domains,
                data.members,
                processor,
                bridge,
                meter,
                worker_stop,
            )
        })
        .map_err(|error| AppError::Stream(format!("Microphone Array worker: {error}")))?;
    Ok(Capture {
        streams,
        stop,
        join: Some(join),
    })
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn run_capture_worker(
    mut domains: Vec<Domain>,
    master_index: usize,
    member_domains: Vec<usize>,
    members: Vec<MicrophoneArrayMember>,
    mut processor: Processor,
    mut bridge: BroadcastRx,
    meter: Option<MeterHandle>,
    stop: Arc<AtomicBool>,
) {
    let mut planar = vec![0.0; members.len() * DSP_BLOCK_FRAMES];
    let mut mono = vec![0.0; DSP_BLOCK_FRAMES];
    while !stop.load(Ordering::SeqCst) {
        if domains[master_index].available_frames() < domains[master_index].next_input_frames() {
            thread::sleep(ARRAY_WAIT);
            continue;
        }
        if !domains[master_index].fill(true) {
            continue;
        }
        for (index, domain) in domains.iter_mut().enumerate() {
            if index != master_index {
                let _ = domain.fill(false);
            }
        }
        for (member_index, member) in members.iter().enumerate() {
            let domain = &domains[member_domains[member_index]];
            let channel = member.channel_index as usize;
            for frame in 0..DSP_BLOCK_FRAMES {
                planar[member_index * DSP_BLOCK_FRAMES + frame] =
                    domain.output[frame * domain.channels + channel];
            }
        }
        if processor
            .process(&planar, DSP_BLOCK_FRAMES, &mut mono)
            .is_err()
        {
            break;
        }
        bridge.apply_commands();
        if let Some(meter) = &meter {
            update_meter(meter, &mono, 1);
        }
        bridge.broadcast(&mono);
    }
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

    #[test]
    fn mvdr_never_silently_falls_back_to_delay_and_sum() {
        let members = [member(0.0), member(0.04)];
        let result = Processor::new(ProcessorConfig {
            sample_rate: 48_000,
            target: SteeringTarget::Direction {
                azimuth_degrees: 90.0,
                elevation_degrees: 0.0,
            },
            algorithm: Algorithm::Mvdr,
            strength: 1.0,
            max_attenuation_db: 18.0,
            gsc_filter_length: 8,
            gsc_adaptation_rate: 0.02,
            members: &members,
        });
        assert!(result.is_err());
    }

    #[test]
    fn synchronizer_bounded_correction_handles_positive_sro() {
        let mut sync = DomainSynchronizer::new(48_000, 48_000, 4_800).unwrap();
        let initial = sync.ratio();
        let corrected = sync.update(5_040);
        assert!(corrected < initial);
        assert!(corrected > initial * (1.0 - MAX_CLOCK_CORRECTION_PPM / 1_000_000.0));
    }
}
