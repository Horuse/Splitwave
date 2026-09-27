//! Test-only allocation guard for real-time code paths.
//!
//! The test binary's global allocator counts allocations made on a thread
//! while that thread is inside `assert_no_alloc`, so a test can prove an audio
//! callback path never touches the heap once it is running.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct CountingAlloc;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn note() {
    // `try_with`: the allocator also runs while thread-locals are torn down.
    let armed = ARMED.try_with(Cell::get).unwrap_or(false);
    if armed {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
    }
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        System.realloc(ptr, layout, new_size)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note();
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

/// Runs `f` and panics if it allocated, reallocated or freed on this thread.
pub fn assert_no_alloc<R>(what: &str, f: impl FnOnce() -> R) -> R {
    COUNT.with(|c| c.set(0));
    ARMED.with(|a| a.set(true));
    let out = f();
    ARMED.with(|a| a.set(false));
    let n = COUNT.with(Cell::get);
    assert_eq!(n, 0, "{what}: {n} heap operations on the real-time path");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_an_allocation() {
        let caught = std::panic::catch_unwind(|| {
            assert_no_alloc("vec", || {
                let v: Vec<u8> = Vec::with_capacity(16);
                std::hint::black_box(v);
            })
        });
        assert!(caught.is_err());
    }

    #[test]
    fn passes_allocation_free_code() {
        let mut buf = [0.0_f32; 64];
        assert_no_alloc("fill", || buf.fill(1.0));
        assert_eq!(buf[63], 1.0);
    }
}
