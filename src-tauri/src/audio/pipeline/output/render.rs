//! Speaker rendering inside the device callback.
//!
//! The device callback is the clock: each callback renders exactly the audio
//! it plays, with no worker thread and no output ring in between. A callback
//! size that differs from the engine block (or a device rate that differs from
//! the pipeline rate) goes through a small adapter holding at most one
//! rendered block.
//!
//! The renderer owns the output graph, so it must never be freed on the audio
//! thread or leaked with a callback closure (cpal's CoreAudio backend leaks the
//! closure of a non-default device). It therefore sits in a slot the callback
//! only try-locks, and the control thread takes it back out on retire, from a
//! dead device too.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::audio::effects::{update_meter, MeterHandle};
use crate::audio::health;
use crate::audio::resample::FixedRateResampler;
use crate::error::AppResult;

use super::super::dag::OutputGraph;
use super::super::worker::{dsp_worker, DspWorker, WorkerCtrl};

/// Longest a retire waits for the stop fade to play out.
const RETIRE_TIMEOUT: Duration = Duration::from_millis(500);

/// A starting speaker fades in, and a retiring one fades out, over this, so
/// starting or stopping never steps a waveform mid-cycle.
const STOP_FADE_MS: u32 = 10;

/// Blocks rendered in a row without output before a callback gives up. The
/// FFT resampler holds input until it has a whole FFT frame (160 frames for
/// 48 -> 44.1 kHz), so several small blocks may go in before any comes out.
const MAX_SILENT_BLOCKS: usize = 64;

/// Per-speaker counters, written relaxed by the callback and read by the
/// non-RT tick thread and the latency report.
#[derive(Clone)]
pub(in crate::audio::pipeline) struct SpeakerIo {
    /// Native clock rate of the physical output stream. Differs from the
    /// graph's pipeline rate when the output resampler is active.
    pub sample_rate: u32,
    /// Samples the device asked for, summed across callbacks.
    pub requested: Arc<AtomicU64>,
    pub callbacks: Arc<AtomicU64>,
    /// Frames the device asked for in its latest callback: its real IO buffer.
    pub callback_frames: Arc<AtomicU32>,
    /// Device frames already rendered but not yet played when the latest
    /// callback started: the block adapter's share of the latency.
    pub carried_frames: Arc<AtomicU32>,
    /// Worst render time since the last read, in permille of the audio it
    /// produced. Above 1000 the device outran the graph.
    pub load_peak_permille: Arc<AtomicU32>,
    /// Renders that took longer than the audio they produced.
    pub overloads: Arc<AtomicU64>,
    /// Delay compensation the running graph is aligned to, in pipeline frames.
    /// Follows graph swaps.
    pub graph_latency_frames: Arc<AtomicU32>,
    /// Filter delay of the output resampler, in device frames.
    pub resampler_delay_frames: usize,
    /// What the device adds past its buffer (safety offset, converters), in
    /// device frames. `None` where the OS does not report it.
    pub hardware_frames: Option<u32>,
}

pub(in crate::audio::pipeline) struct SpeakerRenderer {
    worker: DspWorker,
    block: Box<[f32]>,
    resampler: Option<FixedRateResampler>,
    resampled_channels: usize,
    /// Rendered device-rate audio not yet handed to the device.
    pending: Box<[f32]>,
    pending_pos: usize,
    pending_len: usize,
    channels: usize,
    meter: MeterHandle,
    io: SpeakerIo,
}

impl SpeakerRenderer {
    /// `graph` must already carry the device's channel count.
    pub(in crate::audio::pipeline) fn new(
        graph: OutputGraph,
        device_rate: u32,
        hardware_frames: Option<u32>,
        meter: MeterHandle,
    ) -> AppResult<(Self, WorkerCtrl, SpeakerIo)> {
        let channels = graph.out_channels().max(1);
        let block_frames = graph.block_frames();
        let pipeline_rate = graph.sample_rate();
        let resampler = if pipeline_rate == device_rate {
            None
        } else {
            Some(FixedRateResampler::new(
                pipeline_rate,
                device_rate,
                block_frames,
                channels,
            )?)
        };
        let pending_frames = resampler
            .as_ref()
            .map_or(block_frames, FixedRateResampler::out_max);
        let io = SpeakerIo {
            sample_rate: device_rate,
            requested: Arc::new(AtomicU64::new(0)),
            callbacks: Arc::new(AtomicU64::new(0)),
            callback_frames: Arc::new(AtomicU32::new(0)),
            carried_frames: Arc::new(AtomicU32::new(0)),
            load_peak_permille: Arc::new(AtomicU32::new(0)),
            overloads: Arc::new(AtomicU64::new(0)),
            graph_latency_frames: Arc::new(AtomicU32::new(graph.latency_frames() as u32)),
            resampler_delay_frames: resampler
                .as_ref()
                .map_or(0, FixedRateResampler::delay_frames),
            hardware_frames,
        };
        let (worker, ctrl) = dsp_worker(graph);
        Ok((
            Self {
                worker,
                block: vec![0.0; block_frames * channels].into_boxed_slice(),
                resampler,
                resampled_channels: 0,
                pending: vec![0.0; pending_frames * channels].into_boxed_slice(),
                pending_pos: 0,
                pending_len: 0,
                channels,
                meter,
                io: io.clone(),
            },
            ctrl,
            io,
        ))
    }

    /// Fill `out` (interleaved, device channel width) with the next audio.
    /// `callback_frames` is the device's whole callback, which a caller may
    /// deliver in several slices. RT-safe: no allocation, lock or syscall.
    pub(in crate::audio::pipeline) fn render(&mut self, out: &mut [f32], callback_frames: usize) {
        let started = Instant::now();
        self.io
            .requested
            .fetch_add(out.len() as u64, Ordering::Relaxed);
        self.io.callbacks.fetch_add(1, Ordering::Relaxed);
        self.io
            .callback_frames
            .store(callback_frames as u32, Ordering::Relaxed);
        self.io.carried_frames.store(
            ((self.pending_len - self.pending_pos) / self.channels) as u32,
            Ordering::Relaxed,
        );

        let mut written = 0;
        let mut silent_blocks = 0;
        while written < out.len() {
            if self.pending_pos == self.pending_len {
                if !self.produce() || silent_blocks == MAX_SILENT_BLOCKS {
                    out[written..].fill(0.0);
                    break;
                }
                silent_blocks += 1;
                continue;
            }
            silent_blocks = 0;
            let n = (self.pending_len - self.pending_pos).min(out.len() - written);
            out[written..written + n]
                .copy_from_slice(&self.pending[self.pending_pos..self.pending_pos + n]);
            self.pending_pos += n;
            written += n;
        }

        let frames = (out.len() / self.channels) as u64;
        if frames > 0 {
            let period_ns = frames * 1_000_000_000 / self.io.sample_rate.max(1) as u64;
            let spent_ns = started.elapsed().as_nanos() as u64;
            let permille = (spent_ns * 1000 / period_ns.max(1)).min(u32::MAX as u64) as u32;
            self.io
                .load_peak_permille
                .fetch_max(permille, Ordering::Relaxed);
            if permille > 1000 {
                self.io.overloads.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Render one engine block into `pending`, which may stay empty while the
    /// resampler fills an FFT frame. False when the resampler failed.
    fn produce(&mut self) -> bool {
        let active = self.worker.next_block(&mut self.block);
        self.io.graph_latency_frames.store(
            self.worker.graph().latency_frames() as u32,
            Ordering::Relaxed,
        );
        update_meter(&self.meter, &self.block, self.channels);
        self.pending_pos = 0;
        self.pending_len = 0;
        self.pending_len = match &mut self.resampler {
            None => {
                self.pending[..self.block.len()].copy_from_slice(&self.block);
                self.block.len()
            }
            Some(resampler) => {
                self.resampled_channels = self.resampled_channels.max(active);
                match resampler.process_chunk_into(
                    &self.block,
                    self.resampled_channels,
                    &mut self.pending,
                ) {
                    Ok(n) => n,
                    Err(_) => {
                        health::bump(&health::SPEAKER_RENDER_FAILED_BLOCKS, 1);
                        return false;
                    }
                }
            }
        };
        true
    }
}

/// A speaker's renderer, shared by its device callback and the control
/// thread. The callback only ever try-locks it, and a miss plays silence: the
/// control thread holds it only to put the renderer in or take it back out,
/// and the renderer can be taken back even from a device that stopped calling.
type Slot = Arc<Mutex<Option<SpeakerRenderer>>>;

/// How long a retire waits without a single callback before calling the
/// device dead and taking the renderer back anyway.
const DEAD_DEVICE_AFTER: Duration = Duration::from_millis(100);

/// The audio-thread end of a speaker: renders once the renderer has arrived,
/// silence before that and after a retire.
pub(in crate::audio::pipeline) struct SpeakerCallback {
    slot: Slot,
    retire: Arc<AtomicBool>,
    /// Set once the stop fade has played out.
    faded: Arc<AtomicBool>,
    /// Callbacks so far, for telling a slow retire from a dead device.
    calls: Arc<AtomicU64>,
    /// Frames of the stop fade still to play, once a retire is asked for.
    fade_left: Option<usize>,
    /// Frames played since the renderer arrived, while the start fade runs.
    faded_in: usize,
}

impl SpeakerCallback {
    pub(in crate::audio::pipeline) fn fill(&mut self, out: &mut [f32], callback_frames: usize) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let retiring = self.retire.load(Ordering::Acquire);
        let Ok(mut slot) = self.slot.try_lock() else {
            out.fill(0.0);
            return;
        };
        let Some(r) = slot.as_mut() else {
            out.fill(0.0);
            if retiring {
                self.faded.store(true, Ordering::Release);
            }
            return;
        };
        let total = (r.io.sample_rate * STOP_FADE_MS / 1000).max(1) as usize;
        if retiring {
            // A stop during the start fade goes down from where that fade got to.
            let left = self.fade_left.get_or_insert(self.faded_in.min(total));
            if *left == 0 {
                out.fill(0.0);
                self.faded.store(true, Ordering::Release);
                return;
            }
            r.render(out, callback_frames);
            for frame in out.chunks_exact_mut(r.channels) {
                let gain = *left as f32 / total as f32;
                for s in frame.iter_mut() {
                    *s *= gain;
                }
                *left = left.saturating_sub(1);
            }
            if *left == 0 {
                self.faded.store(true, Ordering::Release);
            }
            return;
        }
        r.render(out, callback_frames);
        if self.faded_in < total {
            for frame in out.chunks_exact_mut(r.channels) {
                let gain = (self.faded_in as f32 / total as f32).min(1.0);
                for s in frame.iter_mut() {
                    *s *= gain;
                }
                self.faded_in += 1;
            }
        }
    }
}

/// The control-thread end of a speaker's handoff.
pub(in crate::audio::pipeline) struct SpeakerLink {
    slot: Slot,
    retire: Arc<AtomicBool>,
    faded: Arc<AtomicBool>,
    calls: Arc<AtomicU64>,
}

pub(in crate::audio::pipeline) fn speaker_link() -> (SpeakerLink, SpeakerCallback) {
    let slot: Slot = Arc::new(Mutex::new(None));
    let retire = Arc::new(AtomicBool::new(false));
    let faded = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicU64::new(0));
    (
        SpeakerLink {
            slot: slot.clone(),
            retire: retire.clone(),
            faded: faded.clone(),
            calls: calls.clone(),
        },
        SpeakerCallback {
            slot,
            retire,
            faded,
            calls,
            fade_left: None,
            faded_in: 0,
        },
    )
}

impl SpeakerLink {
    /// Hand the renderer to the callback. Call once the stream is open, so a
    /// failed open never takes the graph down with its closure.
    pub(in crate::audio::pipeline) fn attach(&mut self, renderer: SpeakerRenderer) {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if slot.replace(renderer).is_some() {
            tracing::error!("speaker renderer attached twice");
        }
    }

    /// Fade the speaker out and take the renderer back, so it drops here and
    /// not on the audio thread or with a callback closure cpal never frees.
    /// Waits for the fade while the device keeps calling back; a device that
    /// has stopped calling gets no fade, but still gives the renderer back.
    pub(in crate::audio::pipeline) fn retire(&mut self) -> Option<SpeakerRenderer> {
        self.retire.store(true, Ordering::Release);
        let started = Instant::now();
        let mut calls = self.calls.load(Ordering::Relaxed);
        let mut last_call = started;
        while !self.faded.load(Ordering::Acquire) {
            let now = Instant::now();
            let seen = self.calls.load(Ordering::Relaxed);
            if seen != calls {
                calls = seen;
                last_call = now;
            }
            if now - last_call >= DEAD_DEVICE_AFTER || now - started >= RETIRE_TIMEOUT {
                tracing::warn!("speaker stopped calling back; taking its renderer without a fade");
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.slot.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::graph::BUFFER_FRAME_OPTIONS;
    use crate::audio::pipeline::dag::graph_tests::{build_with_block, passthrough_graph};

    const SR: u32 = 48_000;

    fn feed(input: &mut rtrb::Producer<f32>, data: &[f32]) {
        let (_, rest) = input.push_partial_slice(data);
        assert!(rest.is_empty(), "input ring holds the whole test signal");
    }

    fn ramp(frames: usize) -> Vec<f32> {
        (0..frames * 2).map(|i| (i / 2) as f32 + 1.0).collect()
    }

    struct Rig {
        r: SpeakerRenderer,
        ctrl: WorkerCtrl,
        io: SpeakerIo,
        input: rtrb::Producer<f32>,
        blocks: Arc<AtomicU64>,
    }

    fn rig(block: usize, device_rate: u32) -> Rig {
        let (valid, _) = passthrough_graph();
        let (built, mut producers) = build_with_block(Some("s"), SR, block, &valid, SR, false);
        let blocks = built.output.blocks.clone();
        let (r, ctrl, io) =
            SpeakerRenderer::new(built.graph, device_rate, None, MeterHandle::new("s".into()))
                .expect("renderer");
        Rig {
            r,
            ctrl,
            io,
            input: producers.remove("m").unwrap(),
            blocks,
        }
    }

    /// Renders `sizes` in turn until `total` frames have been played.
    fn play(r: &mut SpeakerRenderer, sizes: &[usize], total: usize) -> Vec<f32> {
        let mut got = Vec::new();
        let mut i = 0;
        while got.len() < total * 2 {
            let n = sizes[i % sizes.len()];
            let mut out = vec![f32::NAN; n * 2];
            r.render(&mut out, n);
            got.extend_from_slice(&out);
            i += 1;
        }
        got.truncate(total * 2);
        got
    }

    #[test]
    fn callback_matching_the_block_adds_no_latency() {
        for block in BUFFER_FRAME_OPTIONS.map(|n| n as usize) {
            let mut t = rig(block, SR);
            let fed = ramp(block * 8);
            feed(&mut t.input, &fed);
            let got = play(&mut t.r, &[block], block * 8);
            assert_eq!(
                got, fed,
                "{block}: first callback plays the first input frame"
            );
            assert_eq!(t.io.carried_frames.load(Ordering::Relaxed), 0);
            assert_eq!(t.io.callback_frames.load(Ordering::Relaxed), block as u32);
        }
    }

    #[test]
    fn uneven_callbacks_stream_without_gaps_or_repeats() {
        // Device slices that never line up with the 64-frame block.
        let mut t = rig(64, SR);
        let fed = ramp(4096);
        feed(&mut t.input, &fed);
        let got = play(&mut t.r, &[37, 100, 1, 64, 250], 4000);
        assert_eq!(got, fed[..4000 * 2]);
        assert!(
            t.io.carried_frames.load(Ordering::Relaxed) < 64,
            "the adapter never holds a whole block"
        );
    }

    #[test]
    fn device_rate_differs_from_pipeline_rate() {
        // 48 kHz graph into a 44.1 kHz device. Small blocks go into the FFT
        // resampler several times before it emits anything; the device must
        // still get continuous audio, and the graph must run at real time.
        for block in [32, 64, 128, 256, 1024] {
            let mut t = rig(block, 44_100);
            feed(&mut t.input, &vec![0.25_f32; 48_000 * 2]);
            let mut got = Vec::new();
            for _ in 0..100 {
                let mut out = vec![f32::NAN; 441 * 2];
                t.r.render(&mut out, 441);
                got.extend_from_slice(&out);
            }
            let settled = &got[2 * 441 * 2..];
            assert!(
                settled.iter().all(|s| (s - 0.25).abs() < 0.01),
                "{block}: gap or glitch in the resampled stream"
            );
            let blocks = t.blocks.load(Ordering::Relaxed) as i64;
            let want = 48_000 / block as i64;
            assert!(
                (blocks - want).abs() <= 2 + 160 / block as i64,
                "{block}: {blocks} blocks rendered for one second, want ~{want}"
            );
            assert!(t.io.resampler_delay_frames > 0);
        }
    }

    #[test]
    fn resampling_to_the_device_rate_keeps_the_level() {
        // A 48 kHz graph into a 44.1 kHz device: a steady 0.5 comes out at 0.5.
        let mut t = rig(64, 44_100);
        feed(&mut t.input, &vec![0.5; 48_000]);
        let got = play(&mut t.r, &[64], 16_000);
        let steady = &got[8_000 * 2..];
        let worst = steady
            .iter()
            .map(|s| (s - 0.5).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst < 1e-3, "off by {worst}");
    }

    #[test]
    fn graph_swap_lands_between_blocks() {
        let mut t = rig(64, SR);
        let mut out = vec![0.0; 64 * 2];
        t.r.render(&mut out, 64);
        let (valid, _) = passthrough_graph();
        let (built, _) = build_with_block(Some("s"), SR, 64, &valid, SR, false);
        let mut next = built.graph;
        next.set_out_channels(2);
        let blocks = built.output.blocks.clone();
        t.ctrl.send_graph(next).expect("swap");
        t.r.render(&mut out, 64);
        assert_eq!(
            blocks.load(Ordering::Relaxed),
            0,
            "the old graph fades out first"
        );
        assert_eq!(t.blocks.load(Ordering::Relaxed), 2);
        // 10 ms at 48 kHz is 7.5 blocks of 64: the old graph renders 8, then
        // the new one takes over at the next block boundary.
        for _ in 0..20 {
            t.r.render(&mut out, 64);
        }
        assert_eq!(t.blocks.load(Ordering::Relaxed), 9, "old graph stopped");
        assert_eq!(blocks.load(Ordering::Relaxed), 13, "new graph renders");
        assert_eq!(t.io.callbacks.load(Ordering::Relaxed), 22);
    }

    #[test]
    fn rendering_never_touches_the_heap() {
        for (block, device_rate) in [(32, SR), (256, SR), (64, 44_100), (512, 96_000)] {
            let mut t = rig(block, device_rate);
            feed(&mut t.input, &ramp(40_000));
            let mut out = vec![0.0; 1024 * 2];
            for n in [block, 37, 500] {
                t.r.render(&mut out[..n * 2], n);
            }
            crate::audio::rt_guard::assert_no_alloc(
                &format!("render {block} @ {device_rate}"),
                || {
                    for n in [block, 37, 500, 1, block * 2, 1024] {
                        t.r.render(&mut out[..n * 2], n);
                    }
                },
            );
        }
    }

    #[test]
    fn load_is_measured_against_the_callback_period() {
        let mut t = rig(256, SR);
        let mut out = vec![0.0; 256 * 2];
        t.r.render(&mut out, 256);
        let load = t.io.load_peak_permille.load(Ordering::Relaxed);
        assert!(
            load < 1000,
            "a passthrough renders faster than real time: {load}"
        );
    }

    #[test]
    fn callback_plays_silence_until_attached_and_after_retire() {
        let (mut link, mut cb) = speaker_link();
        let mut t = rig(64, SR);
        feed(&mut t.input, &ramp(1024));

        let mut out = vec![1.0; 64 * 2];
        cb.fill(&mut out, 64);
        assert!(out.iter().all(|&s| s == 0.0), "no renderer yet");

        link.attach(t.r);
        cb.fill(&mut out, 64);
        assert_eq!(out[0], 0.0, "renderer arrived and fades in");
        assert!(out[127] > 0.0, "renderer arrived and plays the input");

        // Retire on another thread while the "device" keeps calling back.
        let handle = std::thread::spawn(move || link.retire().is_some());
        let deadline = Instant::now() + Duration::from_secs(2);
        while !handle.is_finished() && Instant::now() < deadline {
            cb.fill(&mut out, 64);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            handle.join().unwrap(),
            "renderer came back to the control thread"
        );
        cb.fill(&mut out, 64);
        assert!(out.iter().all(|&s| s == 0.0), "retired callback is silent");
    }

    #[test]
    fn starting_and_stopping_fade_instead_of_stepping() {
        let (mut link, mut cb) = speaker_link();
        let mut t = rig(64, SR);
        feed(&mut t.input, &vec![0.5; 48_000]);
        link.attach(t.r);
        let mut out = vec![0.0; 64 * 2];
        let mut played: Vec<f32> = Vec::new();
        for _ in 0..20 {
            cb.fill(&mut out, 64);
            played.extend(out.iter().step_by(2));
        }
        assert_eq!(*played.last().unwrap(), 0.5, "faded in");
        cb.retire.store(true, Ordering::Release);
        for _ in 0..20 {
            cb.fill(&mut out, 64);
            played.extend(out.iter().step_by(2));
        }
        let fade = (SR * STOP_FADE_MS / 1000) as f32;
        let worst = played
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst <= 0.5 / fade + 1e-6, "a step of {worst}");
        assert_eq!(*played.last().unwrap(), 0.0);
        assert!(
            link.retire().is_some(),
            "renderer handed back after the fade"
        );
    }

    #[test]
    fn retire_before_the_first_callback_hands_the_renderer_back() {
        // A stream torn down right after opening: the renderer is attached but
        // the device has not called back yet when the retire lands.
        let (mut link, mut cb) = speaker_link();
        let t = rig(64, SR);
        link.attach(t.r);
        let handle = std::thread::spawn(move || link.retire().is_some());
        std::thread::sleep(Duration::from_millis(5));
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut out = vec![0.0; 64 * 2];
        while !handle.is_finished() && Instant::now() < deadline {
            cb.fill(&mut out, 64);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            handle.join().unwrap(),
            "renderer stayed in the callback's queue"
        );
    }

    #[test]
    fn retiring_during_the_fade_in_does_not_jump() {
        let (mut link, mut cb) = speaker_link();
        let mut t = rig(64, SR);
        feed(&mut t.input, &vec![0.5; 48_000]);
        link.attach(t.r);
        let mut out = vec![0.0; 64 * 2];
        let mut played: Vec<f32> = Vec::new();
        cb.fill(&mut out, 64);
        played.extend(out.iter().step_by(2));
        cb.retire.store(true, Ordering::Release);
        for _ in 0..10 {
            cb.fill(&mut out, 64);
            played.extend(out.iter().step_by(2));
        }
        let fade = (SR * STOP_FADE_MS / 1000) as f32;
        let worst = played
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst <= 0.5 / fade + 1e-6, "a step of {worst}");
        assert_eq!(*played.last().unwrap(), 0.0);
    }

    #[test]
    fn a_dead_device_still_gives_its_renderer_back_quickly() {
        // The device stopped calling: no fade will ever play, but the graph
        // must still come back to drop here, and without the full timeout.
        let (mut link, cb) = speaker_link();
        let t = rig(64, SR);
        link.attach(t.r);
        let started = Instant::now();
        assert!(link.retire().is_some(), "the renderer came back");
        assert!(
            started.elapsed() < RETIRE_TIMEOUT,
            "waited {:?}",
            started.elapsed()
        );
        drop(cb);
    }

    #[test]
    fn a_callback_that_misses_the_lock_plays_silence() {
        let (mut link, mut cb) = speaker_link();
        let mut t = rig(64, SR);
        feed(&mut t.input, &vec![0.5; 4_096]);
        link.attach(t.r);
        let held = link.slot.lock().unwrap();
        let mut out = vec![1.0; 64 * 2];
        cb.fill(&mut out, 64);
        assert!(out.iter().all(|&s| s == 0.0));
        drop(held);
    }
}
