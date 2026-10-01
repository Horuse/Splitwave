//! Audio devices that exist only in memory, so a whole pipeline runs where
//! there is no sound hardware (tests, CI). A virtual input plays a tone into
//! the graph on its own clock, as a capture callback would; a virtual speaker
//! pulls the graph on its own clock, as a device callback would, and keeps
//! what it played.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::audio::effects::{update_meter, MeterHandle};
use crate::audio::graph::InputSpec;
use crate::audio::input_bridge::BroadcastRx;
use crate::error::{AppError, AppResult};

/// A capture device: a sine on every channel.
#[derive(Clone, Debug)]
pub struct VirtualInput {
    pub sample_rate: u32,
    pub channels: u32,
    pub tone_hz: f32,
    pub amplitude: f32,
}

/// A playback device. Its callback asks for one engine block in its own
/// frames, as a real device is set up to.
#[derive(Clone, Debug)]
pub struct VirtualSpeaker {
    pub sample_rate: u32,
    pub channels: u32,
}

/// The devices a virtual host offers, by the id a graph names them with: a
/// microphone's `deviceId`, `system` for system audio, `app:<bundleId>` for
/// an app's audio, a speaker's `deviceId`.
#[derive(Default)]
pub struct VirtualDevices {
    inputs: Mutex<HashMap<String, VirtualInput>>,
    speakers: Mutex<HashMap<String, VirtualSpeaker>>,
    played: Mutex<HashMap<String, Arc<Mutex<Vec<f32>>>>>,
}

impl VirtualDevices {
    pub fn add_input(&self, id: &str, input: VirtualInput) {
        self.inputs.lock().unwrap().insert(id.to_string(), input);
    }

    pub fn add_speaker(&self, id: &str, speaker: VirtualSpeaker) {
        self.speakers
            .lock()
            .unwrap()
            .insert(id.to_string(), speaker);
    }

    /// Everything speaker `id` has played so far, interleaved.
    pub fn played(&self, id: &str) -> Vec<f32> {
        self.played
            .lock()
            .unwrap()
            .get(id)
            .map(|p| p.lock().unwrap().clone())
            .unwrap_or_default()
    }

    /// Forgets what every speaker has played.
    pub fn clear_played(&self) {
        for p in self.played.lock().unwrap().values() {
            p.lock().unwrap().clear();
        }
    }

    /// The device a live input spec names, or `None` for inputs that are not
    /// devices (files, network).
    pub(super) fn input_for(&self, spec: &InputSpec) -> Option<AppResult<(String, VirtualInput)>> {
        let id = match spec {
            InputSpec::Microphone { device_id } => device_id.clone(),
            InputSpec::SystemAudio { .. } => "system".to_string(),
            InputSpec::AppAudio { bundle_id } => format!("app:{bundle_id}"),
            _ => return None,
        };
        Some(
            self.inputs
                .lock()
                .unwrap()
                .get(&id)
                .cloned()
                .map(|input| (id.clone(), input))
                .ok_or_else(|| AppError::Device(format!("no virtual input {id:?}"))),
        )
    }

    pub(super) fn speaker(&self, id: &str) -> AppResult<VirtualSpeaker> {
        self.speakers
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::Device(format!("no virtual speaker {id:?}")))
    }

    /// Where speaker `id` keeps what it plays; a reopened speaker carries on.
    pub(super) fn sink(&self, id: &str) -> Arc<Mutex<Vec<f32>>> {
        self.played
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .clone()
    }
}

/// Calls `tick(frames)` once per `period_frames` of `rate`, on the wall
/// clock, from its own thread until dropped. A late wake makes up every
/// period it missed at once, as the device's own clock would have run on.
pub(super) struct DeviceClock {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl DeviceClock {
    pub(super) fn start(
        name: &str,
        rate: u32,
        period_frames: usize,
        mut tick: impl FnMut(usize) + Send + 'static,
    ) -> AppResult<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let period = Duration::from_secs_f64(period_frames as f64 / rate.max(1) as f64);
        let join = thread::Builder::new()
            .name(format!("virtual:{name}"))
            .spawn(move || {
                // A device's callback runs at real-time priority; so does this.
                let _rt = super::RtThread::promote("virtual device", period_frames as u32, rate);
                let started = Instant::now();
                let mut ticks: u32 = 0;
                while !stop_thread.load(Ordering::SeqCst) {
                    let due = started + period * (ticks + 1);
                    let now = Instant::now();
                    if due > now {
                        thread::sleep(due - now);
                        continue;
                    }
                    tick(period_frames);
                    ticks += 1;
                }
            })
            .map_err(|e| AppError::Stream(format!("spawn virtual device: {e}")))?;
        Ok(Self {
            stop,
            join: Some(join),
        })
    }
}

impl Drop for DeviceClock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// A running virtual input: its tone, delivered `io_frames` at a time.
pub(super) fn start_capture(
    id: &str,
    input: &VirtualInput,
    io_frames: usize,
    mut bridge: BroadcastRx,
    meter: Option<MeterHandle>,
) -> AppResult<DeviceClock> {
    let channels = input.channels.max(1) as usize;
    let step = std::f32::consts::TAU * input.tone_hz / input.sample_rate.max(1) as f32;
    let amplitude = input.amplitude;
    let mut phase = 0.0f32;
    let mut buf = vec![0.0f32; io_frames * channels];
    DeviceClock::start(id, input.sample_rate, io_frames, move |frames| {
        for frame in buf[..frames * channels].chunks_exact_mut(channels) {
            frame.fill(amplitude * phase.sin());
            phase = (phase + step) % std::f32::consts::TAU;
        }
        if let Some(m) = &meter {
            update_meter(m, &buf[..frames * channels], channels);
        }
        bridge.apply_commands();
        bridge.broadcast(&buf[..frames * channels]);
    })
}
