//! Host-side manager for Out-of-Process VST3 plugins on Windows.

#![cfg(target_os = "windows")]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::Emitter;

use crate::audio::effects::Effect;
use crate::audio::plugins::bridge::protocol::{HelperEvent, HostCommand};
use crate::audio::plugins::bridge::shm_audio::ShmHost;
use crate::audio::plugins::host_api::{
    alive_flag, tag_state, untag_state, ActivateRequest, AliveFlag, EditorSize, Graveyard,
    HostedNode, PluginHost, PluginParamInfo, PluginStatus, Unsupported, EDITOR_CLOSED_EVENT,
};
use crate::audio::plugins::{ParamRing, PluginFormat};

/// RT audio node communicating with the helper process over Shared Memory.
pub struct BridgeNode {
    shm: ShmHost,
    channels: usize,
    latency: usize,
    alive: AliveFlag,
    params: Arc<ParamRing>,
    param_cursor: usize,
    cmd_tx: Sender<HostCommand>,
}

impl BridgeNode {
    pub fn new(
        shm: ShmHost,
        channels: usize,
        latency: usize,
        alive: AliveFlag,
        params: Arc<ParamRing>,
        cmd_tx: Sender<HostCommand>,
    ) -> Self {
        let param_cursor = params.reader();
        Self {
            shm,
            channels,
            latency,
            alive,
            params,
            param_cursor,
            cmd_tx,
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }
}

impl Effect for BridgeNode {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        // Drain parameter edits and forward to helper
        while let Some((id, value)) = self.params.read(&mut self.param_cursor) {
            let _ = self.cmd_tx.send(HostCommand::SetParam { id, value });
        }

        if let Ok(latency) = self.shm.process(samples, frames, self.channels) {
            self.latency = latency;
        }
    }

    fn latency_frames(&self) -> usize {
        self.latency
    }
}

impl Drop for BridgeNode {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        self.shm.mark_dead();
    }
}

pub struct BridgeSlot {
    pub path: String,
    pub plugin_id: String,
    pub params: Vec<PluginParamInfo>,
    pub has_editor: bool,
    pub cmd_tx: Sender<HostCommand>,
    pub reply_rx: Arc<Mutex<Receiver<HelperEvent>>>,
    pub alive: AliveFlag,
    pub child: Arc<Mutex<Option<Child>>>,
}

pub fn slots() -> &'static Mutex<HashMap<String, BridgeSlot>> {
    static SLOTS: std::sync::OnceLock<Mutex<HashMap<String, BridgeSlot>>> =
        std::sync::OnceLock::new();
    SLOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn graveyard() -> &'static Mutex<Graveyard<(Sender<HostCommand>, Arc<Mutex<Option<Child>>>)>> {
    static GRAVEYARD: std::sync::OnceLock<
        Mutex<Graveyard<(Sender<HostCommand>, Arc<Mutex<Option<Child>>>)>>,
    > = std::sync::OnceLock::new();
    GRAVEYARD.get_or_init(|| Mutex::new(Graveyard::default()))
}

pub fn with_slot<R>(node_id: &str, f: impl FnOnce(&mut BridgeSlot) -> R) -> Option<R> {
    slots().lock().unwrap().get_mut(node_id).map(f)
}

pub struct BridgeHost;

impl PluginHost for BridgeHost {
    fn activate(&self, req: ActivateRequest<'_>) -> Result<HostedNode, String> {
        let node_id = req.node_id.to_string();
        let path = req.path.to_string();
        let plugin_id = req.plugin_id.to_string();
        let session_id = format!("{}_{}", cuid2::create_id(), std::process::id());

        // 1. Create Shared Memory audio channel
        let shm = ShmHost::create(&session_id)
            .map_err(|e| format!("bridge {node_id}: failed to create shm: {e}"))?;

        // 2. Spawn helper child process: splitwave.exe --plugin-bridge <session_id>
        let current_exe = std::env::current_exe()
            .map_err(|e| format!("bridge {node_id}: failed to get current_exe: {e}"))?;

        let mut child = Command::new(current_exe)
            .arg("--plugin-bridge")
            .arg(&session_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("bridge {node_id}: failed to spawn helper: {e}"))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("bridge {node_id}: failed to open helper stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("bridge {node_id}: failed to open helper stdout"))?;

        let (cmd_tx, cmd_rx) = channel::<HostCommand>();
        let (reply_tx, reply_rx) = channel::<HelperEvent>();

        // Background stdin writer
        thread::spawn(move || {
            while let Ok(cmd) = cmd_rx.recv() {
                if let Ok(json) = serde_json::to_string(&cmd) {
                    if writeln!(stdin, "{json}").is_err() || stdin.flush().is_err() {
                        break;
                    }
                }
                if matches!(cmd, HostCommand::Shutdown) {
                    break;
                }
            }
        });

        // Background stdout reader
        let event_node_id = node_id.clone();
        let event_param_ring = req.params.clone();
        let event_reply_tx = reply_tx.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if let Ok(text) = line {
                    if let Ok(event) = serde_json::from_str::<HelperEvent>(&text) {
                        match &event {
                            HelperEvent::ParamEdited { id, value } => {
                                event_param_ring.push(*id, *value);
                            }
                            HelperEvent::EditorClosed => {
                                if let Some(app) = crate::app_handle() {
                                    let _ = app.emit(EDITOR_CLOSED_EVENT, &event_node_id);
                                }
                            }
                            _ => {
                                let _ = event_reply_tx.send(event);
                            }
                        }
                    }
                } else {
                    break;
                }
            }
        });

        // 3. Initialize plugin in helper
        cmd_tx
            .send(HostCommand::Init {
                path: path.clone(),
                plugin_id: plugin_id.clone(),
            })
            .map_err(|e| format!("bridge {node_id}: failed to send Init: {e}"))?;

        let (params, has_editor) = match reply_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(HelperEvent::Loaded { params, has_editor }) => (params, has_editor),
            Ok(HelperEvent::Error { message }) => {
                return Err(format!("bridge {node_id} load error: {message}"));
            }
            other => {
                return Err(format!(
                    "bridge {node_id}: unexpected response to Init: {other:?}"
                ));
            }
        };

        // 4. Activate plugin in helper
        let state = req
            .state
            .and_then(|s| untag_state(&plugin_id, s))
            .map(str::to_string);

        cmd_tx
            .send(HostCommand::Activate {
                sample_rate: req.sample_rate,
                max_frames: req.max_frames,
                channels: req.channels,
                state,
            })
            .map_err(|e| format!("bridge {node_id}: failed to send Activate: {e}"))?;

        let (accepted_channels, latency_frames) =
            match reply_rx.recv_timeout(Duration::from_secs(5)) {
                Ok(HelperEvent::Activated {
                    accepted_channels,
                    latency_frames,
                }) => (accepted_channels, latency_frames),
                Ok(HelperEvent::Error { message }) => {
                    return Err(format!("bridge {node_id} activate error: {message}"));
                }
                other => {
                    return Err(format!(
                        "bridge {node_id}: unexpected response to Activate: {other:?}"
                    ));
                }
            };

        let alive = alive_flag();
        let bridge_node = BridgeNode::new(
            shm,
            accepted_channels,
            latency_frames,
            alive.clone(),
            req.params.clone(),
            cmd_tx.clone(),
        );

        let child_arc = Arc::new(Mutex::new(Some(child)));
        let reply_rx_arc = Arc::new(Mutex::new(reply_rx));

        if req.primary {
            let old = slots().lock().unwrap().insert(
                node_id,
                BridgeSlot {
                    path,
                    plugin_id,
                    params,
                    has_editor,
                    cmd_tx,
                    reply_rx: reply_rx_arc,
                    alive: alive.clone(),
                    child: child_arc.clone(),
                },
            );
            if let Some(old) = old {
                graveyard()
                    .lock()
                    .unwrap()
                    .bury((old.cmd_tx, old.child), old.alive);
            }
        } else {
            graveyard().lock().unwrap().bury((cmd_tx, child_arc), alive);
        }

        Ok(HostedNode::Bridge(bridge_node))
    }

    fn forget(&self, node_id: &str) {
        let slot = slots().lock().unwrap().remove(node_id);
        if let Some(slot) = slot {
            graveyard()
                .lock()
                .unwrap()
                .bury((slot.cmd_tx, slot.child), slot.alive);
        }
    }

    fn status(&self, node_id: &str) -> PluginStatus {
        with_slot(node_id, |slot| PluginStatus {
            path: Some(slot.path.clone()),
            has_editor: slot.has_editor,
        })
        .unwrap_or_default()
    }

    fn params(&self, node_id: &str) -> Vec<PluginParamInfo> {
        with_slot(node_id, |slot| slot.params.clone()).unwrap_or_default()
    }

    fn save_state(&self, node_id: &str) -> Result<Option<String>, Unsupported> {
        let (cmd_tx, reply_rx, plugin_id) = match with_slot(node_id, |slot| {
            (
                slot.cmd_tx.clone(),
                slot.reply_rx.clone(),
                slot.plugin_id.clone(),
            )
        }) {
            Some(v) => v,
            None => return Ok(None),
        };

        if cmd_tx.send(HostCommand::SaveState).is_err() {
            return Ok(None);
        }

        let rx = reply_rx.lock().unwrap();
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(HelperEvent::StateSaved { blob }) => Ok(blob.map(|b| tag_state(&plugin_id, &b))),
            _ => Ok(None),
        }
    }

    fn notify_param_changed(
        &self,
        node_id: &str,
        param_id: u32,
        value: f64,
    ) -> Result<(), Unsupported> {
        with_slot(node_id, move |slot| {
            let _ = slot.cmd_tx.send(HostCommand::SetParam {
                id: param_id,
                value,
            });
        });
        Ok(())
    }

    fn embed_editor(&self, node_id: &str, window: &tauri::Window) -> Result<EditorSize, String> {
        let _ = window; // In bridge mode, the helper process manages its own pure Win32 top-level window
        let (cmd_tx, reply_rx) =
            with_slot(node_id, |slot| (slot.cmd_tx.clone(), slot.reply_rx.clone()))
                .ok_or_else(|| format!("{node_id}: no plugin running"))?;

        cmd_tx
            .send(HostCommand::OpenEditor {
                title: "Plugin Editor".to_string(),
            })
            .map_err(|e| format!("failed to send OpenEditor: {e}"))?;

        let rx = reply_rx.lock().unwrap();
        match rx.recv_timeout(Duration::from_millis(1500)) {
            Ok(HelperEvent::EditorOpened { width, height }) => Ok((width, height)),
            Ok(HelperEvent::Ok) => Ok((800, 600)),
            Ok(HelperEvent::Error { message }) => Err(message),
            other => Err(format!("unexpected response to OpenEditor: {other:?}")),
        }
    }

    fn destroy_editor(&self, node_id: &str) {
        with_slot(node_id, |slot| {
            let _ = slot.cmd_tx.send(HostCommand::CloseEditor);
        });
    }

    fn tick_and_reclaim(&self) {
        let mut freed = graveyard().lock().unwrap().reclaim();
        for (cmd_tx, child_mutex) in freed.drain(..) {
            let _ = cmd_tx.send(HostCommand::Shutdown);
            if let Ok(mut lock) = child_mutex.lock() {
                if let Some(mut child) = lock.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}

impl std::fmt::Debug for BridgeHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", PluginFormat::Vst3)
    }
}
