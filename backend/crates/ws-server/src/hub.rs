//! Fan-out hub: mux-router [`gtmux_mux_router::Event`]s → WS subscribers + per-pane
//! ring buffers for catch-up replay on new attaches.
//!
//! 정본:
//! - `docs/ssot/wire-protocol.md` §2 (12 frame slot ↔ Event mapping)
//! - `docs/adr/0001-tmux-integration-control-mode.md` D7 (`%output` →
//!   per-pane ring buffer → WS binary frame), D8 (Panel Streaming State
//!   pause/continue), D12 (notify mirroring)
//! - `docs/reports/0010-grill-amendments.md` D13 (MT-3 Live Mirror — all
//!   subscribers see the same event stream), D15 (128 KiB ring per pane).
//!
//! The hub holds two pieces of shared state:
//!   * `events: broadcast::Sender<Event>` — every subscriber observes the
//!     same `Event` stream after they call [`Hub::subscribe`]. Capacity is
//!     [`HUB_BROADCAST_CAPACITY`]; lagged subscribers see a
//!     `RecvError::Lagged` and the WS handler re-syncs via [`Hub::snapshot`].
//!   * `ring_buffers: RwLock<HashMap<u32, RingBuffer>>` — per-pane terminal
//!     state, capped per [`crate::ring::RING_BUFFER_CAPACITY`]. Used at
//!     attach time to replay the last bytes the user would have seen.
//!
//! [`Hub::publish`] is the *only* mutator of both pieces of state. The
//! event loop in `gtmux_lifecycle::run_event_loop` is the sole producer in
//! production; tests call `publish` directly.

use std::collections::HashMap;
use std::sync::Arc;

use gtmux_mux_router::Event;
use tokio::sync::{broadcast, RwLock};

use crate::ring::RingBuffer;

/// Broadcast channel capacity for the live event fan-out.
///
/// Sized for the steady-state scenario in `docs/reports/0010-grill-amendments.md`
/// D19 (50 panes × occasional burst). Each subscriber gets its own
/// independent queue at this depth — when a subscriber lags past the cap it
/// gets `RecvError::Lagged` and the WS handler restarts from a fresh
/// snapshot instead of dropping the connection.
pub const HUB_BROADCAST_CAPACITY: usize = 256;

/// Fan-out hub. Cheap to clone — internally holds two `Arc`s.
#[derive(Debug, Clone)]
pub struct Hub {
    events: broadcast::Sender<Event>,
    ring_buffers: Arc<RwLock<HashMap<u32, RingBuffer>>>,
}

impl Hub {
    /// Construct an empty hub. No subscribers, no ring buffers.
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(HUB_BROADCAST_CAPACITY);
        Self {
            events,
            ring_buffers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Subscribe to the live event stream. Each subscriber gets an
    /// independent queue — see [`HUB_BROADCAST_CAPACITY`].
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    /// Publish an event to all subscribers.
    ///
    /// Side effect: when the event carries `%output` bytes (either
    /// [`Event::Output`] or [`Event::ExtendedOutput`]) the bytes are
    /// appended to the matching pane's ring buffer before broadcast. This
    /// ordering matters — a new subscriber that attaches *between* the
    /// append and the broadcast still sees a coherent view, because
    /// `subscribe` only delivers events sent *after* it returns and the
    /// ring buffer already contains the bytes that preceded it.
    ///
    /// SAFETY/CORRECTNESS NOTE: the `RwLock` is held only across the
    /// `append` call (a memcpy + bounded VecDeque mutation). No
    /// async-await sits inside the critical section, so a stuck subscriber
    /// cannot wedge the producer.
    pub async fn publish(&self, event: Event) {
        // 1) Update per-pane ring buffer for output events. Ignore the
        //    broadcast result — when there are no subscribers the channel
        //    returns `Err`; that is the normal idle state, not an error.
        match &event {
            Event::Output { pane_id, bytes } => {
                let mut bufs = self.ring_buffers.write().await;
                bufs.entry(*pane_id)
                    .or_insert_with(RingBuffer::new)
                    .append(bytes);
            }
            Event::ExtendedOutput { pane_id, bytes, .. } => {
                let mut bufs = self.ring_buffers.write().await;
                bufs.entry(*pane_id)
                    .or_insert_with(RingBuffer::new)
                    .append(bytes);
            }
            _ => {}
        }
        // 2) Broadcast. `Err` here means "no live subscribers" — fine; we
        //    have already persisted the bytes (if any) to the ring buffer
        //    so a future subscriber will catch up via `snapshot`.
        let _ = self.events.send(event);
    }

    /// Borrow the per-pane ring buffer snapshot for *every* pane that has
    /// ever produced output. Returned in unspecified order — callers that
    /// care about reproducible replay must sort by `pane_id`.
    ///
    /// Returns owned `Vec<u8>` per pane so the caller may release the lock
    /// before sending the bytes over the (potentially slow) WS sink.
    pub async fn snapshot_all(&self) -> Vec<(u32, Vec<u8>)> {
        let bufs = self.ring_buffers.read().await;
        let mut out: Vec<(u32, Vec<u8>)> = bufs
            .iter()
            .filter(|(_, rb)| !rb.is_empty())
            .map(|(pane_id, rb)| (*pane_id, rb.snapshot()))
            .collect();
        out.sort_by_key(|(pane_id, _)| *pane_id);
        out
    }

    /// Borrow one pane's snapshot. `None` if the pane has not yet produced
    /// any output (or if the buffer was drained past the cap by a later
    /// burst — currently never, but reserved for future eviction policy).
    pub async fn snapshot(&self, pane_id: u32) -> Option<Vec<u8>> {
        let bufs = self.ring_buffers.read().await;
        bufs.get(&pane_id).map(RingBuffer::snapshot)
    }

    /// Current number of live subscribers — useful in tests and metrics.
    pub fn subscriber_count(&self) -> usize {
        self.events.receiver_count()
    }
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_with_no_subscribers_is_silent() {
        let hub = Hub::new();
        // Must not panic / error even though nobody is listening.
        hub.publish(Event::Output {
            pane_id: 1,
            bytes: b"hello".to_vec(),
        })
        .await;
        // Ring buffer still captured the bytes — verifies the "no
        // subscriber" path still persists output for later catch-up.
        let snap = hub.snapshot(1).await.unwrap();
        assert_eq!(snap, b"hello".to_vec());
    }

    #[tokio::test]
    async fn snapshot_unknown_pane_is_none() {
        let hub = Hub::new();
        assert!(hub.snapshot(42).await.is_none());
    }

    #[tokio::test]
    async fn snapshot_all_orders_by_pane_id() {
        let hub = Hub::new();
        hub.publish(Event::Output {
            pane_id: 5,
            bytes: b"five".to_vec(),
        })
        .await;
        hub.publish(Event::Output {
            pane_id: 1,
            bytes: b"one".to_vec(),
        })
        .await;
        let all = hub.snapshot_all().await;
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].0, 1);
        assert_eq!(all[1].0, 5);
        assert_eq!(all[0].1, b"one".to_vec());
        assert_eq!(all[1].1, b"five".to_vec());
    }

    #[tokio::test]
    async fn extended_output_also_lands_in_ring_buffer() {
        let hub = Hub::new();
        hub.publish(Event::ExtendedOutput {
            pane_id: 3,
            age_ms: 1234,
            bytes: b"ext".to_vec(),
        })
        .await;
        assert_eq!(hub.snapshot(3).await.unwrap(), b"ext".to_vec());
    }

    #[tokio::test]
    async fn non_output_events_do_not_touch_ring_buffer() {
        let hub = Hub::new();
        hub.publish(Event::Pause { pane_id: 7 }).await;
        hub.publish(Event::SessionsChanged).await;
        assert!(hub.snapshot(7).await.is_none());
        assert!(hub.snapshot_all().await.is_empty());
    }
}
