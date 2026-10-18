//! The CLAP implementation of `PluginHost`.
//!
//! CLAP requires every main-thread call on one consistent thread, and an editor
//! needs that thread to be the app's UI thread with a live event loop. So
//! instances live in a `thread_local` on the Tauri main thread and every method
//! marshals there and blocks; only the `Send` audio processor travels to the
//! DSP worker. Same shape as the VST3 registry, for the same reason.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use super::clap_host::{Bundles, ClapInstance};
use super::host_api::{
    alive_flag, take_dead, ActivateRequest, AliveFlag, EditorSize, Graveyard, HostedNode,
    PluginHost, PluginParamInfo, PluginStatus, Unsupported,
};
use super::{editor, main_thread, ParamRing, PluginFormat};

/// A node's live plugin. Only the main thread ever touches one.
struct Slot {
    instance: ClapInstance,
    alive: AliveFlag,
}

thread_local! {
    static BUNDLES: RefCell<Bundles> = RefCell::new(Bundles::default());
    static SLOTS: RefCell<HashMap<String, Slot>> = RefCell::new(HashMap::new());
    /// Instances whose node is gone but whose plugin has not been freed yet.
    /// Metering duplicates and extra stereo pairs land here directly.
    static GRAVEYARD: RefCell<Graveyard<ClapInstance>> = RefCell::new(Graveyard::default());
}

/// Runs `f` on the main thread with the node's slot, and blocks. Returns `None`
/// when the node runs no CLAP plugin.
fn with_slot<R: Send + 'static>(
    node_id: &str,
    f: impl FnOnce(&mut Slot) -> R + Send + 'static,
) -> Option<R> {
    let id = node_id.to_string();
    main_thread::run(move || SLOTS.with(|s| s.borrow_mut().get_mut(&id).map(f)))
        .ok()
        .flatten()
}

#[allow(clippy::too_many_arguments)]
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
) -> Result<super::PluginNode, String> {
    let mut instance =
        BUNDLES.with(|b| ClapInstance::new(&mut b.borrow_mut(), &node_id, &path, &plugin_id))?;

    // Before activation: a plugin may size its buffers to what it reads.
    if let Some(blob) = state.as_deref() {
        instance.restore_state(&node_id, blob);
    }

    let alive = alive_flag();
    let node = instance.activate(sample_rate, max_frames, channels, params, alive.clone())?;

    if !primary {
        // The monitor graph builds its own metering-only duplicate. It must not
        // become the editor target, which would leave the GUI driving a silent
        // instance while the audible one plays untouched.
        GRAVEYARD.with(|g| g.borrow_mut().bury(instance, alive));
        return Ok(node);
    }

    if let Some(old) = SLOTS.with(|s| s.borrow_mut().remove(&node_id)) {
        GRAVEYARD.with(|g| g.borrow_mut().bury(old.instance, old.alive));
    }
    // A rebuild invalidates any open editor; drop its window so a reopen embeds
    // into the new instance instead of focusing a stale one.
    editor::close_window(&node_id);
    SLOTS.with(|s| s.borrow_mut().insert(node_id, Slot { instance, alive }));
    Ok(node)
}

/// CLAP's side of the shared host interface. Holds nothing: the instances live
/// in the main-thread tables above.
pub struct ClapHost;

impl PluginHost for ClapHost {
    fn activate(&self, req: ActivateRequest<'_>) -> Result<HostedNode, String> {
        main_thread::ensure_ticker();
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
        .map(HostedNode::Clap)
    }

    /// Retires a node's editor/parameter target without installing a
    /// replacement. Without it, a rebuild that fails to load the plugin leaves
    /// the previous instance installed and the editor driving something the
    /// graph no longer contains.
    fn forget(&self, node_id: &str) {
        let id = node_id.to_string();
        let _ = main_thread::run(move || {
            // The instance outlives this: its processor may still be in the
            // outgoing DAG.
            if let Some(slot) = SLOTS.with(|s| s.borrow_mut().remove(&id)) {
                GRAVEYARD.with(|g| g.borrow_mut().bury(slot.instance, slot.alive));
            }
        });
        editor::close_window(node_id);
    }

    fn status(&self, node_id: &str) -> PluginStatus {
        with_slot(node_id, |slot| PluginStatus {
            path: Some(slot.instance.path().to_string()),
            has_editor: slot.instance.has_editor(),
        })
        .unwrap_or_default()
    }

    fn params(&self, node_id: &str) -> Vec<PluginParamInfo> {
        with_slot(node_id, |slot| slot.instance.params()).unwrap_or_default()
    }

    fn save_state(&self, node_id: &str) -> Result<Option<String>, Unsupported> {
        Ok(with_slot(node_id, |slot| slot.instance.save_state()).flatten())
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
        let id = node_id.to_string();
        let window = window.clone();
        main_thread::run(move || {
            SLOTS.with(|slots| {
                let mut slots = slots.borrow_mut();
                let slot = slots
                    .get_mut(&id)
                    .ok_or_else(|| format!("clap {id}: plugin is not running"))?;
                slot.instance.embed_editor(&id, &window)
            })
        })?
    }

    fn show_editor(&self, node_id: &str) -> Result<(), String> {
        let id = node_id.to_string();
        main_thread::run(move || {
            SLOTS.with(|slots| {
                if let Some(slot) = slots.borrow_mut().get_mut(&id) {
                    slot.instance.show_editor()
                } else {
                    Ok(())
                }
            })
        })?
    }

    fn hide_editor(&self, node_id: &str) -> Result<(), String> {
        let id = node_id.to_string();
        main_thread::run(move || {
            SLOTS.with(|slots| {
                if let Some(slot) = slots.borrow_mut().get_mut(&id) {
                    slot.instance.hide_editor()
                } else {
                    Ok(())
                }
            })
        })?
    }

    /// Already on the main thread by contract, so the view is dropped here
    /// rather than marshalled: the window's own close handler calls this, and
    /// marshalling would deadlock.
    fn destroy_editor(&self, node_id: &str) {
        SLOTS.with(|s| {
            if let Some(slot) = s.borrow_mut().get_mut(node_id) {
                slot.instance.destroy_editor();
            }
        });
    }

    fn tick_and_reclaim(&self) {
        let now = Instant::now();
        SLOTS.with(|s| {
            for slot in s.borrow_mut().values_mut() {
                slot.instance.tick(now);
            }
        });

        let mut freed = GRAVEYARD.with(|g| g.borrow_mut().reclaim());
        let dead = SLOTS.with(|s| take_dead(&mut s.borrow_mut(), |s| &s.alive));
        for (node_id, slot) in dead {
            editor::close_window(&node_id);
            freed.push(slot.instance);
        }
        // Dropped outside every borrow: destroying a plugin runs its own code,
        // which is free to call back into us.
        drop(freed);
    }
}
