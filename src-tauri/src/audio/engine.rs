//! Audio thread entry point.
//!
//! The thread owns all `cpal::Stream`s (which are `!Send` on macOS). All
//! heavy lifting lives in `pipeline::build`; this module is just a command
//! dispatcher.

use std::sync::mpsc::{Receiver, Sender};

use tauri::AppHandle;
use tracing::{error, info};

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
}

pub fn run(rx: Receiver<Command>) {
    info!("audio thread started");
    let mut active: Option<ActivePipeline> = None;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Command::Start { graph, app, reply } => {
                if active.is_some() {
                    let _ = reply.send(Err(AppError::AlreadyRunning));
                    continue;
                }
                match pipeline::build(&graph, app) {
                    Ok(p) => {
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
                    let _ = reply.send(Err(AppError::NotRunning));
                } else {
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
        }
    }

    info!("audio thread stopped");
}
