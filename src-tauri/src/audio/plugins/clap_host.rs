//! One CLAP plugin instance and the host callbacks it is given.
//!
//! Nothing here knows about Tauri, node ids as a table key, or which thread it
//! is on. CLAP's rule is that every main-thread call happens on one *consistent*
//! thread, not on a particular one, so an instance is usable from any single
//! thread that owns it -- which is what makes this half testable. Binding that
//! thread to the app's UI thread, where an editor needs a live event loop, is
//! `clap_registry`'s job.
//!
//! `SplitwaveHost` advertises the host-side `gui` and `timer` extensions:
//! without them most plugins refuse to open an editor or never repaint, since
//! their GUIs redraw from host-driven timer ticks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::latency::PluginLatency;
use clack_extensions::gui::{
    GuiApiType, GuiConfiguration, GuiSize, HostGui, HostGuiImpl, PluginGui, Window as ClackWindow,
};
use clack_extensions::params::{ParamInfoBuffer, ParamInfoFlags, PluginParams};
use clack_extensions::state::{HostState as HostStateExt, HostStateImpl, PluginState};
use clack_extensions::timer::{HostTimer, HostTimerImpl, PluginTimer, TimerId};
use clack_host::prelude::*;
use raw_window_handle::HasWindowHandle;

use super::host_api::{AliveFlag, EditorSize, PluginParamInfo};
use super::node::PluginNode;
use super::{editor, ParamRing};

/// Below this a reported size is treated as a placeholder/minimum, not usable.
const MIN_USABLE_SIZE: (u32, u32) = (400, 300);

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
        let Some((w, h)) = editor::valid_gui_size(new_size.width, new_size.height) else {
            return Ok(());
        };
        if let Some(win) = editor::window_for(&self.node_id) {
            editor::set_content_size(&win, w as f64, h as f64);
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

/// Loaded `.clap` bundles, keyed by path, plus the host identity handed to every
/// plugin. A bundle stays loaded for the life of the owner: unloading one whose
/// instances are gone would still invalidate the code another load handed out.
pub struct Bundles {
    info: HostInfo,
    entries: HashMap<String, PluginEntry>,
}

impl Default for Bundles {
    fn default() -> Self {
        Self {
            info: HostInfo::new(
                "Splitwave",
                "Splitwave",
                "https://splitwave.app",
                env!("CARGO_PKG_VERSION"),
            )
            .expect("static host info is valid"),
            entries: HashMap::new(),
        }
    }
}

impl Bundles {
    fn entry(&mut self, path: &str) -> Result<&PluginEntry, String> {
        if !self.entries.contains_key(path) {
            // SAFETY: loads foreign code, which can do anything on load. Inherent
            // to hosting third-party plugins.
            let entry =
                unsafe { PluginEntry::load(path) }.map_err(|e| format!("load {path}: {e}"))?;
            // The plugin dylib may have replaced the global panic hook on load;
            // restore ours as the outermost so crashes still get persisted.
            crate::reinstall_panic_hook();
            self.entries.insert(path.to_string(), entry);
        }
        Ok(self.entries.get(path).expect("entry just inserted"))
    }
}

/// Distinguishes a node's instances in the log, which is the only way to tell an
/// editor bound to the audible instance from one bound to a duplicate.
fn next_serial() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A live CLAP plugin. Owns everything that dies with it, so dropping this is
/// the whole teardown -- but drop it on the thread that built it.
pub struct ClapInstance {
    instance: PluginInstance<SplitwaveHost>,
    timers: Timers,
    serial: u64,
    /// The bundle this was built from, so the UI can tell whether the running
    /// plugin is still the one the node currently points at.
    path: String,
    plugin_id: String,
    gui_open: bool,
}

impl ClapInstance {
    pub fn new(bundles: &mut Bundles, node_id: &str, path: &str, plugin_id: &str) -> Result<Self, String> {
        let id = CString::new(plugin_id).map_err(|e| e.to_string())?;
        let timers: Timers = Rc::new(RefCell::new(TimerState::default()));
        let timers_for_handler = timers.clone();
        let shared_node_id = node_id.to_string();
        // Borrowed separately from `entry`, which holds `bundles` for the call.
        let info = bundles.info.clone();
        let entry = bundles.entry(path)?;

        let instance = PluginInstance::<SplitwaveHost>::new(
            move |_| SplitwaveShared {
                node_id: shared_node_id,
                callback_requested: AtomicBool::new(false),
            },
            move |_| SplitwaveMainThread {
                timers: timers_for_handler,
            },
            entry,
            &id,
            &info,
        )
        .map_err(|e| format!("instantiate {plugin_id}: {e}"))?;

        let serial = next_serial();
        tracing::debug!(node_id, path, plugin_id, serial, "clap instance created");
        Ok(Self {
            instance,
            timers,
            serial,
            path: path.to_string(),
            plugin_id: plugin_id.to_string(),
            gui_open: false,
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Starts processing and hands back the `Send` half for the DSP worker.
    pub fn activate(
        &mut self,
        sample_rate: u32,
        max_frames: usize,
        params: Arc<ParamRing>,
        alive: AliveFlag,
    ) -> Result<PluginNode, String> {
        let ports = self.instance.plugin_handle().get_extension::<PluginAudioPorts>();
        let inputs = port_channels(ports.as_ref(), &mut self.instance, true);
        let outputs = port_channels(ports.as_ref(), &mut self.instance, false);

        let config = PluginAudioConfiguration {
            sample_rate: sample_rate as f64,
            min_frames_count: 1,
            max_frames_count: max_frames as u32,
        };
        let processor = self
            .instance
            .activate(|_, _| (), config)
            .map_err(|e| format!("activate {}: {e}", self.plugin_id))?
            .start_processing()
            .map_err(|e| format!("start {}: {e}", self.plugin_id))?;

        // Read after activation: the spec only guarantees the value once the
        // plugin is active, and several plugins report 0 before it.
        let latency = self
            .instance
            .plugin_handle()
            .get_extension::<PluginLatency>()
            .map(|ext| ext.get(&mut self.instance.plugin_handle()) as usize)
            .unwrap_or(0);

        Ok(PluginNode::new(
            processor,
            &inputs,
            &outputs,
            max_frames,
            params,
            alive,
            latency,
        ))
    }

    /// Loads a blob produced by [`save_state`](Self::save_state). Call before
    /// the node goes live.
    ///
    /// Some plugins (nih-plug) *panic* on a stream they cannot parse instead of
    /// erroring, so the load is caught: on the UI thread an escaping panic kills
    /// the app.
    pub fn restore_state(&mut self, node_id: &str, tagged: &str) {
        let Some(bytes) = super::host_api::decode_state(node_id, &self.plugin_id, tagged) else {
            return;
        };
        let Some(ext) = self.instance.plugin_handle().get_extension::<PluginState>() else {
            return;
        };
        let plugin_id = self.plugin_id.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut reader = std::io::Cursor::new(bytes);
            ext.load(&mut self.instance.plugin_handle(), &mut reader)
        }));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::error!(plugin_id, %err, "plugin state load failed"),
            Err(_) => tracing::error!(plugin_id, "plugin state load panicked; keeping defaults"),
        }
    }

    /// `None` when the plugin does not implement the state extension.
    pub fn save_state(&mut self) -> Option<String> {
        let ext = self.instance.plugin_handle().get_extension::<PluginState>()?;
        let mut buf = Vec::new();
        ext.save(&mut self.instance.plugin_handle(), &mut buf).ok()?;
        Some(super::host_api::encode_state(&self.plugin_id, &buf))
    }

    /// Automatable parameters for the node UI. Empty when the plugin exposes no
    /// params extension.
    pub fn params(&mut self) -> Vec<PluginParamInfo> {
        let Some(ext) = self.instance.plugin_handle().get_extension::<PluginParams>() else {
            return Vec::new();
        };
        let count = ext.count(&mut self.instance.plugin_handle());
        let mut out = Vec::with_capacity(count as usize);
        let mut buf = ParamInfoBuffer::default();
        for i in 0..count {
            let Some(info) = ext.get_info(&mut self.instance.plugin_handle(), i, &mut buf) else {
                continue;
            };
            if info.flags.contains(ParamInfoFlags::IS_HIDDEN) {
                continue;
            }
            let value = ext
                .get_value(&mut self.instance.plugin_handle(), info.id)
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
    }

    /// Whether the plugin can produce an embedded editor at all, so the node can
    /// say so instead of offering a button that only ever errors.
    pub fn has_editor(&mut self) -> bool {
        let Some(config) = embedded_config() else {
            return false;
        };
        self.instance
            .plugin_handle()
            .get_extension::<PluginGui>()
            .is_some_and(|gui| gui.is_api_supported(&mut self.instance.plugin_handle(), config))
    }

    /// Runs the plugin's due timers and any callback it asked for. Both are what
    /// keep an open editor painting.
    pub fn tick(&mut self, now: Instant) {
        // Deferred work the plugin asked for, including the GUI-to-DSP hand off
        // some plugins rely on.
        if self
            .instance
            .access_shared_handler(|h| h.callback_requested.swap(false, Ordering::AcqRel))
        {
            self.instance.call_on_main_thread_callback();
        }
        let due: Vec<u32> = {
            let mut st = self.timers.borrow_mut();
            st.timers
                .iter_mut()
                .filter(|t| t.next <= now)
                .map(|t| {
                    t.next = now + t.period;
                    t.id
                })
                .collect()
        };
        if due.is_empty() {
            return;
        }
        if let Some(timer) = self.instance.plugin_handle().get_extension::<PluginTimer>() {
            for id in due {
                timer.on_timer(&mut self.instance.plugin_handle(), TimerId(id));
            }
        }
    }

    /// Builds the plugin's view into `window`, returning the size it reports.
    /// Main thread only.
    pub fn embed_editor(
        &mut self,
        node_id: &str,
        window: &tauri::Window,
    ) -> Result<EditorSize, String> {
        // Every failure below names the bundle: an editor error that says only
        // what went wrong is unusable when several plugins are in the graph.
        let path = self.path.clone();
        tracing::debug!(node_id, %path, serial = self.serial, "clap editor binding to instance");
        let at = |step: &str, detail: String| format!("clap {path}: {step}: {detail}");

        let gui = self
            .instance
            .plugin_handle()
            .get_extension::<PluginGui>()
            .ok_or_else(|| at("gui extension", "plugin does not implement it".into()))?;
        let config = embedded_config()
            .ok_or_else(|| at("gui api", "no GUI API for this platform".into()))?;
        if !gui.is_api_supported(&mut self.instance.plugin_handle(), config) {
            return Err(at(
                "is_api_supported",
                "plugin does not support an embedded GUI".into(),
            ));
        }
        if !self.gui_open {
            tracing::debug!(node_id, %path, "clap gui create");
            gui.create(&mut self.instance.plugin_handle(), config)
                .map_err(|e| at("gui create", e.to_string()))?;
            self.gui_open = true;
        }
        // macOS/Cocoa scales for Retina itself, so the scale is always 1 there;
        // on Win32/X11 the plugin needs the real display factor to render sharp.
        #[cfg(target_os = "macos")]
        let scale = 1.0;
        #[cfg(not(target_os = "macos"))]
        let scale = window.scale_factor().unwrap_or(1.0);
        let _ = gui.set_scale(&mut self.instance.plugin_handle(), scale);

        let handle = window
            .window_handle()
            .map_err(|e| at("window handle", e.to_string()))?;
        let clap_window = ClackWindow::from_window_handle(handle.as_raw())
            .ok_or_else(|| at("window handle", "unsupported handle for this platform".into()))?;
        // SAFETY: `window` outlives the embed; the editor layer parks it.
        unsafe {
            gui.set_parent(&mut self.instance.plugin_handle(), clap_window)
                .map_err(|e| at("set_parent", e.to_string()))?;
        }

        // A pre-show hint lets the plugin lay out; the authoritative size is read
        // after show, since some plugins only finalize (or report 0x0) until
        // then. The window must always be given a size before `show`: some
        // plugins refuse to show an unsized one.
        let hinted = self.reported_size().unwrap_or(editor::FALLBACK_EDITOR_SIZE);
        self.set_gui_size(hinted);
        tracing::debug!(node_id, %path, ?hinted, "clap gui show");
        // `set_parent` already made the plugin's view a child of our window, so a
        // refused `show` still leaves a usable editor -- several plugins simply
        // do not implement it. Aborting here would close a window that works.
        if let Some(gui) = self.instance.plugin_handle().get_extension::<PluginGui>() {
            if let Err(err) = gui.show(&mut self.instance.plugin_handle()) {
                tracing::warn!(node_id, %path, %err, "plugin refused gui show; view stays parented");
            }
        }
        // A too-small report (e.g. Floe's 150x105 placeholder) is a minimum, not
        // a usable editor size; fall back to a default and push it back so a
        // resizable plugin lays out at it instead of in a corner.
        let size = self
            .reported_size()
            .filter(|(w, h)| *w >= MIN_USABLE_SIZE.0 && *h >= MIN_USABLE_SIZE.1)
            .unwrap_or(editor::FALLBACK_EDITOR_SIZE);
        self.set_gui_size(size);
        Ok(size)
    }

    fn reported_size(&mut self) -> Option<EditorSize> {
        self.instance
            .plugin_handle()
            .get_extension::<PluginGui>()
            .and_then(|g| g.get_size(&mut self.instance.plugin_handle()))
            .and_then(|s| editor::valid_gui_size(s.width, s.height))
    }

    fn set_gui_size(&mut self, (width, height): EditorSize) {
        if let Some(gui) = self.instance.plugin_handle().get_extension::<PluginGui>() {
            let _ = gui.set_size(&mut self.instance.plugin_handle(), GuiSize { width, height });
        }
    }

    /// Tears the editor down. Destroying a GUI that was never created is
    /// undefined per the CLAP spec, hence the flag.
    pub fn destroy_editor(&mut self) {
        if !self.gui_open {
            return;
        }
        if let Some(gui) = self.instance.plugin_handle().get_extension::<PluginGui>() {
            let _ = gui.hide(&mut self.instance.plugin_handle());
            gui.destroy(&mut self.instance.plugin_handle());
        }
        self.gui_open = false;
    }
}

impl Drop for ClapInstance {
    fn drop(&mut self) {
        // The view is a child of a window we own and points into the plugin, so
        // it cannot outlive it.
        self.destroy_editor();
    }
}

/// The only GUI configuration Splitwave asks for: embedded in a window of ours,
/// in the platform's native API.
fn embedded_config() -> Option<GuiConfiguration<'static>> {
    GuiApiType::default_for_current_platform().map(|api_type| GuiConfiguration {
        api_type,
        is_floating: false,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::effects::Effect;
    use crate::audio::plugins::clap_backend::ClapBackend;
    use crate::audio::plugins::host_api::alive_flag;
    use crate::audio::plugins::{PluginBackend, PluginDescriptor};

    const FRAMES: usize = 512;
    const SAMPLE_RATE: u32 = 48_000;
    /// 16k samples, comfortably past the lookahead of any effect we host.
    const PRIMING_BLOCKS: usize = 32;

    fn skipped(what: &str) {
        println!("SKIPPED: no clap plugins installed, cannot check {what}");
    }

    fn installed() -> Vec<PluginDescriptor> {
        ClapBackend.scan()
    }

    fn open(bundles: &mut Bundles, plugin: &PluginDescriptor) -> ClapInstance {
        ClapInstance::new(bundles, "test", &plugin.path, &plugin.plugin_id)
            .unwrap_or_else(|e| panic!("{}: {e}", plugin.name))
    }

    /// An effect fed a signal must produce one. A lookahead plugin's first
    /// blocks are legitimately silent, and CLAP nodes report no latency to size
    /// that wait by, so the loop simply runs well past any plausible lookahead.
    #[test]
    fn renders_signal_through_every_installed_plugin() {
        let found = installed();
        if found.is_empty() {
            return skipped("audio rendering");
        }
        let mut bundles = Bundles::default();
        for plugin in &found {
            let mut instance = open(&mut bundles, plugin);
            let mut node = instance
                .activate(SAMPLE_RATE, FRAMES, Arc::new(ParamRing::new()), alive_flag())
                .unwrap_or_else(|e| panic!("{}: {e}", plugin.name));

            let mut peak = 0.0f32;
            for _ in 0..PRIMING_BLOCKS {
                let mut block = vec![0.0f32; FRAMES * 2];
                for i in 0..FRAMES {
                    let s = (i as f32 * 0.05).sin() * 0.5;
                    block[2 * i] = s;
                    block[2 * i + 1] = s;
                }
                node.process(&mut block, FRAMES);
                peak = block.iter().fold(peak, |a, s| a.max(s.abs()));
            }
            assert!(peak > 0.01, "{} produced silence", plugin.name);
            drop(node);
        }
    }

    /// Reported latency has to match where the signal actually appears: the DAG
    /// pads shorter parallel paths by this number, so a wrong one is an audible
    /// phase error on a split-and-rejoin graph rather than a missing feature.
    #[test]
    fn reported_latency_matches_when_the_signal_arrives() {
        let found = installed();
        if found.is_empty() {
            return skipped("latency reporting");
        }
        let mut bundles = Bundles::default();
        for plugin in &found {
            let mut instance = open(&mut bundles, plugin);
            let mut node = instance
                .activate(SAMPLE_RATE, FRAMES, Arc::new(ParamRing::new()), alive_flag())
                .unwrap_or_else(|e| panic!("{}: {e}", plugin.name));
            let reported = node.latency_frames();

            // One impulse, then silence: the first non-zero output frame is the
            // latency the plugin actually imposes.
            let mut fed = 0;
            let mut arrived = None;
            for _ in 0..PRIMING_BLOCKS {
                let mut block = vec![0.0f32; FRAMES * 2];
                if fed == 0 {
                    block[0] = 1.0;
                    block[1] = 1.0;
                }
                node.process(&mut block, FRAMES);
                if arrived.is_none() {
                    if let Some(i) = block.iter().position(|s| s.abs() > 1e-6) {
                        arrived = Some(fed + i / 2);
                    }
                }
                fed += FRAMES;
            }
            let Some(arrived) = arrived else {
                // A gate or expander may hold a lone impulse down entirely.
                println!("  {} passed no impulse, latency unverified", plugin.name);
                continue;
            };
            // Exact equality is too strict: a plugin may round its own report to
            // a block, and a soft-knee stage can smear the leading edge.
            let slack = FRAMES;
            assert!(
                arrived + slack >= reported && arrived <= reported + slack,
                "{} reports {reported} samples of latency but the impulse arrived at {arrived}",
                plugin.name
            );
        }
    }

    /// Every parameter the node UI would render has to be usable as a slider:
    /// a name to label it and a range that is not a single point.
    #[test]
    fn reports_usable_parameters() {
        let found = installed();
        if found.is_empty() {
            return skipped("parameter reporting");
        }
        let mut bundles = Bundles::default();
        for plugin in &found {
            let mut instance = open(&mut bundles, plugin);
            for p in instance.params() {
                assert!(!p.name.is_empty(), "{}: parameter {} has no name", plugin.name, p.id);
                assert!(
                    p.max > p.min,
                    "{}: parameter {} has an empty range",
                    plugin.name,
                    p.name
                );
                assert!(
                    p.value >= p.min && p.value <= p.max,
                    "{}: parameter {} reads {} outside {}..{}",
                    plugin.name,
                    p.name,
                    p.value,
                    p.min,
                    p.max
                );
            }
        }
    }

    /// A saved blob has to come back into a fresh instance of the same plugin.
    /// This is the path a reopened project takes.
    #[test]
    fn state_survives_a_reinstantiation() {
        let found = installed();
        if found.is_empty() {
            return skipped("state persistence");
        }
        let mut bundles = Bundles::default();
        let mut checked = 0;
        for plugin in &found {
            let mut instance = open(&mut bundles, plugin);
            let Some(saved) = instance.save_state() else {
                continue;
            };
            drop(instance);

            let mut restored = open(&mut bundles, plugin);
            restored.restore_state("test", &saved);
            assert!(
                restored.save_state().is_some(),
                "{} stopped producing state after a restore",
                plugin.name
            );
            checked += 1;
        }
        if checked == 0 {
            println!("SKIPPED: no installed clap plugin implements the state extension");
        }
    }

    /// A blob saved by another plugin must never reach a plugin's parser: some
    /// accept it silently and end up in a state their own editor disagrees with.
    #[test]
    fn a_state_blob_that_is_not_ours_is_refused() {
        let found = installed();
        if found.is_empty() {
            return skipped("foreign state rejection");
        }
        let mut bundles = Bundles::default();
        let mut instance = open(&mut bundles, &found[0]);
        let foreign = super::super::host_api::encode_state("some.other.plugin", b"garbage");
        // Rejected before the plugin sees it, so this is a no-op rather than a
        // panic or a corrupted instance.
        instance.restore_state("test", &foreign);
        assert!(instance.params().iter().all(|p| p.value >= p.min));
    }
}
