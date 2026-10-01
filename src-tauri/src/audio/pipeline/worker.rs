use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rtrb::{Consumer, Producer, RingBuffer};
use tracing::{info, warn};

use audio_thread_priority::{
    demote_current_thread_from_real_time, promote_current_thread_to_real_time, RtPriorityHandle,
};

use crate::audio::clock::ClockSource;
use crate::error::{AppError, AppResult};

use super::dag::OutputGraph;

/// Let input rings collect a few cpal buffers before starting the clock --
/// otherwise the first block is all zeros.
pub(super) const DSP_PREROLL: Duration = Duration::from_millis(50);

pub(crate) struct RtThread(Option<RtPriorityHandle>);

impl RtThread {
    pub(crate) fn promote(worker: &'static str, max_frames: u32, sample_rate: u32) -> Self {
        info!(
            worker,
            max_frames, sample_rate, "promoting worker thread to real-time"
        );
        match promote_current_thread_to_real_time(max_frames, sample_rate) {
            Ok(handle) => Self(Some(handle)),
            Err(e) => {
                warn!(worker, error = %e, "real-time promotion failed, running at normal priority");
                Self(None)
            }
        }
    }
}

impl Drop for RtThread {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            if let Err(e) = demote_current_thread_from_real_time(handle) {
                warn!(error = %e, "real-time demotion failed");
            }
        }
    }
}

/// A swap that changes what is heard fades the running graph out over this,
/// swaps, and fades the new one in over it again, so an edit never steps the
/// output.
const SWAP_FADE_MS: usize = 10;

pub(super) struct DspWorker {
    pub graph: OutputGraph,
    /// A graph waiting for the running one to fade out.
    pending: Option<OutputGraph>,
    /// Gain the running graph plays at: ramping to 0 while `pending` waits,
    /// back to 1 after the swap.
    gain: f32,
    /// Hot-swap channel: main thread pushes a freshly-built `OutputGraph`
    /// here; worker takes ownership at the next block boundary.
    cmd_rx: Consumer<OutputGraph>,
    /// Returns the old `OutputGraph` to main so its `Drop` (which may free
    /// ring buffers) doesn't run on the RT thread. At most three go back
    /// between two sends (two queued swaps and one pending), so it never
    /// fills.
    old_graph_tx: Producer<OutputGraph>,
}

/// Main-thread handle to a running worker -- used to push graph swaps.
pub(super) struct WorkerCtrl {
    cmd_tx: Producer<OutputGraph>,
    old_graph_rx: Consumer<OutputGraph>,
}

impl WorkerCtrl {
    pub(super) fn send_graph(&mut self, graph: OutputGraph) -> AppResult<()> {
        // Drain previous swap's returned graph before sending the next, so
        // its `Drop` runs here on main and not later on the RT thread.
        self.drain_old();
        self.cmd_tx
            .push(graph)
            .map_err(|_| AppError::Stream("worker swap queue full".into()))?;
        Ok(())
    }

    fn drain_old(&mut self) {
        while self.old_graph_rx.pop().is_ok() {}
    }
}

pub(super) fn dsp_worker(graph: OutputGraph) -> (DspWorker, WorkerCtrl) {
    let (cmd_tx, cmd_rx) = RingBuffer::<OutputGraph>::new(2);
    let (old_tx, old_rx) = RingBuffer::<OutputGraph>::new(4);
    (
        DspWorker {
            graph,
            pending: None,
            gain: 1.0,
            cmd_rx,
            old_graph_tx: old_tx,
        },
        WorkerCtrl {
            cmd_tx,
            old_graph_rx: old_rx,
        },
    )
}

impl DspWorker {
    /// Drain any graph swaps queued by main. RT-safe -- alloc-free pop +
    /// alloc-free push of the displaced graph back to main. A swap never
    /// changes the block size: that is an engine format change, which reopens
    /// the stream instead.
    ///
    /// A graph that plays exactly what is heard now, only carried over, goes
    /// in at once. Any other waits for the running graph to fade out; later
    /// swaps wait behind it, since each was built against the one before.
    #[inline]
    fn drain_swaps(&mut self) {
        while self.pending.is_none() {
            let Ok(new_graph) = self.cmd_rx.pop() else {
                return;
            };
            debug_assert_eq!(new_graph.block_frames(), self.graph.block_frames());
            // A different width cannot share the caller's block, so it goes in
            // at once.
            if new_graph.plays_like(&self.graph)
                || new_graph.out_channels() != self.graph.out_channels()
            {
                self.swap_in(new_graph);
            } else {
                self.pending = Some(new_graph);
            }
        }
    }

    /// Makes `new_graph` the running one, carrying over what it can.
    fn swap_in(&mut self, mut new_graph: OutputGraph) {
        new_graph.adopt_from(&mut self.graph);
        let old = std::mem::replace(&mut self.graph, new_graph);
        let _ = self.old_graph_tx.push(old);
    }

    /// Take any queued graph swap, then render one block into `block`
    /// (`block_frames * out_channels` long). Returns the output channels the
    /// graph actually drives. RT-safe.
    #[inline]
    pub(super) fn next_block(&mut self, block: &mut [f32]) -> usize {
        self.drain_swaps();
        self.graph.process_block(block);
        let active = self.graph.active_output_channels();
        let target = if self.pending.is_some() { 0.0 } else { 1.0 };
        if self.gain != target || self.gain != 1.0 {
            let channels = self.graph.out_channels().max(1);
            let fade_len = (self.graph.sample_rate() as usize * SWAP_FADE_MS / 1000).max(1);
            let step = 1.0 / fade_len as f32;
            for frame in block.chunks_exact_mut(channels) {
                self.gain = if target > self.gain {
                    (self.gain + step).min(target)
                } else {
                    (self.gain - step).max(target)
                };
                for s in frame.iter_mut() {
                    *s *= self.gain;
                }
            }
        }
        if self.gain <= 0.0 {
            if let Some(new_graph) = self.pending.take() {
                self.swap_in(new_graph);
            }
        }
        active
    }

    pub(super) fn graph(&self) -> &OutputGraph {
        &self.graph
    }

    /// See `OutputGraph::output_stalled`.
    pub(super) fn output_stalled(&mut self) {
        self.graph.output_stalled();
    }

    /// Timer-paced workers (recording, monitoring, wire senders): produce a
    /// block each wall-clock period, so a file source (which decodes faster
    /// than real time) can't over-run a sink. A missed deadline becomes
    /// silence, never a rate error. Speakers render in their device callback.
    pub(super) fn run<F>(
        mut self,
        stop: Arc<AtomicBool>,
        mut clock: Box<dyn ClockSource>,
        realtime: Option<(&'static str, u32)>,
        mut sink: F,
    ) where
        F: FnMut(&[f32], usize) -> AppResult<()>,
    {
        thread::sleep(DSP_PREROLL);
        let mut block = vec![0.0_f32; self.graph.block_frames() * self.graph.out_channels()];
        let mut rt = None;

        loop {
            self.drain_swaps();
            let promote_after_wait = rt.is_none() && clock.realtime_ready();
            let proceed = clock.wait_for_tick(&stop);
            if !proceed {
                break;
            }
            if promote_after_wait {
                if let Some((name, sample_rate)) = realtime {
                    rt = Some(RtThread::promote(
                        name,
                        self.graph.block_frames() as u32,
                        sample_rate,
                    ));
                }
            }

            let active_channels = self.next_block(&mut block);
            if let Err(e) = sink(&block, active_channels) {
                warn!(error = %e, "DSP worker sink failed; stopping");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::dag::graph_tests::{build, passthrough_graph};
    use super::*;

    fn graph(channels: usize) -> OutputGraph {
        let (valid, speaker) = passthrough_graph();
        let (mut built, _) = build(Some(&speaker), 48_000, &valid, 48_000, false);
        built.graph.set_out_channels(channels);
        built.graph
    }

    fn block_for(worker: &DspWorker) -> Vec<f32> {
        vec![0.0; worker.graph.block_frames() * worker.graph.out_channels()]
    }

    #[test]
    fn swap_hands_the_old_graph_back_to_main() {
        let (mut worker, mut ctrl) = dsp_worker(graph(2));
        ctrl.send_graph(graph(4)).expect("queue swap");
        worker.drain_swaps();
        // A new width cannot share the caller's block, so it goes in at once.
        assert_eq!(worker.graph.out_channels(), 4, "worker runs the new graph");
        assert_eq!(
            ctrl.old_graph_rx.slots(),
            1,
            "displaced graph goes back to main instead of dropping on the RT thread"
        );
        ctrl.send_graph(graph(6)).expect("queue swap");
        assert_eq!(
            ctrl.old_graph_rx.slots(),
            0,
            "next send drains the returned graph"
        );
    }

    #[test]
    fn only_the_latest_queued_graph_survives_a_drain() {
        let (mut worker, mut ctrl) = dsp_worker(graph(2));
        ctrl.send_graph(graph(4)).expect("first swap");
        ctrl.send_graph(graph(6)).expect("second swap");
        worker.drain_swaps();
        assert_eq!(worker.graph.out_channels(), 6);
        assert_eq!(ctrl.old_graph_rx.slots(), 2);
    }

    /// A live-less passthrough at 64-frame blocks, fed `value` throughout.
    fn constant_graph(value: f32) -> (OutputGraph, rtrb::Producer<f32>) {
        let (valid, speaker) = passthrough_graph();
        let (built, mut producers) = super::super::dag::graph_tests::build_with_block(
            Some(&speaker),
            48_000,
            64,
            &valid,
            48_000,
            false,
        );
        let mut input = producers.remove("m").unwrap();
        let data = vec![value; 48_000];
        if let Ok(mut chunk) = input.write_chunk(data.len()) {
            let (a, b) = chunk.as_mut_slices();
            a.copy_from_slice(&data[..a.len()]);
            b.copy_from_slice(&data[a.len()..a.len() + b.len()]);
            chunk.commit_all();
        }
        (built.graph, input)
    }

    #[test]
    fn a_swap_that_changes_the_sound_dips_instead_of_stepping() {
        let (old, _old_in) = constant_graph(0.5);
        let (new, _new_in) = constant_graph(-0.5);
        let (mut worker, mut ctrl) = dsp_worker(old);
        let mut block = block_for(&worker);
        worker.next_block(&mut block);
        ctrl.send_graph(new).expect("queue swap");
        let mut played: Vec<f32> = Vec::new();
        for _ in 0..20 {
            worker.next_block(&mut block);
            played.extend(block.iter().step_by(2));
        }
        let fade = 48_000 * SWAP_FADE_MS / 1000;
        let worst = played
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst <= 1.0 / fade as f32 + 1e-6, "a step of {worst}");
        assert_eq!(*played.last().unwrap(), -0.5, "the new graph plays alone");
        assert_eq!(
            ctrl.old_graph_rx.slots(),
            1,
            "the old graph went back to main"
        );
    }

    #[test]
    fn a_swap_that_carries_nodes_over_does_not_fade() {
        use super::super::dag::graph_tests::{fresh_registry, parallel_with_lookahead, rebuild};
        let mut registry = fresh_registry();
        let (a, _producers) = rebuild(
            &parallel_with_lookahead(false, 2.0, false),
            &mut registry,
            &std::collections::HashMap::new(),
            false,
        );
        let (b, _) = rebuild(
            &parallel_with_lookahead(true, 2.0, false),
            &mut registry,
            &a.carried,
            false,
        );
        let (mut worker, mut ctrl) = dsp_worker(a.graph);
        ctrl.send_graph(b.graph).expect("queue swap");
        let mut block = block_for(&worker);
        worker.next_block(&mut block);
        assert!(
            worker.pending.is_none(),
            "an edit nobody hears goes in at once"
        );
        assert_eq!(
            ctrl.old_graph_rx.slots(),
            1,
            "the old graph went back at once"
        );
    }

    /// `valid` at 64-frame blocks with `previous` to carry from, its mic fed
    /// `value` throughout.
    fn carrying_graph(
        valid: &crate::audio::graph::ValidGraph,
        registry: &mut crate::audio::effects::EffectRegistry,
        previous: &std::collections::HashMap<String, super::super::dag::CarriedNode>,
        value: f32,
    ) -> (super::super::dag::BuiltOutputGraph, rtrb::Producer<f32>) {
        let (built, mut producers) =
            super::super::dag::graph_tests::rebuild(valid, registry, previous, false);
        let mut input = producers
            .remove("m")
            .unwrap_or_else(|| RingBuffer::new(1).0);
        let _ = input.push_partial_slice(&vec![value; 48_000]);
        (built, input)
    }

    #[test]
    fn a_carrying_swap_mid_crossfade_does_not_step() {
        // An edit lands while the previous one is still fading: the graph
        // fading out must finish its fade, not stop dead.
        use super::super::dag::graph_tests::{
            fresh_registry, passthrough_graph, passthrough_with_meter,
        };
        let none = std::collections::HashMap::new();
        let mut reg_a = fresh_registry();
        let (a, _in_a) = carrying_graph(&passthrough_graph().0, &mut reg_a, &none, 0.25);
        let mut reg_b = fresh_registry();
        let (b, _in_b) = carrying_graph(&passthrough_graph().0, &mut reg_b, &none, -0.25);
        let (c, _) = carrying_graph(&passthrough_with_meter(), &mut reg_b, &b.carried, 0.0);
        let (mut worker, mut ctrl) = dsp_worker(a.graph);
        let mut block = block_for(&worker);
        let mut played: Vec<f32> = Vec::new();
        let mut play = |worker: &mut DspWorker, played: &mut Vec<f32>| {
            worker.next_block(&mut block);
            played.extend(block.iter().step_by(2));
        };
        for _ in 0..8 {
            play(&mut worker, &mut played);
        }
        ctrl.send_graph(b.graph).expect("first swap");
        play(&mut worker, &mut played);
        play(&mut worker, &mut played);
        ctrl.send_graph(c.graph).expect("second swap");
        for _ in 0..12 {
            play(&mut worker, &mut played);
        }
        let worst = played[64 * 6..]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);
        let fade = 48_000 * SWAP_FADE_MS / 1000;
        assert!(worst <= 1.0 / fade as f32 + 1e-4, "a step of {worst}");
    }

    #[test]
    fn swapping_never_touches_the_heap() {
        // Adopting carried nodes, crossfading and handing old graphs back all
        // happen on the audio thread.
        use super::super::dag::graph_tests::{fresh_registry, parallel_with_lookahead};
        let none = std::collections::HashMap::new();
        let mut reg = fresh_registry();
        let (a, _in_a) = carrying_graph(
            &parallel_with_lookahead(false, 2.0, false),
            &mut reg,
            &none,
            0.25,
        );
        let mut reg_fresh = fresh_registry();
        let (fresh, _in_fresh) = carrying_graph(
            &parallel_with_lookahead(false, 2.0, false),
            &mut reg_fresh,
            &none,
            -0.25,
        );
        let (carried, _) = carrying_graph(
            &parallel_with_lookahead(true, 2.0, false),
            &mut reg_fresh,
            &fresh.carried,
            0.0,
        );
        let (mut worker, mut ctrl) = dsp_worker(a.graph);
        let mut block = block_for(&worker);
        worker.next_block(&mut block);
        ctrl.send_graph(fresh.graph).expect("fresh swap");
        crate::audio::rt_guard::assert_no_alloc("crossfading swap", || {
            for _ in 0..4 {
                worker.next_block(&mut block);
            }
        });
        ctrl.send_graph(carried.graph).expect("carrying swap");
        crate::audio::rt_guard::assert_no_alloc("carrying swap", || {
            for _ in 0..16 {
                worker.next_block(&mut block);
            }
        });
    }

    #[test]
    fn full_swap_queue_is_an_error_not_a_block() {
        let (_worker, mut ctrl) = dsp_worker(graph(2));
        ctrl.send_graph(graph(2)).expect("first swap");
        ctrl.send_graph(graph(2)).expect("second swap");
        assert!(ctrl.send_graph(graph(2)).is_err());
    }
}
