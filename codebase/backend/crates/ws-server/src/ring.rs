//! Per-pane ring buffer for `%output` catch-up replay.
//!
//! 정본:
//! - ADR-0001 D7 (`docs/adr/0001-tmux-integration-control-mode.md`):
//!   "디코딩된 바이트를 페인별 ring buffer에 append. 크기 = 128 KB 기본 …
//!   메모리 전용 — disk 영속화 금지."
//! - `docs/reports/0010-grill-amendments.md` D15 (ring buffer 128 KB cap).
//!
//! The buffer is a byte queue with a hard size cap. When `append` would push
//! the contents past [`RING_BUFFER_CAPACITY`], the oldest bytes are dropped
//! until the new payload fits. This is the simplest catch-up policy that
//! preserves the most recent terminal state — the only thing the user can
//! still interpret after a long suspension.

use std::collections::VecDeque;

/// Per-pane ring buffer capacity in bytes (ADR-0001 D7, Grill D15).
///
/// Sized to balance terminal-history usefulness against backend RAM: at
/// 50 panes × 128 KiB the worst-case footprint is 6.4 MiB, well within
/// ADR-0011's `< 30 MB` baseline target.
pub const RING_BUFFER_CAPACITY: usize = 128 * 1024;

/// Bounded byte queue that drops oldest bytes when the cap is exceeded.
///
/// Cloning is intentionally NOT derived — a clone would silently double the
/// peak memory budget. Callers wanting a snapshot use [`Self::snapshot`],
/// which produces a `Vec<u8>` that is decoupled from the live buffer.
#[derive(Debug, Default)]
pub struct RingBuffer {
    inner: VecDeque<u8>,
}

impl RingBuffer {
    /// Create an empty buffer. Capacity is pre-allocated up to the cap so the
    /// first burst does not pay growth cost (the same memory we'd hold at
    /// steady state — no overcommit).
    pub fn new() -> Self {
        Self {
            inner: VecDeque::with_capacity(RING_BUFFER_CAPACITY),
        }
    }

    /// Number of bytes currently buffered.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// `true` when nothing has been written (or everything has been drained
    /// past the cap and a zero-length burst arrived).
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Append `bytes`. If the new total would exceed [`RING_BUFFER_CAPACITY`],
    /// the oldest bytes are dropped from the front until the cap fits. A
    /// single burst larger than the cap keeps only its trailing
    /// [`RING_BUFFER_CAPACITY`] bytes (oldest bytes within the burst itself
    /// are dropped, matching tmux's `pause-after` semantics where the most
    /// recent state is the only meaningful UX surface).
    pub fn append(&mut self, bytes: &[u8]) {
        if bytes.len() >= RING_BUFFER_CAPACITY {
            // Burst alone overruns the cap. Discard everything older and
            // keep only the trailing window.
            self.inner.clear();
            let tail = &bytes[bytes.len() - RING_BUFFER_CAPACITY..];
            self.inner.extend(tail.iter().copied());
            return;
        }
        let combined = self.inner.len() + bytes.len();
        if combined > RING_BUFFER_CAPACITY {
            let drop = combined - RING_BUFFER_CAPACITY;
            // `drain` on the front of a VecDeque is O(drop) but does not
            // reallocate — the underlying ring slots are reused.
            self.inner.drain(..drop);
        }
        self.inner.extend(bytes.iter().copied());
    }

    /// Copy the current contents into a contiguous `Vec`. The snapshot is
    /// owned — the live buffer can continue to mutate without affecting it.
    /// Used by the WS handler at attach time to send a PANE_OUT replay frame.
    pub fn snapshot(&self) -> Vec<u8> {
        let (front, back) = self.inner.as_slices();
        let mut out = Vec::with_capacity(front.len() + back.len());
        out.extend_from_slice(front);
        out.extend_from_slice(back);
        out
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn append_under_cap_preserves_all_bytes() {
        let mut rb = RingBuffer::new();
        rb.append(b"hello ");
        rb.append(b"world");
        assert_eq!(rb.snapshot(), b"hello world".to_vec());
        assert_eq!(rb.len(), 11);
    }

    #[test]
    fn append_at_cap_keeps_full_window() {
        let mut rb = RingBuffer::new();
        let exact = vec![0xAAu8; RING_BUFFER_CAPACITY];
        rb.append(&exact);
        assert_eq!(rb.len(), RING_BUFFER_CAPACITY);
        assert_eq!(rb.snapshot(), exact);
    }

    #[test]
    fn append_over_cap_drops_oldest_bytes() {
        let mut rb = RingBuffer::new();
        // Fill with 'A', then push 100 bytes of 'B' — first 100 'A's are gone.
        rb.append(&[b'A'; RING_BUFFER_CAPACITY]);
        rb.append(&[b'B'; 100]);
        let snap = rb.snapshot();
        assert_eq!(snap.len(), RING_BUFFER_CAPACITY);
        // Tail = 100 'B's.
        assert!(snap.ends_with(&[b'B'; 100]));
        // Head no longer starts with 'A' at position 0 of the original burst.
        assert_eq!(snap[0], b'A');
        // The first 100 'A's are gone — count remaining A's.
        let a_count = snap.iter().filter(|&&b| b == b'A').count();
        assert_eq!(a_count, RING_BUFFER_CAPACITY - 100);
    }

    #[test]
    fn append_single_burst_larger_than_cap() {
        let mut rb = RingBuffer::new();
        // Pre-existing content that must be dropped.
        rb.append(b"older content");
        // A single burst of 2× cap — buffer must equal the trailing window.
        let mut big = Vec::with_capacity(RING_BUFFER_CAPACITY * 2);
        for i in 0..(RING_BUFFER_CAPACITY * 2) {
            big.push((i & 0xFF) as u8);
        }
        rb.append(&big);
        let snap = rb.snapshot();
        assert_eq!(snap.len(), RING_BUFFER_CAPACITY);
        assert_eq!(snap, big[big.len() - RING_BUFFER_CAPACITY..]);
    }

    #[test]
    fn snapshot_is_independent_of_live_buffer() {
        let mut rb = RingBuffer::new();
        rb.append(b"first");
        let snap = rb.snapshot();
        rb.append(b" second");
        // Old snapshot must still equal the original content.
        assert_eq!(snap, b"first".to_vec());
        assert_eq!(rb.snapshot(), b"first second".to_vec());
    }

    #[test]
    fn empty_buffer_snapshot_is_empty() {
        let rb = RingBuffer::new();
        assert!(rb.is_empty());
        assert_eq!(rb.snapshot(), Vec::<u8>::new());
    }
}
