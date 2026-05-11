use std::sync::mpsc::{Receiver, Sender};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::SampleFormat;
use rtrb::RingBuffer;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::{error, info};

use crate::audio::device::{self, DeviceKind};
use crate::audio::graph::ValidGraph;
use crate::error::{AppError, AppResult};

const RING_BUFFER_MS: f32 = 100.0;
const STATE_EVENT: &str = "audio://state";

/// Commands sent from Tauri command threads to the dedicated audio thread.
/// The audio thread owns `cpal::Stream` because it is `!Send` on macOS.
pub enum Command {
    Start {
        graph: ValidGraph,
        app: AppHandle,
        reply: Sender<AppResult<()>>,
    },
    Stop {
        reply: Sender<AppResult<()>>,
    },
}

/// Holds the two streams. Dropping it stops both.
struct RunningStream {
    _input: cpal::Stream,
    _output: cpal::Stream,
}

/// Audio thread main loop. Owns the streams; never blocks the UI thread.
pub fn run(rx: Receiver<Command>) {
    info!("audio thread started");
    let mut active: Option<RunningStream> = None;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Command::Start { graph, app, reply } => {
                if active.is_some() {
                    let _ = reply.send(Err(AppError::AlreadyRunning));
                    continue;
                }
                match build_streams(&graph, app) {
                    Ok(stream) => {
                        active = Some(stream);
                        let _ = reply.send(Ok(()));
                    }
                    Err(e) => {
                        error!(error = %e, "failed to start pipeline");
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Command::Stop { reply } => {
                if active.take().is_none() {
                    let _ = reply.send(Err(AppError::NotRunning));
                } else {
                    let _ = reply.send(Ok(()));
                }
            }
        }
    }

    info!("audio thread stopped");
}

/// Build and start input + output streams bridged by an SPSC ring buffer.
fn build_streams(graph: &ValidGraph, app: AppHandle) -> AppResult<RunningStream> {
    let input_device = device::find(DeviceKind::Input, &graph.input_device_id)?;
    let output_device = device::find(DeviceKind::Output, &graph.output_device_id)?;

    let input_cfg = input_device
        .default_input_config()
        .map_err(|e| AppError::Device(format!("input config: {e}")))?;
    let output_cfg = output_device
        .default_output_config()
        .map_err(|e| AppError::Device(format!("output config: {e}")))?;

    // For the demo we require f32 + matching sample-rate/channels.
    // Resampling and format conversion are out of scope.
    if input_cfg.sample_format() != SampleFormat::F32
        || output_cfg.sample_format() != SampleFormat::F32
    {
        return Err(AppError::Validation(
            "device requires non-f32 sample format (not supported in demo)".into(),
        ));
    }
    if input_cfg.sample_rate() != output_cfg.sample_rate() {
        return Err(AppError::Validation(format!(
            "sample-rate mismatch: input={} output={}",
            input_cfg.sample_rate().0,
            output_cfg.sample_rate().0,
        )));
    }

    // Channel-count adaptation: we always store mono frames in the ring.
    // Input callback collapses N channels to mono by averaging; output callback
    // duplicates the mono sample to M channels. No resampling.
    let in_channels = input_cfg.channels() as usize;
    let out_channels = output_cfg.channels() as usize;
    if in_channels == 0 || out_channels == 0 {
        return Err(AppError::Validation("device has zero channels".into()));
    }

    let sample_rate = input_cfg.sample_rate().0 as f32;
    let ring_size = ((sample_rate * RING_BUFFER_MS / 1000.0) as usize).max(1024);
    let (mut producer, mut consumer) = RingBuffer::<f32>::new(ring_size);

    let app_in = app.clone();
    let err_in = move |e: cpal::StreamError| {
        let _ = app_in.emit(
            STATE_EVENT,
            json!({ "kind": "error", "message": format!("input: {e}") }),
        );
    };
    let app_out = app.clone();
    let err_out = move |e: cpal::StreamError| {
        let _ = app_out.emit(
            STATE_EVENT,
            json!({ "kind": "error", "message": format!("output: {e}") }),
        );
    };

    let input_stream = input_device
        .build_input_stream(
            &input_cfg.config(),
            move |data: &[f32], _| {
                // Collapse interleaved input frames to mono by averaging the
                // channels of each frame, then push to the ring. Never lock.
                for frame in data.chunks_exact(in_channels) {
                    let sum: f32 = frame.iter().sum();
                    let mono = sum / in_channels as f32;
                    if producer.push(mono).is_err() {
                        break;
                    }
                }
            },
            err_in,
            None,
        )
        .map_err(|e| AppError::Stream(format!("input build: {e}")))?;

    let output_stream = output_device
        .build_output_stream(
            &output_cfg.config(),
            move |data: &mut [f32], _| {
                // For each output frame, pop one mono sample and duplicate it
                // across the output channels. Silence on underrun.
                for frame in data.chunks_mut(out_channels) {
                    let s = consumer.pop().unwrap_or(0.0);
                    for ch in frame.iter_mut() {
                        *ch = s;
                    }
                }
            },
            err_out,
            None,
        )
        .map_err(|e| AppError::Stream(format!("output build: {e}")))?;

    input_stream
        .play()
        .map_err(|e| AppError::Stream(format!("input play: {e}")))?;
    output_stream
        .play()
        .map_err(|e| AppError::Stream(format!("output play: {e}")))?;

    tracing::info!(
        sr = sample_rate as u32,
        in_channels,
        out_channels,
        in_device = %graph.input_device_id,
        out_device = %graph.output_device_id,
        "pipeline started"
    );

    Ok(RunningStream {
        _input: input_stream,
        _output: output_stream,
    })
}
