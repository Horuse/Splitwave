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
use std::sync::{mpsc, Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::gui::{
    GuiApiType, GuiConfiguration, GuiSize, HostGui, HostGuiImpl, PluginGui, Window as ClackWindow,
};
use clack_extensions::timer::{HostTimer, HostTimerImpl, PluginTimer, TimerId};
use clack_host::prelude::*;
use raw_window_handle::HasWindowHandle;
use tauri::Emitter;

use super::node::PluginNode;

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

pub struct SplitwaveShared;

impl<'a> SharedHandler<'a> for SplitwaveShared {
    fn request_restart(&self) {}
    fn request_process(&self) {}
    fn request_callback(&self) {}
}

impl HostGuiImpl for SplitwaveShared {
    fn resize_hints_changed(&self) {}
    // The host window is sized to the plugin at embed time; just acknowledge.
    fn request_resize(&self, _new_size: GuiSize) -> Result<(), HostError> {
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
        builder.register::<HostGui>().register::<HostTimer>();
    }
}

struct Slot {
    instance: PluginInstance<SplitwaveHost>,
    timers: Timers,
    gui_open: bool,
}

struct HostState {
    info: HostInfo,
    entries: HashMap<String, PluginEntry>,
    instances: HashMap<String, Slot>,
    // Instances replaced on rebuild are parked here, never dropped: their
    // processor may still be draining in the outgoing DAG, and dropping the
    // instance would destroy the plugin mid-process.
    graveyard: Vec<PluginInstance<SplitwaveHost>>,
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

fn ensure_ticker() {
    static TICKER: Once = Once::new();
    TICKER.call_once(|| {
        std::thread::Builder::new()
            .name("plugin-timer".into())
            .spawn(|| loop {
                std::thread::sleep(Duration::from_millis(16));
                if let Some(app) = crate::app_handle() {
                    let _ = app.run_on_main_thread(tick_timers);
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
/// node. Safe to call from any non-main thread.
pub fn activate_clap(
    node_id: &str,
    path: &str,
    plugin_id: &str,
    sample_rate: u32,
    max_frames: usize,
) -> Result<PluginNode, String> {
    ensure_ticker();
    let node_id = node_id.to_string();
    let path = path.to_string();
    let plugin_id = plugin_id.to_string();
    on_main(move |state| -> Result<PluginNode, String> {
        if !state.entries.contains_key(&path) {
            let entry =
                unsafe { PluginEntry::load(&path) }.map_err(|e| format!("load {path}: {e}"))?;
            state.entries.insert(path.clone(), entry);
        }
        let entry = state.entries.get(&path).expect("entry just inserted");
        let id = CString::new(plugin_id.clone()).map_err(|e| e.to_string())?;

        let timers: Timers = Rc::new(RefCell::new(TimerState::default()));
        let timers_for_handler = timers.clone();
        let mut instance = PluginInstance::<SplitwaveHost>::new(
            |_| SplitwaveShared,
            move |_| SplitwaveMainThread {
                timers: timers_for_handler,
            },
            entry,
            &id,
            &state.info,
        )
        .map_err(|e| format!("instantiate {plugin_id}: {e}"))?;

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

        let node = PluginNode::new(processor, &input_channels, &output_channels, max_frames);

        if let Some(mut old) = state.instances.remove(&node_id) {
            if old.gui_open {
                if let Some(gui) = old.instance.plugin_handle().get_extension::<PluginGui>() {
                    gui.destroy(&mut old.instance.plugin_handle());
                }
            }
            state.graveyard.push(old.instance);
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
                timers,
                gui_open: false,
            },
        );
        Ok(node)
    })
    .and_then(|r| r)
}

/// Native host windows that plugin editors are embedded into, keyed by node id.
/// `tauri::Window` is `Send + Sync`, so this lives outside the main-thread
/// state and can be created/closed from the command thread.
fn editor_windows() -> &'static Mutex<HashMap<String, tauri::Window>> {
    static WINDOWS: OnceLock<Mutex<HashMap<String, tauri::Window>>> = OnceLock::new();
    WINDOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Opens the plugin editor embedded in a native host window. The tested plugins
/// only support embedded GUIs (`is_floating=false`), so the host must own the
/// window and hand its native handle to the plugin via `set_parent`.
pub fn open_editor(node_id: &str, title: &str) -> Result<(), String> {
    let app = crate::app_handle().ok_or("app handle not ready")?;
    if let Some(w) = editor_windows().lock().unwrap().get(node_id) {
        let _ = w.set_focus();
        return Ok(());
    }
    let window = tauri::WindowBuilder::new(app, format!("plugin-editor-{node_id}"))
        .title(if title.is_empty() { "Plugin" } else { title })
        .inner_size(400.0, 300.0)
        .build()
        .map_err(|e| e.to_string())?;

    let nid = node_id.to_string();
    window.on_window_event(move |ev| {
        // The plugin's view is a child of this window: tear the GUI down before
        // the window goes away, and tell the FE node its editor button is stale.
        if matches!(ev, tauri::WindowEvent::CloseRequested { .. }) {
            unembed_current_thread(&nid);
            editor_windows().lock().unwrap().remove(&nid);
            if let Some(app) = crate::app_handle() {
                let _ = app.emit(EDITOR_CLOSED_EVENT, &nid);
            }
        }
    });
    editor_windows()
        .lock()
        .unwrap()
        .insert(node_id.to_string(), window.clone());

    let info = embed_editor(node_id, window.clone())?;
    // The plugin owns its size; only let the user resize when it says it can.
    let _ = window.set_resizable(info.resizable);
    let _ = window.set_size(tauri::LogicalSize::new(info.width as f64, info.height as f64));
    Ok(())
}

/// Emitted with the node id when a plugin editor window is closed via its
/// titlebar, so the FE node can reset its open/close button.
pub const EDITOR_CLOSED_EVENT: &str = "plugin://editor-closed";

struct EmbedInfo {
    width: u32,
    height: u32,
    resizable: bool,
}

fn embed_editor(node_id: &str, window: tauri::Window) -> Result<EmbedInfo, String> {
    let node_id = node_id.to_string();
    on_main(move |state| embed(state, &node_id, &window)).and_then(|r| r)
}

fn embed(state: &mut HostState, node_id: &str, window: &tauri::Window) -> Result<EmbedInfo, String> {
    let slot = state
        .instances
        .get_mut(node_id)
        .ok_or("plugin is not running")?;
    let gui = slot
        .instance
        .plugin_handle()
        .get_extension::<PluginGui>()
        .ok_or("plugin has no GUI")?;
    let api = GuiApiType::default_for_current_platform().ok_or("no GUI API for this platform")?;
    let config = GuiConfiguration {
        api_type: api,
        is_floating: false,
    };
    if !gui.is_api_supported(&mut slot.instance.plugin_handle(), config) {
        return Err("plugin does not support an embedded GUI".into());
    }
    if !slot.gui_open {
        gui.create(&mut slot.instance.plugin_handle(), config)
            .map_err(|e| format!("gui create: {e}"))?;
        slot.gui_open = true;
    }
    let _ = gui.set_scale(&mut slot.instance.plugin_handle(), 1.0);

    let handle = window.window_handle().map_err(|e| e.to_string())?;
    let clap_window =
        ClackWindow::from_window_handle(handle.as_raw()).ok_or("unsupported window handle")?;
    // SAFETY: `window` outlives the embed; it is parked in `editor_windows`.
    unsafe {
        gui.set_parent(&mut slot.instance.plugin_handle(), clap_window)
            .map_err(|e| format!("set_parent: {e}"))?;
    }

    let resizable = gui.can_resize(&mut slot.instance.plugin_handle());
    let size = gui
        .get_size(&mut slot.instance.plugin_handle())
        .map(|s| (s.width, s.height))
        .unwrap_or((400, 300));
    let _ = gui.set_size(
        &mut slot.instance.plugin_handle(),
        GuiSize {
            width: size.0,
            height: size.1,
        },
    );
    gui.show(&mut slot.instance.plugin_handle())
        .map_err(|e| format!("gui show: {e}"))?;
    Ok(EmbedInfo {
        width: size.0,
        height: size.1,
        resizable,
    })
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
    let nid = node_id.to_string();
    let _ = on_main(move |state| unembed(state, &nid));
    if let Some(w) = editor_windows().lock().unwrap().remove(node_id) {
        let _ = w.close();
    }
    Ok(())
}
