//! Wakes a helper thread from the audio path without blocking it: the helper
//! sleeps until there is work, the audio path rings once it has left some.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread::{self, Thread};
use std::time::Duration;

#[derive(Default)]
pub struct Doorbell {
    thread: OnceLock<Thread>,
    /// Set by a ring, taken by the next wait: a ring is never lost, even one
    /// before anyone answers.
    rung: AtomicBool,
}

impl Doorbell {
    /// Called once by the thread that waits, before it first waits.
    pub fn answer_here(&self) {
        let _ = self.thread.set(thread::current());
    }

    /// RT-safe: never blocks (see docs/CONCEPT.md, "RT audio path").
    pub fn ring(&self) {
        self.rung.store(true, Ordering::Release);
        if let Some(t) = self.thread.get() {
            t.unpark();
        }
    }

    /// Sleeps until rung or `timeout` passes; returns at once if rung since
    /// the last wait. Call only from the thread that answered.
    pub fn wait(&self, timeout: Duration) {
        if !self.rung.swap(false, Ordering::Acquire) {
            thread::park_timeout(timeout);
            self.rung.store(false, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn a_ring_before_anyone_answers_is_kept() {
        let bell = Doorbell::default();
        bell.ring();
        bell.answer_here();
        let started = Instant::now();
        bell.wait(Duration::from_secs(30));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the early ring was lost"
        );
    }
}
