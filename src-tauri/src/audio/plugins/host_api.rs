//! The one interface every plugin format implements.
//!
//! Threading contract, taken from CLAP because it is the stricter of the two:
//! every method is safe to call from any thread *except* the Tauri main thread,
//! and blocks until it has an answer. A host that needs the main thread
//! marshals there itself; callers never know which one does.
//!
//! No method has a default body. A capability added here stops compiling until
//! every format implements it, which is the whole point: the alternative is
//! wiring each new capability into each host by hand and forgetting one.

use std::sync::Arc;

use super::{ParamRing, PluginFormat, PluginNode};
use crate::audio::effects::Effect;

/// Emitted with the node id when a plugin editor window is closed via its
/// titlebar, so the frontend node can reset its open/close button.
pub const EDITOR_CLOSED_EVENT: &str = "plugin://editor-closed";

/// One automatable plugin parameter, sent to the frontend for the node UI.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginParamInfo {
    pub id: u32,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub value: f64,
    /// Stepped params (int/enum/toggle) render as discrete steps, not a slider.
    pub stepped: bool,
    /// Read-only params are shown but not editable.
    pub read_only: bool,
}

/// What the node needs to know about its plugin between rebuilds: which one is
/// actually running, and whether it can show an editor. A rebuild is not
/// instant, so `path` lagging behind the node's own selection is how the UI
/// knows to wait instead of acting on the outgoing plugin.
#[derive(serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatus {
    pub path: Option<String>,
    pub has_editor: bool,
}

/// A capability this format does not have. Returned rather than silently doing
/// nothing, so a caller can tell "no such thing here" from "done".
#[derive(Debug)]
pub struct Unsupported {
    pub format: PluginFormat,
    pub capability: &'static str,
}

pub struct ActivateRequest<'a> {
    pub node_id: &'a str,
    pub path: &'a str,
    /// Which plugin inside the bundle; formats that address a single plugin per
    /// reference leave it empty.
    pub plugin_id: &'a str,
    pub sample_rate: u32,
    pub max_frames: usize,
    pub state: Option<&'a str>,
    /// Marks the editor and parameter target. Other builds of the same node are
    /// metering duplicates or extra stereo pairs.
    pub primary: bool,
    pub params: Arc<ParamRing>,
}

/// The RT side of a hosted plugin. Deliberately a concrete enum rather than a
/// trait object: the audio path dispatches statically so LLVM can inline each
/// format's inner loop.
pub enum HostedNode {
    Clap(PluginNode),
    #[cfg(target_os = "macos")]
    Au(super::AuNode),
}

impl Effect for HostedNode {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        match self {
            HostedNode::Clap(n) => n.process(samples, frames),
            #[cfg(target_os = "macos")]
            HostedNode::Au(n) => n.process(samples, frames),
        }
    }

    #[inline]
    fn latency_frames(&self) -> usize {
        match self {
            HostedNode::Clap(n) => n.latency_frames(),
            #[cfg(target_os = "macos")]
            HostedNode::Au(n) => n.latency_frames(),
        }
    }
}

/// Marks a saved state blob with the plugin that produced it. A blob handed to
/// a different plugin is garbage to its parser, and some plugins accept it
/// silently and end up in a state their own editor disagrees with.
///
/// The separator is never in base64's alphabet, so the payload is whatever
/// follows the last one.
const STATE_TAG_SEP: char = '|';

pub fn tag_state(owner: &str, payload: &str) -> String {
    format!("{owner}{STATE_TAG_SEP}{payload}")
}

/// The payload, or `None` when the blob belongs to a different plugin.
pub fn untag_state<'a>(owner: &str, tagged: &'a str) -> Option<&'a str> {
    match tagged.rsplit_once(STATE_TAG_SEP) {
        Some((saved_by, payload)) if saved_by == owner => Some(payload),
        _ => None,
    }
}

/// Logical size of an embedded editor view.
pub type EditorSize = (u32, u32);

pub trait PluginHost: Sync {
    /// Instantiates and activates the plugin, returning its RT node.
    fn activate(&self, req: ActivateRequest<'_>) -> Result<HostedNode, String>;

    /// Drops this host's editor/parameter hold on the node. The instance itself
    /// lives until its RT node is gone.
    fn forget(&self, node_id: &str);

    fn status(&self, node_id: &str) -> PluginStatus;
    fn params(&self, node_id: &str) -> Vec<PluginParamInfo>;

    /// Serialized instance state for project persistence.
    fn save_state(&self, node_id: &str) -> Result<Option<String>, Unsupported>;

    /// Tells the plugin's own editor that the host moved a parameter behind its
    /// back. Called off the RT thread.
    fn notify_param_changed(&self, node_id: &str, param_id: u32) -> Result<(), Unsupported>;

    /// Builds the plugin's view into `window`, returning the view's own size so
    /// the caller can fit the window to it.
    fn embed_editor(&self, node_id: &str, window: &tauri::Window) -> Result<EditorSize, String>;

    /// Tears the view down. Must run before the host window closes, since the
    /// plugin's view is a child of it.
    ///
    /// The one method that inverts the threading contract: it must be called
    /// *on* the main thread. Both callers are already there, and a host that
    /// marshalled internally would deadlock when invoked from the window's own
    /// close handler.
    fn destroy_editor(&self, node_id: &str);

    /// Main thread, on the shared 16 ms tick. Drives whatever the format needs
    /// pumping (timers, deferred callbacks) and frees instances whose RT node
    /// has left the graph -- the one place a plugin may be destroyed.
    fn tick_and_reclaim(&self);
}
