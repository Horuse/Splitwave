//! Audio thread entry point.
//!
//! The thread owns all `cpal::Stream`s (which are `!Send` on macOS). All
//! heavy lifting lives in `pipeline::build`; this module is just a command
//! dispatcher.

use std::sync::mpsc::{Receiver, Sender};

use tauri::AppHandle;
use tracing::{error, info, warn};

use crate::audio::graph::ValidGraph;
use crate::audio::pipeline::{self, ActivePipeline};
use crate::error::{AppError, AppResult};

pub enum Command {
    Start {
        graph: ValidGraph,
        app: AppHandle,
        reply: Sender<AppResult<()>>,
    },
    Stop {
        reply: Sender<AppResult<()>>,
    },
    /// Hot reconfigure: keep the running pipeline, diff against `graph`,
    /// and only touch what changed. Errors with `NotRunning` if no pipeline
    /// has been started yet.
    Reconcile {
        graph: ValidGraph,
        app: AppHandle,
        reply: Sender<AppResult<()>>,
    },
    /// Live parameter update for an effect node. Silently no-ops when the
    /// pipeline isn't running or the node id isn't an effect.
    UpdateEffect {
        node_id: String,
        data: serde_json::Value,
        reply: Sender<AppResult<()>>,
    },
    /// Seek an audio-file input to a given frame index. Silent no-op when
    /// the node isn't an AudioFile.
    SeekAudioFile {
        node_id: String,
        frame: i64,
        reply: Sender<AppResult<()>>,
    },
    /// Toggle loop-on-EOF for an audio-file input. Silent no-op when the
    /// node isn't an AudioFile.
    SetAudioFileLoop {
        node_id: String,
        enabled: bool,
        reply: Sender<AppResult<()>>,
    },
    /// Live volume update for an input node. Silent no-op when not running.
    SetInputVolume {
        node_id: String,
        scalar: f32,
        reply: Sender<AppResult<()>>,
    },
    IsRunning {
        reply: Sender<bool>,
    },
}

pub fn run(rx: Receiver<Command>) {
    info!("audio thread started");
    let mut active: Option<ActivePipeline> = None;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Command::Start { graph, app, reply } => {
                if active.is_some() {
                    warn!("start ignored: pipeline already running");
                    let _ = reply.send(Err(AppError::AlreadyRunning));
                    continue;
                }
                match pipeline::build(&graph, app) {
                    Ok(p) => {
                        info!("pipeline built and running");
                        active = Some(p);
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
                    warn!("stop ignored: pipeline not running");
                    let _ = reply.send(Err(AppError::NotRunning));
                } else {
                    info!("pipeline torn down");
                    let _ = reply.send(Ok(()));
                }
            }
            Command::Reconcile { graph, app, reply } => {
                match active.as_mut() {
                    None => {
                        let _ = reply.send(Err(AppError::NotRunning));
                    }
                    Some(p) => {
                        let r = p.reconcile(&graph, app);
                        if let Err(e) = &r {
                            error!(error = %e, "reconcile failed");
                        }
                        let _ = reply.send(r);
                    }
                }
            }
            Command::UpdateEffect {
                node_id,
                data,
                reply,
            } => {
                if let Some(p) = &active {
                    p.update_effect(&node_id, &data);
                }
                let _ = reply.send(Ok(()));
            }
            Command::SeekAudioFile {
                node_id,
                frame,
                reply,
            } => {
                if let Some(p) = &active {
                    p.seek_audio_file(&node_id, frame);
                }
                let _ = reply.send(Ok(()));
            }
            Command::SetAudioFileLoop {
                node_id,
                enabled,
                reply,
            } => {
                if let Some(p) = &active {
                    p.set_audio_file_loop(&node_id, enabled);
                }
                let _ = reply.send(Ok(()));
            }
            Command::SetInputVolume {
                node_id,
                scalar,
                reply,
            } => {
                if let Some(p) = &active {
                    p.set_input_volume(&node_id, scalar);
                }
                let _ = reply.send(Ok(()));
            }
            Command::IsRunning { reply } => {
                let _ = reply.send(active.is_some());
            }
        }
    }

    info!("audio thread stopped");
}
