use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use rtrb::Producer;

use crate::audio::effects::{EffectControl, GrHandle, LufsHandle, MeterHandle, WaveformHandle};
use crate::audio::health;
use crate::audio::streams::bulk_push_counted;

use super::nodes::{
    add_block_at, add_mapped, add_to_channel, parse_ch, parse_stereo, target_route, DagNode,
    OutputMeta, SourceMeta, TerminalEdge,
};
use super::DSP_BLOCK_FRAMES;

pub(super) struct DelayLine {
    buf: Box<[f32]>,
    scratch: Box<[f32]>,
    pos: usize,
}

impl DelayLine {
    pub(super) fn new(delay_frames: usize, channels: usize) -> Self {
        Self {
            buf: vec![0.0; delay_frames * channels].into_boxed_slice(),
            scratch: vec![0.0; DSP_BLOCK_FRAMES * channels].into_boxed_slice(),
            pos: 0,
        }
    }

    pub(super) fn delayed<'a>(&'a mut self, input: &'a [f32]) -> &'a [f32] {
        let cap = self.buf.len();
        if cap == 0 {
            return input;
        }
        let n = input.len().min(self.scratch.len());
        let mut pos = self.pos;
        for i in 0..n {
            self.scratch[i] = self.buf[pos];
            self.buf[pos] = input[i];
            pos = if pos + 1 == cap { 0 } else { pos + 1 };
        }
        self.pos = pos;
        &self.scratch[..n]
    }
}

/// Per-output DAG runtime: sources + effects in topological order plus the
/// terminal edges whose buffers get summed into the final output.
pub struct OutputGraph {
    pub(super) sample_rate: u32,
    /// Interleaved channel width of `process_block`'s output. Stereo unless a
    /// speaker sets it to the device's channel count.
    pub(super) out_channels: usize,
    pub(super) nodes: Vec<DagNode>,
    pub(super) terminals: Vec<TerminalEdge>,
    /// Lookahead the graph's delay compensation has aligned every path to: the
    /// deepest cumulative effect latency from any source to this output. The
    /// whole mix is delayed by this, so it is the graph's own latency.
    pub(super) latency_frames: usize,
    /// Blocks produced by `process_block`. A clone lives in this build's
    /// `BuiltOutputGraph::output` so the non-RT tick thread can compare this
    /// worker's real block rate against `sample_rate / DSP_BLOCK_FRAMES`.
    pub(super) blocks: Arc<AtomicU64>,
}

impl OutputGraph {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn out_channels(&self) -> usize {
        self.out_channels
    }

    pub fn latency_frames(&self) -> usize {
        self.latency_frames
    }

    pub fn set_out_channels(&mut self, channels: usize) {
        self.out_channels = channels;
    }

    pub fn active_output_channels(&self) -> usize {
        self.terminals
            .iter()
            .map(|terminal| match terminal.route {
                Some((offset, width)) => offset + width,
                None => {
                    self.nodes[terminal.src_idx]
                        .out_buf_for_handle(terminal.source_handle.as_deref())
                        .len()
                        / DSP_BLOCK_FRAMES
                }
            })
            .max()
            .unwrap_or(1)
            .clamp(1, self.out_channels)
    }

    /// Attach a publish ring to a fan-out effect node; its `out_buf` is pushed
    /// there each block for another output's ring-source to read.
    pub fn attach_tap(&mut self, node_idx: usize, prod: Producer<f32>) {
        if let Some(DagNode::Effect(e)) = self.nodes.get_mut(node_idx) {
            e.taps.push(prod);
        }
    }

    /// Fill `output` (`DSP_BLOCK_FRAMES * out_channels` long) with one block of
    /// mixed audio at `sample_rate`.
    pub fn process_block(&mut self, output: &mut [f32]) {
        self.blocks.fetch_add(1, Ordering::Relaxed);
        for node in &mut self.nodes {
            match node {
                DagNode::Source(s) => s.fill_block(),
                DagNode::Producer(p) => p.process(),
                DagNode::Effect(_) | DagNode::Consumer(_) => {}
            }
        }
        // `split_at_mut` gives mutable access to effect `i` while keeping
        // immutable access to its upstreams (all at indices < i by topo sort).
        for i in 0..self.nodes.len() {
            let (head, tail) = self.nodes.split_at_mut(i);
            if let DagNode::Consumer(cons) = &mut tail[0] {
                for (_, buf) in cons.channel_bufs.iter_mut() {
                    for s in buf.iter_mut() {
                        *s = 0.0;
                    }
                }
                for edge in &mut cons.incoming {
                    let src = head[edge.src_idx].out_buf_for_handle(edge.source_handle.as_deref());
                    let target = edge.target_handle.as_deref();
                    let src = match &mut edge.delay {
                        Some(d) => d.delayed(src),
                        None => src,
                    };
                    let Some((_, buf)) = cons
                        .channel_bufs
                        .iter_mut()
                        .find(|(h, _)| Some(h.as_str()) == target)
                    else {
                        continue;
                    };
                    add_mapped(src, buf);
                }
                for (i, (_, buf)) in cons.channel_bufs.iter().enumerate() {
                    if let Some(prod) = cons.send_producers.get_mut(i) {
                        bulk_push_counted(prod, buf, &health::TAP_RING_OVERRUN_SAMPLES);
                    }
                }
                continue;
            }
            if let DagNode::Effect(eff) = &mut tail[0] {
                for s in eff.out_buf.iter_mut() {
                    *s = 0.0;
                }
                for edge in &mut eff.incoming {
                    let src = head[edge.src_idx].out_buf_for_handle(edge.source_handle.as_deref());
                    let route = edge.target_handle.as_deref().and_then(target_route);
                    let src = match &mut edge.delay {
                        Some(d) => d.delayed(src),
                        None => src,
                    };
                    match route {
                        Some((off, 1)) => add_to_channel(src, &mut eff.out_buf, off),
                        Some((off, _)) => add_block_at(src, &mut eff.out_buf, off),
                        None => add_mapped(src, &mut eff.out_buf),
                    }
                }
                if let Some(sc_buf) = eff.sidechain_buf.as_mut() {
                    for s in sc_buf.iter_mut() {
                        *s = 0.0;
                    }
                    for edge in &mut eff.sidechain {
                        let src =
                            head[edge.src_idx].out_buf_for_handle(edge.source_handle.as_deref());
                        let src = match &mut edge.delay {
                            Some(d) => d.delayed(src),
                            None => src,
                        };
                        add_mapped(src, sc_buf);
                    }
                }
                if !eff.bypass.load(Ordering::Relaxed) {
                    eff.run(DSP_BLOCK_FRAMES);
                }
                let w = eff.out_buf.len() / DSP_BLOCK_FRAMES;
                for (h, buf) in eff.handle_bufs.iter_mut() {
                    if let Some(a) = parse_stereo(h) {
                        let c0 = (a - 1).min(w - 1);
                        let c1 = a.min(w - 1);
                        for f in 0..DSP_BLOCK_FRAMES {
                            buf[f * 2] = eff.out_buf[f * w + c0];
                            buf[f * 2 + 1] = eff.out_buf[f * w + c1];
                        }
                    } else if let Some(k) = parse_ch(h) {
                        let c = (k - 1).min(w - 1);
                        for f in 0..DSP_BLOCK_FRAMES {
                            buf[f] = eff.out_buf[f * w + c];
                        }
                    }
                }
                // Publish the processed block to every consuming output's ring.
                for prod in eff.taps.iter_mut() {
                    bulk_push_counted(prod, &eff.out_buf, &health::TAP_RING_OVERRUN_SAMPLES);
                }
            }
        }
        for s in output.iter_mut() {
            *s = 0.0;
        }
        for terminal in &mut self.terminals {
            let src =
                self.nodes[terminal.src_idx].out_buf_for_handle(terminal.source_handle.as_deref());
            let src = match &mut terminal.delay {
                Some(d) => d.delayed(src),
                None => src,
            };
            match terminal.route {
                Some((off, 1)) => add_to_channel(src, output, off),
                Some((off, _)) => add_block_at(src, output, off),
                None => add_mapped(src, output),
            }
        }
    }
}

pub(in crate::audio::pipeline) struct BuiltOutputGraph {
    pub graph: OutputGraph,
    pub controls: Vec<(String, EffectControl)>,
    pub bypasses: Vec<(String, Arc<AtomicBool>)>,
    pub meters: Vec<MeterHandle>,
    pub lufs: Vec<LufsHandle>,
    pub gr_handles: Vec<GrHandle>,
    pub scopes: Vec<WaveformHandle>,
    pub sources: Vec<SourceMeta>,
    pub output: OutputMeta,
    /// Effect node id -> (node index, channel width). Used to attach publish
    /// taps to nodes that fan out to other outputs.
    pub node_meta: HashMap<String, (usize, usize)>,
}

#[cfg(test)]
mod tests {
    use super::super::nodes::{add_mapped, crossfade_into, SPLICE_FADE_FRAMES};
    use super::*;

    #[test]
    fn delay_line_shifts_without_losing_samples() {
        const PAD_FRAMES: usize = 482;
        let mut line = DelayLine::new(PAD_FRAMES, 2);
        let mut fed: Vec<f32> = Vec::new();
        let mut got: Vec<f32> = Vec::new();
        for b in 0..4 {
            let mut input = vec![0.0_f32; DSP_BLOCK_FRAMES * 2];
            for f in 0..DSP_BLOCK_FRAMES {
                let v = (b * DSP_BLOCK_FRAMES + f) as f32;
                input[f * 2] = v;
                input[f * 2 + 1] = -v;
            }
            fed.extend_from_slice(&input);
            let mut dst = vec![0.0_f32; DSP_BLOCK_FRAMES * 2];
            add_mapped(line.delayed(&input), &mut dst);
            got.extend_from_slice(&dst);
        }
        let shift = PAD_FRAMES * 2;
        for i in shift..got.len() {
            assert_eq!(got[i], fed[i - shift], "sample {i} differs");
        }
    }

    #[test]
    fn delay_line_fills_a_whole_mono_block() {
        const PAD_FRAMES: usize = 482;
        let mut line = DelayLine::new(PAD_FRAMES, 1);
        let mut fed: Vec<f32> = Vec::new();
        let mut got: Vec<f32> = Vec::new();
        for b in 0..4 {
            let mut input = vec![0.0_f32; DSP_BLOCK_FRAMES];
            for (f, s) in input.iter_mut().enumerate() {
                *s = (b * DSP_BLOCK_FRAMES + f) as f32 + 1.0;
            }
            fed.extend_from_slice(&input);
            let mut dst = vec![0.0_f32; DSP_BLOCK_FRAMES * 2];
            add_mapped(line.delayed(&input), &mut dst);
            got.extend_from_slice(&dst);
        }
        for b in 1..4 {
            for f in 0..DSP_BLOCK_FRAMES {
                let i = b * DSP_BLOCK_FRAMES * 2 + f * 2;
                assert_ne!(got[i], 0.0, "left silent at block {b} frame {f}");
                assert_eq!(got[i], got[i + 1], "channels differ at block {b} frame {f}");
            }
        }
        for f in PAD_FRAMES..fed.len() {
            assert_eq!(got[f * 2], fed[f - PAD_FRAMES], "frame {f} differs");
        }
    }

    #[test]
    fn crossfade_joins_without_a_step() {
        const CH: usize = 2;
        let mut dst = vec![1.0_f32; SPLICE_FADE_FRAMES * CH];
        let incoming = vec![0.0_f32; SPLICE_FADE_FRAMES * CH];
        crossfade_into(&mut dst, &incoming, &[], CH);

        assert_eq!(dst[0], 1.0, "first frame must stay pure outgoing");
        assert_eq!(dst[1], 1.0, "both channels of the first frame agree");
        let last = (SPLICE_FADE_FRAMES - 1) * CH;
        assert_eq!(dst[last], 0.0, "last frame must reach pure incoming");
        assert_eq!(dst[last + 1], 0.0, "both channels of the last frame agree");

        for f in 1..SPLICE_FADE_FRAMES {
            assert!(dst[f * CH] < dst[(f - 1) * CH], "fade must be monotonic");
            assert_eq!(dst[f * CH], dst[f * CH + 1], "channels share a weight");
        }
    }

    #[test]
    fn add_mapped_maps_by_channel() {
        let mut src = vec![0.0; DSP_BLOCK_FRAMES * 4];
        for f in 0..DSP_BLOCK_FRAMES {
            for c in 0..4 {
                src[f * 4 + c] = c as f32 + 1.0;
            }
        }
        let mut dst = vec![0.0; DSP_BLOCK_FRAMES * 2];
        add_mapped(&src, &mut dst);
        assert_eq!(dst[0], 1.0);
        assert_eq!(dst[1], 2.0);

        let mut stereo = vec![0.0; DSP_BLOCK_FRAMES * 2];
        for f in 0..DSP_BLOCK_FRAMES {
            stereo[f * 2] = 1.0;
            stereo[f * 2 + 1] = 3.0;
        }
        let mut mono = vec![0.0; DSP_BLOCK_FRAMES];
        add_mapped(&stereo, &mut mono);
        assert!((mono[0] - 2.0).abs() < 1e-6);

        let src = vec![0.5; DSP_BLOCK_FRAMES * 3];
        let mut dst = vec![0.25; DSP_BLOCK_FRAMES * 3];
        add_mapped(&src, &mut dst);
        assert!((dst[0] - 0.75).abs() < 1e-6);
    }
}
