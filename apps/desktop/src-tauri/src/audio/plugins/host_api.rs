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

use super::editor::EditorWindow;
use super::{ParamRing, PluginFormat, PluginNode};
use crate::audio::effects::offload::{BlockProcessor, Offload};
use crate::audio::effects::Effect;
use crate::audio::graph::MAX_BUFFER_FRAMES;

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
    /// One step in the `min..max` units for stepped params (int/enum/toggle),
    /// 0 for continuous ones.
    pub step: f64,
    /// Read-only params are shown but not editable.
    pub read_only: bool,
}

/// One audio bus a plugin declares.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PortBus {
    /// The plugin's own name for the bus ("Sidechain", "Main").
    pub name: String,
    pub channels: u32,
    /// The plugin's main bus. Every input that is not main is a sidechain the
    /// node offers a socket for. A plugin can have several main buses or none
    /// at all, so the position in the list says nothing about the role.
    pub main: bool,
}

/// The audio buses a plugin declares, in the plugin's own order.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PortLayout {
    pub inputs: Vec<PortBus>,
    pub outputs: Vec<PortBus>,
}

impl PortLayout {
    /// Channels of the first main input; 0 for a plugin with none (a generator).
    pub fn main_in(&self) -> usize {
        self.inputs
            .iter()
            .find(|b| b.main)
            .map_or(0, |b| b.channels as usize)
    }

    /// Channels of the first main output; 0 for a plugin with none.
    pub fn main_out(&self) -> usize {
        self.outputs
            .iter()
            .find(|b| b.main)
            .map_or(0, |b| b.channels as usize)
    }

    /// Channels of each sidechain, in the plugin's own order.
    pub fn sidechains(&self) -> Vec<usize> {
        self.inputs
            .iter()
            .filter(|b| !b.main)
            .map(|b| b.channels as usize)
            .collect()
    }
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
    /// Copies of the plugin the node runs, one per group of channels when the
    /// node is wider than the plugin. 0 while nothing is running.
    pub copies: usize,
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
    /// What the plugin declared when it was picked. The instance takes the
    /// buses the plugin itself reports; this is what the node's width was
    /// sized from, and a plugin that now reports something else is refused.
    pub layout: &'a PortLayout,
    /// Run the plugin's main buses at this many channels if it takes them; a
    /// plugin that refuses keeps its own widths, and the node reports which. CLAP
    /// ports are fixed at instantiation, so it never widens.
    pub widen: Option<usize>,
    /// Whether anything is wired to each sidechain input, in the plugin's own
    /// order. A sidechain nothing feeds stays switched off.
    pub wired_extras: &'a [bool],
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
}

impl HostedNode {
    /// Channels the plugin reads on its main input.
    pub fn in_channels(&self) -> usize {
        match self {
            HostedNode::Clap(n) => n.in_channels(),
            #[cfg(target_os = "macos")]
            HostedNode::Au(n) => n.in_channels(),
            HostedNode::Vst3(n) => n.in_channels(),
        }
    }

    /// Channels the plugin writes on its main output, which is the width the
    /// node carries on from here.
    pub fn out_channels(&self) -> usize {
        match self {
            HostedNode::Clap(n) => n.out_channels(),
            #[cfg(target_os = "macos")]
            HostedNode::Au(n) => n.out_channels(),
            HostedNode::Vst3(n) => n.out_channels(),
        }
    }
}

impl HostedNode {
    /// Channels of each extra input (sidechain), in port order. They follow the
    /// main channels in every block `process` is given.
    pub fn aux_buses(&self) -> Vec<usize> {
        match self {
            HostedNode::Clap(n) => n.aux_buses().to_vec(),
            #[cfg(target_os = "macos")]
            HostedNode::Au(n) => n.aux_buses().to_vec(),
            HostedNode::Vst3(n) => n.aux_buses().to_vec(),
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
        }
    }

    #[inline]
    fn latency_frames(&self) -> usize {
        match self {
            HostedNode::Clap(n) => n.latency_frames(),
            #[cfg(target_os = "macos")]
            HostedNode::Au(n) => n.latency_frames(),
            HostedNode::Vst3(n) => n.latency_frames(),
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

/// Delays the channels a plugin does not touch by the plugin's own latency, so
/// they stay in step with the ones it does.
struct ChannelDelay {
    buf: Vec<f32>,
    pos: usize,
    frames: usize,
    cols: usize,
}

impl ChannelDelay {
    fn new(frames: usize, cols: usize) -> Self {
        Self {
            buf: vec![0.0; frames * cols],
            pos: 0,
            frames,
            cols,
        }
    }

    /// In place over `block`, `cols` interleaved.
    fn process(&mut self, block: &mut [f32]) {
        if self.frames == 0 {
            return;
        }
        for row in block.chunks_exact_mut(self.cols) {
            let held = &mut self.buf[self.pos * self.cols..(self.pos + 1) * self.cols];
            for (a, b) in held.iter_mut().zip(row.iter_mut()) {
                std::mem::swap(a, b);
            }
            self.pos = (self.pos + 1) % self.frames;
        }
    }
}

/// One running copy of the plugin, with its own offload thread when real time.
struct Lane {
    backend: HostedBackend,
    /// One block at `max(in, out) + aux_total` wide, the shape the node and the
    /// offload both work in. Sized once, so the audio path never allocates.
    joined: Vec<f32>,
    /// Holds this copy back to the latency of the slowest one, so every channel
    /// group of the node leaves at the same moment. Each copy times its own
    /// offload pad, and unequal pads would put the groups out of phase.
    lag: Option<ChannelDelay>,
}

/// A hosted plugin, run either on the offload thread or in place. The node's
/// block is `node_w` wide. With `copies` on, a node wider than the plugin gets
/// one copy per group of the plugin's own width, so every channel is processed.
/// With it off there is one copy: it reads the first `in_ch` channels and
/// writes the first `out_ch`, and any channel beyond both passes through
/// untouched (delayed to match the plugin).
pub struct HostedEffect {
    lanes: Vec<Lane>,
    in_ch: usize,
    out_ch: usize,
    node_w: usize,
    copies: bool,
    /// Channels of each sidechain input, laid out after the main ones.
    aux: Vec<usize>,
    aux_total: usize,
    /// The node's channels past what the plugin handles, and their delay.
    pass: Option<ChannelDelay>,
    pass_buf: Vec<f32>,
    /// Scratch for the lag delay of a copy's own channels.
    lag_buf: Vec<f32>,
    latency: usize,
}

enum HostedBackend {
    Offloaded(Offload),
    // An offline render outruns the offload thread and would read back silence.
    Inline { node: HostedNode },
}

impl HostedEffect {
    /// How many copies a node of `node_w` channels needs for a plugin that
    /// reads and writes `plugin_w`. Copies need the same width in and out to
    /// line the groups up, so any other plugin stays a single instance.
    pub fn copies_needed(auto_copies: bool, node_w: usize, in_ch: usize, out_ch: usize) -> usize {
        if auto_copies && in_ch > 0 && in_ch == out_ch {
            node_w.div_ceil(in_ch).max(1)
        } else {
            1
        }
    }

    /// `nodes` are the copies, the first being the editor target. They are all
    /// the same plugin, so they share one shape.
    pub fn new(
        nodes: Vec<HostedNode>,
        realtime: bool,
        sample_rate: u32,
        block_frames: usize,
        burst_blocks: usize,
        node_w: usize,
    ) -> Self {
        let first = nodes.first().expect("a plugin has at least one copy");
        let plugin_latency = first.latency_frames();
        let (in_ch, out_ch) = (first.in_channels(), first.out_channels());
        let main_w = in_ch.max(out_ch);
        let aux = first.aux_buses();
        let aux_total: usize = aux.iter().sum();
        let width = main_w + aux_total;
        let copies = nodes.len() > 1;
        let direct = !copies && aux_total == 0 && in_ch == out_ch && node_w == in_ch;
        let built: Vec<(Lane, usize)> = nodes
            .into_iter()
            .map(|node| {
                let joined = if direct {
                    Vec::new()
                } else {
                    vec![0.0; MAX_BUFFER_FRAMES * width]
                };
                let lane = |backend| Lane {
                    backend,
                    joined: joined.clone(),
                    lag: None,
                };
                if !realtime {
                    return (lane(HostedBackend::Inline { node }), plugin_latency);
                }
                let processor = HostedProcessor {
                    node,
                    width,
                    scratch: Vec::with_capacity(MAX_BUFFER_FRAMES * width),
                };
                // A plugin takes the engine's blocks as they come; the offload's
                // pad is latency it adds, not a block it works in.
                match Offload::spawn(
                    "plugin",
                    processor,
                    width,
                    None,
                    block_frames,
                    burst_blocks,
                    sample_rate,
                ) {
                    Ok(o) => {
                        let own = plugin_latency + o.latency_frames();
                        (lane(HostedBackend::Offloaded(o)), own)
                    }
                    Err(p) => (lane(HostedBackend::Inline { node: p.node }), plugin_latency),
                }
            })
            .collect();
        let latency = built.iter().map(|(_, own)| *own).max().unwrap_or(0);
        let mut lanes: Vec<Lane> = Vec::with_capacity(built.len());
        for (k, (mut lane, own)) in built.into_iter().enumerate() {
            let owned = if copies {
                out_ch.min(node_w.saturating_sub(k * in_ch))
            } else {
                out_ch
            };
            if latency > own && owned > 0 {
                lane.lag = Some(ChannelDelay::new(latency - own, owned));
            }
            lanes.push(lane);
        }
        // Channels past what the plugin handles. With copies every channel has
        // one, so nothing is left over.
        let pass_cols = if copies {
            0
        } else {
            node_w.saturating_sub(main_w)
        };
        Self {
            lanes,
            in_ch,
            out_ch,
            node_w,
            copies,
            aux,
            aux_total,
            pass: (pass_cols > 0).then(|| ChannelDelay::new(latency, pass_cols)),
            pass_buf: vec![0.0; MAX_BUFFER_FRAMES * pass_cols],
            lag_buf: vec![0.0; MAX_BUFFER_FRAMES * out_ch],
            latency,
        }
    }

    /// How many copies of the plugin this node runs.
    pub fn copy_count(&self) -> usize {
        self.lanes.len()
    }
}

impl HostedEffect {
    /// `main` is the node's `node_w` wide block. `extras[i]` feeds sidechain
    /// `i` at the node's width, and may be empty when nothing is wired to it. A
    /// signal wider than the plugin's port loses the surplus channels, and a
    /// port wider than the signal keeps silence in the rest.
    pub fn process_with_inputs(&mut self, main: &mut [f32], extras: &[Vec<f32>], frames: usize) {
        let w = self.node_w;
        if frames == 0 || main.len() < frames * w {
            return;
        }
        let (i, o) = (self.in_ch, self.out_ch);
        let main_w = i.max(o);
        let stride = main_w + self.aux_total;
        for (k, lane) in self.lanes.iter_mut().enumerate() {
            if lane.joined.is_empty() {
                match &mut lane.backend {
                    HostedBackend::Offloaded(off) => off.process(&mut main[..frames * w]),
                    HostedBackend::Inline { node } => node.process(main, frames),
                }
                continue;
            }
            // The channels this copy reads: its own group, and the last one
            // repeated where the group runs past the node (a lone channel
            // fills both sides of a stereo plugin, as it always has).
            let (base, last) = if self.copies {
                (k * i, (w - 1).min(k * i + i - 1))
            } else {
                (0, i.saturating_sub(1))
            };
            let src_of = |c: usize| (base + c).min(last);
            let joined = &mut lane.joined[..frames * stride];
            for f in 0..frames {
                let row = &mut joined[f * stride..(f + 1) * stride];
                for c in 0..i {
                    row[c] = main[f * w + src_of(c)];
                }
                row[i..main_w].fill(0.0);
                let mut offset = main_w;
                for (bus, &a) in self.aux.iter().enumerate() {
                    let side = &mut row[offset..offset + a];
                    match extras.get(bus) {
                        Some(s) if s.len() >= frames * w => {
                            for (c, v) in side.iter_mut().enumerate() {
                                *v = if self.copies {
                                    s[f * w + src_of(c.min(i.saturating_sub(1)))]
                                } else if c < w {
                                    s[f * w + c]
                                } else {
                                    0.0
                                };
                            }
                        }
                        _ => side.fill(0.0),
                    }
                    offset += a;
                }
            }
            match &mut lane.backend {
                HostedBackend::Offloaded(off) => off.process(joined),
                HostedBackend::Inline { node } => node.process(joined, frames),
            }
            // Only the channels this copy owns are written back.
            let owned = if self.copies {
                o.min(w.saturating_sub(base))
            } else {
                o
            };
            for f in 0..frames {
                main[f * w + base..f * w + base + owned]
                    .copy_from_slice(&joined[f * stride..f * stride + owned]);
                if !self.copies && i > o {
                    // Channels the plugin consumed and did not write back are silent.
                    main[f * w + o..f * w + i].fill(0.0);
                }
            }
            if let Some(lag) = &mut lane.lag {
                let held = &mut self.lag_buf[..frames * owned];
                for f in 0..frames {
                    held[f * owned..(f + 1) * owned]
                        .copy_from_slice(&main[f * w + base..f * w + base + owned]);
                }
                lag.process(held);
                for f in 0..frames {
                    main[f * w + base..f * w + base + owned]
                        .copy_from_slice(&held[f * owned..(f + 1) * owned]);
                }
            }
        }
        if let Some(delay) = &mut self.pass {
            let cols = w - main_w;
            let held = &mut self.pass_buf[..frames * cols];
            for f in 0..frames {
                held[f * cols..(f + 1) * cols].copy_from_slice(&main[f * w + main_w..(f + 1) * w]);
            }
            delay.process(held);
            for f in 0..frames {
                main[f * w + main_w..(f + 1) * w].copy_from_slice(&held[f * cols..(f + 1) * cols]);
            }
        }
    }
}

impl Effect for HostedEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        self.process_with_inputs(samples, &[], frames);
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

    /// The buses the plugin declares, read from a short-lived instance that is
    /// never activated and never becomes any node's editor target.
    fn probe(&self, path: &str, plugin_id: &str) -> Result<PortLayout, String>;

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
    fn embed_editor(&self, node_id: &str, window: &EditorWindow) -> Result<EditorSize, String>;

    /// Notifies the plugin that its editor window was shown (unhidden) or focused.
    fn show_editor(&self, node_id: &str) -> Result<(), String>;

    /// Notifies the plugin that its editor window was hidden.
    fn hide_editor(&self, node_id: &str) -> Result<(), String>;

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
