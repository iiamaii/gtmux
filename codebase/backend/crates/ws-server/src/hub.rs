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
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use gtmux_pty_backend::{BackendNotify, PaneId, PtyBackend};
use tokio::sync::broadcast;
use tracing::debug;

/// Async cookie validation interface used by the WS handshake (D10 α —
/// ADR-0020 D10 *additive* path). Implementations return `true` when the
/// cookie value identifies a live auth session. Decoupled from
/// [`gtmux_http_api::auth::SessionTable`] so this crate does not depend
/// on `gtmux-http-api` — keeps the dep graph acyclic.
///
/// Stage 5 D10 α: cookie auth is *additive* — the WS handshake accepts a
/// request if **either** the subprotocol `bearer.<token>` validates **or**
/// the validator returns true for the cookie. The deprecated path
/// (D10 β / γ) will retire the bearer over time; this trait survives
/// both transitions.
#[async_trait]
pub trait CookieValidator: Send + Sync {
    /// `true` when `cookie_value` maps to a live, non-expired auth session.
    /// Errors / lock poisoning / missing entries all collapse to `false` —
    /// the WS handshake then falls back to the bearer path.
    async fn validate(&self, cookie_value: &str) -> bool;
}

/// Dependency injection seam for the http-api layer's UUID ↔ PaneId bridge
/// (`TerminalMap`). The WS handshake's catch-up replay calls this to re-emit
/// `0x88 TERMINAL_SPAWNED` frames for *every alive terminal* — so a freshly
/// connected client (page reload, WS auto-reconnect) immediately learns the
/// numeric PaneId for every UUID on the canvas. Without this, `0x88` would
/// only fire on *fresh* spawns and the FE `XtermHost` of any pre-existing
/// terminal item would be stuck on the "Terminal stream connecting…"
/// placeholder until the user spawns a new terminal.
///
/// See `docs/reports/0040-terminal-panel-integration-verification.md` §1.2
/// for the gap analysis and §5 for the recommended option-A fix.
#[async_trait]
pub trait TerminalUuidProvider: Send + Sync {
    /// Snapshot of every alive `(pane_id, terminal_uuid)` binding. Order is
    /// unspecified. Implementations should be lock-free / `Arc::clone`-cheap
    /// — the catch-up replay calls this once per WS handshake.
    async fn alive_bindings(&self) -> Vec<(u64, Arc<str>)>;
}

/// Slice next-2 (ADR-0025 D6): join `SessionLayout.items` ∩ `TerminalMap`
/// to surface the *current `PaneId` set* of the terminals attached to
/// `session_name`. Used by the WS handler's `pane_output` filter so a
/// connection only forwards frames whose `PaneId` is in its session's
/// set — D2 of ADR-0025 defines the set as
/// `{ TerminalMap.by_uuid[item.id] : item ∈ layout, item.type == "terminal" }`.
///
/// Implementations should treat unmatched UUIDs (terminal item present
/// in layout but no live `PaneId`) as *omitted* — the filter's
/// false-negative-is-safe invariant (D3) covers them via the next
/// `0x88 TERMINAL_SPAWNED` event.
#[async_trait]
pub trait SessionPaneSetProvider: Send + Sync {
    /// Resolve `session_name` to the current PaneId set. Missing
    /// session or empty layout → empty set. Called once per WS
    /// handshake (cold load); hot updates come through the hub's event
    /// channels.
    async fn pane_ids_for_session(&self, session_name: &str) -> std::collections::HashSet<u64>;
}

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

/// Fan-out channel depth for terminal-list-change deltas (Stage 5-D P1).
/// Same characteristic as TERMINAL_DIED: low-frequency, one event per
/// attach_confirm batch.
const TERMINAL_LIST_CHANGE_BROADCAST_CAPACITY: usize = 32;

/// Fan-out channel depth for terminal-spawned UUID↔PaneId binding events.
/// One event per spawn (attach_confirm batches and future 5-D P2
/// `POST /api/sessions/:name/terminals`) — same low-frequency profile.
const TERMINAL_SPAWNED_BROADCAST_CAPACITY: usize = 64;

/// Fan-out channel depth for Stage 5-C manipulation events
/// (0x81/0x82/0x83/0x84). These can be relatively bursty —
/// `VIEWPORT_CHANGED` may stream during a canvas drag — so the cap matches
/// the live pane-output broadcast (`HUB_BROADCAST_CAPACITY = 256`). A
/// slow subscriber will surface `RecvError::Lagged`; the dispatcher logs
/// and continues — the next event refreshes the state.
const MANIPULATION_BROADCAST_CAPACITY: usize = 256;

/// Fan-out channel depth for Stage 5-D P2 mount-cascade events. One
/// event per `POST /api/sessions/:name/terminals` call — same
/// low-frequency profile as terminal-spawned.
const MOUNT_CASCADE_BROADCAST_CAPACITY: usize = 64;

/// Capacity for the `0x89 SERVER_SHUTDOWN` channel (Slice D-5).
/// Server shutdown is a once-per-lifetime event so any reasonable
/// capacity is fine — 16 leaves plenty of headroom for the WS handler
/// loop to drain even under heavy contention.
const SERVER_SHUTDOWN_BROADCAST_CAPACITY: usize = 16;

/// Capacity for the `SessionChange` channel (Slice next-2, ADR-0025
/// D4). Cookie session changes are rare events (workspace switch, ~1
/// per minute peak), so a small cap suffices. 64 leaves headroom for
/// bursts of test churn without straining the broadcast layer.
const SESSION_CHANGE_BROADCAST_CAPACITY: usize = 64;

/// Capacity for the `AttachReplay` channel (ADR-0021 D8 amend ②,
/// 0075/0076/0077 — rebind history replay). One event per *added*
/// alive UUID in a layout PUT — bounded by user-initiated
/// `[Attach to this session]` / `[Change terminal]` clicks (sub-Hz).
/// 16 is generous for the typical single-user workload.
const ATTACH_REPLAY_BROADCAST_CAPACITY: usize = 16;

/// Payload of a `TerminalDied` broadcast. `uuid` is the schema-side
/// terminal id; `reason` is `"exit"` (process self-exited) or `"killed"`
/// (signal-driven exit). `Arc<str>` over `String` so the broadcast clone
/// per WS subscriber is a refcount bump, not a heap copy.
#[derive(Clone, Debug)]
pub struct TerminalDiedEvent {
    pub uuid: Arc<str>,
    pub reason: &'static str,
    /// PaneId of the dead terminal, carried so the WS handler's
    /// session pane-set filter (ADR-0025 D3) can `set.remove(pane_id)`
    /// without a `TerminalMap` round-trip. The PaneId monotonically
    /// increments per spawn, so removing a dead one will never collide
    /// with a future live PaneId.
    pub pane_id: u64,
}

/// Payload of a `TerminalListChange` broadcast (Stage 5-D path P1).
/// Emitted by `attach_confirm_handler` after a successful spawn batch.
/// The WS dispatcher filters per-connection:
///   * subscribers whose `cookie → session_name` matches `trigger_session`
///     → frame **suppressed** (their layout already reflects the spawn)
///   * subscribers whose cookie has no session or maps to a different
///     session → 0x87 TERMINAL_LIST_UPDATE envelope
///
/// `Arc<[Arc<str>]>` so the broadcast clone per WS subscriber is a single
/// refcount bump regardless of how many UUIDs were added in the batch.
#[derive(Clone, Debug)]
pub struct TerminalListChangeEvent {
    pub trigger_session: Arc<str>,
    pub added: Arc<[Arc<str>]>,
    pub removed: Arc<[Arc<str>]>,
}

/// Payload of a `TerminalSpawned` broadcast (FE Issue C unblock).
/// Emitted by `AppState::spawn_terminal_with_uuid` immediately after the
/// terminal_map register succeeds. The binding lets the FE switch its
/// `XtermHost` from legacy-paneId mode to terminal-UUID mode without an
/// extra `GET /api/terminals` roundtrip.
///
/// Server-wide broadcast — any attached webpage may mirror the new
/// terminal in its sidebar / future layout PUT, so all subscribers should
/// see the binding.
#[derive(Clone, Debug)]
pub struct TerminalSpawnedEvent {
    pub terminal_id: Arc<str>,
    pub pane_id: u64,
}

/// Payload of a `SessionChange` broadcast (Slice next-2, ADR-0025 D4).
/// Published by [`Hub::set_session_for_owner`] /
/// [`Hub::clear_session_for_owner`] whenever a Webpage's attached
/// session changes. WS handlers filter by their own owner key and
/// recompute the `pane_output` filter set.
///
/// `owner_key` carries the per-Webpage identity `auth_cookie + 0x1f +
/// webpage_id` (ADR-0019 D5.6), not just the auth cookie — distinct tabs
/// sharing one cookie own distinct attach state.
///
/// `new_session` is `None` when the Webpage detached (it is now in the
/// *legacy demo path* — no filter, server-wide pass).
#[derive(Clone, Debug)]
pub struct SessionChangeEvent {
    pub owner_key: Arc<str>,
    pub new_session: Option<Arc<str>>,
}

/// Payload of a `ServerShutdown` broadcast (Slice D-5, ADR-0014 D12).
/// Emitted by `POST /api/shutdown`'s detached background task ~50 ms
/// after the 202 response. Server-wide — every connected webpage
/// receives this and is expected to flip its reconnect banner to the
/// *intentional shutdown* branch before the WS close arrives.
#[derive(Clone, Debug)]
pub struct ServerShutdownEvent {
    /// Free-form `enum` string. Known values: `"user_initiated"` (MVP),
    /// `"oom"` / `"upgrade"` (P1+). FE forward-compat: unknown values
    /// fall back to `"user_initiated"` semantics.
    pub reason: Arc<str>,
    /// Mirror of the `expected_exit_code` in the `POST /api/shutdown`
    /// 202 body, so the FE can correlate the HTTP response with the
    /// WS frame and surface a single toast.
    pub expected_exit_code: i32,
}

/// Payload of a `MountCascade` broadcast (Stage 5-D path P2).
/// Emitted by `POST /api/sessions/:name/terminals` after the terminal
/// spawns and its UUID↔PaneId binding has been published. Routed to the
/// trigger session **only** — other attached webpages receive a
/// `TerminalListChange` instead (P1 pool refresh path).
///
/// Coordinates are server-determined defaults; the FE's
/// `handleMountCascade` appends a fresh `TerminalItem` at these values
/// and then PUTs the updated layout (BE itself does not persist the
/// item — see `docs/reports/0037-backend-review-action-items.md` §6.4).
#[derive(Clone, Debug)]
pub struct MountCascadeEvent {
    pub trigger_session: Arc<str>,
    pub terminal_id: Arc<str>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Payload of a `Manipulation` broadcast (Stage 5-C echo-minus-sender,
/// ADR-0021 D5). Carries the *enriched* inner payload (original body +
/// trailing varint-len + UTF-8 session_id) plus the routing identifiers
/// the dispatcher needs to enforce echo-minus-sender + session-scoped
/// delivery for `0x81..=0x84`.
///
/// `sender_conn_id` = the `connection_id` minted at the originating WS
/// handshake. Each subscriber filters by:
///   * `sender_conn_id == my_conn_id` → skip (own echo)
///   * `session_id != my_session` → skip (cross-session leak guard)
///
/// `frame_type` is the raw `FrameType` byte so the subscriber can re-wrap
/// without parsing the body.
#[derive(Clone, Debug)]
pub struct ManipulationEvent {
    pub sender_conn_id: Arc<str>,
    pub session_id: Arc<str>,
    pub frame_type: u8,
    pub payload: Bytes,
}

/// Payload of an `AttachReplay` broadcast (ADR-0021 D8 amend ②,
/// 0075/0076 work package). Emitted by `put_layout_handler` for every
/// terminal_id newly *added* to a session's layout that resolves to an
/// alive `PaneId`. The receiving session's WS handler forwards the bytes
/// as a single `PANE_OUT` envelope so the xterm panel renders the
/// existing ring buffer immediately on mount.
///
/// `session` is the owner-scope routing key — WS handlers compare it
/// with `hub.session_for_owner(self.owner_key)` and forward only on
/// match. Carrying the session inside the envelope (instead of relying
/// on the per-connection `session_pane_set` filter) makes the broadcast
/// race-immune against the set's hot-update ordering (ADR-0025 amend
/// ③ scope) — the envelope itself is the routing truth.
#[derive(Clone, Debug)]
pub struct AttachReplayEvent {
    /// Session whose layout newly references the terminal — the owner-
    /// scope match key. Only WS connections whose owner_key resolves
    /// to this session forward the envelope.
    pub session: Arc<str>,
    /// Numeric `PaneId` whose ring buffer is being replayed. Used as
    /// the `pane_id` field of the resulting `0x02 PANE_OUT` envelope so
    /// the FE routes bytes to the matching `XtermHost`.
    pub pane_id: u64,
    /// Ring-buffer snapshot at emit time. `Bytes` is `Arc`-backed so
    /// broadcast cloning is a ref-count bump, not a copy.
    pub bytes: Bytes,
}

/// Per-connection heartbeat timings (ADR-0021 D6). Production defaults are
/// 15s ping / 30s pong timeout. Snapshotted once per WS upgrade so a runtime
/// override (via [`Hub::set_heartbeat_timings`]) only affects subsequent
/// connections — mirrors the disconnect/heartbeat sink snapshot pattern.
#[derive(Clone, Copy, Debug)]
pub struct HeartbeatTimings {
    pub ping_interval: Duration,
    pub pong_timeout: Duration,
}

impl Default for HeartbeatTimings {
    fn default() -> Self {
        Self {
            ping_interval: Duration::from_secs(15),
            pong_timeout: Duration::from_secs(30),
        }
    }
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
    /// Heartbeat timings (ADR-0021 D6). Defaults to 15s ping / 30s pong
    /// timeout. Tests override via [`set_heartbeat_timings`] to shrink the
    /// timeout path. Snapshotted at WS upgrade time so a runtime change only
    /// affects subsequent connections.
    heartbeat_timings: Arc<std::sync::RwLock<HeartbeatTimings>>,
    /// Owner key → session_name registry (Stage 5-A / ADR-0021 D5,
    /// ADR-0019 D5.6). The owner key is `auth_cookie + 0x1f + webpage_id`
    /// so each browser tab keeps a distinct binding. The http-api
    /// `attach_handler` writes here after a successful flock + owner
    /// reverse-map insert; `detach_handler` (and the WS-disconnect-driven
    /// `release_lock_for_owner`) clears the entry. The WS handler
    /// consults this map when routing session-scoped envelopes (5-C) so
    /// a frame emitted on session A is never delivered to a subscriber
    /// whose owner is attached to session B.
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
    /// Terminal-list change broadcast (Stage 5-D path P1 / ADR-0021 D3).
    /// Published by the http-api `attach_confirm_handler` after each
    /// spawn batch. Routing is per-WS-subscriber: matches against
    /// `session_table` to filter out the trigger session and emit
    /// `0x87 TERMINAL_LIST_UPDATE` only to *other* sessions' webpages.
    terminal_list_change_events: broadcast::Sender<TerminalListChangeEvent>,
    /// Terminal-spawned UUID↔PaneId binding broadcast (FE Issue C unblock).
    /// Published by `AppState::spawn_terminal_with_uuid` immediately after
    /// register succeeds. Server-wide — all WS subscribers receive a
    /// `0x88 TERMINAL_SPAWNED` envelope so the FE can update its local
    /// UUID→PaneId map ahead of the next `GET /api/terminals` poll.
    terminal_spawned_events: broadcast::Sender<TerminalSpawnedEvent>,
    /// Mount-cascade broadcast (Stage 5-D path P2). Published by
    /// `POST /api/sessions/:name/terminals` to direct the trigger
    /// session's webpage to append a fresh `TerminalItem` at the
    /// server-provided coordinates. Subscribers filter by
    /// `hub.session_for_owner(my_cookie) == event.trigger_session`.
    mount_cascade_events: broadcast::Sender<MountCascadeEvent>,
    /// Manipulation broadcast (Stage 5-C, ADR-0021 D5). Published by the
    /// inbound 0x81..=0x84 dispatch path once it knows the sender's
    /// session. Each subscriber filters by `sender_conn_id` and
    /// `session_id` — the trigger connection is skipped, and only
    /// subscribers whose cookie maps to the same session receive the
    /// frame. The payload carries the original inner bytes plus a
    /// trailing varint-len + UTF-8 session_id (echo-minus-sender wire).
    manipulation_events: broadcast::Sender<ManipulationEvent>,
    /// Cookie validator hook (Stage 5 D10 α / ADR-0020 D10 additive). When
    /// set, the WS handshake accepts a request whose `gtmux_auth` cookie
    /// validates here as an **alternative** to the subprotocol bearer
    /// token. `None` (test paths and pre-wired boot phase) leaves auth at
    /// the legacy bearer-only path.
    cookie_validator: Arc<std::sync::Mutex<Option<Arc<dyn CookieValidator>>>>,
    /// Terminal UUID provider (Stage 5 / 0040 §5). Set once at boot — the WS
    /// handshake snapshots this on every new connection to re-emit `0x88
    /// TERMINAL_SPAWNED` frames for the catch-up replay. `None` (test paths
    /// + pre-wired boot phase) leaves the catch-up unchanged — fresh-spawn
    /// `0x88` frames still arrive via [`publish_terminal_spawned`].
    terminal_uuid_provider: Arc<std::sync::Mutex<Option<Arc<dyn TerminalUuidProvider>>>>,
    /// Server-shutdown broadcast (Slice D-5, ADR-0014 D12). Published
    /// by `POST /api/shutdown`'s detached task once the 202 response
    /// has been flushed. Server-wide — every webpage's WS handler
    /// turns one event into a `0x89 SERVER_SHUTDOWN` envelope before
    /// the close frame (1000 normal) arrives.
    server_shutdown_events: broadcast::Sender<ServerShutdownEvent>,
    /// Session-change broadcast (Slice next-2, ADR-0025 D4). Emitted
    /// by `set_session_for_owner` / `clear_session_for_owner` so WS
    /// handlers can refresh their per-connection PaneId filter set
    /// when the cookie's attached session changes (workspace switch,
    /// implicit detach-on-reattach).
    session_change_events: broadcast::Sender<SessionChangeEvent>,
    /// Session pane-set provider (Slice next-2, ADR-0025 D6). Set
    /// once at boot — the WS handshake snapshots this on every new
    /// connection to seed its filter set, and the runtime layout
    /// events recompute it whenever a session's terminal-item set
    /// changes. `None` (test paths + boot bootstrap) keeps the WS
    /// handler in the *legacy demo path* (server-wide PaneId pass).
    session_pane_set_provider: Arc<std::sync::Mutex<Option<Arc<dyn SessionPaneSetProvider>>>>,
    /// Rebind history replay broadcast (ADR-0021 D8 amend ②,
    /// 0075/0076/0077). Published by `put_layout_handler` for every
    /// terminal_id newly *added* to a session's layout that has an
    /// alive `PaneId`. Subscribers (one per WS handler) compare the
    /// envelope's `session` with `hub.session_for_owner(self.owner)`
    /// — only the owning WS forwards the bytes as a `PANE_OUT`
    /// envelope.
    attach_replay_events: broadcast::Sender<AttachReplayEvent>,
}

impl Hub {
    /// Build a hub backed by `backend`. Spawns the multiplexer driver task
    /// before returning, so subscribers attached immediately afterwards
    /// observe every byte emitted from this point forward.
    pub fn new(backend: PtyBackend) -> Self {
        let (pane_output, _) = broadcast::channel(HUB_BROADCAST_CAPACITY);
        let (layout_events, _) = broadcast::channel(LAYOUT_BROADCAST_CAPACITY);
        let (terminal_died_events, _) = broadcast::channel(TERMINAL_DIED_BROADCAST_CAPACITY);
        let (terminal_list_change_events, _) =
            broadcast::channel(TERMINAL_LIST_CHANGE_BROADCAST_CAPACITY);
        let (terminal_spawned_events, _) = broadcast::channel(TERMINAL_SPAWNED_BROADCAST_CAPACITY);
        let (manipulation_events, _) = broadcast::channel(MANIPULATION_BROADCAST_CAPACITY);
        let (mount_cascade_events, _) = broadcast::channel(MOUNT_CASCADE_BROADCAST_CAPACITY);
        let (server_shutdown_events, _) = broadcast::channel(SERVER_SHUTDOWN_BROADCAST_CAPACITY);
        let (session_change_events, _) = broadcast::channel(SESSION_CHANGE_BROADCAST_CAPACITY);
        let (attach_replay_events, _) = broadcast::channel(ATTACH_REPLAY_BROADCAST_CAPACITY);

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
            heartbeat_timings: Arc::new(std::sync::RwLock::new(HeartbeatTimings::default())),
            session_table: Arc::new(std::sync::RwLock::new(HashMap::new())),
            terminal_died_events,
            terminal_list_change_events,
            terminal_spawned_events,
            manipulation_events,
            mount_cascade_events,
            cookie_validator: Arc::new(std::sync::Mutex::new(None)),
            terminal_uuid_provider: Arc::new(std::sync::Mutex::new(None)),
            server_shutdown_events,
            session_change_events,
            session_pane_set_provider: Arc::new(std::sync::Mutex::new(None)),
            attach_replay_events,
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

    /// Override the per-connection heartbeat timings (ADR-0021 D6). Production
    /// callers leave the defaults (15s ping / 30s pong timeout); test code
    /// shrinks them so the timeout path is exercised in milliseconds. The
    /// snapshot taken in [`heartbeat_timings`] is per-WS-upgrade, so an
    /// override after a connection is live does not retro-apply.
    pub fn set_heartbeat_timings(&self, timings: HeartbeatTimings) {
        if let Ok(mut slot) = self.heartbeat_timings.write() {
            *slot = timings;
        }
    }

    /// Snapshot of the active heartbeat timings. See [`disconnect_sink`] for
    /// the cloning rationale — `handle_socket` calls this once at upgrade.
    pub fn heartbeat_timings(&self) -> HeartbeatTimings {
        self.heartbeat_timings
            .read()
            .map(|s| *s)
            .unwrap_or_default()
    }

    /// Record that the Webpage identified by `owner_key`
    /// (= `auth_cookie + 0x1f + webpage_id`, ADR-0019 D5.6) is currently
    /// attached to `session_name` (Stage 5-A / ADR-0021 D5). Called by the
    /// http-api `attach_handler` after a successful flock + owner
    /// reverse-map insert. Idempotent: re-attaching the same owner to a
    /// different session replaces the entry; re-attaching to the same
    /// session is a no-op write.
    ///
    /// A poisoned [`std::sync::RwLock`] is treated as a soft failure: the
    /// update is dropped and a warn-level trace fires. The kernel-level
    /// session lock is the source of truth — this table only steers WS
    /// frame routing, so a missed update degrades to "session-scoped frame
    /// behaves like server-wide" until the next attach refreshes it.
    pub fn set_session_for_owner(&self, owner_key: &str, session_name: &str) {
        let changed = match self.session_table.write() {
            Ok(mut t) => {
                let prev = t.insert(owner_key.to_string(), session_name.to_string());
                prev.as_deref() != Some(session_name)
            }
            Err(_) => {
                tracing::warn!(
                    owner_len = owner_key.len(),
                    "hub session_table: write poisoned; skipping set_session_for_owner"
                );
                false
            }
        };
        if changed {
            // Slice next-2 (ADR-0025 D4): notify the WS handler that
            // owns this Webpage so it can recompute its filter set.
            let _ = self.session_change_events.send(SessionChangeEvent {
                owner_key: Arc::from(owner_key),
                new_session: Some(Arc::from(session_name)),
            });
        }
    }

    /// Drop the owner → session_name binding for `owner_key`. Called by
    /// the http-api `detach_handler` and by the WS-disconnect-driven
    /// `release_lock_for_owner`. Idempotent: a missing entry is a no-op.
    pub fn clear_session_for_owner(&self, owner_key: &str) {
        let removed = match self.session_table.write() {
            Ok(mut t) => t.remove(owner_key).is_some(),
            Err(_) => {
                tracing::warn!(
                    owner_len = owner_key.len(),
                    "hub session_table: write poisoned; skipping clear_session_for_owner"
                );
                false
            }
        };
        if removed {
            // ADR-0025 D5: owner reverts to the legacy demo path
            // (no filter, server-wide pass). WS handler will rebuild
            // its filter set to `None`.
            let _ = self.session_change_events.send(SessionChangeEvent {
                owner_key: Arc::from(owner_key),
                new_session: None,
            });
        }
    }

    /// Look up the session that the Webpage identified by `owner_key` is
    /// currently attached to. Returns `None` when the owner has no active
    /// attach or the table is poisoned. Used by the WS dispatcher (Stage
    /// 5-C) to decide whether a session-scoped envelope is in-scope for
    /// a given connection.
    pub fn session_for_owner(&self, owner_key: &str) -> Option<String> {
        self.session_table
            .read()
            .ok()
            .and_then(|t| t.get(owner_key).cloned())
    }

    /// Remove every owner entry currently pointing at `session_name`.
    /// Used for session-wide teardown (e.g. `DELETE /api/sessions/:name`)
    /// where every Webpage that thought it owned this session must drop
    /// its routing entry. The two maps (`session_locks_by_owner` on the
    /// http-api side, this `session_table`) must stay in lock-step or the
    /// WS dispatcher would surface stale "still attached" routing.
    pub fn clear_sessions_by_name(&self, session_name: &str) {
        let removed_owners: Vec<String> = match self.session_table.write() {
            Ok(mut t) => {
                let to_remove: Vec<String> = t
                    .iter()
                    .filter(|(_, v)| v.as_str() == session_name)
                    .map(|(k, _)| k.clone())
                    .collect();
                for k in &to_remove {
                    t.remove(k);
                }
                to_remove
            }
            Err(_) => {
                tracing::warn!(
                    session = %session_name,
                    "hub session_table: write poisoned; skipping clear_sessions_by_name"
                );
                Vec::new()
            }
        };
        for owner in &removed_owners {
            let _ = self.session_change_events.send(SessionChangeEvent {
                owner_key: Arc::from(owner.as_str()),
                new_session: None,
            });
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
    pub fn publish_terminal_died(&self, uuid: &str, reason: &'static str, pane_id: u64) {
        let _ = self.terminal_died_events.send(TerminalDiedEvent {
            uuid: Arc::from(uuid),
            reason,
            pane_id,
        });
    }

    /// Subscribe to the UUID-carrying terminal-died broadcast.
    pub fn subscribe_terminal_died(&self) -> broadcast::Receiver<TerminalDiedEvent> {
        self.terminal_died_events.subscribe()
    }

    /// Broadcast a terminal-list change delta (Stage 5-D path P1). Called
    /// by `attach_confirm_handler` once a spawn batch lands; the WS
    /// dispatcher decides per-connection whether to emit a
    /// `0x87 TERMINAL_LIST_UPDATE` envelope based on the trigger session.
    /// `added`/`removed` are passed by slice so callers don't need to
    /// pre-build the `Arc<[Arc<str>]>` payload — the helper interns them.
    pub fn publish_terminal_list_change(
        &self,
        trigger_session: &str,
        added: &[String],
        removed: &[String],
    ) {
        let added_arc: Arc<[Arc<str>]> = added
            .iter()
            .map(|s| Arc::<str>::from(s.as_str()))
            .collect::<Vec<_>>()
            .into();
        let removed_arc: Arc<[Arc<str>]> = removed
            .iter()
            .map(|s| Arc::<str>::from(s.as_str()))
            .collect::<Vec<_>>()
            .into();
        let _ = self
            .terminal_list_change_events
            .send(TerminalListChangeEvent {
                trigger_session: Arc::from(trigger_session),
                added: added_arc,
                removed: removed_arc,
            });
    }

    /// Subscribe to terminal-list change deltas. Each WS connection pulls
    /// from this channel and filters via [`Hub::session_for_owner`] —
    /// see WS handler's `terminal_list_change_rx` select arm.
    pub fn subscribe_terminal_list_change(&self) -> broadcast::Receiver<TerminalListChangeEvent> {
        self.terminal_list_change_events.subscribe()
    }

    /// Broadcast a UUID↔PaneId binding event (FE Issue C unblock). Called
    /// by `AppState::spawn_terminal_with_uuid` right after register
    /// succeeds. Server-wide — all WS subscribers receive
    /// `0x88 TERMINAL_SPAWNED`.
    pub fn publish_terminal_spawned(&self, terminal_id: &str, pane_id: u64) {
        let _ = self.terminal_spawned_events.send(TerminalSpawnedEvent {
            terminal_id: Arc::from(terminal_id),
            pane_id,
        });
    }

    /// Subscribe to UUID↔PaneId binding events.
    pub fn subscribe_terminal_spawned(&self) -> broadcast::Receiver<TerminalSpawnedEvent> {
        self.terminal_spawned_events.subscribe()
    }

    /// Broadcast a server-shutdown notify (Slice D-5, ADR-0014 D12).
    /// Called by `POST /api/shutdown`'s detached task ~50 ms after the
    /// 202 response. Server-wide — every WS subscriber sees a single
    /// `0x89 SERVER_SHUTDOWN` envelope right before the connection is
    /// closed with code 1000.
    pub fn publish_server_shutdown(&self, reason: &str, expected_exit_code: i32) {
        let _ = self.server_shutdown_events.send(ServerShutdownEvent {
            reason: Arc::from(reason),
            expected_exit_code,
        });
    }

    /// Subscribe to server-shutdown notifications. Each WS handler
    /// pulls from this channel and emits one `0x89` envelope per event,
    /// then exits the connection loop so the close-handshake path runs.
    pub fn subscribe_server_shutdown(&self) -> broadcast::Receiver<ServerShutdownEvent> {
        self.server_shutdown_events.subscribe()
    }

    /// Subscribe to cookie session-change events (Slice next-2, ADR-0025
    /// D4). Each WS handler filters by its own cookie and recomputes
    /// its `pane_output` filter set.
    pub fn subscribe_session_change(&self) -> broadcast::Receiver<SessionChangeEvent> {
        self.session_change_events.subscribe()
    }

    /// Register the session pane-set provider (Slice next-2, ADR-0025
    /// D6). Called once at boot from `gtmux-cli` after the http-api
    /// `AppState` is built — supplies the http-api side of the trait.
    pub fn set_session_pane_set_provider(&self, provider: Arc<dyn SessionPaneSetProvider>) {
        match self.session_pane_set_provider.lock() {
            Ok(mut slot) => *slot = Some(provider),
            Err(e) => tracing::error!(
                error = ?e,
                "hub session_pane_set_provider lock poisoned"
            ),
        }
    }

    /// Borrow the registered session pane-set provider, if any.
    pub fn session_pane_set_provider(&self) -> Option<Arc<dyn SessionPaneSetProvider>> {
        self.session_pane_set_provider
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().cloned())
    }

    /// Broadcast a manipulation event (Stage 5-C). Called by the WS
    /// dispatcher after it receives an inbound `0x81..=0x84` frame and
    /// resolves the sender's session via [`Hub::session_for_owner`].
    /// Subscribers (including the sender's own connection) all see the
    /// event; per-subscriber filtering happens in the dispatcher loop.
    pub fn publish_manipulation(&self, event: ManipulationEvent) {
        let _ = self.manipulation_events.send(event);
    }

    /// Subscribe to manipulation events.
    pub fn subscribe_manipulation(&self) -> broadcast::Receiver<ManipulationEvent> {
        self.manipulation_events.subscribe()
    }

    /// Broadcast a mount-cascade event (Stage 5-D path P2). Called by
    /// `POST /api/sessions/:name/terminals` after a successful spawn.
    /// Per-subscriber filter (only trigger session receives) runs in the
    /// WS dispatcher's `mount_cascade_rx` arm.
    pub fn publish_mount_cascade(&self, event: MountCascadeEvent) {
        let _ = self.mount_cascade_events.send(event);
    }

    /// Subscribe to mount-cascade events.
    pub fn subscribe_mount_cascade(&self) -> broadcast::Receiver<MountCascadeEvent> {
        self.mount_cascade_events.subscribe()
    }

    /// Broadcast a rebind history replay (ADR-0021 D8 amend ②,
    /// 0075/0076/0077). Called by `put_layout_handler` after the
    /// disk-of-truth swap + `attach_index.apply_diff` for every *added*
    /// terminal_id that resolves to an alive `PaneId`.
    ///
    /// `session` is the owner-scope routing key; only the WS handler
    /// whose `owner_key` resolves to this session forwards the bytes.
    /// Failure (no subscriber, cap hit) is silent — the disk-of-truth
    /// invariant is not affected, at most a single history replay is
    /// lost and the next live `PANE_OUT` resumes normally.
    pub fn publish_attach_replay(&self, session: Arc<str>, pane_id: u64, bytes: Bytes) {
        let _ = self.attach_replay_events.send(AttachReplayEvent {
            session,
            pane_id,
            bytes,
        });
    }

    /// Subscribe to rebind history replay events.
    pub fn subscribe_attach_replay(&self) -> broadcast::Receiver<AttachReplayEvent> {
        self.attach_replay_events.subscribe()
    }

    /// Register the cookie validator (Stage 5 D10 α). Called once at boot
    /// from the http-api wiring layer so the WS handshake can accept
    /// cookie-based auth as an alternative to the subprotocol bearer.
    /// Replaces any previously-registered validator; safe to call
    /// multiple times. Mirrors the [`set_disconnect_sink`] pattern.
    pub fn set_cookie_validator(&self, validator: Arc<dyn CookieValidator>) {
        if let Ok(mut slot) = self.cookie_validator.lock() {
            *slot = Some(validator);
        }
    }

    /// Snapshot of the current cookie validator. The WS handshake clones
    /// this once per upgrade so a validator registered mid-flight is
    /// honoured only by subsequent handshakes — matches the sink-snapshot
    /// rationale documented above.
    pub fn cookie_validator(&self) -> Option<Arc<dyn CookieValidator>> {
        self.cookie_validator.lock().ok().and_then(|s| s.clone())
    }

    /// Register the terminal UUID provider (0040 §5 option A). Called once
    /// at boot from the http-api wiring layer so the WS handshake can emit
    /// catch-up `0x88 TERMINAL_SPAWNED` frames for every alive binding.
    /// Replaces any previously-registered provider; safe to call multiple
    /// times. Mirrors the [`set_cookie_validator`] pattern.
    pub fn set_terminal_uuid_provider(&self, provider: Arc<dyn TerminalUuidProvider>) {
        if let Ok(mut slot) = self.terminal_uuid_provider.lock() {
            *slot = Some(provider);
        }
    }

    /// Snapshot of the current terminal UUID provider. The WS handshake
    /// clones this once per upgrade so a provider registered mid-flight is
    /// honoured only by subsequent handshakes.
    pub fn terminal_uuid_provider(&self) -> Option<Arc<dyn TerminalUuidProvider>> {
        self.terminal_uuid_provider
            .lock()
            .ok()
            .and_then(|s| s.clone())
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
        hub.publish_terminal_died("uuid", "exit", 1);
    }

    #[tokio::test]
    async fn terminal_died_subscriber_receives_event() {
        let hub = Hub::new(PtyBackend::new());
        let mut rx = hub.subscribe_terminal_died();
        hub.publish_terminal_died("11111111-2222-4333-8444-555555555555", "killed", 1);
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
        hub.publish_terminal_died("uuid", "exit", 1);
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
    async fn terminal_list_change_publish_silent_without_subscribers() {
        let hub = Hub::new(PtyBackend::new());
        hub.publish_terminal_list_change("demo", &["u1".to_string()], &[]);
    }

    #[tokio::test]
    async fn terminal_list_change_subscriber_receives_event() {
        let hub = Hub::new(PtyBackend::new());
        let mut rx = hub.subscribe_terminal_list_change();
        hub.publish_terminal_list_change(
            "demo",
            &["u1".to_string(), "u2".to_string()],
            &["u3".to_string()],
        );
        let got = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        assert_eq!(&*got.trigger_session, "demo");
        assert_eq!(got.added.len(), 2);
        assert_eq!(&*got.added[0], "u1");
        assert_eq!(&*got.added[1], "u2");
        assert_eq!(got.removed.len(), 1);
        assert_eq!(&*got.removed[0], "u3");
    }

    #[tokio::test]
    async fn terminal_spawned_publish_silent_without_subscribers() {
        let hub = Hub::new(PtyBackend::new());
        hub.publish_terminal_spawned("uuid", 42);
    }

    #[tokio::test]
    async fn manipulation_publish_silent_without_subscribers() {
        let hub = Hub::new(PtyBackend::new());
        hub.publish_manipulation(ManipulationEvent {
            sender_conn_id: Arc::from("c1"),
            session_id: Arc::from("alpha"),
            frame_type: 0x81,
            payload: Bytes::from_static(&[0x00]),
        });
    }

    #[tokio::test]
    async fn mount_cascade_publish_silent_without_subscribers() {
        let hub = Hub::new(PtyBackend::new());
        hub.publish_mount_cascade(MountCascadeEvent {
            trigger_session: Arc::from("alpha"),
            terminal_id: Arc::from("uuid"),
            x: 80.0,
            y: 80.0,
            w: 720.0,
            h: 420.0,
        });
    }

    #[tokio::test]
    async fn mount_cascade_subscriber_receives_event() {
        let hub = Hub::new(PtyBackend::new());
        let mut rx = hub.subscribe_mount_cascade();
        hub.publish_mount_cascade(MountCascadeEvent {
            trigger_session: Arc::from("alpha"),
            terminal_id: Arc::from("11111111-2222-4333-8444-555555555555"),
            x: 96.0,
            y: 112.0,
            w: 720.0,
            h: 420.0,
        });
        let got = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        assert_eq!(&*got.trigger_session, "alpha");
        assert_eq!(&*got.terminal_id, "11111111-2222-4333-8444-555555555555");
        assert_eq!(got.x, 96.0);
        assert_eq!(got.y, 112.0);
    }

    #[tokio::test]
    async fn manipulation_subscriber_receives_event() {
        let hub = Hub::new(PtyBackend::new());
        let mut rx = hub.subscribe_manipulation();
        hub.publish_manipulation(ManipulationEvent {
            sender_conn_id: Arc::from("c1"),
            session_id: Arc::from("alpha"),
            frame_type: 0x83,
            payload: Bytes::from_static(&[0x00, 0x01, 0x02]),
        });
        let got = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        assert_eq!(&*got.sender_conn_id, "c1");
        assert_eq!(&*got.session_id, "alpha");
        assert_eq!(got.frame_type, 0x83);
        assert_eq!(got.payload.as_ref(), &[0x00, 0x01, 0x02]);
    }

    #[tokio::test]
    async fn terminal_spawned_subscriber_receives_event() {
        let hub = Hub::new(PtyBackend::new());
        let mut rx = hub.subscribe_terminal_spawned();
        hub.publish_terminal_spawned("11111111-2222-4333-8444-555555555555", 7);
        let got = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        assert_eq!(&*got.terminal_id, "11111111-2222-4333-8444-555555555555");
        assert_eq!(got.pane_id, 7);
    }

    #[tokio::test]
    async fn terminal_list_change_supports_empty_deltas() {
        // Wire layer always emits both arrays (possibly empty); the hub
        // event must round-trip that contract without coercing to None.
        let hub = Hub::new(PtyBackend::new());
        let mut rx = hub.subscribe_terminal_list_change();
        hub.publish_terminal_list_change("demo", &[], &[]);
        let got = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        assert_eq!(got.added.len(), 0);
        assert_eq!(got.removed.len(), 0);
    }

    #[tokio::test]
    async fn session_table_empty_by_default() {
        let hub = Hub::new(PtyBackend::new());
        assert_eq!(hub.session_for_owner("anybody"), None);
    }

    #[tokio::test]
    async fn session_table_set_then_lookup() {
        let hub = Hub::new(PtyBackend::new());
        hub.set_session_for_owner("c1", "demo");
        assert_eq!(hub.session_for_owner("c1"), Some("demo".to_string()));
        assert_eq!(hub.session_for_owner("c2"), None);
    }

    #[tokio::test]
    async fn session_table_set_replaces_existing() {
        let hub = Hub::new(PtyBackend::new());
        hub.set_session_for_owner("c1", "demo");
        hub.set_session_for_owner("c1", "other");
        assert_eq!(hub.session_for_owner("c1"), Some("other".to_string()));
    }

    #[tokio::test]
    async fn session_table_clear_is_idempotent() {
        let hub = Hub::new(PtyBackend::new());
        // Clearing a non-existent entry must not panic / error.
        hub.clear_session_for_owner("nobody");
        hub.set_session_for_owner("c1", "demo");
        hub.clear_session_for_owner("c1");
        assert_eq!(hub.session_for_owner("c1"), None);
        // Second clear is still a no-op.
        hub.clear_session_for_owner("c1");
        assert_eq!(hub.session_for_owner("c1"), None);
    }

    #[tokio::test]
    async fn session_table_clear_by_name_drops_matching_cookies() {
        let hub = Hub::new(PtyBackend::new());
        hub.set_session_for_owner("c1", "demo");
        hub.set_session_for_owner("c2", "demo");
        hub.set_session_for_owner("c3", "other");
        hub.clear_sessions_by_name("demo");
        assert_eq!(hub.session_for_owner("c1"), None);
        assert_eq!(hub.session_for_owner("c2"), None);
        assert_eq!(hub.session_for_owner("c3"), Some("other".to_string()));
    }

    #[tokio::test]
    async fn session_table_clones_share_state() {
        // Hub clones share the same Arc-backed session_table — required so
        // the WS handler clone (used in handshake) sees writes performed
        // by the http-api handler clone.
        let h1 = Hub::new(PtyBackend::new());
        let h2 = h1.clone();
        h1.set_session_for_owner("c1", "demo");
        assert_eq!(h2.session_for_owner("c1"), Some("demo".to_string()));
        h2.clear_session_for_owner("c1");
        assert_eq!(h1.session_for_owner("c1"), None);
    }

    // ── TerminalUuidProvider (0040 §5 option A) ──────────────────────────

    struct StubUuidProvider(Vec<(u64, &'static str)>);

    #[async_trait]
    impl TerminalUuidProvider for StubUuidProvider {
        async fn alive_bindings(&self) -> Vec<(u64, Arc<str>)> {
            self.0
                .iter()
                .map(|(p, u)| (*p, Arc::<str>::from(*u)))
                .collect()
        }
    }

    #[tokio::test]
    async fn terminal_uuid_provider_unset_by_default() {
        let hub = Hub::new(PtyBackend::new());
        assert!(hub.terminal_uuid_provider().is_none());
    }

    #[tokio::test]
    async fn terminal_uuid_provider_set_then_get() {
        let hub = Hub::new(PtyBackend::new());
        let provider = Arc::new(StubUuidProvider(vec![(7, "uuid-a"), (8, "uuid-b")]));
        hub.set_terminal_uuid_provider(provider);
        let got = hub
            .terminal_uuid_provider()
            .expect("provider set")
            .alive_bindings()
            .await;
        assert_eq!(got.len(), 2);
        assert!(got.iter().any(|(p, u)| *p == 7 && u.as_ref() == "uuid-a"));
        assert!(got.iter().any(|(p, u)| *p == 8 && u.as_ref() == "uuid-b"));
    }

    #[tokio::test]
    async fn terminal_uuid_provider_set_replaces_existing() {
        let hub = Hub::new(PtyBackend::new());
        hub.set_terminal_uuid_provider(Arc::new(StubUuidProvider(vec![(1, "first")])));
        hub.set_terminal_uuid_provider(Arc::new(StubUuidProvider(vec![(2, "second")])));
        let got = hub
            .terminal_uuid_provider()
            .expect("provider set")
            .alive_bindings()
            .await;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, 2);
        assert_eq!(got[0].1.as_ref(), "second");
    }

    #[tokio::test]
    async fn terminal_uuid_provider_clones_share_state() {
        let h1 = Hub::new(PtyBackend::new());
        let h2 = h1.clone();
        h1.set_terminal_uuid_provider(Arc::new(StubUuidProvider(vec![(42, "shared")])));
        let got = h2
            .terminal_uuid_provider()
            .expect("provider visible via clone")
            .alive_bindings()
            .await;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0], (42, Arc::<str>::from("shared")));
    }
}
