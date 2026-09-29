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

/// A swapped-in graph fades in over this while the one it replaces fades out,
/// so an edit to a running pipeline never steps the output.
const SWAP_FADE_MS: usize = 10;

pub(super) struct DspWorker {
    pub graph: OutputGraph,
    /// The graph being replaced, still rendering while it fades out.
    fading: Option<OutputGraph>,
    fade_pos: usize,
    /// Holds the fading graph's block.
    fade_block: Box<[f32]>,
    /// Hot-swap channel: main thread pushes a freshly-built `OutputGraph`
    /// here; worker takes ownership at the next block boundary.
    cmd_rx: Consumer<OutputGraph>,
    /// Returns the old `OutputGraph` to main so its `Drop` (which may free
    /// ring buffers) doesn't run on the RT thread.
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
    // Room for a graph queued behind one still fading out.
    let (old_tx, old_rx) = RingBuffer::<OutputGraph>::new(4);
    let fade_block = vec![0.0; graph.block_frames() * graph.out_channels()].into_boxed_slice();
    (
        DspWorker {
            graph,
            fading: None,
            fade_pos: 0,
            fade_block,
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
    #[inline]
    fn drain_swaps(&mut self) {
        while let Ok(mut new_graph) = self.cmd_rx.pop() {
            debug_assert_eq!(new_graph.block_frames(), self.graph.block_frames());
            // Nodes the new graph carries over keep playing without a seam;
            // the old graph, emptied of them, cannot render a fade.
            let carried = new_graph.adopt_from(&mut self.graph);
            let old = std::mem::replace(&mut self.graph, new_graph);
            if carried {
                if let Some(older) = self.fading.take() {
                    let _ = self.old_graph_tx.push(older);
                }
                let _ = self.old_graph_tx.push(old);
                continue;
            }
            // A swap landing mid-fade cuts the older graph short; the newer
            // one fades from the graph that was playing.
            if let Some(older) = self.fading.replace(old) {
                let _ = self.old_graph_tx.push(older);
            }
            self.fade_pos = 0;
        }
    }

    /// Take any queued graph swap, then render one block into `block`
    /// (`block_frames * out_channels` long). Returns the output channels the
    /// graph actually drives. RT-safe.
    #[inline]
    pub(super) fn next_block(&mut self, block: &mut [f32]) -> usize {
        self.drain_swaps();
        self.graph.process_block(block);
        let active = self.graph.active_output_channels();
        let Some(old) = self.fading.as_mut() else {
            return active;
        };
        let channels = self.graph.out_channels().max(1);
        let frames = block.len() / channels;
        let fade_len = (self.graph.sample_rate() as usize * SWAP_FADE_MS / 1000).max(1);
        let old_active = old.active_output_channels();
        if old.out_channels() == channels && self.fade_block.len() == block.len() {
            old.process_block(&mut self.fade_block);
            for (f, (new, old)) in block
                .chunks_exact_mut(channels)
                .zip(self.fade_block.chunks_exact(channels))
                .enumerate()
            {
                let t = ((self.fade_pos + f) as f32 / fade_len as f32).min(1.0);
                for (n, o) in new.iter_mut().zip(old) {
                    *n = *n * t + *o * (1.0 - t);
                }
            }
            self.fade_pos += frames;
        } else {
            self.fade_pos = fade_len;
        }
        if self.fade_pos >= fade_len {
            if let Some(old) = self.fading.take() {
                let _ = self.old_graph_tx.push(old);
            }
            return active;
        }
        active.max(old_active)
    }

    pub(super) fn graph(&self) -> &OutputGraph {
        &self.graph
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
        assert_eq!(worker.graph.out_channels(), 4, "worker runs the new graph");
        assert_eq!(
            ctrl.old_graph_rx.slots(),
            0,
            "the old graph is still fading out"
        );
        let mut block = block_for(&worker);
        worker.next_block(&mut block);
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
        assert_eq!(
            ctrl.old_graph_rx.slots(),
            1,
            "the skipped graph never plays"
        );
        let mut block = block_for(&worker);
        worker.next_block(&mut block);
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
    fn a_swap_crossfades_instead_of_stepping() {
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
        assert!(worker.fading.is_none(), "an emptied graph cannot fade out");
        assert_eq!(
            ctrl.old_graph_rx.slots(),
            1,
            "the old graph went back at once"
        );
    }

    #[test]
    fn full_swap_queue_is_an_error_not_a_block() {
        let (_worker, mut ctrl) = dsp_worker(graph(2));
        ctrl.send_graph(graph(2)).expect("first swap");
        ctrl.send_graph(graph(2)).expect("second swap");
        assert!(ctrl.send_graph(graph(2)).is_err());
    }
}
