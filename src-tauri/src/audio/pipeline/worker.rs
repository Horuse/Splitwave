use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rtrb::{Consumer, Producer, RingBuffer};
use tracing::warn;

use crate::audio::clock::ClockSource;
use crate::error::{AppError, AppResult};

use super::dag::{OutputGraph, DSP_BLOCK_FRAMES};

/// Let input rings collect a few cpal buffers before starting the clock --
/// otherwise the first block is all zeros.
pub(super) const DSP_PREROLL: Duration = Duration::from_millis(50);

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
        mut sink: F,
    ) where
        F: FnMut(&[f32]) -> AppResult<()>,
    {
        thread::sleep(DSP_PREROLL);
        let mut block = vec![0.0_f32; DSP_BLOCK_FRAMES * self.graph.out_channels()];

        loop {
            self.drain_swaps();
            let proceed = clock.wait_for_tick(&stop);
            if !proceed {
                break;
            }

            self.graph.process_block(&mut block);
            if let Err(e) = sink(&block) {
                warn!(error = %e, "DSP worker sink failed; stopping");
                break;
            }
        }
    }
}
