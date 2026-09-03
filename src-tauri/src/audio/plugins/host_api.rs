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

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};

use super::{ParamRing, PluginFormat, PluginNode};
use crate::audio::effects::offload::{BlockProcessor, Offload};
use crate::audio::effects::Effect;
use crate::audio::pipeline::dag::DSP_BLOCK_FRAMES;

/// Emitted with the node id when a plugin editor window is closed via its
/// titlebar, so the frontend node can reset its open/close button.
pub const EDITOR_CLOSED_EVENT: &str = "plugin://editor-closed";

/// One automatable plugin parameter, sent to the frontend for the node UI.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// Channels the node carries. The host offers the plugin this width and
    /// reports back what it took, which is never more and never a value the
    /// plugin did not agree to.
    pub channels: usize,
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
    Vst3(super::vst3_node::Vst3Node),
    #[cfg(target_os = "windows")]
    Bridge(super::bridge::BridgeNode),
}

impl HostedNode {
    /// Channels this instance was configured for. `2` means the pipeline must
    /// drive it one stereo pair at a time.
    pub fn channels(&self) -> usize {
        match self {
            HostedNode::Clap(n) => n.channels(),
            #[cfg(target_os = "macos")]
            HostedNode::Au(n) => n.channels(),
            HostedNode::Vst3(n) => n.channels(),
            #[cfg(target_os = "windows")]
            HostedNode::Bridge(n) => n.channels(),
        }
    }
}

impl Effect for HostedNode {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        match self {
            HostedNode::Clap(n) => n.process(samples, frames),
            #[cfg(target_os = "macos")]
            HostedNode::Au(n) => n.process(samples, frames),
            HostedNode::Vst3(n) => n.process(samples, frames),
            #[cfg(target_os = "windows")]
            HostedNode::Bridge(n) => n.process(samples, frames),
        }
    }

    #[inline]
    fn latency_frames(&self) -> usize {
        match self {
            HostedNode::Clap(n) => n.latency_frames(),
            #[cfg(target_os = "macos")]
            HostedNode::Au(n) => n.latency_frames(),
            HostedNode::Vst3(n) => n.latency_frames(),
            #[cfg(target_os = "windows")]
            HostedNode::Bridge(n) => n.latency_frames(),
        }
    }
}

// Feeds the offload thread; the node itself keeps writing in place.
struct HostedProcessor {
    node: HostedNode,
    width: usize,
    scratch: Vec<f32>,
}

impl BlockProcessor for HostedProcessor {
    fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
        self.scratch.clear();
        self.scratch.extend_from_slice(input);
        let frames = input.len() / self.width;
        self.node.process(&mut self.scratch, frames);
        output.extend_from_slice(&self.scratch);
    }
}

/// A hosted plugin, run either on the offload thread or in place.
pub struct HostedEffect {
    backend: HostedBackend,
    channels: usize,
    latency: usize,
}

enum HostedBackend {
    Offloaded(Offload),
    // An offline render outruns the offload thread and would read back silence.
    Inline { node: HostedNode },
}

impl HostedEffect {
    pub fn new(node: HostedNode, realtime: bool) -> Self {
        let latency = node.latency_frames();
        let width = node.channels();
        if !realtime {
            return Self {
                backend: HostedBackend::Inline { node },
                channels: width,
                latency,
            };
        }
        let processor = HostedProcessor {
            node,
            width,
            scratch: Vec::with_capacity(DSP_BLOCK_FRAMES * width),
        };
        match Offload::spawn("plugin", processor, width) {
            Ok(o) => {
                let latency = latency + o.latency_frames();
                Self {
                    backend: HostedBackend::Offloaded(o),
                    channels: width,
                    latency,
                }
            }
            Err(p) => Self {
                backend: HostedBackend::Inline { node: p.node },
                channels: width,
                latency,
            },
        }
    }
}

impl Effect for HostedEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        if frames == 0 {
            return;
        }
        match &mut self.backend {
            HostedBackend::Offloaded(o) => o.process(&mut samples[..frames * self.channels]),
            HostedBackend::Inline { node } => node.process(samples, frames),
        }
    }

    fn latency_frames(&self) -> usize {
        self.latency
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

/// A tagged blob as base64, for the formats whose state is bytes.
pub fn encode_state(owner: &str, bytes: &[u8]) -> String {
    tag_state(owner, &STANDARD.encode(bytes))
}

/// Inverse of [`encode_state`]. `None` when the blob belongs to another plugin
/// or is not decodable; both are reported here so no caller has to.
pub fn decode_state(node_id: &str, owner: &str, tagged: &str) -> Option<Vec<u8>> {
    let Some(payload) = untag_state(owner, tagged) else {
        tracing::warn!(node_id, owner, "discarding state saved by another plugin");
        return None;
    };
    match STANDARD.decode(payload) {
        Ok(bytes) => Some(bytes),
        Err(err) => {
            tracing::error!(node_id, owner, %err, "plugin state is not valid base64");
            None
        }
    }
}

/// Cleared by an RT node's `Drop`, which is how a host learns the plugin has
/// left the graph and may finally be freed.
pub type AliveFlag = Arc<AtomicBool>;

pub fn alive_flag() -> AliveFlag {
    Arc::new(AtomicBool::new(true))
}

/// Instances kept alive until their RT node leaves the outgoing graph: dropping
/// one earlier would destroy a plugin mid-process. Every format needs this, and
/// for the same reason, so the bookkeeping is written once.
pub struct Graveyard<T> {
    graves: Vec<(T, AliveFlag)>,
}

impl<T> Default for Graveyard<T> {
    fn default() -> Self {
        Self { graves: Vec::new() }
    }
}

impl<T> Graveyard<T> {
    pub fn bury(&mut self, instance: T, alive: AliveFlag) {
        self.graves.push((instance, alive));
    }

    /// The instances whose node is gone. Returned rather than dropped in place:
    /// destroying a plugin runs its own code, which is free to call back into
    /// the table this was invoked from.
    #[must_use]
    pub fn reclaim(&mut self) -> Vec<T> {
        let mut freed = Vec::new();
        let mut i = 0;
        while i < self.graves.len() {
            if self.graves[i].1.load(Ordering::Acquire) {
                i += 1;
            } else {
                freed.push(self.graves.swap_remove(i).0);
            }
        }
        freed
    }
}

/// Removes and returns the slots whose RT node has left the graph. The caller
/// decides what a slot's teardown involves; the sweep only decides which ones.
pub fn take_dead<S>(
    slots: &mut HashMap<String, S>,
    alive: impl Fn(&S) -> &AliveFlag,
) -> Vec<(String, S)> {
    let dead: Vec<String> = slots
        .iter()
        .filter(|(_, slot)| !alive(slot).load(Ordering::Acquire))
        .map(|(id, _)| id.clone())
        .collect();
    dead.into_iter()
        .filter_map(|id| slots.remove(&id).map(|slot| (id, slot)))
        .collect()
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

    /// Tells the plugin's own editor that the host moved a parameter behind
    /// its back, so its display follows. Called off the RT thread. `value` is
    /// in the format's own scale, which for the two implemented formats means
    /// what the node UI already sends.
    fn notify_param_changed(
        &self,
        node_id: &str,
        param_id: u32,
        value: f64,
    ) -> Result<(), Unsupported>;

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

#[cfg(test)]
mod tests {
    use super::*;

    /// State tagging is what stops one plugin's blob reaching another's parser.
    /// Every format relies on it, so it is tested once here rather than thrice.
    #[test]
    fn a_tagged_blob_comes_back_only_to_its_owner() {
        let tagged = tag_state("com.example.reverb", "cGF5bG9hZA==");
        assert_eq!(
            untag_state("com.example.reverb", &tagged),
            Some("cGF5bG9hZA==")
        );
        assert_eq!(untag_state("com.example.delay", &tagged), None);
    }

    /// A payload is base64 and a VST3 owner is 32 hex digits, so neither can
    /// contain the separator: the split must take the last one regardless.
    #[test]
    fn an_owner_containing_the_separator_still_round_trips() {
        let owner = "vendor|product";
        let tagged = tag_state(owner, "Ymxvbg==");
        assert_eq!(untag_state(owner, &tagged), Some("Ymxvbg=="));
        assert_eq!(untag_state("vendor", &tagged), None);
    }

    #[test]
    fn an_untagged_blob_belongs_to_nobody() {
        assert_eq!(untag_state("com.example.reverb", "cGF5bG9hZA=="), None);
        assert_eq!(untag_state("", ""), None);
    }
}
