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

/// Fan-out hub. Cheap to clone — internally holds three `Arc`s.
#[derive(Debug, Clone)]
pub struct Hub {
    events: broadcast::Sender<Event>,
    ring_buffers: Arc<RwLock<HashMap<u32, RingBuffer>>>,
    /// Most-recent `%session-changed` payload. tmux emits this exactly once
    /// per control-mode attach (typically at boot), so late WS subscribers
    /// would miss it without an explicit catch-up channel. We mirror the
    /// payload here and replay it from [`handle_socket`] just before the
    /// ring-buffer flush.
    last_session: Arc<RwLock<Option<(u32, String)>>>,
    /// Web-domain LAYOUT_CHANGED broadcast. Carries the 16-byte raw ETag
    /// of the most recently committed canvas layout. Separate channel
    /// from `events` because the ordering vs. mux events is independent
    /// (LAYOUT_CHANGED is idempotent — clients revalidate via
    /// `If-None-Match`) and conflating them would bloat `Event`.
    layout_events: broadcast::Sender<[u8; 16]>,
}

impl Hub {
    /// Construct an empty hub. No subscribers, no ring buffers.
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(HUB_BROADCAST_CAPACITY);
        let (layout_events, _) = broadcast::channel(16);
        Self {
            events,
            ring_buffers: Arc::new(RwLock::new(HashMap::new())),
            last_session: Arc::new(RwLock::new(None)),
            layout_events,
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
            Event::SessionChanged { session_id, name } => {
                // Cache for late subscribers — tmux only emits this on
                // control-mode attach, so WS clients that connect after
                // boot would otherwise never see it.
                let mut slot = self.last_session.write().await;
                *slot = Some((*session_id, name.clone()));
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

    /// Most-recent cached `%session-changed`, if one has been observed.
    /// `None` until tmux's control-mode attach emits the first event.
    pub async fn snapshot_session(&self) -> Option<(u32, String)> {
        self.last_session.read().await.clone()
    }

    /// Broadcast a new canvas-layout ETag to every live WS subscriber.
    /// The HTTP `layout_put_handler` calls this after a successful PUT so
    /// the SPA can revalidate via `If-None-Match` and re-hydrate its
    /// panels store.
    pub fn publish_layout_changed(&self, etag: [u8; 16]) {
        // Send result is `Err` only when nobody is subscribed — fine for
        // pre-WS-connection puts. Drop silently.
        let _ = self.layout_events.send(etag);
    }

    /// Subscribe to the layout-change broadcast.
    pub fn subscribe_layout(&self) -> broadcast::Receiver<[u8; 16]> {
        self.layout_events.subscribe()
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

    #[tokio::test]
    async fn session_changed_cached_for_late_subscriber() {
        let hub = Hub::new();
        // No publish yet — snapshot must be None.
        assert!(hub.snapshot_session().await.is_none());
        // Publish before any subscriber exists (the broadcast send returns
        // Err in this state, which is the normal startup race).
        hub.publish(Event::SessionChanged {
            session_id: 0,
            name: "demo".to_string(),
        })
        .await;
        let snap = hub.snapshot_session().await.expect("cache populated");
        assert_eq!(snap, (0, "demo".to_string()));

        // A later publish replaces the cache wholesale.
        hub.publish(Event::SessionChanged {
            session_id: 1,
            name: "work".to_string(),
        })
        .await;
        assert_eq!(
            hub.snapshot_session().await.unwrap(),
            (1, "work".to_string())
        );
    }
}
