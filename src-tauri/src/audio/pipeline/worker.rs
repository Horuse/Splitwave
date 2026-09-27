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

use super::dag::{OutputGraph, DSP_BLOCK_FRAMES};

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

pub(super) struct DspWorker {
    pub graph: OutputGraph,
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
    let (old_tx, old_rx) = RingBuffer::<OutputGraph>::new(2);
    (
        DspWorker {
            graph,
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
    /// alloc-free push of the displaced graph back to main.
    #[inline]
    fn drain_swaps(&mut self) {
        while let Ok(new_graph) = self.cmd_rx.pop() {
            let old = std::mem::replace(&mut self.graph, new_graph);
            let _ = self.old_graph_tx.push(old);
        }
    }

    /// All workers ride the transport clock: produce a block each wall-clock
    /// period. Speaker is device-paced; recording/monitoring share the same
    /// cadence so a file source (which decodes faster than real time) can't
    /// over-run a sink. A missed deadline becomes silence, never a rate error.
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
        let mut block = vec![0.0_f32; DSP_BLOCK_FRAMES * self.graph.out_channels()];
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
                        DSP_BLOCK_FRAMES as u32,
                        sample_rate,
                    ));
                }
            }

            self.graph.process_block(&mut block);
            let active_channels = self.graph.active_output_channels();
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

    #[test]
    fn swap_hands_the_old_graph_back_to_main() {
        let (mut worker, mut ctrl) = dsp_worker(graph(2));
        ctrl.send_graph(graph(4)).expect("queue swap");
        worker.drain_swaps();
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

    #[test]
    fn full_swap_queue_is_an_error_not_a_block() {
        let (_worker, mut ctrl) = dsp_worker(graph(2));
        ctrl.send_graph(graph(2)).expect("first swap");
        ctrl.send_graph(graph(2)).expect("second swap");
        assert!(ctrl.send_graph(graph(2)).is_err());
    }
}
