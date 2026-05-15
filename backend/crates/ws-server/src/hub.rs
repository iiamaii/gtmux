//! Hub — PtyBackend wrapper + WS-facing fan-out channels.
//!
//! After ADR-0013 (PTY direct, no tmux) the old per-Event hub model is gone.
//! The replacement responsibilities are narrow:
//!
//! - **layout_events** — broadcast a 16-byte ETag every time the layout
//!   JSON is overwritten on disk. Consumed by the WS handler so the SPA
//!   can revalidate via `If-None-Match` (kept identical to the legacy API
//!   so `gtmux_http_api` did not need to change shape — see line 122/144
//!   of `crates/http-api/src/lib.rs`).
//! - **pane_output** — single multiplexed `(PaneId, Bytes)` broadcast.
//!   Internally a background task subscribes to every Pane's per-pane
//!   broadcast in [`gtmux_pty_backend`] and forwards the bytes here,
//!   so the WS handler can drive ALL panes off one `select!` arm
//!   without dynamic `StreamMap` gymnastics.
//! - **backend access** — [`Hub::backend`] exposes the supervisor so the
//!   WS handler can call `send_input`, `resize`, `kill`, `spawn`, etc.,
//!   and so test code can inspect pane state without a custom shim.
//!
//! Catch-up replay (per-pane ring buffer flush at WS attach) is owned by
//! the WS handler itself, *not* by Hub — the handler iterates
//! `backend.pane_ids()` and calls `backend.subscribe_output(id).0` to get
//! the ring snapshot. Putting the catch-up in Hub would force per-subscriber
//! state that the broadcast model would never naturally express.

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use gtmux_pty_backend::{BackendNotify, PaneId, PtyBackend};
use tokio::sync::broadcast;
use tracing::debug;

/// Fan-out channel depth for the multiplexed pane output stream. Sized
/// for 50-pane × occasional-burst (mirrors the legacy `HUB_BROADCAST_CAPACITY`
/// value before §2.4 of `docs/reports/0026-stage-b-carry-forward.md` —
/// keep until measurement says otherwise).
pub const HUB_BROADCAST_CAPACITY: usize = 256;

/// Fan-out channel depth for the layout-changed signal. 16 is generous —
/// the layout PUT is human-paced.
const LAYOUT_BROADCAST_CAPACITY: usize = 16;

/// Fan-out channel depth for the terminal-died signal (Stage 5-B). A pane
/// dying is a low-frequency event (~one per kill or process exit), so a
/// small queue is enough; same order-of-magnitude as the layout broadcast.
const TERMINAL_DIED_BROADCAST_CAPACITY: usize = 32;

/// Payload of a `TerminalDied` broadcast. `uuid` is the schema-side
/// terminal id; `reason` is `"exit"` (process self-exited) or `"killed"`
/// (signal-driven exit). `Arc<str>` over `String` so the broadcast clone
/// per WS subscriber is a refcount bump, not a heap copy.
#[derive(Clone, Debug)]
pub struct TerminalDiedEvent {
    pub uuid: Arc<str>,
    pub reason: &'static str,
}

/// `Hub` is cheap to clone — internal state is `Arc<…>` + broadcast senders.
#[derive(Clone)]
pub struct Hub {
    backend: PtyBackend,
    /// Multiplexed `(pane_id, bytes)` live output stream. Each WS subscriber
    /// receives an independent queue at [`HUB_BROADCAST_CAPACITY`] depth.
    pane_output: broadcast::Sender<(PaneId, Bytes)>,
    /// Web-domain LAYOUT_CHANGED broadcast — 16-byte ETag of the most
    /// recently committed canvas layout. Kept name-compatible with the
    /// pre-Stage-B Hub so `gtmux_http_api::layout_put_handler` keeps
    /// working without an edit.
    layout_events: broadcast::Sender<[u8; 16]>,
    /// JoinHandle of the multiplexer driver task. Kept inside an `Arc`
    /// so cloning `Hub` is cheap; the task lives for the lifetime of the
    /// Server (it exits when both the backend notify channel and the
    /// existing per-pane broadcasts have all closed — i.e. when
    /// [`PtyBackend`]'s last clone is dropped).
    _mux_task: Arc<tokio::task::JoinHandle<()>>,
    /// Optional disconnect sink (ADR-0019 D6 + ADR-0021 D6). The WS handler
    /// emits each closing connection's cookie value here so a downstream
    /// consumer (the http-api layer) can release the cross-server session
    /// lock automatically. `None` when no consumer has registered — the
    /// channel is then a no-op and the lock is released only via explicit
    /// `DELETE /api/sessions/:name/attach`.
    disconnect_tx: Arc<std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>>,
    /// Optional heartbeat sink (ADR-0019 D6.2). The WS handler emits the
    /// connection's cookie value on every Ping/Pong receive so the http-api
    /// layer can refresh the matching `.lock` file's `lease_until_unix`
    /// field — keeping the modal expiry hint accurate without blocking the
    /// OS-flock truth.
    heartbeat_tx: Arc<std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>>,
    /// Cookie → session_name registry (Stage 5-A / ADR-0021 D5). The
    /// http-api `attach_handler` writes here after a successful flock + cookie
    /// reverse-map insert; `detach_handler` (and the WS-disconnect-driven
    /// `release_lock_for_cookie`) clears the entry. The WS handler consults
    /// this map when routing session-scoped envelopes (5-C) so a frame
    /// emitted on session A is never delivered to a subscriber whose cookie
    /// is attached to session B.
    ///
    /// `std::sync::RwLock` over `tokio::sync::RwLock`: every operation here
    /// is a sub-microsecond hash-table touch with no `.await` point inside
    /// the critical section — mirrors the existing sink pattern above.
    session_table: Arc<std::sync::RwLock<HashMap<String, String>>>,
    /// UUID-carrying terminal-death broadcast (Stage 5-B / ADR-0021 D5).
    /// Published by the http-api `handle_pane_died` consumer once the
    /// matching UUID has been resolved from [`crate::ring`]'s map. Each WS
    /// subscriber turns one event into one `0x85 TERMINAL_DIED` envelope.
    /// Always server-wide — a terminal can be mirrored into multiple
    /// sessions (ADR-0021 D1), and routing-by-session belongs to the
    /// session-scoped frame family (5-C), not here.
    terminal_died_events: broadcast::Sender<TerminalDiedEvent>,
}

impl Hub {
    /// Build a hub backed by `backend`. Spawns the multiplexer driver task
    /// before returning, so subscribers attached immediately afterwards
    /// observe every byte emitted from this point forward.
    pub fn new(backend: PtyBackend) -> Self {
        let (pane_output, _) = broadcast::channel(HUB_BROADCAST_CAPACITY);
        let (layout_events, _) = broadcast::channel(LAYOUT_BROADCAST_CAPACITY);
        let (terminal_died_events, _) = broadcast::channel(TERMINAL_DIED_BROADCAST_CAPACITY);

        let mux_backend = backend.clone();
        let mux_tx = pane_output.clone();
        let mux_task = tokio::spawn(async move {
            run_multiplexer(mux_backend, mux_tx).await;
        });

        Self {
            backend,
            pane_output,
            layout_events,
            _mux_task: Arc::new(mux_task),
            disconnect_tx: Arc::new(std::sync::Mutex::new(None)),
            heartbeat_tx: Arc::new(std::sync::Mutex::new(None)),
            session_table: Arc::new(std::sync::RwLock::new(HashMap::new())),
            terminal_died_events,
        }
    }

    /// Register a sink that receives the cookie value of every closing WS
    /// connection (ADR-0019 D6 / ADR-0021 D6). The http-api layer uses this
    /// to auto-release any cross-server session lock the cookie still holds.
    /// Replaces a previously-registered sink; safe to call multiple times.
    pub fn set_disconnect_sink(&self, tx: tokio::sync::mpsc::UnboundedSender<String>) {
        if let Ok(mut slot) = self.disconnect_tx.lock() {
            *slot = Some(tx);
        }
    }

    /// Register a sink that receives the cookie value of every Ping/Pong on
    /// any live WS connection (ADR-0019 D6.2). The http-api layer uses this
    /// to refresh the `.lock` file's `lease_until_unix` body.
    pub fn set_heartbeat_sink(&self, tx: tokio::sync::mpsc::UnboundedSender<String>) {
        if let Ok(mut slot) = self.heartbeat_tx.lock() {
            *slot = Some(tx);
        }
    }

    /// Snapshot of the current disconnect sink. The WS handler clones this
    /// so a freshly-registered sink that arrives mid-connection is honoured
    /// only by subsequent connections — never an in-flight one (avoids a
    /// half-state where a new sink misses a "closing" event from a socket
    /// whose snapshot still pointed at the old sink).
    pub fn disconnect_sink(&self) -> Option<tokio::sync::mpsc::UnboundedSender<String>> {
        self.disconnect_tx.lock().ok().and_then(|s| s.clone())
    }

    /// Snapshot of the current heartbeat sink. See [`disconnect_sink`] for
    /// the cloning rationale.
    pub fn heartbeat_sink(&self) -> Option<tokio::sync::mpsc::UnboundedSender<String>> {
        self.heartbeat_tx.lock().ok().and_then(|s| s.clone())
    }

    /// Record that `cookie` is currently attached to `session_name`
    /// (Stage 5-A / ADR-0021 D5). Called by the http-api `attach_handler`
    /// after a successful flock + cookie reverse-map insert. Idempotent:
    /// re-attaching the same cookie to a different session replaces the
    /// entry; re-attaching to the same session is a no-op write.
    ///
    /// A poisoned [`std::sync::RwLock`] is treated as a soft failure: the
    /// update is dropped and a warn-level trace fires. The kernel-level
    /// session lock is the source of truth — this table only steers WS
    /// frame routing, so a missed update degrades to "session-scoped frame
    /// behaves like server-wide" until the next attach refreshes it.
    pub fn set_session_for_cookie(&self, cookie: &str, session_name: &str) {
        match self.session_table.write() {
            Ok(mut t) => {
                t.insert(cookie.to_string(), session_name.to_string());
            }
            Err(_) => {
                tracing::warn!(
                    cookie_len = cookie.len(),
                    "hub session_table: write poisoned; skipping set_session_for_cookie"
                );
            }
        }
    }

    /// Drop the cookie → session_name binding for `cookie`. Called by the
    /// http-api `detach_handler` and by the WS-disconnect-driven
    /// `release_lock_for_cookie`. Idempotent: a missing entry is a no-op.
    pub fn clear_session_for_cookie(&self, cookie: &str) {
        match self.session_table.write() {
            Ok(mut t) => {
                t.remove(cookie);
            }
            Err(_) => {
                tracing::warn!(
                    cookie_len = cookie.len(),
                    "hub session_table: write poisoned; skipping clear_session_for_cookie"
                );
            }
        }
    }

    /// Look up the session that `cookie` is currently attached to.
    /// Returns `None` when the cookie has no active attach or the table is
    /// poisoned. Used by the WS dispatcher (Stage 5-C) to decide whether
    /// a session-scoped envelope is in-scope for a given connection.
    pub fn session_for_cookie(&self, cookie: &str) -> Option<String> {
        self.session_table
            .read()
            .ok()
            .and_then(|t| t.get(cookie).cloned())
    }

    /// Remove every cookie entry currently pointing at `session_name`.
    /// Called by the http-api `detach_handler`, which mirrors the same
    /// `retain(|_, v| v != name)` behaviour on `session_locks_by_cookie` —
    /// the two maps must stay in lock-step or the WS dispatcher would
    /// surface stale "still attached" routing for cookies that just
    /// detached.
    pub fn clear_sessions_by_name(&self, session_name: &str) {
        match self.session_table.write() {
            Ok(mut t) => {
                t.retain(|_, v| v != session_name);
            }
            Err(_) => {
                tracing::warn!(
                    session = %session_name,
                    "hub session_table: write poisoned; skipping clear_sessions_by_name"
                );
            }
        }
    }

    /// Borrow the underlying [`PtyBackend`]. The WS handler uses this for
    /// `send_input` / `resize` / `kill` / `spawn` / `subscribe_output`
    /// (ring snapshot for catch-up replay).
    pub fn backend(&self) -> &PtyBackend {
        &self.backend
    }

    /// Subscribe to the multiplexed live pane-output stream. Every WS
    /// connection should subscribe *before* doing catch-up replay so
    /// bytes emitted during the replay window are not lost.
    pub fn subscribe_pane_output(&self) -> broadcast::Receiver<(PaneId, Bytes)> {
        self.pane_output.subscribe()
    }

    /// Subscribe to backend-level notifications (`pane-spawned`,
    /// `pane-died`, `layout-changed`, `server-ready`). The WS handler
    /// translates each variant to the matching `0x07 NOTIFY_MIRROR`
    /// envelope.
    pub fn subscribe_notify(&self) -> broadcast::Receiver<BackendNotify> {
        self.backend.subscribe_notify()
    }

    /// Broadcast a new canvas-layout ETag. Called from
    /// `gtmux_http_api::layout_put_handler` after a successful PUT —
    /// signature preserved across Stage B for API compatibility.
    pub fn publish_layout_changed(&self, etag: [u8; 16]) {
        // `Err` only means "no subscribers"; that is the normal startup
        // state, not an error.
        let _ = self.layout_events.send(etag);
    }

    /// Subscribe to the layout-change broadcast.
    pub fn subscribe_layout(&self) -> broadcast::Receiver<[u8; 16]> {
        self.layout_events.subscribe()
    }

    /// Broadcast a UUID-carrying terminal-died event (Stage 5-B). Called by
    /// `AppState::handle_pane_died` once the kernel/SIGCHLD-driven death
    /// has been resolved to a schema UUID. The send is silent on
    /// "no subscribers" — that's the normal idle state, identical to
    /// [`publish_layout_changed`].
    pub fn publish_terminal_died(&self, uuid: &str, reason: &'static str) {
        let _ = self.terminal_died_events.send(TerminalDiedEvent {
            uuid: Arc::from(uuid),
            reason,
        });
    }

    /// Subscribe to the UUID-carrying terminal-died broadcast.
    pub fn subscribe_terminal_died(&self) -> broadcast::Receiver<TerminalDiedEvent> {
        self.terminal_died_events.subscribe()
    }

    /// Live subscriber count on the multiplexed pane-output channel.
    /// Used in tests + future operational dashboards.
    pub fn subscriber_count(&self) -> usize {
        self.pane_output.receiver_count()
    }
}

/// Multiplexer driver: subscribe to every Pane's per-pane broadcast in
/// [`PtyBackend`] and fan the bytes into `tx`. Tracks newly-spawned panes
/// via the backend's notify channel.
async fn run_multiplexer(backend: PtyBackend, tx: broadcast::Sender<(PaneId, Bytes)>) {
    let mut notify = backend.subscribe_notify();

    // Hook up every pane that already exists. In normal startup the
    // backend is freshly constructed and `pane_ids()` is empty, but a
    // Hub built around a *re-attached* backend (future feature) would
    // need this.
    for id in backend.pane_ids() {
        spawn_pane_forwarder(&backend, id, tx.clone());
    }

    // Subscribe to PaneSpawned events to wire up forwarders for future
    // panes. PaneDied / LayoutChanged / ServerReady are *not* relevant
    // to the multiplexer (those flow on a separate notify channel that
    // each WS subscriber consumes directly via `subscribe_notify`); we
    // pattern-match exhaustively so future variants flag a compile
    // error when added.
    while let Ok(n) = notify.recv().await {
        match n {
            BackendNotify::PaneSpawned { id, .. } => {
                spawn_pane_forwarder(&backend, id, tx.clone());
            }
            BackendNotify::PaneDied { .. }
            | BackendNotify::LayoutChanged
            | BackendNotify::ServerReady => {
                // not our concern — handled by per-WS subscribers directly
            }
        }
    }
    debug!("hub multiplexer: backend notify closed, exiting");
}

/// Spawn one forwarder task per pane. The task drains the pane's
/// per-pane broadcast and forwards into the multiplexed channel until
/// the pane closes its broadcast (which happens when
/// [`gtmux_pty_backend::PaneHandle`] is dropped).
fn spawn_pane_forwarder(backend: &PtyBackend, id: PaneId, tx: broadcast::Sender<(PaneId, Bytes)>) {
    let Some((_replay, mut rx)) = backend.subscribe_output(id) else {
        // The pane was killed between the spawned notify and this
        // subscribe. Nothing to forward.
        return;
    };
    // Drop the replay snapshot — per-connection catch-up does its own
    // replay via `backend.subscribe_output(id).0` so the WS subscriber
    // controls the ordering against `subscribe_pane_output`.
    drop(_replay);
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(bytes) => {
                    // `Err` from `send` means "no subscribers right now",
                    // which is a normal state (no WS clients attached).
                    // We keep draining so the broadcast cap does not fill
                    // up and stall the pane's reader thread.
                    let _ = tx.send((id, bytes));
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    debug!(pane = %id, skipped = n, "pane forwarder lagged");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!(pane = %id, "pane forwarder: source closed");
                    return;
                }
            }
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_hub_has_no_subscribers() {
        let backend = PtyBackend::new();
        let hub = Hub::new(backend);
        assert_eq!(hub.subscriber_count(), 0);
    }

    #[tokio::test]
    async fn layout_publish_with_no_subscribers_is_silent() {
        let backend = PtyBackend::new();
        let hub = Hub::new(backend);
        // Must not panic / error even though nobody is listening.
        hub.publish_layout_changed([0u8; 16]);
    }

    #[tokio::test]
    async fn layout_subscriber_receives_etag() {
        let backend = PtyBackend::new();
        let hub = Hub::new(backend);
        let mut rx = hub.subscribe_layout();
        let etag = [42u8; 16];
        hub.publish_layout_changed(etag);
        let got = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        assert_eq!(got, etag);
    }

    #[tokio::test]
    async fn terminal_died_publish_silent_without_subscribers() {
        let hub = Hub::new(PtyBackend::new());
        // Must not panic / error.
        hub.publish_terminal_died("uuid", "exit");
    }

    #[tokio::test]
    async fn terminal_died_subscriber_receives_event() {
        let hub = Hub::new(PtyBackend::new());
        let mut rx = hub.subscribe_terminal_died();
        hub.publish_terminal_died("11111111-2222-4333-8444-555555555555", "killed");
        let got = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        assert_eq!(&*got.uuid, "11111111-2222-4333-8444-555555555555");
        assert_eq!(got.reason, "killed");
    }

    #[tokio::test]
    async fn terminal_died_broadcasts_to_multiple_subscribers() {
        let hub = Hub::new(PtyBackend::new());
        let mut r1 = hub.subscribe_terminal_died();
        let mut r2 = hub.subscribe_terminal_died();
        hub.publish_terminal_died("uuid", "exit");
        let a = tokio::time::timeout(std::time::Duration::from_millis(100), r1.recv())
            .await
            .expect("t1")
            .expect("r1");
        let b = tokio::time::timeout(std::time::Duration::from_millis(100), r2.recv())
            .await
            .expect("t2")
            .expect("r2");
        assert_eq!(&*a.uuid, "uuid");
        assert_eq!(&*b.uuid, "uuid");
    }

    #[tokio::test]
    async fn session_table_empty_by_default() {
        let hub = Hub::new(PtyBackend::new());
        assert_eq!(hub.session_for_cookie("anybody"), None);
    }

    #[tokio::test]
    async fn session_table_set_then_lookup() {
        let hub = Hub::new(PtyBackend::new());
        hub.set_session_for_cookie("c1", "demo");
        assert_eq!(hub.session_for_cookie("c1"), Some("demo".to_string()));
        assert_eq!(hub.session_for_cookie("c2"), None);
    }

    #[tokio::test]
    async fn session_table_set_replaces_existing() {
        let hub = Hub::new(PtyBackend::new());
        hub.set_session_for_cookie("c1", "demo");
        hub.set_session_for_cookie("c1", "other");
        assert_eq!(hub.session_for_cookie("c1"), Some("other".to_string()));
    }

    #[tokio::test]
    async fn session_table_clear_is_idempotent() {
        let hub = Hub::new(PtyBackend::new());
        // Clearing a non-existent entry must not panic / error.
        hub.clear_session_for_cookie("nobody");
        hub.set_session_for_cookie("c1", "demo");
        hub.clear_session_for_cookie("c1");
        assert_eq!(hub.session_for_cookie("c1"), None);
        // Second clear is still a no-op.
        hub.clear_session_for_cookie("c1");
        assert_eq!(hub.session_for_cookie("c1"), None);
    }

    #[tokio::test]
    async fn session_table_clear_by_name_drops_matching_cookies() {
        let hub = Hub::new(PtyBackend::new());
        hub.set_session_for_cookie("c1", "demo");
        hub.set_session_for_cookie("c2", "demo");
        hub.set_session_for_cookie("c3", "other");
        hub.clear_sessions_by_name("demo");
        assert_eq!(hub.session_for_cookie("c1"), None);
        assert_eq!(hub.session_for_cookie("c2"), None);
        assert_eq!(hub.session_for_cookie("c3"), Some("other".to_string()));
    }

    #[tokio::test]
    async fn session_table_clones_share_state() {
        // Hub clones share the same Arc-backed session_table — required so
        // the WS handler clone (used in handshake) sees writes performed
        // by the http-api handler clone.
        let h1 = Hub::new(PtyBackend::new());
        let h2 = h1.clone();
        h1.set_session_for_cookie("c1", "demo");
        assert_eq!(h2.session_for_cookie("c1"), Some("demo".to_string()));
        h2.clear_session_for_cookie("c1");
        assert_eq!(h1.session_for_cookie("c1"), None);
    }
}
