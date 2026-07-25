//! The VST3 implementation of `PluginHost`.
//!
//! VST3 puts every call except `process` on the host's UI thread, so instances
//! live in a `thread_local` on the Tauri main thread and every method marshals
//! there and blocks. Same shape as CLAP, and for the same reason: a plugin
//! initialised off the main thread is free to misbehave, and some do.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::host::{self, FALLBACK_EDITOR_SIZE};
use super::host_api::{
    tag_state, untag_state, ActivateRequest, EditorSize, HostedNode, PluginHost, PluginParamInfo,
    PluginStatus, Unsupported,
};
use super::vst3_backend::Vst3Module;
use super::vst3_com::EditListener;
use super::vst3_editor::EditorView;
use super::vst3_host::Vst3Instance;
use super::{ParamRing, PluginFormat};

/// A node's live plugin. Only the main thread ever touches one.
struct Slot {
    instance: Vst3Instance,
    /// Cleared by the RT node's `Drop`, which is how the sweep learns the
    /// plugin has left the graph.
    alive: Arc<AtomicBool>,
    path: String,
    plugin_id: String,
    editor: Option<EditorView>,
}

thread_local! {
    static SLOTS: RefCell<HashMap<String, Slot>> = RefCell::new(HashMap::new());
    /// Instances whose node is gone but whose plugin has not been freed yet.
    /// Metering duplicates and extra stereo pairs land here directly.
    static GRAVEYARD: RefCell<Vec<(Vst3Instance, Arc<AtomicBool>)>> = const { RefCell::new(Vec::new()) };
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
    host::run_on_main(move || SLOTS.with(|s| s.borrow_mut().get_mut(&id).map(f)))
        .ok()
        .flatten()
}

fn activate_on_main(
    node_id: String,
    path: String,
    plugin_id: String,
    sample_rate: u32,
    max_frames: usize,
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

    let alive = Arc::new(AtomicBool::new(true));
    let node = instance
        .activate(sample_rate, max_frames, params, alive.clone())
        .map_err(|e| at(&e))?;

    if primary {
        // Replacing a slot drops the plugin it held, here on the main thread.
        SLOTS.with(|s| {
            s.borrow_mut().insert(
                node_id,
                Slot {
                    instance,
                    alive,
                    path,
                    plugin_id,
                    editor: None,
                },
            )
        });
    } else {
        // A metering duplicate has no editor and no parameters to answer for;
        // it only has to outlive its node.
        GRAVEYARD.with(|g| g.borrow_mut().push((instance, alive)));
    }
    Ok(node)
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
        let (sample_rate, max_frames, primary, params) =
            (req.sample_rate, req.max_frames, req.primary, req.params);

        host::run_on_main(move || {
            activate_on_main(
                node_id,
                path,
                plugin_id,
                sample_rate,
                max_frames,
                state,
                primary,
                params,
            )
        })?
        .map(HostedNode::Vst3)
    }

    fn forget(&self, node_id: &str) {
        let id = node_id.to_string();
        let _ = host::run_on_main(move || {
            let slot = SLOTS.with(|s| s.borrow_mut().remove(&id));
            // The instance outlives this: its RT node may still be in a graph
            // that has not been swapped out yet.
            if let Some(slot) = slot {
                GRAVEYARD.with(|g| g.borrow_mut().push((slot.instance, slot.alive)));
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
        with_slot(node_id, move |slot| slot.instance.set_param(param_id, value));
        Ok(())
    }

    fn embed_editor(&self, node_id: &str, window: &tauri::Window) -> Result<EditorSize, String> {
        // A raw pointer is not `Send`; the address is, and the window outlives
        // the editor it hosts.
        let view_addr = window
            .ns_view()
            .map_err(|e| format!("vst3 {node_id}: content view: {e}"))? as usize;
        let (_, titlebar) = host::decoration_overhead(window);
        let id = node_id.to_string();
        let resize_target = window.clone();

        let size = host::run_on_main(move || {
            SLOTS.with(|slots| {
                let mut slots = slots.borrow_mut();
                let slot = slots
                    .get_mut(&id)
                    .ok_or_else(|| format!("vst3 {id}: no plugin loaded"))?;

                let resize = Box::new(move |w: u32, h: u32| {
                    let _ = resize_target.set_size(tauri::LogicalSize::new(w as f64, h as f64));
                });
                let attached = EditorView::attach(
                    &slot.instance.controller,
                    view_addr as *mut c_void,
                    titlebar,
                    resize,
                )
                .map_err(|e| format!("vst3 {id}: {e}"))?;

                let Some((view, size)) = attached else {
                    return Err(format!("vst3 {id}: plugin has no editor"));
                };
                slot.editor = Some(view);
                Ok(size)
            })
        })??;

        Ok(host::valid_gui_size(size.0, size.1).unwrap_or(FALLBACK_EDITOR_SIZE))
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
        let mut freed = Vec::new();

        GRAVEYARD.with(|g| {
            let mut graves = g.borrow_mut();
            let mut i = 0;
            while i < graves.len() {
                if graves[i].1.load(Ordering::Acquire) {
                    i += 1;
                } else {
                    freed.push(graves.swap_remove(i).0);
                }
            }
        });

        let dead: Vec<String> = SLOTS.with(|s| {
            s.borrow()
                .iter()
                .filter(|(_, slot)| !slot.alive.load(Ordering::Acquire))
                .map(|(id, _)| id.clone())
                .collect()
        });
        for node_id in dead {
            let slot = SLOTS.with(|s| s.borrow_mut().remove(&node_id));
            let Some(mut slot) = slot else { continue };
            // The view is a child of the editor window and points into the
            // plugin, so it goes before the plugin does.
            slot.editor = None;
            host::close_editor_window(&node_id);
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
