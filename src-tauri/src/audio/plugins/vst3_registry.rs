//! The VST3 implementation of `PluginHost`.
//!
//! VST3 puts every call except `process` on the host's UI thread, so instances
//! live in a `thread_local` on the Tauri main thread and every method marshals
//! there and blocks. Same shape as CLAP, and for the same reason: a plugin
//! initialised off the main thread is free to misbehave, and some do.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Arc;

use tauri::Emitter;

use super::host_api::{
    alive_flag, tag_state, untag_state, ActivateRequest, AliveFlag, EditorSize, Graveyard,
    HostedNode, PluginHost, PluginParamInfo, PluginStatus, Unsupported,
};
use super::vst3_backend::Vst3Module;
use super::vst3_com::EditListener;
use super::vst3_editor::EditorView;
use super::vst3_host::Vst3Instance;
use super::{editor, main_thread, ParamRing, PluginFormat};

/// A node's live plugin. Only the main thread ever touches one.
struct Slot {
    instance: Vst3Instance,
    alive: AliveFlag,
    path: String,
    plugin_id: String,
    editor: Option<EditorView>,
}

thread_local! {
    static SLOTS: RefCell<HashMap<String, Slot>> = RefCell::new(HashMap::new());
    /// Instances whose node is gone but whose plugin has not been freed yet.
    /// Metering duplicates and extra stereo pairs land here directly.
    static GRAVEYARD: RefCell<Graveyard<Vst3Instance>> = RefCell::new(Graveyard::default());
}

/// Carries edits made inside the plugin's own window to the audio thread. The
/// ring is the only path from the editor to the processor.
struct ToRing(Arc<ParamRing>);

impl EditListener for ToRing {
    fn param_edited(&self, id: u32, value: f64) {
        self.0.push(id, value);
    }

    fn restart(&self, flags: i32) {
        // Latency and bus changes would need the graph rebuilt around the
        // plugin, which we do not do yet; saying so beats a silent wrong mix.
        tracing::warn!(flags, "vst3: plugin asked for a restart, ignored");
    }
}

/// Runs `f` on the main thread with the slot table, and blocks. Returns the
/// closure's own `None` when the node runs no VST3 plugin.
fn with_slot<R: Send + 'static>(
    node_id: &str,
    f: impl FnOnce(&mut Slot) -> R + Send + 'static,
) -> Option<R> {
    let id = node_id.to_string();
    main_thread::run(move || SLOTS.with(|s| s.borrow_mut().get_mut(&id).map(f)))
        .ok()
        .flatten()
}

fn activate_on_main(
    node_id: String,
    path: String,
    plugin_id: String,
    sample_rate: u32,
    max_frames: usize,
    channels: usize,
    state: Option<String>,
    primary: bool,
    params: Arc<ParamRing>,
) -> Result<super::vst3_node::Vst3Node, String> {
    let at = |step: &str| format!("vst3 {node_id} [{path}]: {step}");

    let module = Vst3Module::open(std::path::Path::new(&path)).map_err(|e| at(&e))?;
    let mut instance = Vst3Instance::new(module, &plugin_id).map_err(|e| at(&e))?;

    // Before activation: a plugin may size its buffers to what it reads.
    if let Some(blob) = state.as_deref().and_then(|s| untag_state(&plugin_id, s)) {
        if let Err(err) = instance.restore_state(blob) {
            tracing::warn!(node_id, path, %err, "vst3: saved state rejected, using defaults");
        }
    }

    instance.listen(Box::new(ToRing(params.clone())));

    let alive = alive_flag();
    let node = instance
        .activate(sample_rate, max_frames, channels, params, alive.clone())
        .map_err(|e| at(&e))?;

    if primary {
        // A rebuild that keeps this node_id re-activates it before the RT
        // graph has swapped away from the old one, so the old instance's RT
        // node may still be mid-`process()` on the DSP worker. Dropping it
        // here would run `terminate`/`setActive(0)` on that same COM object
        // out from under the RT call -- bury it like `forget` does instead,
        // and let `tick_and_reclaim` free it once its `alive` flag is clear.
        let old = SLOTS.with(|s| {
            s.borrow_mut().insert(
                node_id.clone(),
                Slot {
                    instance,
                    alive,
                    path: path.clone(),
                    plugin_id: plugin_id.clone(),
                    editor: None,
                },
            )
        });
        if let Some(mut old) = old {
            let same_plugin = old.path == path && old.plugin_id == plugin_id;
            GRAVEYARD.with(|g| g.borrow_mut().bury(old.instance, old.alive));

            if same_plugin {
                // Same plugin, pipeline rebuilt: re-attach to existing window!
                if let Some(window) = editor::window_for(&node_id) {
                    // IMPORTANT: Drop the old editor view first so its removed() and
                    // peer teardown happen BEFORE the new view calls attached()!
                    old.editor = None;

                    let attached = SLOTS.with(|s| {
                        if let Some(slot) = s.borrow_mut().get_mut(&node_id) {
                            attach_slot_editor(slot, &node_id, &window)
                        } else {
                            Err("slot missing".into())
                        }
                    });

                    match attached {
                        Ok(size) => {
                            let (width, height) = editor::valid_gui_size(size.0, size.1)
                                .unwrap_or(editor::FALLBACK_EDITOR_SIZE);
                            editor::set_content_size(&window, width as f64, height as f64);
                            SLOTS.with(|s| {
                                if let Some(slot) = s.borrow().get(&node_id) {
                                    if let Some(ref ed) = slot.editor {
                                        ed.on_size(width, height);
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            tracing::warn!(node_id, error = %e, "failed to re-attach VST3 editor on rebuild; closing window");
                            editor::close_window(&node_id);
                            if let Some(app) = crate::app_handle() {
                                let _ = app.emit(super::host_api::EDITOR_CLOSED_EVENT, &node_id);
                            }
                        }
                    }
                }
            } else {
                // Different plugin chosen on this node: close the previous editor
                // window so the new one opens cleanly with its own UI and geometry.
                old.editor = None;
                editor::close_window(&node_id);
                if let Some(app) = crate::app_handle() {
                    let _ = app.emit(super::host_api::EDITOR_CLOSED_EVENT, &node_id);
                }
            }
        }
    } else {
        // A metering duplicate has no editor and no parameters to answer for;
        // it only has to outlive its node.
        GRAVEYARD.with(|g| g.borrow_mut().bury(instance, alive));
    }
    Ok(node)
}

fn attach_slot_editor(
    slot: &mut Slot,
    node_id: &str,
    window: &tauri::Window,
) -> Result<EditorSize, String> {
    let view_addr = parent_handle(window).map_err(|e| format!("vst3 {node_id}: {e}"))?;
    let (_, titlebar) = editor::decoration_overhead(window);
    let resize_target = window.clone();

    // Cleanly drop any previous editor view before attaching the new one.
    slot.editor = None;

    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        let parent_obj = view_addr as *mut AnyObject;
        let subviews: *mut AnyObject = msg_send![parent_obj, subviews];
        let count: usize = msg_send![subviews, count];
        for i in (0..count).rev() {
            let sv: *mut AnyObject = msg_send![subviews, objectAtIndex: i];
            let _: () = msg_send![sv, removeFromSuperview];
        }
    }

    let view = slot
        .instance
        .take_view()
        .ok_or_else(|| format!("vst3 {node_id}: plugin has no editor"))?;

    let resize = Box::new(move |w: u32, h: u32| {
        let _ = resize_target.set_size(tauri::LogicalSize::new(w as f64, h as f64));
    });
    let (view, size) = EditorView::attach(view, view_addr as *mut c_void, titlebar, resize)
        .map_err(|e| format!("vst3 {node_id}: {e}"))?;

    slot.editor = Some(view);
    Ok(size)
}

/// The window a plugin view is parented to, as the address VST3 expects for
/// this platform's `attached` type.
fn parent_handle(window: &tauri::Window) -> Result<usize, String> {
    #[cfg(target_os = "macos")]
    {
        window
            .ns_view()
            .map(|view| view as usize)
            .map_err(|e| format!("content view: {e}"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let handle = window
            .window_handle()
            .map_err(|e| format!("window handle: {e}"))?;
        match handle.as_raw() {
            RawWindowHandle::Win32(h) => Ok(h.hwnd.get() as usize),
            RawWindowHandle::Xlib(h) => Ok(h.window as usize),
            RawWindowHandle::Xcb(h) => Ok(h.window.get() as usize),
            // VST3 defines no Wayland platform type, so there is nothing to
            // parent into; running the session under XWayland is the way out.
            other => Err(format!(
                "VST3 editors need an X11 or Win32 window, got {other:?}"
            )),
        }
    }
}

pub struct Vst3Host;

impl PluginHost for Vst3Host {
    fn activate(&self, req: ActivateRequest<'_>) -> Result<HostedNode, String> {
        let (node_id, path, plugin_id) = (
            req.node_id.to_string(),
            req.path.to_string(),
            req.plugin_id.to_string(),
        );
        let state = req.state.map(str::to_string);
        let (sample_rate, max_frames, channels, primary, params) = (
            req.sample_rate,
            req.max_frames,
            req.channels,
            req.primary,
            req.params,
        );

        main_thread::run(move || {
            activate_on_main(
                node_id,
                path,
                plugin_id,
                sample_rate,
                max_frames,
                channels,
                state,
                primary,
                params,
            )
        })?
        .map(HostedNode::Vst3)
    }

    fn forget(&self, node_id: &str) {
        let id = node_id.to_string();
        let _ = main_thread::run(move || {
            let slot = SLOTS.with(|s| s.borrow_mut().remove(&id));
            // The instance outlives this: its RT node may still be in a graph
            // that has not been swapped out yet.
            if let Some(slot) = slot {
                GRAVEYARD.with(|g| g.borrow_mut().bury(slot.instance, slot.alive));
            }
        });
    }

    fn status(&self, node_id: &str) -> PluginStatus {
        with_slot(node_id, |slot| PluginStatus {
            path: Some(slot.path.clone()),
            has_editor: slot.instance.has_editor(),
        })
        .unwrap_or_default()
    }

    fn params(&self, node_id: &str) -> Vec<PluginParamInfo> {
        with_slot(node_id, |slot| slot.instance.params()).unwrap_or_default()
    }

    fn save_state(&self, node_id: &str) -> Result<Option<String>, Unsupported> {
        Ok(with_slot(node_id, |slot| {
            slot.instance
                .save_state()
                .map(|blob| tag_state(&slot.plugin_id, &blob))
        })
        .flatten())
    }

    fn notify_param_changed(
        &self,
        node_id: &str,
        param_id: u32,
        value: f64,
    ) -> Result<(), Unsupported> {
        with_slot(node_id, move |slot| {
            slot.instance.set_param(param_id, value)
        });
        Ok(())
    }

    fn embed_editor(&self, node_id: &str, window: &tauri::Window) -> Result<EditorSize, String> {
        let id = node_id.to_string();
        let win = window.clone();

        main_thread::run(move || {
            SLOTS.with(|slots| {
                let mut slots = slots.borrow_mut();
                let slot = slots
                    .get_mut(&id)
                    .ok_or_else(|| format!("vst3 {id}: no plugin loaded"))?;

                attach_slot_editor(slot, &id, &win)
            })
        })?
    }

    fn show_editor(&self, node_id: &str) -> Result<(), String> {
        let id = node_id.to_string();
        main_thread::run(move || {
            SLOTS.with(|slots| {
                let slots = slots.borrow();
                let Some(slot) = slots.get(&id) else {
                    return Ok(());
                };
                if let Some(ref editor) = slot.editor {
                    editor.on_focus(true);
                    if let Some(window) = editor::window_for(&id) {
                        #[cfg(target_os = "macos")]
                        unsafe {
                            use objc2::msg_send;
                            use objc2::runtime::AnyObject;
                            if let Ok(addr) = parent_handle(&window) {
                                let parent_obj = addr as *mut AnyObject;
                                let _: () = msg_send![parent_obj, setNeedsDisplay: true];
                                if let Some(v) = editor::last_subview(addr as *mut std::ffi::c_void)
                                {
                                    let _: () = msg_send![v, setNeedsDisplay: true];
                                }
                            }
                        }
                        #[cfg(target_os = "windows")]
                        unsafe {
                            use windows::Win32::Foundation::HWND;
                            use windows::Win32::Graphics::Gdi::{InvalidateRect, UpdateWindow};
                            if let Ok(addr) = parent_handle(&window) {
                                let hwnd = HWND(addr as _);
                                let _ = InvalidateRect(Some(hwnd), None, true);
                                let _ = UpdateWindow(hwnd);
                            }
                        }
                    }
                }
                Ok(())
            })
        })?
    }

    fn hide_editor(&self, node_id: &str) -> Result<(), String> {
        let id = node_id.to_string();
        main_thread::run(move || {
            SLOTS.with(|slots| {
                let slots = slots.borrow();
                if let Some(slot) = slots.get(&id) {
                    if let Some(ref editor) = slot.editor {
                        editor.on_focus(false);
                    }
                }
                Ok(())
            })
        })?
    }

    /// Already on the main thread by contract, so the view is dropped here
    /// rather than marshalled: the window's own close handler calls this, and
    /// marshalling would deadlock.
    fn destroy_editor(&self, node_id: &str) {
        SLOTS.with(|s| {
            if let Some(slot) = s.borrow_mut().get_mut(node_id) {
                slot.editor = None;
            }
        });
    }

    /// Frees plugins whose RT node has left the graph. The main thread holds
    /// the last reference, which is the only place VST3 allows it to go.
    fn tick_and_reclaim(&self) {
        // X11 editors only repaint and react to input when the host services
        // the descriptors and timers they registered with us.
        #[cfg(target_os = "linux")]
        super::vst3_runloop::tick();

        let mut freed = GRAVEYARD.with(|g| g.borrow_mut().reclaim());

        let dead = SLOTS.with(|s| super::host_api::take_dead(&mut s.borrow_mut(), |s| &s.alive));
        for (node_id, mut slot) in dead {
            // The view is a child of the editor window and points into the
            // plugin, so it goes before the plugin does.
            slot.editor = None;
            editor::close_window(&node_id);
            freed.push(slot.instance);
        }

        // Dropped outside every borrow: terminating a plugin runs its own code,
        // which is free to call back into us.
        drop(freed);
    }
}

impl std::fmt::Debug for Vst3Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", PluginFormat::Vst3)
    }
}
