//! Audio thread entry point.
//!
//! The thread owns all `cpal::Stream`s (which are `!Send` on macOS). All
//! heavy lifting lives in `pipeline::build`; this module is just a command
//! dispatcher.

use std::sync::mpsc::{Receiver, Sender};

use tauri::{AppHandle, Emitter};
use tracing::{error, info, warn};

use crate::audio::graph::ValidGraph;
use crate::audio::pipeline::{self, ActivePipeline};
use crate::error::{AppError, AppResult};

const STATE_EVENT: &str = "audio://state";

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
    /// Pause or resume an audio-file input. Silent no-op when not an AudioFile.
    SetAudioFilePaused {
        node_id: String,
        paused: bool,
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
    /// Current speaker output buffering latency in milliseconds (0 when idle).
    OutputLatencyMs {
        reply: Sender<u32>,
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
                    info!("stop: pipeline was already idle");
                } else {
                    info!("pipeline torn down");
                }
                let _ = reply.send(Ok(()));
            }
            Command::Reconcile { graph, app, reply } => match active.as_mut() {
                None => {
                    let _ = reply.send(Err(AppError::NotRunning));
                }
                Some(p) => {
                    let r = p.reconcile(&graph, app.clone());
                    if let Err(e) = &r {
                        error!(error = %e, "reconcile failed, clearing active pipeline");
                        active = None;
                        let _ = app.emit(STATE_EVENT, serde_json::json!({ "kind": "stopped" }));
                    }
                    let _ = reply.send(r);
                }
            },
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
            Command::SetAudioFilePaused {
                node_id,
                paused,
                reply,
            } => {
                if let Some(p) = &active {
                    p.set_audio_file_paused(&node_id, paused);
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
            Command::OutputLatencyMs { reply } => {
                let ms = active.as_ref().map(|p| p.output_latency_ms()).unwrap_or(0);
                let _ = reply.send(ms);
            }
        }
    }

    info!("audio thread stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    /// Drives `engine::run` on a thread with a command queue; returns the
    /// sender plus a join guard that ends the run when dropped.
    struct EngineHarness {
        /// Taken and dropped in `Drop` BEFORE joining, so the engine's
        /// `recv()` sees the disconnect and the run loop exits.
        tx: Option<Sender<Command>>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl EngineHarness {
        fn spawn() -> Self {
            let (tx, rx) = channel::<Command>();
            let thread = std::thread::spawn(move || run(rx));
            Self {
                tx: Some(tx),
                thread: Some(thread),
            }
        }

        fn is_running(&self) -> bool {
            let (reply, rx) = channel();
            self.tx
                .as_ref()
                .expect("live sender")
                .send(Command::IsRunning { reply })
                .expect("send");
            rx.recv_timeout(Duration::from_secs(5)).expect("reply")
        }

        fn output_latency(&self) -> u32 {
            let (reply, rx) = channel();
            self.tx
                .as_ref()
                .expect("live sender")
                .send(Command::OutputLatencyMs { reply })
                .expect("send");
            rx.recv_timeout(Duration::from_secs(5)).expect("reply")
        }
    }

    impl Drop for EngineHarness {
        fn drop(&mut self) {
            if let Some(t) = self.thread.take() {
                drop(self.tx.take()); // disconnect first, then join
                let _ = t.join();
            }
        }
    }

    #[test]
    fn idle_engine_reports_not_running_and_zero_latency() {
        let harness = EngineHarness::spawn();
        assert!(!harness.is_running());
        assert_eq!(harness.output_latency(), 0);
    }

    #[test]
    fn idle_commands_ack_without_a_pipeline() {
        let harness = EngineHarness::spawn();
        macro_rules! ok_cmd {
            ($cmd:expr) => {{
                let (reply, rx) = channel();
                let cmd = $cmd(reply);
                harness
                    .tx
                    .as_ref()
                    .expect("live sender")
                    .send(cmd)
                    .expect("send");
                rx.recv_timeout(Duration::from_secs(5))
                    .expect("reply")
                    .expect("no-op succeeds");
            }};
        }
        ok_cmd!(|reply| Command::Stop { reply });
        ok_cmd!(|reply| Command::UpdateEffect {
            node_id: "n".into(),
            data: serde_json::json!({}),
            reply
        });
        ok_cmd!(|reply| Command::SeekAudioFile {
            node_id: "n".into(),
            frame: 10,
            reply
        });
        ok_cmd!(|reply| Command::SetAudioFileLoop {
            node_id: "n".into(),
            enabled: true,
            reply
        });
        ok_cmd!(|reply| Command::SetAudioFilePaused {
            node_id: "n".into(),
            paused: true,
            reply
        });
        ok_cmd!(|reply| Command::SetInputVolume {
            node_id: "n".into(),
            scalar: 0.5,
            reply
        });
        assert!(!harness.is_running(), "still idle after all no-ops");
    }

    #[test]
    fn run_exits_when_the_channel_closes() {
        // Dropping the sender ends the recv loop; the thread joins cleanly in
        // the harness Drop (no hang, no leaked audio thread).
        let harness = EngineHarness::spawn();
        assert!(!harness.is_running());
        drop(harness);
    }
}
