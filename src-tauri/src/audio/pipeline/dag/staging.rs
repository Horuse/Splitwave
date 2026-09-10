use crate::audio::health;

/// Fixed-capacity FIFO; allocates once. Overrun clamps and counts drops --
/// wrapping the write head past the read head would corrupt subsequent pops.
pub(super) struct StagingRing {
    buf: Box<[f32]>,
    head: usize,
    tail: usize,
    len: usize,
    dropped: u64,
}

impl StagingRing {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: vec![0.0_f32; capacity].into_boxed_slice(),
            head: 0,
            tail: 0,
            len: 0,
            dropped: 0,
        }
    }

    #[inline]
    pub(super) fn len(&self) -> usize {
        self.len
    }

    #[allow(dead_code)]
    #[inline]
    pub(super) fn dropped(&self) -> u64 {
        self.dropped
    }

    pub(super) fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.len = 0;
    }

    pub(super) fn pop_into(&mut self, dst: &mut [f32]) -> usize {
        let n = dst.len().min(self.len);
        let cap = self.buf.len();
        for slot in dst.iter_mut().take(n) {
            *slot = self.buf[self.head];
            self.head = if self.head + 1 == cap {
                0
            } else {
                self.head + 1
            };
        }
        self.len -= n;
        n
    }

    pub(super) fn extend_from_slice(&mut self, src: &[f32]) {
        let cap = self.buf.len();
        let free = cap - self.len;
        debug_assert!(
            src.len() <= free,
            "StagingRing overrun: have {} + {} new > cap {}",
            self.len,
            src.len(),
            cap
        );
        let take = src.len().min(free);
        for &v in &src[..take] {
            self.buf[self.tail] = v;
            self.tail = if self.tail + 1 == cap {
                0
            } else {
                self.tail + 1
            };
        }
        self.len += take;
        let overrun = (src.len() - take) as u64;
        self.dropped = self.dropped.saturating_add(overrun);
        health::bump(&health::STAGING_OVERRUN_SAMPLES, overrun);
    }
}
