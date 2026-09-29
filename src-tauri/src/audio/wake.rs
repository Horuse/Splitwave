//! Wakes a helper thread from the audio path without blocking it: the helper
//! sleeps until there is work, the audio path rings once it has left some.

use std::sync::OnceLock;
use std::thread::{self, Thread};
use std::time::Duration;

#[derive(Default)]
pub struct Doorbell {
    thread: OnceLock<Thread>,
}

impl Doorbell {
    /// Called once by the thread that waits, before it first waits.
    pub fn answer_here(&self) {
        let _ = self.thread.set(thread::current());
    }

    /// RT-safe: never blocks (see docs/CONCEPT.md, "RT audio path"). A ring
    /// before anyone answers is dropped; the waiter's timeout covers it.
    pub fn ring(&self) {
        if let Some(t) = self.thread.get() {
            t.unpark();
        }
    }

    /// Sleeps until rung or `timeout` passes. Call only from the thread that
    /// answered.
    pub fn wait(&self, timeout: Duration) {
        thread::park_timeout(timeout);
    }
}
