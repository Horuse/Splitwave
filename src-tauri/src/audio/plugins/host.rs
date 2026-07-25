//! CLAP main-thread ownership. CLAP requires every main-thread call
//! (instantiate, activate, params, GUI) on one consistent thread, and on every
//! OS a plugin's editor needs that thread to be the app's UI thread with a live
//! event loop. So instances live in a thread-local on the Tauri main thread;
//! engine-thread callers marshal in via `run_on_main_thread` and only the
//! `Send` audio processor travels to the DSP worker.
//!
//! `SplitwaveHost` advertises the host-side `gui` and `timer` extensions:
//! without them most plugins refuse to open an editor or never repaint, since
//! their GUIs redraw from host-driven timer ticks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::gui::{
    GuiApiType, GuiConfiguration, GuiSize, HostGui, HostGuiImpl, PluginGui, Window as ClackWindow,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use clack_extensions::params::{ParamInfoBuffer, ParamInfoFlags, PluginParams};
use clack_extensions::state::{HostState as HostStateExt, HostStateImpl, PluginState};
use clack_extensions::timer::{HostTimer, HostTimerImpl, PluginTimer, TimerId};
use clack_host::prelude::*;
use raw_window_handle::HasWindowHandle;
use tauri::Emitter;

use super::host_api::{
    ActivateRequest, EditorSize, HostedNode, PluginHost, PluginParamInfo, PluginStatus, Unsupported,
};
use super::node::PluginNode;
use super::{ParamRing, PluginFormat};

struct TimerReg {
    id: u32,
    period: Duration,
    next: Instant,
}

#[derive(Default)]
struct TimerState {
    next_id: u32,
    timers: Vec<TimerReg>,
}

type Timers = Rc<RefCell<TimerState>>;

pub struct SplitwaveShared {
    node_id: String,
    /// The plugin asked the host to run its `on_main_thread` callback. A plugin
    /// that marshals its own GUI edits to its audio side this way (FabFilter
    /// does) never applies them if the host ignores the request.
    callback_requested: AtomicBool,
}

impl<'a> SharedHandler<'a> for SplitwaveShared {
    // Restart means deactivate and reactivate the processor, which lives on the
    // DSP worker; not implemented, so say so rather than drop it silently.
    fn request_restart(&self) {
        tracing::warn!(node_id = %self.node_id, "plugin requested a restart; not implemented");
    }
    fn request_process(&self) {}
    fn request_callback(&self) {
        self.callback_requested.store(true, Ordering::Release);
    }
}

impl HostGuiImpl for SplitwaveShared {
    fn resize_hints_changed(&self) {}
    // The plugin drives its own size (e.g. a scale button in its UI); grow the
    // host window to match. Sizes are logical (we run with scale handled by the
    // OS), so a LogicalSize maps 1:1.
    fn request_resize(&self, new_size: GuiSize) -> Result<(), HostError> {
        // Some plugins fire a 0x0 / placeholder request during init; obeying it
        // would shrink the window to the OS minimum.
        let Some((w, h)) = valid_gui_size(new_size.width, new_size.height) else {
            return Ok(());
        };
        if let Some(win) = editor_windows().lock().unwrap().get(&self.node_id) {
            set_content_size(win, w as f64, h as f64);
        }
        Ok(())
    }
    fn request_show(&self) -> Result<(), HostError> {
        Ok(())
    }
    fn request_hide(&self) -> Result<(), HostError> {
        Ok(())
    }
    fn closed(&self, _was_destroyed: bool) {}
}

pub struct SplitwaveMainThread {
    timers: Timers,
}

impl<'a> MainThreadHandler<'a> for SplitwaveMainThread {}

impl HostStateImpl for SplitwaveMainThread {
    // Persistence is pulled on demand via `save_state`, not pushed on dirty.
    fn mark_dirty(&mut self) {}
}

impl HostTimerImpl for SplitwaveMainThread {
    fn register_timer(&mut self, period_ms: u32) -> Result<TimerId, HostError> {
        let mut st = self.timers.borrow_mut();
        let id = st.next_id;
        st.next_id += 1;
        // 30 Hz floor per the CLAP guidance; some plugins ask for 0.
        let period = Duration::from_millis(period_ms.max(15) as u64);
        st.timers.push(TimerReg {
            id,
            period,
            next: Instant::now() + period,
        });
        Ok(TimerId(id))
    }

    fn unregister_timer(&mut self, timer_id: TimerId) -> Result<(), HostError> {
        self.timers.borrow_mut().timers.retain(|t| t.id != timer_id.0);
        Ok(())
    }
}

pub struct SplitwaveHost;

impl HostHandlers for SplitwaveHost {
    type Shared<'a> = SplitwaveShared;
    type MainThread<'a> = SplitwaveMainThread;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &SplitwaveShared) {
        builder
            .register::<HostGui>()
            .register::<HostTimer>()
            .register::<HostStateExt>();
    }
}

/// Distinguishes a node's instances in the log, which is the only way to tell
/// an editor bound to the audible instance from one bound to a duplicate.
fn next_instance_serial() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

struct Slot {
    instance: PluginInstance<SplitwaveHost>,
    serial: u64,
    /// The bundle this was built from, so the UI can tell whether the running
    /// plugin is still the one the node currently points at.
    path: String,
    plugin_id: String,
    timers: Timers,
    gui_open: bool,
    // False once the matching `PluginNode` (and its processor) is dropped from
    // the DAG; the reclaim sweep then frees this instance.
    alive: Arc<AtomicBool>,
}

// A retired instance kept alive until its processor leaves the outgoing DAG:
// dropping it earlier would destroy the plugin mid-process. `alive` goes false
// when that processor is dropped, and `reclaim_dead` frees it on the next tick.
struct Grave {
    // Held only for its `Drop`: retaining the entry keeps the plugin alive.
    #[allow(dead_code)]
    instance: PluginInstance<SplitwaveHost>,
    alive: Arc<AtomicBool>,
}

struct HostState {
    info: HostInfo,
    entries: HashMap<String, PluginEntry>,
    instances: HashMap<String, Slot>,
    graveyard: Vec<Grave>,
}

fn default_state() -> HostState {
    HostState {
        info: HostInfo::new(
            "Splitwave",
            "Splitwave",
            "https://splitwave.app",
            env!("CARGO_PKG_VERSION"),
        )
        .expect("static host info is valid"),
        entries: HashMap::new(),
        instances: HashMap::new(),
        graveyard: Vec::new(),
    }
}

thread_local! {
    static HOST: RefCell<Option<HostState>> = const { RefCell::new(None) };
}

/// Runs `f` on the Tauri main thread (the CLAP main thread) and blocks for its
/// result. Callers must not be the main thread themselves, or this deadlocks.
fn on_main<R: Send + 'static>(
    f: impl FnOnce(&mut HostState) -> R + Send + 'static,
) -> Result<R, String> {
    let app = crate::app_handle().ok_or_else(|| "app handle not ready".to_string())?;
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        HOST.with(|cell| {
            let mut slot = cell.borrow_mut();
            let state = slot.get_or_insert_with(default_state);
            let _ = tx.send(f(state));
        });
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| "main-thread plugin op timed out".to_string())
}

/// Runs `f` on the Tauri main thread and blocks for its result. Callers must
/// not be the main thread themselves, or this deadlocks.
pub(super) fn run_on_main<R: Send + 'static>(
    f: impl FnOnce() -> R + Send + 'static,
) -> Result<R, String> {
    let app = crate::app_handle().ok_or_else(|| "app handle not ready".to_string())?;
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| "main-thread plugin op timed out".to_string())
}

/// Drives all registered plugin timers; runs on the main thread. Ticking is
/// what makes editor windows paint and stay responsive.
fn tick_timers() {
    HOST.with(|cell| {
        let mut slot = cell.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return;
        };
        let now = Instant::now();
        for s in state.instances.values_mut() {
            // Deferred work the plugin asked for, including the GUI-to-DSP hand
            // off some plugins rely on.
            if s.instance
                .access_shared_handler(|h| h.callback_requested.swap(false, Ordering::AcqRel))
            {
                s.instance.call_on_main_thread_callback();
            }
            let due: Vec<u32> = {
                let mut st = s.timers.borrow_mut();
                let mut due = Vec::new();
                for t in st.timers.iter_mut() {
                    if t.next <= now {
                        due.push(t.id);
                        t.next = now + t.period;
                    }
                }
                due
            };
            if due.is_empty() {
                continue;
            }
            if let Some(timer) = s.instance.plugin_handle().get_extension::<PluginTimer>() {
                for id in due {
                    timer.on_timer(&mut s.instance.plugin_handle(), TimerId(id));
                }
            }
        }
    });
}

/// Frees plugin instances whose processor has left the DAG (`alive == false`):
/// graveyard entries retired by a rebuild, and live slots whose node was
/// removed or whose pipeline stopped. Runs on the main thread, so dropping an
/// instance here is the one safe place to destroy a plugin. Editor teardown
/// happens first because the plugin's view is a child of our native window.
fn reclaim_dead() {
    HOST.with(|cell| {
        let mut slot = cell.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return;
        };
        state
            .graveyard
            .retain(|g| g.alive.load(Ordering::Acquire));

        let dead: Vec<String> = state
            .instances
            .iter()
            .filter(|(_, s)| !s.alive.load(Ordering::Acquire))
            .map(|(id, _)| id.clone())
            .collect();
        for id in dead {
            if let Some(mut s) = state.instances.remove(&id) {
                if s.gui_open {
                    if let Some(gui) = s.instance.plugin_handle().get_extension::<PluginGui>() {
                        gui.destroy(&mut s.instance.plugin_handle());
                    }
                }
                if let Some(w) = editor_windows().lock().unwrap().remove(&id) {
                    let _ = w.close();
                }
            }
        }
    });
}

fn ensure_ticker() {
    static TICKER: Once = Once::new();
    TICKER.call_once(|| {
        std::thread::Builder::new()
            .name("plugin-timer".into())
            .spawn(|| loop {
                std::thread::sleep(Duration::from_millis(16));
                if let Some(app) = crate::app_handle() {
                    let _ = app.run_on_main_thread(|| {
                        for host in super::registry::hosts() {
                            host.tick_and_reclaim();
                        }
                    });
                }
            })
            .ok();
    });
}

/// Channel count per audio port. Falls back to a single stereo port when the
/// plugin does not expose the audio-ports extension.
fn port_channels(
    ports: Option<&PluginAudioPorts>,
    handle: &mut PluginInstance<SplitwaveHost>,
    is_input: bool,
) -> Vec<u32> {
    let Some(ports) = ports else {
        return vec![2];
    };
    let count = ports.count(&mut handle.plugin_handle(), is_input);
    (0..count)
        .map(|i| {
            let mut buf = AudioPortInfoBuffer::default();
            ports
                .get(&mut handle.plugin_handle(), i, is_input, &mut buf)
                .map(|p| p.channel_count)
                .unwrap_or(2)
        })
        .collect()
}

/// Loads, instantiates and activates a CLAP plugin, returning its `Send` audio
/// node.
#[allow(clippy::too_many_arguments)]
fn activate_clap(
    node_id: &str,
    path: &str,
    plugin_id: &str,
    sample_rate: u32,
    max_frames: usize,
    state_b64: Option<String>,
    primary: bool,
    param_ring: Arc<ParamRing>,
) -> Result<PluginNode, String> {
    ensure_ticker();
    let node_id = node_id.to_string();
    let path = path.to_string();
    let plugin_id = plugin_id.to_string();
    on_main(move |state| -> Result<PluginNode, String> {
        if !state.entries.contains_key(&path) {
            let entry =
                unsafe { PluginEntry::load(&path) }.map_err(|e| format!("load {path}: {e}"))?;
            // The plugin dylib may have replaced the global panic hook on load;
            // restore ours as the outermost so crashes still get persisted.
            crate::reinstall_panic_hook();
            state.entries.insert(path.clone(), entry);
        }
        let entry = state.entries.get(&path).expect("entry just inserted");
        let id = CString::new(plugin_id.clone()).map_err(|e| e.to_string())?;

        let timers: Timers = Rc::new(RefCell::new(TimerState::default()));
        let timers_for_handler = timers.clone();
        let shared_node_id = node_id.clone();
        let mut instance = PluginInstance::<SplitwaveHost>::new(
            move |_| SplitwaveShared {
                node_id: shared_node_id,
                callback_requested: AtomicBool::new(false),
            },
            move |_| SplitwaveMainThread {
                timers: timers_for_handler,
            },
            entry,
            &id,
            &state.info,
        )
        .map_err(|e| format!("instantiate {plugin_id}: {e}"))?;

        let serial = next_instance_serial();
        tracing::debug!(node_id, path, plugin_id, primary, serial, "clap instance created");

        let ports = instance.plugin_handle().get_extension::<PluginAudioPorts>();
        let input_channels = port_channels(ports.as_ref(), &mut instance, true);
        let output_channels = port_channels(ports.as_ref(), &mut instance, false);

        let config = PluginAudioConfiguration {
            sample_rate: sample_rate as f64,
            min_frames_count: 1,
            max_frames_count: max_frames as u32,
        };
        let processor = instance
            .activate(|_, _| (), config)
            .map_err(|e| format!("activate {plugin_id}: {e}"))?
            .start_processing()
            .map_err(|e| format!("start {plugin_id}: {e}"))?;

        // Restore saved state before the node goes live. A malformed blob is
        // logged and skipped, never fatal. Some plugins (nih-plug) *panic* on a
        // stream they can't parse instead of erroring, so the load is caught:
        // this runs on the UI thread, where an escaping panic kills the app.
        tracing::debug!(
            node_id,
            plugin_id,
            state_bytes = state_b64.as_ref().map(|s| s.len()).unwrap_or(0),
            "clap instance restoring state"
        );
        let own_state = state_b64.as_deref().and_then(|tagged| {
            let payload = super::host_api::untag_state(&plugin_id, tagged);
            if payload.is_none() {
                tracing::warn!(node_id, plugin_id, "discarding state saved by another plugin");
            }
            payload
        });
        if let Some(b64) = own_state {
            if let Some(ext) = instance.plugin_handle().get_extension::<PluginState>() {
                match STANDARD.decode(b64) {
                    Ok(bytes) => {
                        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let mut reader = std::io::Cursor::new(bytes);
                            ext.load(&mut instance.plugin_handle(), &mut reader)
                        }));
                        match res {
                            Ok(Ok(())) => {}
                            Ok(Err(e)) => {
                                tracing::error!(plugin_id, error = %e, "plugin state load failed")
                            }
                            Err(_) => tracing::error!(
                                plugin_id,
                                "plugin state load panicked; keeping defaults"
                            ),
                        }
                    }
                    Err(e) => tracing::error!(plugin_id, error = %e, "plugin state decode failed"),
                }
            }
        }

        let alive = Arc::new(AtomicBool::new(true));
        let node = PluginNode::new(
            processor,
            &input_channels,
            &output_channels,
            max_frames,
            param_ring.clone(),
            alive.clone(),
        );

        // The monitor graph builds its own metering-only duplicate. It must not
        // become the editor target (which would leave the GUI driving a silent
        // instance while the audible one plays untouched), so park it in the
        // graveyard: it lives as long as its processor, then the sweep frees it.
        if !primary {
            state.graveyard.push(Grave { instance, alive });
            return Ok(node);
        }

        if let Some(mut old) = state.instances.remove(&node_id) {
            if old.gui_open {
                if let Some(gui) = old.instance.plugin_handle().get_extension::<PluginGui>() {
                    gui.destroy(&mut old.instance.plugin_handle());
                }
            }
            state.graveyard.push(Grave {
                instance: old.instance,
                alive: old.alive,
            });
        }
        // A rebuild invalidates any open editor; drop its window so a reopen
        // embeds into the new instance instead of focusing a stale one.
        if let Some(w) = editor_windows().lock().unwrap().remove(&node_id) {
            let _ = w.close();
        }
        state.instances.insert(
            node_id.clone(),
            Slot {
                instance,
                serial,
                path: path.clone(),
                plugin_id: plugin_id.clone(),
                timers,
                gui_open: false,
                alive,
            },
        );
        Ok(node)
    })
    .and_then(|r| r)
}

/// Retires a node's editor/parameter target without installing a replacement.
/// Used when a rebuild fails to load the plugin: without it the previous
/// plugin's instance stays installed and the editor goes on driving something
/// the graph no longer contains. The instance itself is graveyarded rather than
/// dropped, since its processor may still be in the outgoing DAG.
pub fn forget_instance(node_id: &str) {
    let nid = node_id.to_string();
    let _ = on_main(move |state| {
        let Some(mut slot) = state.instances.remove(&nid) else {
            return;
        };
        if slot.gui_open {
            if let Some(gui) = slot.instance.plugin_handle().get_extension::<PluginGui>() {
                gui.destroy(&mut slot.instance.plugin_handle());
            }
        }
        state.graveyard.push(Grave {
            instance: slot.instance,
            alive: slot.alive,
        });
    });
    if let Some(w) = editor_windows().lock().unwrap().remove(node_id) {
        let _ = w.close();
    }
}

/// Serializes a running plugin's state to base64, or `None` if the plugin isn't
/// running or does not implement the state extension.
fn save_state(node_id: &str) -> Option<String> {
    let node_id = node_id.to_string();
    on_main(move |state| {
        let slot = state.instances.get_mut(&node_id)?;
        let ext = slot.instance.plugin_handle().get_extension::<PluginState>()?;
        let mut buf = Vec::new();
        ext.save(&mut slot.instance.plugin_handle(), &mut buf).ok()?;
        Some(super::host_api::tag_state(&slot.plugin_id, &STANDARD.encode(&buf)))
    })
    .ok()
    .flatten()
}

/// Enumerates a running plugin's parameters (id, range, current value) for the
/// node UI. Empty when the plugin isn't running or exposes no params extension.
fn get_plugin_params(node_id: &str) -> Vec<PluginParamInfo> {
    let node_id = node_id.to_string();
    on_main(move |state| {
        let Some(slot) = state.instances.get_mut(&node_id) else {
            return Vec::new();
        };
        let Some(ext) = slot.instance.plugin_handle().get_extension::<PluginParams>() else {
            return Vec::new();
        };
        let count = ext.count(&mut slot.instance.plugin_handle());
        let mut out = Vec::with_capacity(count as usize);
        let mut buf = ParamInfoBuffer::default();
        for i in 0..count {
            let Some(info) = ext.get_info(&mut slot.instance.plugin_handle(), i, &mut buf) else {
                continue;
            };
            if info.flags.contains(ParamInfoFlags::IS_HIDDEN) {
                continue;
            }
            let value = ext
                .get_value(&mut slot.instance.plugin_handle(), info.id)
                .unwrap_or(info.default_value);
            out.push(PluginParamInfo {
                id: info.id.get(),
                name: String::from_utf8_lossy(info.name).into_owned(),
                min: info.min_value,
                max: info.max_value,
                default: info.default_value,
                value,
                stepped: info.flags.contains(ParamInfoFlags::IS_STEPPED),
                read_only: info.flags.contains(ParamInfoFlags::IS_READONLY),
            });
        }
        out
    })
    .unwrap_or_default()
}

fn status(node_id: &str) -> PluginStatus {
    let node_id = node_id.to_string();
    on_main(move |state| {
        let Some(slot) = state.instances.get_mut(&node_id) else {
            return PluginStatus::default();
        };
        let path = Some(slot.path.clone());
        let has_editor = slot
            .instance
            .plugin_handle()
            .get_extension::<PluginGui>()
            .zip(GuiApiType::default_for_current_platform())
            .is_some_and(|(gui, api_type)| {
                gui.is_api_supported(
                    &mut slot.instance.plugin_handle(),
                    GuiConfiguration {
                        api_type,
                        is_floating: false,
                    },
                )
            });
        PluginStatus { path, has_editor }
    })
    .unwrap_or_default()
}

/// Closes a node's editor window if one is open. Shared with the format hosts,
/// which have to take the window down alongside the instance it belongs to.
pub(super) fn close_editor_window(node_id: &str) {
    if let Some(w) = editor_windows().lock().unwrap().remove(node_id) {
        let _ = w.close();
    }
}

/// Native host windows that plugin editors are embedded into, keyed by node id.
/// `tauri::Window` is `Send + Sync`, so this lives outside the main-thread
/// state and can be created/closed from the command thread.
fn editor_windows() -> &'static Mutex<HashMap<String, tauri::Window>> {
    static WINDOWS: OnceLock<Mutex<HashMap<String, tauri::Window>>> = OnceLock::new();
    WINDOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Sizes the window so its content area (below the title bar) is `w` x `h`
/// logical px. The plugin view fills the content area, so the title bar's
/// height (outer minus inner) is added -- otherwise the bar overlaps the top of
/// the plugin and the bottom gets clipped.
/// Standard title-bar height (logical px) used when the window reports no
/// decoration overhead, which tao does on macOS (`outer_size == inner_size`).
#[cfg(target_os = "macos")]
const TITLEBAR_LOGICAL: f64 = 28.0;
#[cfg(not(target_os = "macos"))]
const TITLEBAR_LOGICAL: f64 = 32.0;

/// Logical px the window decoration takes beyond its content, as (width,
/// height). The content view runs the full height of the window, under the
/// title bar, so this is also how far down a child view must start to clear it.
pub(super) fn decoration_overhead(window: &tauri::Window) -> (f64, f64) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let (dw, measured_dh) = match (window.inner_size(), window.outer_size()) {
        (Ok(inner), Ok(outer)) => (
            outer.width.saturating_sub(inner.width) as f64 / scale,
            outer.height.saturating_sub(inner.height) as f64 / scale,
        ),
        _ => (0.0, 0.0),
    };
    // tao returns outer == inner on macOS, so the measurement is 0; fall back to
    // the platform title-bar height so the plugin renders below the bar.
    let dh = if measured_dh > 0.5 {
        measured_dh
    } else {
        TITLEBAR_LOGICAL
    };
    (dw, dh)
}

fn set_content_size(window: &tauri::Window, w: f64, h: f64) {
    let (dw, dh) = decoration_overhead(window);
    let _ = window.set_size(tauri::LogicalSize::new(w + dw, h + dh));
}

/// Opens the plugin editor embedded in a native host window. The tested plugins
/// only support embedded GUIs (`is_floating=false`), so the host must own the
/// window and hand its native handle to the plugin via `set_parent`.
pub fn open_editor(node_id: &str, title: &str) -> Result<(), String> {
    tracing::debug!(node_id, title, "opening plugin editor");
    let app = crate::app_handle().ok_or("app handle not ready")?;
    if let Some(w) = editor_windows().lock().unwrap().get(node_id) {
        let _ = w.set_focus();
        return Ok(());
    }
    let window = tauri::WindowBuilder::new(app, format!("plugin-editor-{node_id}"))
        .title(if title.is_empty() { "Plugin" } else { title })
        .inner_size(FALLBACK_EDITOR_SIZE.0 as f64, FALLBACK_EDITOR_SIZE.1 as f64)
        // Always resizable with a small floor: even when a plugin reports a bad
        // size or does not reflow, the user can enlarge the window to reveal it.
        .resizable(true)
        .min_inner_size(200.0, 150.0)
        .build()
        .map_err(|e| format!("editor window for {node_id}: {e}"))?;

    let nid = node_id.to_string();
    window.on_window_event(move |ev| {
        // The plugin's view is a child of this window: tear the GUI down before
        // the window goes away, and tell the FE node its editor button is stale.
        if matches!(ev, tauri::WindowEvent::CloseRequested { .. }) {
            // Already the main thread, which is where `destroy_editor` belongs.
            if let Some(host) = super::registry::for_node(&nid) {
                host.destroy_editor(&nid);
            }
            editor_windows().lock().unwrap().remove(&nid);
            if let Some(app) = crate::app_handle() {
                let _ = app.emit(super::host_api::EDITOR_CLOSED_EVENT, &nid);
            }
        }
    });
    editor_windows()
        .lock()
        .unwrap()
        .insert(node_id.to_string(), window.clone());

    let embedded = match super::registry::for_node(node_id) {
        Some(host) => host.embed_editor(node_id, &window),
        None => Err(format!("{node_id}: no plugin is running on this node")),
    };

    // The window exists before the plugin view does, so a failed embed would
    // otherwise leave an empty one on screen and the caller none the wiser.
    let (width, height) = match embedded {
        Ok(size) => size,
        Err(e) => {
            tracing::error!(node_id, error = %e, "plugin editor embed failed");
            editor_windows().lock().unwrap().remove(node_id);
            let _ = window.close();
            return Err(e);
        }
    };
    tracing::debug!(node_id, width, height, "plugin editor embedded");

    set_content_size(&window, width as f64, height as f64);
    Ok(())
}

pub(super) fn embed_editor(node_id: &str, window: tauri::Window) -> Result<EditorSize, String> {
    let node_id = node_id.to_string();
    on_main(move |state| embed(state, &node_id, &window)).and_then(|r| r)
}

fn embed(state: &mut HostState, node_id: &str, window: &tauri::Window) -> Result<EditorSize, String> {
    let slot = state
        .instances
        .get_mut(node_id)
        .ok_or_else(|| format!("clap {node_id}: plugin is not running"))?;
    // Every failure below names the bundle: an editor error that says only what
    // went wrong is unusable when several plugins are in the graph. Cloned
    // rather than borrowed, because the steps need `slot` mutably.
    let path = slot.path.clone();
    tracing::debug!(node_id, %path, serial = slot.serial, "clap editor binding to instance");
    let at = |step: &str, detail: String| format!("clap {path}: {step}: {detail}");
    let gui = slot
        .instance
        .plugin_handle()
        .get_extension::<PluginGui>()
        .ok_or_else(|| at("gui extension", "plugin does not implement it".into()))?;
    let api = GuiApiType::default_for_current_platform()
        .ok_or_else(|| at("gui api", "no GUI API for this platform".into()))?;
    let config = GuiConfiguration {
        api_type: api,
        is_floating: false,
    };
    if !gui.is_api_supported(&mut slot.instance.plugin_handle(), config) {
        return Err(at(
            "is_api_supported",
            "plugin does not support an embedded GUI".into(),
        ));
    }
    if !slot.gui_open {
        tracing::debug!(node_id, %path, "clap gui create");
        gui.create(&mut slot.instance.plugin_handle(), config)
            .map_err(|e| at("gui create", e.to_string()))?;
        slot.gui_open = true;
    }
    // macOS/Cocoa scales for Retina itself, so the scale is always 1 there;
    // on Win32/X11 the plugin needs the real display factor to render sharp.
    #[cfg(target_os = "macos")]
    let scale = 1.0;
    #[cfg(not(target_os = "macos"))]
    let scale = window.scale_factor().unwrap_or(1.0);
    let _ = gui.set_scale(&mut slot.instance.plugin_handle(), scale);

    let handle = window
        .window_handle()
        .map_err(|e| at("window handle", e.to_string()))?;
    let clap_window = ClackWindow::from_window_handle(handle.as_raw())
        .ok_or_else(|| at("window handle", "unsupported handle for this platform".into()))?;
    // SAFETY: `window` outlives the embed; it is parked in `editor_windows`.
    unsafe {
        gui.set_parent(&mut slot.instance.plugin_handle(), clap_window)
            .map_err(|e| at("set_parent", e.to_string()))?;
    }

    let read = |slot: &mut Slot| {
        slot.instance
            .plugin_handle()
            .get_extension::<PluginGui>()
            .and_then(|g| g.get_size(&mut slot.instance.plugin_handle()))
            .and_then(|s| valid_gui_size(s.width, s.height))
    };
    // A pre-show hint lets the plugin lay out; the authoritative size is read
    // after show, since some plugins only finalize (or report 0x0) until then.
    // The embedded window must always be given a size before `show`, even when
    // the plugin reports none: some refuse to show an unsized window.
    let hinted = read(slot).unwrap_or(FALLBACK_EDITOR_SIZE);
    let _ = gui.set_size(
        &mut slot.instance.plugin_handle(),
        GuiSize {
            width: hinted.0,
            height: hinted.1,
        },
    );
    tracing::debug!(node_id, %path, ?hinted, "clap gui show");
    // `set_parent` already made the plugin's view a child of our window, so a
    // refused `show` still leaves a usable editor -- several plugins simply do
    // not implement it. Aborting here would close a window that works.
    if let Err(e) = gui.show(&mut slot.instance.plugin_handle()) {
        tracing::warn!(node_id, %path, error = %e, "plugin refused gui show; view stays parented");
    }
    // A too-small report (e.g. Floe's 150x105 placeholder) is a minimum, not a
    // usable editor size; fall back to a default and push it back so a resizable
    // plugin actually lays out at it instead of a corner.
    let (width, height) = read(slot)
        .filter(|(w, h)| *w >= MIN_USABLE_SIZE.0 && *h >= MIN_USABLE_SIZE.1)
        .unwrap_or(FALLBACK_EDITOR_SIZE);
    let _ = gui.set_size(
        &mut slot.instance.plugin_handle(),
        GuiSize { width, height },
    );
    Ok((width, height))
}

/// Fallback editor size for plugins that report a nonsensical one.
pub(super) const FALLBACK_EDITOR_SIZE: (u32, u32) = (800, 600);
/// Below this a reported size is treated as a placeholder/minimum, not usable.
const MIN_USABLE_SIZE: (u32, u32) = (400, 300);

/// Rejects the degenerate sizes plugins report before their view exists (0x0)
/// or absurd values, so the window is never opened invisibly small or huge.
pub(super) fn valid_gui_size(w: u32, h: u32) -> Option<(u32, u32)> {
    (w >= 100 && h >= 100 && w <= 8000 && h <= 8000).then_some((w, h))
}

fn unembed(state: &mut HostState, node_id: &str) {
    if let Some(slot) = state.instances.get_mut(node_id) {
        if slot.gui_open {
            if let Some(gui) = slot.instance.plugin_handle().get_extension::<PluginGui>() {
                let _ = gui.hide(&mut slot.instance.plugin_handle());
                gui.destroy(&mut slot.instance.plugin_handle());
            }
            slot.gui_open = false;
        }
    }
}

fn unembed_current_thread(node_id: &str) {
    HOST.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            unembed(state, node_id);
        }
    });
}

/// Tears down the plugin editor and closes its native window.
pub fn close_editor(node_id: &str) -> Result<(), String> {
    // The plugin's view is a child of this window, so it goes first -- and it
    // goes on the main thread, which is the one place AppKit and CLAP agree on.
    let nid = node_id.to_string();
    let _ = run_on_main(move || {
        if let Some(host) = super::registry::for_node(&nid) {
            host.destroy_editor(&nid);
        }
    });
    close_editor_window(node_id);
    Ok(())
}

/// CLAP's side of the shared host interface. Holds nothing: the instances live
/// in the main-thread `HOST` thread-local above.
pub struct ClapHost;

impl PluginHost for ClapHost {
    fn activate(&self, req: ActivateRequest<'_>) -> Result<HostedNode, String> {
        activate_clap(
            req.node_id,
            req.path,
            req.plugin_id,
            req.sample_rate,
            req.max_frames,
            req.state.map(str::to_string),
            req.primary,
            req.params,
        )
        .map(HostedNode::Clap)
    }

    // A CLAP slot's lifetime is governed by its `alive` flag and the reclaim
    // sweep, not by a registry entry, so releasing the hold means retiring the
    // instance -- which is exactly what `forget_instance` does.
    fn forget(&self, node_id: &str) {
        forget_instance(node_id);
    }

    fn status(&self, node_id: &str) -> PluginStatus {
        status(node_id)
    }

    fn params(&self, node_id: &str) -> Vec<PluginParamInfo> {
        get_plugin_params(node_id)
    }

    fn save_state(&self, node_id: &str) -> Result<Option<String>, Unsupported> {
        Ok(save_state(node_id))
    }

    // CLAP parameter changes ride in the process call as events, so the plugin
    // sees them itself and there is nothing to announce.
    fn notify_param_changed(
        &self,
        _node_id: &str,
        _param_id: u32,
        _value: f64,
    ) -> Result<(), Unsupported> {
        Err(Unsupported {
            format: PluginFormat::Clap,
            capability: "notify_param_changed",
        })
    }

    fn embed_editor(&self, node_id: &str, window: &tauri::Window) -> Result<EditorSize, String> {
        embed_editor(node_id, window.clone())
    }

    fn destroy_editor(&self, node_id: &str) {
        unembed_current_thread(node_id);
    }

    fn tick_and_reclaim(&self) {
        tick_timers();
        reclaim_dead();
    }
}
