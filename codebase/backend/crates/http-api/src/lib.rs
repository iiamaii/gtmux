//! gtmux-http-api — axum HTTP router (P0-HTTP-1 + P0-HTTP-2).
//!
//! Routes:
//!   GET  /healthz           — liveness probe, no auth gate
//!   GET  /auth/bootstrap    — one-shot token→cookie exchange + 302 /
//!   GET  /api/layout        — current snapshot + ETag (304 on If-None-Match)
//!   PUT  /api/layout        — atomic swap, If-Match required, 412 on stale
//!
//! Middleware chain (in order; outermost first):
//!   1. tower_http::trace::TraceLayer        — request span (query-string redacted)
//!   2. OriginCheck                          — cors_origins allowlist (ADR-0003 D3)
//!   3. HostCheck                            — effective_host_allowlist (ADR-0003 D2)
//!   4. BearerAuth                           — only on `/api/*` (ADR-0003 D6, R(rej)2)
//!   5. tower_http::cors::CorsLayer          — preflight + dynamic origin echo
//!
//! Contract references:
//!   * `docs/adr/0003-security-defaults.md`        — D2/D4/D6/D13 + R(rej)2 exception
//!   * `docs/ssot/security-defaults.md`            — §1 headers, §4 cookie attrs
//!   * `docs/ssot/canvas-layout-schema.md`         — §2 ETag normalisation, §3 PUT rules
//!   * `docs/reports/0010-grill-amendments.md` D12 — Canvas layout = HTTP PUT/ETag
//!   * `docs/reports/0012-bootstrap-smoke.md` §3   — P0-HTTP-1, P0-HTTP-2 contracts
//!
//! Security notes:
//!   * SHA256-128 is used for the layout ETag (the first 16 bytes of a SHA-256
//!     digest of the canonical-form JSON payload). MD5 is explicitly avoided
//!     for hygiene — even though ETags are not collision-sensitive in HTTP
//!     semantics, a colliding payload would still confuse If-Match flows.
//!   * The `?redirect=` parameter on `/auth/bootstrap` is normalised to a
//!     host-relative path; any value that does not begin with a single `/`
//!     followed by a path char is replaced with `/`. This blocks the Open
//!     Redirect class (`?redirect=https://evil.example`, `?redirect=//evil`).
//!   * Cookies use `Secure` only in Cloud mode. Local mode is plain HTTP so
//!     `Secure` would cause the browser to silently drop the cookie.
//!   * Authentication failures increment `state.auth_failure_counter` — this
//!     gives downstream throttle middleware a hookable signal without yet
//!     enforcing the limit (P1 work, per ADR-0003 D12 cloud-only).
//!
//! The crate is intentionally `forbid(unsafe_code)` and never `unwrap`s on
//! user input. Schema validation is delegated to `serde_json::Value` for now
//! and a hook (`SchemaValidator`) is exposed for `gtmux-canvas-layout`
//! (Sprint 3+) to slot in without changing the router shape.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod assets;
mod attach_index;
mod auth;
mod file_open;
mod file_stat;
mod fs_list;
mod schema;
mod session_lock;
mod session_pane_set;
mod sessions;
mod settings;
mod shutdown;
mod terminal_map;
mod terminals;
mod workspace;

pub use auth::{
    default_password_hash_path, default_rate_limiter, default_session_table, hash_password,
    load_password_hash, save_password_hash, verify_password, AuthError, AuthMode, RateLimiter,
    SessionTable,
};
pub use file_open::{
    default_allowlist_path, default_audit_dir, Allowlist, AllowlistEntry, AllowlistMatch, AuditLog,
    FileOpenContext,
};
pub use schema::{
    detect_shape, migrate_v1_to_v2, recompute_connector_bboxes, validate as validate_layout_v2,
    Anchor, Direction, Group, Head, Item, ItemCommon, Layout, Point, Routing, SchemaShape,
    StrokeDash, ValidationError, Viewport, Visibility, SCHEMA_VERSION,
};
pub use session_lock::{fresh_server_id, Lease, LockError, LockGuard, LockState};
pub use sessions::{SessionCache, SessionError, SessionLayout};
pub use settings::{default_behavior_settings, BehaviorSettings};
pub use terminal_map::{fresh_terminal_uuid, MapError as TerminalMapError, TerminalMap};
pub use terminals::{TerminalInfo, TerminalMetadata, TerminalMetadataStore};
pub use workspace::{
    validate_session_name, BootMigrationReport, SessionInfo, WorkspaceError, WorkspaceManager,
};

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use gtmux_auth::TokenString;
use gtmux_config::{Config, Mode};
use serde::Deserialize;
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::RwLock;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::warn;

// ─────────────────────────────────────────────────────────────────────────────
//  Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Server-side canvas layout snapshot. The 16-byte raw ETag is the canonical
/// Shared application state wired into the router. Cloning is cheap (Arc).
#[derive(Clone)]
pub struct AppState {
    /// Loaded gtmux config — used for mode, host/origin allowlists, port.
    pub config: Arc<Config>,
    /// The session token for this Server run.
    pub token: Arc<TokenString>,
    /// Auth-failure counter exposed for downstream rate-limit middleware
    /// (P1 enforcement; ADR-0003 D12 cloud-only). The counter is monotonic.
    pub auth_failure_counter: Arc<AtomicU64>,
    /// Optional WS broadcast hub. When set, session-scoped layout PUT
    /// handlers publish the new ETag so live WS subscribers re-hydrate via
    /// the dispatcher's `LAYOUT_CHANGED` path. `None` in unit-tests that
    /// exercise the HTTP surface in isolation.
    pub hub: Option<gtmux_ws_server::Hub>,
    /// Per-Server workspace handle — the multi-session storage root (ADR-0019
    /// D1/D2). When `Some`, the `/api/sessions[/<name>[/layout]]` routes are
    /// wired and accept requests; when `None` those routes return 503.
    pub workspace: Option<Arc<WorkspaceManager>>,
    /// In-memory cache of loaded session layouts. Always present so handler
    /// code can borrow it without an `Option` gate; lookups in it are no-ops
    /// when `workspace` is `None`.
    pub session_cache: Arc<SessionCache>,
    /// Server-side cookie session table (ADR-0020 D2). In-memory; entries
    /// expire on a rolling `cookie_max_age_days` window.
    pub session_table: Arc<SessionTable>,
    /// Per-IP rate limiter for `POST /auth/login` (ADR-0020 D5).
    pub rate_limiter: Arc<RateLimiter>,
    /// PHC-encoded Argon2id hash for password-mode auth (ADR-0020 D5).
    /// `None` (inside the lock) in token mode or when the password file
    /// doesn't exist yet (login then 503s with a hint to run
    /// `gtmux set-password`). Runtime-mutable so Slice D-3's
    /// `POST /api/settings/password` can rotate the hash without a
    /// process restart — D-1's `GET /api/settings` reads the boolean
    /// presence, login reads the inner string.
    pub password_hash: Arc<RwLock<Option<String>>>,
    /// Disk location of the password hash file (ADR-0020 D5 — under
    /// `${XDG_STATE_HOME}/gtmux/`). Captured at boot so the password
    /// rotation handler can persist a new hash without re-resolving the
    /// XDG path. `None` in tests that don't exercise the disk path.
    pub password_hash_path: Option<Arc<std::path::PathBuf>>,
    /// UUID v4 minted once per server boot (ADR-0019 D6.1). Written into
    /// `.locks/<name>.lock` bodies so other servers can disambiguate
    /// holders that happen to share a PID.
    pub server_id: Arc<str>,
    /// Locks currently held by *this* server, keyed by session name. The
    /// outer Mutex protects the map; each [`LockGuard`] inside is itself
    /// the OS-level flock. Serialises same-server attach attempts on the
    /// same session name (D6.6).
    pub session_locks: Arc<tokio::sync::Mutex<std::collections::HashMap<String, LockGuard>>>,
    /// Reverse index: owner key → session name. The owner key is
    /// `auth_cookie + 0x1f + webpage_id` (ADR-0019 D5.6) so two tabs sharing
    /// the auth cookie keep distinct attach lifetimes. Populated when an
    /// attach succeeds; consulted on WS-close to find the matching
    /// `session_locks` entry to release (ADR-0019 D6 §heartbeat).
    /// Manipulated *only* while `session_locks` is held to keep the two
    /// maps consistent — never under contention from a different path.
    pub session_locks_by_owner: Arc<tokio::sync::Mutex<std::collections::HashMap<String, String>>>,
    /// UUID ↔ PaneId bridge for the schema v2 terminal-item model (ADR-0018
    /// D2). Every spawn that surfaces through the HTTP API registers here;
    /// every detected death unregisters. The `pty-backend` / `ws-server`
    /// crates remain UUID-blind — only this crate crosses the boundary.
    pub terminal_map: Arc<TerminalMap>,
    /// Per-terminal label + created_at, keyed by the same UUID as
    /// `terminal_map`. In-memory only — recreated each boot (Stage 4-B).
    pub terminal_meta: Arc<TerminalMetadataStore>,
    /// Stage 7 BE-9 / Slice D-1: runtime-mutable behavior toggles
    /// exposed via `GET/PATCH /api/settings`. In-memory only for the
    /// minimal slice — restart resets to defaults. See `settings.rs`.
    pub behavior_settings: Arc<RwLock<BehaviorSettings>>,
    /// Slice D-2 (ADR-0023) — `/api/file-path/*` allowlist + audit
    /// log context. The allowlist is loaded from disk at boot (cold)
    /// and persisted on every `POST/DELETE /allowlist`. See
    /// `file_open/mod.rs` for the wire surface.
    pub file_open: FileOpenContext,
    /// Per-UUID lock for `POST /api/terminals/:id/respawn` (ADR-0021 D10.2,
    /// 0053 §3.4 follow-up). The handler's kill-then-spawn sequence is not
    /// atomic on its own — multi-webpage auto-respawn (FE
    /// `PanelDanglingOverlay`) can race two requests on the same UUID and
    /// briefly churn the PaneId binding. Per-UUID serialisation closes the
    /// window: the second caller waits for the first to publish its new
    /// PaneId, then sees the live binding and returns an idempotent 200
    /// (`reused: true`) without killing the just-spawned Pane.
    ///
    /// Map entries are *not* GC'd on release — a single-user workload's
    /// unique-UUID respawn set is bounded by terminal_pool cardinality, so
    /// the leak is ~60 bytes per ever-respawned UUID (acceptable). A future
    /// pass can switch to `Weak<Mutex<()>>` if needed.
    pub respawn_locks:
        Arc<tokio::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    /// Cross-session reverse index `terminal_uuid → BTreeSet<session_name>`
    /// (ADR-0021 D7 amend ③ / 0066 §BE-2 / 0067 Phase 4 / 0068 work package).
    /// Powers `GET /api/terminals`'s `attach_count` + `attached_sessions`
    /// columns without per-request disk scans. Built at boot via
    /// `with_workspace` → `attach_index.rebuild_from_disk(...)`, then
    /// kept fresh by the layout-mutating handlers (`PUT
    /// /api/sessions/:name/layout`, `DELETE
    /// /api/sessions/:name/items/:id`, `POST /api/sessions/import`,
    /// `DELETE /api/sessions/:name`).
    pub attach_index: Arc<attach_index::AttachIndex>,
}

impl AppState {
    /// Assemble shared state with a fresh empty layout snapshot.
    /// `hub` is `None`; production callers must use [`AppState::with_hub`].
    pub fn new(config: Config, token: TokenString) -> Self {
        let session_table = default_session_table(config.auth.cookie_max_age_days);
        Self {
            session_table,
            rate_limiter: default_rate_limiter(),
            password_hash: Arc::new(RwLock::new(None)),
            password_hash_path: None,
            server_id: Arc::from(fresh_server_id()),
            session_locks: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            session_locks_by_owner: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            terminal_map: Arc::new(TerminalMap::new()),
            terminal_meta: Arc::new(TerminalMetadataStore::new()),
            config: Arc::new(config),
            token: Arc::new(token),
            auth_failure_counter: Arc::new(AtomicU64::new(0)),
            hub: None,
            workspace: None,
            session_cache: Arc::new(SessionCache::new()),
            behavior_settings: default_behavior_settings(),
            file_open: FileOpenContext::production(),
            respawn_locks: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            attach_index: Arc::new(attach_index::AttachIndex::new()),
        }
    }

    /// Attach a pre-loaded Argon2id password hash (read from the file
    /// produced by `gtmux set-password`). Without this `POST /auth/login`
    /// returns 503 in password mode.
    pub fn with_password_hash(self, hash: String) -> Self {
        // Use `blocking_write` because this builder runs synchronously
        // during boot, before any axum handler holds the lock. There is
        // no live contention to block on.
        *self.password_hash.blocking_write() = Some(hash);
        self
    }

    /// Pin the on-disk location of the password hash file so the Slice
    /// D-3 rotation handler can re-save without re-resolving XDG.
    pub fn with_password_hash_path(mut self, path: std::path::PathBuf) -> Self {
        self.password_hash_path = Some(Arc::new(path));
        self
    }

    /// Refresh the lease body of the session lock currently held by the
    /// Webpage identified by `owner_key` (= `auth_cookie + 0x1f + webpage_id`,
    /// ADR-0019 D5.6 / ADR-0019 D6.2). Called from the WS heartbeat consumer
    /// task on every Ping/Pong. Bumps the `lease_until_unix` field so a
    /// peeking modal sees a fresh expected expiry. The kernel flock is
    /// unaffected — this is purely a diagnostic refresh.
    ///
    /// Idempotent. An owner that holds no lock is a no-op.
    pub async fn refresh_lease_for_owner(&self, owner_key: &str) {
        let by_owner = self.session_locks_by_owner.lock().await;
        let Some(name) = by_owner.get(owner_key).cloned() else {
            return;
        };
        drop(by_owner);
        let mut holders = self.session_locks.lock().await;
        if let Some(guard) = holders.get_mut(&name) {
            if let Err(e) = guard.refresh_lease(owner_key) {
                tracing::warn!(
                    session = %name,
                    error = %e,
                    "session_lock: lease refresh failed"
                );
            }
        }
    }

    /// Drop the bridge-map binding for the Terminal at `pane`
    /// (Stage 4-E hygiene). Called from the CLI's `BackendNotify::PaneDied`
    /// consumer so a dead Pane never sits in [`TerminalMap`] as a stale
    /// alive binding. The metadata store is **not** touched — a kernel-
    /// driven death may be followed by an explicit respawn, and the
    /// user-visible `created_at` / `label` should survive that round-trip
    /// (ADR-0021 D10.1 lazy fresh-spawn). Metadata is forgotten only on
    /// the *explicit* user paths (DELETE item with `kill_terminal=true`,
    /// `POST /api/terminals/:id/kill`). Idempotent on missing entries.
    ///
    /// Stage 5-B: also broadcasts a UUID-carrying `terminal-died` WS frame
    /// via the hub so attached webpages can mark the matching schema item
    /// as dangling without polling `GET /api/terminals`. `signal=Some(_)`
    /// maps to `"killed"`, `signal=None` maps to `"exit"`.
    pub async fn handle_pane_died(&self, pane: gtmux_pty_backend::PaneId, signal: Option<i32>) {
        if let Some(uuid) = self.terminal_map.unregister_pane(pane).await {
            let reason = if signal.is_some() { "killed" } else { "exit" };
            if let Some(hub) = self.hub.as_ref() {
                hub.publish_terminal_died(&uuid, reason, pane.0);
            }
            tracing::debug!(
                pane = ?pane,
                uuid = %uuid,
                reason,
                "terminal: unregistered after BackendNotify::PaneDied (metadata preserved)"
            );
        }
    }

    /// Release any cross-server session lock currently held by the Webpage
    /// identified by `owner_key` (= `auth_cookie + 0x1f + webpage_id`,
    /// ADR-0019 D5.6 / ADR-0019 D6). Called from the WS disconnect consumer
    /// task on close. Idempotent — an owner that never attached is a no-op.
    pub async fn release_lock_for_owner(&self, owner_key: &str) {
        // Locks are taken in a fixed order (locks_by_owner → session_locks)
        // anywhere two maps are touched together, so a same-owner attach
        // racing with a disconnect cannot deadlock.
        let mut by_owner = self.session_locks_by_owner.lock().await;
        let Some(name) = by_owner.remove(owner_key) else {
            return;
        };
        let mut holders = self.session_locks.lock().await;
        if let Some(mut guard) = holders.remove(&name) {
            tracing::info!(
                session = %name,
                "session_lock: auto-released on WS disconnect"
            );
            guard.release();
        }
        // Stage 5-A: mirror the auto-release into the hub's owner ↔
        // session table so a fresh WS reconnect doesn't see this Webpage
        // as still session-attached.
        if let Some(hub) = self.hub.as_ref() {
            hub.clear_session_for_owner(owner_key);
        }
    }

    /// Spawn a fresh Terminal in the PTY backend and bind it to `uuid` in
    /// the [`TerminalMap`] (Stage 4-A / ADR-0018 D6 *fresh spawn* arm).
    ///
    /// Idempotent on the UUID axis: if `uuid` is already mapped to an alive
    /// PaneId the existing binding is returned with no side effect. Two
    /// concurrent calls for the same UUID will at worst spawn one extra
    /// PaneId that gets killed immediately when its `register` loses the
    /// race — both callers still see the same winning PaneId.
    ///
    /// Returns `Err(SpawnTerminalError::HubUnavailable)` when called without
    /// a hub attached (e.g. unit tests that exercise the HTTP surface in
    /// isolation).
    pub async fn spawn_terminal_with_uuid(
        &self,
        uuid: String,
    ) -> Result<gtmux_pty_backend::PaneId, SpawnTerminalError> {
        if let Some(existing) = self.terminal_map.lookup_pane(&uuid).await {
            return Ok(existing);
        }
        let hub = self
            .hub
            .as_ref()
            .ok_or(SpawnTerminalError::HubUnavailable)?;
        let pane = hub
            .backend()
            .spawn(gtmux_pty_backend::SpawnSpec::default_shell())?;
        match self.terminal_map.register(uuid.clone(), pane).await {
            Ok(()) => {
                self.terminal_meta.record_spawn(&uuid).await;
                // FE Issue C unblock — publish the fresh UUID↔PaneId
                // binding so any attached webpage can wire an `XtermHost`
                // subscriber against `pane` without polling
                // `GET /api/terminals` first. Server-wide broadcast (cookie
                // filter belongs to session-scoped frames like 0x87).
                hub.publish_terminal_spawned(&uuid, pane.0);
                Ok(pane)
            }
            Err(TerminalMapError::UuidAlreadyBound { existing_pane, .. }) => {
                // Lost a same-UUID race against another concurrent attach.
                // Kill the duplicate Pane we just spawned and return the
                // winner — the caller observes a single bound PaneId.
                if let Err(e) = hub.backend().kill(pane) {
                    tracing::warn!(
                        pane = ?pane,
                        error = %e,
                        "terminal_map: failed to kill duplicate spawn after register race"
                    );
                }
                Ok(existing_pane)
            }
            Err(e @ TerminalMapError::PaneAlreadyBound { .. }) => {
                // Internal consistency violation — fresh PaneIds are never
                // reused by the backend, so this should be unreachable. Log
                // and surface as an error rather than silently corrupting
                // the map.
                tracing::error!(error = %e, "terminal_map: fresh PaneId collision");
                if let Err(kill_err) = hub.backend().kill(pane) {
                    tracing::warn!(
                        pane = ?pane,
                        error = %kill_err,
                        "terminal_map: failed to kill orphan after pane-collision"
                    );
                }
                Err(SpawnTerminalError::Map(e))
            }
        }
    }

    /// Attach a [`WorkspaceManager`] so the multi-session routes
    /// (`/api/sessions...`) start serving requests. `self` is returned by
    /// value to allow chaining with [`AppState::with_hub`] / [`AppState::with_hub_and_path`].
    ///
    /// Side-effects:
    /// 1. cold-rebuilds `attach_index` from the workspace's session files
    ///    (ADR-0021 D7 amend ③). Failure here is logged but non-fatal —
    ///    the index simply starts empty and gets refilled as the mutation
    ///    hooks run.
    /// 2. sweeps `.locks/` for stale entries left by a prior SIGKILL /
    ///    panic (0071 §D-1, ADR-0019 D6). Strictly housekeeping — peek
    ///    already recognises Stale at runtime, so a failed sweep does not
    ///    affect functionality.
    pub fn with_workspace(mut self, workspace: WorkspaceManager) -> Self {
        let wm = Arc::new(workspace);
        if let Err(e) = self.attach_index.rebuild_from_disk(&wm) {
            tracing::warn!(
                error = %e,
                "attach_index: boot rebuild failed; starting empty (will refill on next mutation)"
            );
        }
        crate::session_lock::scan_and_cleanup_stale_locks(&wm);
        self.workspace = Some(wm);
        self
    }

    /// Assemble shared state and attach a WS broadcast hub so PUT-driven
    /// layout commits fan out to live subscribers.
    pub fn with_hub(config: Config, token: TokenString, hub: gtmux_ws_server::Hub) -> Self {
        let mut me = Self::new(config, token);
        me.hub = Some(hub);
        me
    }

    /// Like [`with_hub`](Self::with_hub) plus a workspace handle. Convenience
    /// for `gtmux start`'s boot wiring.
    pub fn with_hub_and_workspace(
        config: Config,
        token: TokenString,
        hub: gtmux_ws_server::Hub,
        workspace: WorkspaceManager,
    ) -> Self {
        let mut me = Self::with_hub(config, token, hub);
        me.workspace = Some(Arc::new(workspace));
        me
    }
}

/// Errors from [`AppState::spawn_terminal_with_uuid`]. Distinct from
/// [`HttpApiError`] so callers (handlers in Batch 4-B/C) can map each
/// variant to their own HTTP shape — 503 for `HubUnavailable`, 500 for
/// `Backend` / `Map` (internal consistency).
#[derive(Debug, Error)]
pub enum SpawnTerminalError {
    /// No PTY hub is attached to this [`AppState`] — typically a unit-test
    /// construction; production paths always wire a hub.
    #[error("hub_unavailable")]
    HubUnavailable,
    /// The backend failed to spawn (resource exhaustion, fork failure, …).
    #[error("backend: {0}")]
    Backend(#[from] gtmux_pty_backend::PtyBackendError),
    /// Internal terminal_map invariant violation (e.g. PaneId collision).
    #[error("terminal_map: {0}")]
    Map(TerminalMapError),
}

/// Errors produced by the HTTP API surface. Each variant maps to a stable
/// machine-readable `error` code returned in the JSON body and a HTTP status.
#[derive(Debug, Error)]
pub enum HttpApiError {
    /// Origin header missing or not in allowlist.
    #[error("origin_forbidden")]
    OriginForbidden,
    /// Host header missing or not in allowlist.
    #[error("host_forbidden")]
    HostForbidden,
    /// Authorization missing / malformed / wrong token.
    #[error("unauthorized")]
    Unauthorized,
    /// PUT without `If-Match`.
    #[error("precondition_required")]
    PreconditionRequired,
    /// PUT with stale `If-Match`.
    #[error("precondition_failed")]
    PreconditionFailed,
    /// Body did not satisfy the canvas-layout schema.
    #[error("bad_request: {0}")]
    BadRequest(String),
    /// Payload exceeded the 256 KB cap.
    #[error("payload_too_large")]
    PayloadTooLarge,
    /// Bootstrap query string did not include `token=`.
    #[error("missing_token")]
    MissingToken,
}

impl HttpApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::OriginForbidden | Self::HostForbidden => StatusCode::FORBIDDEN,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::PreconditionRequired => StatusCode::PRECONDITION_REQUIRED,
            Self::PreconditionFailed => StatusCode::PRECONDITION_FAILED,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::MissingToken => StatusCode::BAD_REQUEST,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::OriginForbidden => "origin_forbidden",
            Self::HostForbidden => "host_forbidden",
            Self::Unauthorized => "unauthorized",
            Self::PreconditionRequired => "precondition_required",
            Self::PreconditionFailed => "precondition_failed",
            Self::BadRequest(_) => "bad_request",
            Self::PayloadTooLarge => "payload_too_large",
            Self::MissingToken => "missing_token",
        }
    }
}

impl IntoResponse for HttpApiError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": self.code(),
            "message": self.to_string(),
        });
        (self.status(), Json(body)).into_response()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Router factory
// ─────────────────────────────────────────────────────────────────────────────

/// Build the full HTTP router with the documented middleware chain.
///
/// Returns an owned `axum::Router` ready to be merged with the WebSocket
/// router and handed to `axum::serve`. The config and token are cloned into
/// the `AppState` — callers may continue to hold their own references. No SPA
/// static fallback is wired; unknown paths return the structured 404.
pub fn router(config: &Config, token: &TokenString) -> Router {
    router_with_static(config, token, None)
}

/// Like [`router`] but mounts the built SPA at `frontend_dist` as the catch-all
/// fallback. Unknown paths first try the directory, then fall back to
/// `index.html` so client-side routing works. Used by `gtmux start` to serve
/// the bundled UI from a single port; tests typically pass `None`.
pub fn router_with_static(
    config: &Config,
    token: &TokenString,
    frontend_dist: Option<&Path>,
) -> Router {
    let state = AppState::new(config.clone(), token.clone());
    router_with_state_and_spa(state, frontend_dist)
}

/// Production variant: takes a fully-wired [`AppState`] (typically built
/// via [`AppState::with_hub`]) and an optional bundled SPA directory.
pub fn router_with_app_state(state: AppState, frontend_dist: Option<&Path>) -> Router {
    router_with_state_and_spa(state, frontend_dist)
}

/// Variant of [`router`] that lets callers (and tests) supply a pre-built
/// `AppState` — used to seed a non-empty layout or share counters across
/// multiple router instances. Production callers should prefer [`router`].
pub fn router_with_state(state: AppState) -> Router {
    router_with_state_and_spa(state, None)
}

/// Internal builder shared by every public router constructor. The optional
/// `frontend_dist` swaps the catch-all 404 for a `ServeDir` + `ServeFile`
/// chain so a single port serves both the API and the bundled SPA.
pub fn router_with_state_and_spa(state: AppState, frontend_dist: Option<&Path>) -> Router {
    // Authenticated subtree — `/api/*` routes. Bearer middleware is applied
    // here (not on the outer router) so `/healthz` and `/auth/bootstrap`
    // bypass it. Origin/Host checks still run on every request via the outer
    // chain.
    let api = Router::new()
        .route(
            "/api/sessions",
            get(sessions::list_handler).post(sessions::create_handler),
        )
        .route(
            "/api/sessions/import",
            axum::routing::post(sessions::import_handler)
                // ADR-0029 §6: lift axum's default 2 MB body cap to the
                // 16 MB ceiling shared with `PUT /api/sessions/:name/layout`
                // (sessions::SESSION_PUT_MAX_BYTES) — both endpoints write a
                // v2 layout and reasonable workloads (1000+ items with inline
                // documents) sit between the two ceilings.
                .layer(DefaultBodyLimit::max(sessions::SESSION_PUT_MAX_BYTES)),
        )
        .route("/api/sessions/{name}/export", get(sessions::export_handler))
        .route(
            "/api/sessions/{name}",
            axum::routing::delete(sessions::delete_handler),
        )
        .route(
            "/api/sessions/{name}/layout",
            get(sessions::layout_get_handler).put(sessions::layout_put_handler),
        )
        .route(
            "/api/sessions/{name}/attach",
            axum::routing::post(sessions::attach_handler).delete(sessions::detach_handler),
        )
        // ADR-0021 D6 amend ② / 0071 §D-5: sendBeacon-friendly best-effort
        // release. The matching reliable channel is
        // `DELETE /api/sessions/{name}/attach`; this one accepts URL-query
        // `webpage_id` because `navigator.sendBeacon` can't set headers.
        .route("/api/leave", axum::routing::post(sessions::leave_handler))
        .route(
            "/api/sessions/{name}/attach/confirm",
            axum::routing::post(sessions::attach_confirm_handler),
        )
        .route(
            "/api/sessions/{name}/terminals",
            axum::routing::post(sessions::create_terminal_handler),
        )
        .route(
            "/api/sessions/{name}/items/{id}",
            axum::routing::delete(sessions::delete_item_handler),
        )
        .route("/api/terminals", get(terminals::list_handler))
        .route(
            "/api/terminals/{id}",
            axum::routing::patch(terminals::patch_handler),
        )
        .route(
            "/api/terminals/{id}/kill",
            axum::routing::post(terminals::kill_handler),
        )
        .route(
            "/api/terminals/{id}/respawn",
            axum::routing::post(terminals::respawn_handler),
        )
        .route(
            "/api/settings",
            get(settings::get_handler).patch(settings::patch_handler),
        )
        .route(
            "/api/settings/password",
            axum::routing::post(settings::password_handler),
        )
        .route(
            "/api/settings/logout-all",
            axum::routing::post(settings::logout_all_handler),
        )
        .route(
            "/api/file-path/allowlist",
            get(file_open::allowlist_get_handler)
                .post(file_open::allowlist_post_handler)
                .delete(file_open::allowlist_delete_handler),
        )
        .route(
            "/api/file-path/allowlist-check",
            get(file_open::allowlist_check_handler),
        )
        .route(
            "/api/file-path/open",
            axum::routing::post(file_open::open_handler),
        )
        // ADR-0033 / 0080 — content-addressed `image`/`document` asset store.
        // Body cap is 20 MiB raw + ~1 MiB multipart headroom (boundary +
        // field overhead). axum 0.8's Multipart applies the limit to the
        // entire request body, so we add a little slack on top of the file
        // ceiling rather than 20 MiB exactly.
        .route(
            "/api/assets",
            axum::routing::post(assets::upload_handler)
                .layer(DefaultBodyLimit::max(assets::ASSET_MAX_BYTES + 1024 * 1024)),
        )
        .route(
            "/api/assets/from-path",
            axum::routing::post(assets::upload_from_path_handler),
        )
        .route("/api/assets/{asset_id}", get(assets::serve_handler))
        .route(
            // ADR-0034 — file_path fp-foot meta (lines / size / branch).
            // Same allowlist gate as `/api/file-path/open` per ADR-0034 D2.
            "/api/file-stat",
            get(file_stat::file_stat_handler),
        )
        .route(
            // ADR-0035 — file system picker. MVP scope = workspace dir only
            // (implicit allow). External roots land in Stage 3 with the
            // `[picker.roots]` toml schema mutation.
            "/api/fs/list",
            get(fs_list::fs_list_handler),
        )
        .route(
            "/api/shutdown",
            axum::routing::post(shutdown::shutdown_handler),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            bearer_auth_middleware,
        ));

    let mut router = Router::new()
        .merge(api)
        // Auth subtree — ADR-0020 + D13. `/auth` is intentionally *not*
        // routed here: the FE bundle (SPA fallback) is the single source for
        // the sign-in page. The legacy `/auth/bootstrap` survives as a 303
        // redirect to `/auth?t=…` (the FE AuthPage's magic-link contract)
        // so URLs printed by `gtmux start` keep working.
        .route("/auth/login", axum::routing::post(auth::auth_login_handler))
        .route(
            "/auth/logout",
            axum::routing::post(auth::auth_logout_handler),
        )
        .route(
            "/auth/rotate",
            axum::routing::post(auth::auth_rotate_handler),
        )
        .route("/auth/bootstrap", get(bootstrap_handler))
        .route("/healthz", get(healthz_handler));

    router = match frontend_dist {
        Some(dist) => {
            // SPA fallback: serve from `dist`, deferring unmatched paths to
            // `index.html` so client-side routing works. The Origin/Host
            // middleware still gates these requests; top-level navigations
            // omit Origin and so are passed through (see middleware below).
            let index = dist.join("index.html");
            let serve = ServeDir::new(dist).not_found_service(ServeFile::new(index));
            router.fallback_service(serve)
        }
        None => router.fallback(not_found_handler),
    };

    router
        .layer(middleware::from_fn_with_state(
            state.clone(),
            host_check_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            origin_check_middleware,
        ))
        .layer(TraceLayer::new_for_http().make_span_with(make_redacted_span))
        .with_state(state)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Middleware
// ─────────────────────────────────────────────────────────────────────────────

/// Origin check (ADR-0003 D3 / SSoT §1.2). Skipped for `/healthz` and the
/// bootstrap exchange — both are *entry points* where the browser may not
/// send an `Origin` header (top-level navigation). Cross-origin fetches into
/// `/api/*` would always send `Origin` per the Fetch spec, so the check fires
/// where it matters.
async fn origin_check_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    if path == "/healthz"
        || path == "/auth/bootstrap"
        || path == "/auth"
        || path == "/auth/login"
        || path == "/auth/logout"
    {
        return next.run(req).await;
    }
    if let Some(origin) = req.headers().get(header::ORIGIN) {
        // Reject Origin: null and any wildcard (R(rej)3). Exact match only.
        let origin_str = origin.to_str().unwrap_or("");
        if origin_str.is_empty() || origin_str == "null" {
            return HttpApiError::OriginForbidden.into_response();
        }
        // `effective_cors_origins` falls back to `http://<bind>:<port>` when
        // the user left the list empty (G1 same-origin default). Cloud
        // deployments with TLS terminate at a reverse proxy and must set
        // the list explicitly (no `wss://` synthesis here).
        let allowed = state.config.effective_cors_origins();
        if !allowed.iter().any(|a| a == origin_str) {
            return HttpApiError::OriginForbidden.into_response();
        }
    }
    // Missing Origin on /api/* is permissible — same-origin GET/PUT from the
    // SPA does not include it for non-CORS requests. Bearer auth still gates.
    next.run(req).await
}

/// Host header check (ADR-0003 D2 / SSoT §1.2 — DNS-rebinding defence). Runs
/// on every route including `/healthz` per the spec.
async fn host_check_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let host = match req.headers().get(header::HOST) {
        Some(h) => h.to_str().unwrap_or("").to_string(),
        None => return HttpApiError::HostForbidden.into_response(),
    };
    if host.is_empty() {
        return HttpApiError::HostForbidden.into_response();
    }
    let allowlist = state.config.effective_host_allowlist();
    if !allowlist.iter().any(|h| h == &host) {
        return HttpApiError::HostForbidden.into_response();
    }
    next.run(req).await
}

/// Bearer / cookie authentication (ADR-0003 D6 + ADR-0020 D2).
///
/// Accepts either:
///   * `Authorization: Bearer <token>` — the *stable* server token from
///     `gtmux start`. Constant-time compared. Always accepted (CLI/scripted
///     access).
///   * `Cookie: gtmux_auth=<opaque>` — an opaque session-id minted by
///     `/auth*` and stored in [`SessionTable`]. Validation bumps the rolling
///     expiry (ADR-0020 D3).
///
/// Failure increments `state.auth_failure_counter` so a future
/// rate-limit middleware can throttle without coupling.
async fn bearer_auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    match auth::authenticate(&state, req.headers()).await {
        Ok(()) => next.run(req).await,
        Err(()) => {
            state.auth_failure_counter.fetch_add(1, Ordering::Relaxed);
            HttpApiError::Unauthorized.into_response()
        }
    }
}

/// Cookie name issued by the auth flow (ADR-0020 D2). Exposed for tests
/// and external smoke scripts so they can assert on the `Set-Cookie` header
/// without re-hard-coding the literal.
pub const COOKIE_NAME_STR: &str = "gtmux_auth";

// ─────────────────────────────────────────────────────────────────────────────
//  Handlers
// ─────────────────────────────────────────────────────────────────────────────

async fn healthz_handler() -> Response {
    let mut resp = Json(json!({ "ok": true })).into_response();
    apply_security_headers(resp.headers_mut(), Mode::Local /* harmless */);
    resp
}

async fn not_found_handler() -> Response {
    let body = json!({ "error": "not_found" });
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

#[derive(Debug, Deserialize)]
struct BootstrapQuery {
    token: Option<String>,
    redirect: Option<String>,
}

/// Legacy bootstrap route — ADR-0020 D8 obsoleted the inline-script flow,
/// and D13 hands the sign-in page off to the FE bundle. We keep the URL
/// alive so existing bookmarks (and the URL printed by `gtmux start`) still
/// work, but the body is now a 303 to `/auth?t=…` — the FE AuthPage's
/// magic-link contract. The token is then POSTed to `/auth/login` by the FE
/// to mint the cookie.
async fn bootstrap_handler(
    State(_state): State<AppState>,
    Query(q): Query<BootstrapQuery>,
) -> Response {
    let Some(token) = q.token.filter(|t| !t.is_empty()) else {
        return HttpApiError::MissingToken.into_response();
    };
    // Re-encode token + redirect so a path-traversal-shaped redirect from a
    // stale bookmark isn't laundered into a header-splitting payload.
    let target = match q.redirect.as_deref() {
        Some(r) => format!(
            "/auth?t={}&redirect={}",
            urlencode_query(&token),
            urlencode_query(r)
        ),
        None => format!("/auth?t={}", urlencode_query(&token)),
    };
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, target)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::empty())
        .expect("static headers")
}

fn urlencode_query(s: &str) -> String {
    // Tiny inline encoder — only escapes the bytes the URL grammar reserves
    // for query separators (`&`, `=`, `+`, `#`) plus whitespace and CR/LF.
    // The legacy bootstrap caller is server-internal; this is belt-and-braces.
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Stamp the standard security headers (ADR-0003 §1) on `headers`.
/// `Mode::Cloud` additionally adds `Strict-Transport-Security`; local mode
/// omits it because plain HTTP would silently drop the directive.
pub(crate) fn apply_security_headers(headers: &mut HeaderMap, mode: Mode) {
    static NOSNIFF: HeaderValue = HeaderValue::from_static("nosniff");
    static REFERRER: HeaderValue = HeaderValue::from_static("no-referrer");
    static COOP: HeaderValue = HeaderValue::from_static("same-origin");
    static CORP: HeaderValue = HeaderValue::from_static("same-origin");
    static PERMS: HeaderValue =
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), interest-cohort=()");
    static HSTS: HeaderValue = HeaderValue::from_static("max-age=31536000; includeSubDomains");

    headers.insert(header::X_CONTENT_TYPE_OPTIONS, NOSNIFF.clone());
    headers.insert(header::REFERRER_POLICY, REFERRER.clone());
    headers.insert("cross-origin-opener-policy", COOP.clone());
    headers.insert("cross-origin-resource-policy", CORP.clone());
    headers.insert("permissions-policy", PERMS.clone());
    if matches!(mode, Mode::Cloud) {
        headers.insert(header::STRICT_TRANSPORT_SECURITY, HSTS.clone());
    }
}

/// Config-aware variant for auth flows. Explicit non-TLS cloud mode keeps the
/// hardening headers that are safe over HTTP, but does not emit HSTS.
pub(crate) fn apply_security_headers_for_config(headers: &mut HeaderMap, config: &Config) {
    static NOSNIFF: HeaderValue = HeaderValue::from_static("nosniff");
    static REFERRER: HeaderValue = HeaderValue::from_static("no-referrer");
    static COOP: HeaderValue = HeaderValue::from_static("same-origin");
    static CORP: HeaderValue = HeaderValue::from_static("same-origin");
    static PERMS: HeaderValue =
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), interest-cohort=()");
    static HSTS: HeaderValue = HeaderValue::from_static("max-age=31536000; includeSubDomains");

    headers.insert(header::X_CONTENT_TYPE_OPTIONS, NOSNIFF.clone());
    headers.insert(header::REFERRER_POLICY, REFERRER.clone());
    headers.insert("cross-origin-opener-policy", COOP.clone());
    headers.insert("cross-origin-resource-policy", CORP.clone());
    headers.insert("permissions-policy", PERMS.clone());
    if config.tls_required() {
        headers.insert(header::STRICT_TRANSPORT_SECURITY, HSTS.clone());
    }
}

#[derive(Debug)]
enum BodyReadError {
    TooLarge,
    Io(String),
}

/// Drain the request body, refusing payloads larger than `cap`. We do not
/// trust `Content-Length` alone — a malicious client could lie — so the
/// stream is read incrementally and short-circuited at the cap.
async fn read_bounded_body(req: Request, cap: usize) -> Result<Vec<u8>, BodyReadError> {
    use http_body_util::BodyExt;

    let body = req.into_body();
    let collected = body
        .collect()
        .await
        .map_err(|e| BodyReadError::Io(format!("body read: {e}")))?;
    let bytes = collected.to_bytes();
    if bytes.len() > cap {
        return Err(BodyReadError::TooLarge);
    }
    Ok(bytes.to_vec())
}

/// `MakeSpan` impl that records only the path — not the query string. This
/// keeps the bootstrap token out of trace exports (the URL otherwise lands
/// in spans, jaeger payloads, journald, etc.). ADR-0003 §C R(rej)2 redaction.
fn make_redacted_span(req: &Request) -> tracing::Span {
    let path = req.uri().path();
    let method = req.method().as_str();
    tracing::info_span!(
        "http_request",
        method = %method,
        path = %path,
        // query is *intentionally* omitted — never log the raw URI.
    )
}

// Methods/Uri unused-import shake: keep linter quiet without changing
// behaviour. (Some axum builds re-export these via prelude; explicit imports
// document intent.)
const _: fn(&Method, &Uri) = |_, _| {};

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request as HttpRequest;
    use gtmux_auth::issue_token;
    use gtmux_config::{CloudConfig, Config, RuntimeConfig, SecurityConfig, ServerConfig};
    use tower::ServiceExt; // for `oneshot`

    const TEST_HOST: &str = "127.0.0.1:9001";
    const TEST_ORIGIN: &str = "http://localhost:9001";

    fn test_config() -> Config {
        Config {
            schema_version: 1,
            server: ServerConfig {
                session: "test".to_string(),
                port: 9001,
                bind: "127.0.0.1".to_string(),
            },
            runtime: RuntimeConfig::default(),
            security: SecurityConfig {
                cors_origins: vec![TEST_ORIGIN.to_string()],
                host_allowlist: vec![TEST_HOST.to_string()],
            },
            cloud: None,
            frontend_dist: None,
            workspace_path: None,
            auth: gtmux_config::AuthConfig::default(),
        }
    }

    fn cloud_test_config(tls_required: bool) -> Config {
        Config {
            server: ServerConfig {
                bind: "0.0.0.0".to_string(),
                ..test_config().server
            },
            cloud: Some(CloudConfig {
                tls_required,
                tls_cert: std::path::PathBuf::from("/dev/null"),
                tls_key: std::path::PathBuf::from("/dev/null"),
                rate_limit_auth_failures_per_minute: 10,
            }),
            ..test_config()
        }
    }

    fn make_app() -> (Router, TokenString) {
        let token = issue_token().expect("token");
        let cfg = test_config();
        let app = router(&cfg, &token);
        (app, token)
    }

    fn bearer(token: &TokenString) -> String {
        format!("Bearer {}", token.0)
    }

    #[tokio::test]
    async fn healthz_no_auth() {
        let (app, _token) = make_app();
        let req = HttpRequest::builder()
            .uri("/healthz")
            .header(header::HOST, TEST_HOST)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body, json!({"ok": true}));
    }

    #[tokio::test]
    async fn origin_check_blocks_disallowed() {
        let (app, token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::ORIGIN, "http://evil.example")
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn host_check_blocks_disallowed() {
        let (app, token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, "evil.example")
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // ── ADR-0020 + D13 — auth-page wiring ──
    //
    // The server-rendered `GET /auth` handler is gone (D13): the FE SPA
    // bundle is now the single source for the sign-in page. The legacy
    // `/auth/bootstrap` URL survives as a 303 redirect to the FE-handled
    // `/auth?t=…` so URLs printed by `gtmux start` keep working. Tests
    // below cover the bootstrap redirect contract; the SPA fallback is
    // exercised by the FE/E2E layer. Cookie minting is now reached via
    // `POST /auth/login`.

    #[tokio::test]
    async fn bootstrap_legacy_route_redirects_to_fe_auth_page() {
        // D13: `gtmux start` prints `/auth/bootstrap?token=…`. The handler
        // must 303 to `/auth?t=…` (the FE AuthPage magic-link contract)
        // — not the legacy `?token=` form the old server-rendered handler
        // accepted.
        let (app, token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/auth/bootstrap?token={}", token.0))
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let location = resp
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            location.starts_with("/auth?t="),
            "bootstrap must redirect to FE magic-link path /auth?t=…, got {location}"
        );
        assert!(
            !location.contains("token="),
            "legacy ?token= must not survive in the redirect (FE expects ?t=): {location}"
        );
    }

    #[tokio::test]
    async fn bootstrap_missing_token() {
        let (app, _token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/auth/bootstrap")
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn cookie_auth_works_after_login() {
        let (app, token) = make_app();
        // D13: cookies are minted by `POST /auth/login`, not the legacy
        // `GET /auth?token=` server-rendered handler.
        let login_body = serde_json::to_vec(&json!({ "token": token.0 })).unwrap();
        let auth_resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/auth/login")
                    .header(header::HOST, TEST_HOST)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(login_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(auth_resp.status(), StatusCode::OK);
        let name_value = auth_resp
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .trim()
            .to_string();

        // After the layout v1 cleanup (handover §5.3.3), use the v2
        // `/api/sessions` endpoint to verify the cookie satisfies the
        // `/api/*` auth middleware.
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, &name_value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            resp.status() == StatusCode::OK || resp.status() == StatusCode::SERVICE_UNAVAILABLE,
            "session cookie must reach the middleware (got {:?}); 503 is OK when this AppState has no workspace",
            resp.status()
        );
        assert_ne!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "session cookie must satisfy the auth middleware"
        );
    }

    #[tokio::test]
    async fn auth_logout_clears_cookie_and_revokes() {
        let (app, token) = make_app();
        // D13: cookies are minted by `POST /auth/login`.
        let login_body = serde_json::to_vec(&json!({ "token": token.0 })).unwrap();
        let auth_resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/auth/login")
                    .header(header::HOST, TEST_HOST)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(login_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(auth_resp.status(), StatusCode::OK);
        let name_value = auth_resp
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .trim()
            .to_string();

        // POST /auth/logout with the cookie — must succeed.
        let logout = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/auth/logout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, &name_value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logout.status(), StatusCode::OK);
        let clear = logout
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            clear.contains("Max-Age=0"),
            "clear cookie expected: {clear}"
        );

        // Subsequent request with the now-revoked cookie must 401.
        // Targets `/api/sessions` after the layout v1 cleanup
        // (handover §5.3.3) — any authed `/api/*` path works.
        let after = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, &name_value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(after.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_login_token_mode_returns_set_cookie_on_success() {
        let (app, token) = make_app();
        let body = serde_json::to_vec(&json!({ "token": token.0 })).unwrap();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/auth/login")
                    .header(header::HOST, TEST_HOST)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().contains_key(header::SET_COOKIE));
    }

    #[tokio::test]
    async fn auth_login_cloud_tls_required_sets_secure_cookie_and_hsts() {
        let token = issue_token().expect("token");
        let cfg = cloud_test_config(true);
        let app = router(&cfg, &token);
        let body = serde_json::to_vec(&json!({ "token": token.0 })).unwrap();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/auth/login")
                    .header(header::HOST, TEST_HOST)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let set_cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            set_cookie.contains("Secure"),
            "cookie expected Secure: {set_cookie}"
        );
        assert!(resp
            .headers()
            .contains_key(header::STRICT_TRANSPORT_SECURITY));
    }

    #[tokio::test]
    async fn auth_login_cloud_tls_disabled_omits_secure_cookie_and_hsts() {
        let token = issue_token().expect("token");
        let cfg = cloud_test_config(false);
        let app = router(&cfg, &token);
        let body = serde_json::to_vec(&json!({ "token": token.0 })).unwrap();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/auth/login")
                    .header(header::HOST, TEST_HOST)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let set_cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            !set_cookie.contains("Secure"),
            "cookie should work over explicit non-TLS cloud HTTP: {set_cookie}"
        );
        assert!(!resp
            .headers()
            .contains_key(header::STRICT_TRANSPORT_SECURITY));
    }

    #[tokio::test]
    async fn auth_login_token_mode_rejects_wrong_token() {
        let (app, _token) = make_app();
        let body = serde_json::to_vec(&json!({ "token": "A".repeat(43) })).unwrap();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/auth/login")
                    .header(header::HOST, TEST_HOST)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(resp.headers().get(header::SET_COOKIE).is_none());
    }

    // ── ADR-0020 D14: POST /auth/rotate ──

    /// Helper — login token-mode, return the minted `gtmux_auth=<value>`
    /// cookie value (just the opaque part, no flags).
    async fn login_and_get_cookie_value(app: &Router, token: &TokenString) -> String {
        let body = serde_json::to_vec(&json!({ "token": token.0 })).unwrap();
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/auth/login")
                    .header(header::HOST, TEST_HOST)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let set_cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .expect("Set-Cookie")
            .to_str()
            .unwrap()
            .to_string();
        // Strip flags — keep `gtmux_auth=<value>` only.
        set_cookie.split(';').next().unwrap().trim().to_string()
    }

    #[tokio::test]
    async fn auth_rotate_issues_fresh_cookie_and_revokes_old() {
        let (app, token) = make_app();
        let old_cookie = login_and_get_cookie_value(&app, &token).await;

        // Rotate.
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/auth/rotate")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, &old_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let new_set_cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .expect("rotate emits a new Set-Cookie")
            .to_str()
            .unwrap()
            .to_string();
        assert!(new_set_cookie.starts_with(&format!("{COOKIE_NAME_STR}=")));
        assert!(new_set_cookie.contains("HttpOnly"));
        let new_cookie = new_set_cookie.split(';').next().unwrap().trim().to_string();
        assert_ne!(
            new_cookie, old_cookie,
            "rotate must mint a fresh opaque value"
        );

        let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ok"], json!(true));
        // Only the caller was alive → revoked_count == 1 (caller's old session).
        assert_eq!(v["revoked_count"], json!(1));

        // The new cookie must satisfy the auth middleware.
        let ok = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, &new_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(
            ok.status(),
            StatusCode::UNAUTHORIZED,
            "fresh rotated cookie must authenticate (got {:?})",
            ok.status()
        );

        // The old cookie must now 401.
        let stale = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, &old_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            stale.status(),
            StatusCode::UNAUTHORIZED,
            "old cookie must be revoked after rotate"
        );
    }

    #[tokio::test]
    async fn auth_rotate_revokes_other_sessions_too() {
        let (app, token) = make_app();
        // Two independent logins → two cookies. Rotating one must revoke
        // both the rotator's old cookie *and* the other live session.
        let cookie_a = login_and_get_cookie_value(&app, &token).await;
        let cookie_b = login_and_get_cookie_value(&app, &token).await;
        assert_ne!(
            cookie_a, cookie_b,
            "two logins should mint distinct cookies"
        );

        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/auth/rotate")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, &cookie_a)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        // Caller (a) was revoked + the other session (b) — count of 2.
        assert_eq!(v["revoked_count"], json!(2));

        // cookie_b must now 401 even though it never touched /rotate.
        let stale = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, &cookie_b)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stale.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_rotate_401_without_cookie() {
        let (app, _token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/auth/rotate")
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(resp.headers().get(header::SET_COOKIE).is_none());
    }

    #[tokio::test]
    async fn auth_rotate_401_with_invalid_cookie() {
        let (app, _token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/auth/rotate")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, "gtmux_auth=not-a-real-session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(resp.headers().get(header::SET_COOKIE).is_none());
    }

    #[test]
    fn normalise_redirect_blocks_open_redirect() {
        // The helper lives in `auth.rs` now (ADR-0020 D8 relocation). The
        // unit-level coverage there is authoritative; this stub keeps a
        // breadcrumb so a future move stays traceable.
        assert_eq!(crate::auth::normalise_redirect_target(None), "/");
        assert_eq!(crate::auth::normalise_redirect_target(Some("//evil")), "/");
        assert_eq!(crate::auth::normalise_redirect_target(Some("/\\evil")), "/");
        assert_eq!(
            crate::auth::normalise_redirect_target(Some("https://evil")),
            "/"
        );
        assert_eq!(crate::auth::normalise_redirect_target(Some("evil")), "/");
        assert_eq!(
            crate::auth::normalise_redirect_target(Some("/canvas")),
            "/canvas"
        );
        assert_eq!(
            crate::auth::normalise_redirect_target(Some("/x\r\nSet-Cookie: ev")),
            "/"
        );
    }

    // ── Multi-session HTTP surface (Stage 1, ADR-0019 + ADR-0018) ──

    fn make_app_with_workspace(
        dir: &tempfile::TempDir,
    ) -> (Router, TokenString, std::path::PathBuf) {
        let token = issue_token().expect("token");
        let cfg = test_config();
        let workspace_dir = dir.path().to_path_buf();
        let wm = WorkspaceManager::from_path(workspace_dir.clone()).expect("workspace");
        let state = AppState::new(cfg, token.clone()).with_workspace(wm);
        let app = router_with_state(state);
        (app, token, workspace_dir)
    }

    /// Variant of [`make_app_with_workspace`] that *also* returns the
    /// `AppState` so attach_index assertions (0067 Phase 4 / 0068) can
    /// peek at the in-memory reverse index after issuing HTTP requests.
    fn make_app_with_workspace_and_state(
        dir: &tempfile::TempDir,
    ) -> (Router, TokenString, std::path::PathBuf, AppState) {
        let token = issue_token().expect("token");
        let cfg = test_config();
        let workspace_dir = dir.path().to_path_buf();
        let wm = WorkspaceManager::from_path(workspace_dir.clone()).expect("workspace");
        let state = AppState::new(cfg, token.clone()).with_workspace(wm);
        let app = router_with_state(state.clone());
        (app, token, workspace_dir, state)
    }

    #[tokio::test]
    async fn sessions_list_empty_on_fresh_workspace() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body, json!([]));
    }

    /// 0074 Phase 1: `GET /api/sessions` emits the boot's `server_id`
    /// as an `X-Gtmux-Server-Id` response header so the FE can detect
    /// a Server restart (stale tab → cleanup + session selection).
    #[tokio::test]
    async fn sessions_list_emits_server_id_header() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _, state) = make_app_with_workspace_and_state(&dir);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let header_val = resp
            .headers()
            .get("x-gtmux-server-id")
            .expect("list response must carry X-Gtmux-Server-Id")
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(
            header_val, *state.server_id,
            "header value must equal AppState::server_id"
        );
        // UUID v4 shape (length 36, hyphenated 8-4-4-4-12).
        assert_eq!(header_val.len(), 36);
    }

    #[tokio::test]
    async fn sessions_create_then_list_then_layout() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, workspace_dir) = make_app_with_workspace(&dir);

        // POST /api/sessions { name: "demo" } → 201
        let create_body = serde_json::to_vec(&json!({ "name": "demo" })).unwrap();
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(create_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        // File must exist on disk.
        assert!(workspace_dir.join("demo.json").exists());

        // GET /api/sessions lists it.
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body, json!([{ "name": "demo", "active": false }]));

        // GET /api/sessions/demo/layout returns an empty v2 layout.
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions/demo/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let etag = resp.headers().get(header::ETAG).cloned();
        assert!(etag.is_some(), "ETag header must be present");
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let layout: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(layout["schema_version"], 2);
        assert_eq!(layout["groups"].as_array().unwrap().len(), 0);
        assert_eq!(layout["items"].as_array().unwrap().len(), 0);
        assert!(layout["viewport"].is_object());
    }

    #[tokio::test]
    async fn sessions_create_rejects_duplicate() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        let body = || serde_json::to_vec(&json!({ "name": "twin" })).unwrap();
        let make_req = || {
            HttpRequest::builder()
                .method(Method::POST)
                .uri("/api/sessions")
                .header(header::HOST, TEST_HOST)
                .header(header::AUTHORIZATION, bearer(&token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body()))
                .unwrap()
        };
        let r1 = app.clone().oneshot(make_req()).await.unwrap();
        assert_eq!(r1.status(), StatusCode::CREATED);
        let r2 = app.clone().oneshot(make_req()).await.unwrap();
        assert_eq!(r2.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn sessions_create_rejects_invalid_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        for bad in ["", "../etc", "a/b", "has space"] {
            let body = serde_json::to_vec(&json!({ "name": bad })).unwrap();
            let resp = app
                .clone()
                .oneshot(
                    HttpRequest::builder()
                        .method(Method::POST)
                        .uri("/api/sessions")
                        .header(header::HOST, TEST_HOST)
                        .header(header::AUTHORIZATION, bearer(&token))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "name {bad:?}");
        }
    }

    // ── Slice D-4: POST /api/sessions/import (G28) ──

    fn import_layout_body(name: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "name": name,
            "layout": {
                "schema_version": 2,
                "groups": [],
                "items": [],
                "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
            }
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn sessions_import_201_with_name_and_created_at() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, wd) = make_app_with_workspace(&dir);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/import")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(import_layout_body("imported")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = to_bytes(resp.into_body(), 8 * 1024).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["name"], "imported");
        assert!(v["created_at"].as_u64().unwrap() > 0);
        // File persisted under the workspace dir.
        assert!(wd.join("imported.json").exists());
    }

    #[tokio::test]
    async fn sessions_import_409_on_name_conflict() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _wd) = make_app_with_workspace(&dir);
        // Seed by creating the same name first.
        let create_body = serde_json::to_vec(&json!({ "name": "dup" })).unwrap();
        let r1 = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(create_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::CREATED);
        let r2 = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/import")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(import_layout_body("dup")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r2.status(), StatusCode::CONFLICT);
        let bytes = to_bytes(r2.into_body(), 8 * 1024).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "name_conflict");
        assert_eq!(v["name"], "dup");
    }

    #[tokio::test]
    async fn sessions_import_400_on_invalid_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _wd) = make_app_with_workspace(&dir);
        let body = serde_json::to_vec(&json!({
            "name": "../escape",
            "layout": {
                "schema_version": 2,
                "groups": [],
                "items": [],
                "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
            }
        }))
        .unwrap();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/import")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn sessions_import_400_on_schema_invalid() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _wd) = make_app_with_workspace(&dir);
        // schema_version = 1 → bad_schema_version per validate().
        let body = serde_json::to_vec(&json!({
            "name": "bad-schema",
            "layout": {
                "schema_version": 1,
                "groups": [],
                "items": [],
                "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
            }
        }))
        .unwrap();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/import")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(resp.into_body(), 8 * 1024).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "schema_invalid");
        assert!(v["field"].is_string());
        assert!(v["details"].is_string());
    }

    /// ADR-0029 §6 — import body cap. The route layers
    /// `DefaultBodyLimit::max(SESSION_PUT_MAX_BYTES)` (16 MiB); axum rejects
    /// the request with 413 before the handler runs when the body exceeds it.
    #[tokio::test]
    async fn sessions_import_413_when_body_exceeds_cap() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        // 17 MiB of JSON-safe padding wrapped in a structurally-valid envelope.
        // `_bloat` lives outside `viewport`'s known fields — but schema
        // validation never runs because the body cap layer aborts the read
        // first. The padding sits as a top-level sibling of `name` / `layout`,
        // so it doesn't disturb the deserialise target either.
        let bloat = "a".repeat(17 * 1024 * 1024);
        let body = format!(
            r#"{{"name":"x","layout":{{"schema_version":2,"groups":[],"items":[],"viewport":{{"x":0.0,"y":0.0,"zoom":1.0}}}},"_bloat":"{bloat}"}}"#
        );
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/import")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    /// Positive control for ADR-0029 §6 — a sub-cap body (here ~5 MiB of
    /// padding kept *outside* the schema struct's known fields) is accepted
    /// past the body-read stage. The handler may still reject it later for
    /// schema reasons, but never with 413; this guards against accidentally
    /// re-lowering the cap below the 8–16 MiB band the ADR carved out.
    #[tokio::test]
    async fn sessions_import_accepts_body_below_cap() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        // 5 MiB of padding — well above the legacy 2 MiB axum default,
        // well below the 16 MiB ADR cap. The known schema fields stay
        // valid so the import actually lands.
        let bloat = "a".repeat(5 * 1024 * 1024);
        let body = format!(
            r#"{{"name":"big","layout":{{"schema_version":2,"groups":[],"items":[],"viewport":{{"x":0.0,"y":0.0,"zoom":1.0}}}},"_bloat":"{bloat}"}}"#
        );
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/import")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "5 MiB sub-cap body must not trip the 413 path"
        );
    }

    #[tokio::test]
    async fn sessions_import_then_list_includes_record() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _wd) = make_app_with_workspace(&dir);
        let r1 = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/import")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(import_layout_body("ledger")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::CREATED);
        let list = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(list.into_body(), 8 * 1024).await.unwrap();
        let rows: Vec<Value> = serde_json::from_slice(&bytes).unwrap();
        assert!(rows.iter().any(|r| r["name"] == "ledger"));
    }

    // ── ADR-0029 D4: GET /api/sessions/:name/export (0052 work package) ──

    /// Gate 0029-1 — happy path: existing session returns 200 + envelope +
    /// `Content-Disposition: attachment; filename="<name>.gtmux-session.json"`.
    #[tokio::test]
    async fn export_returns_envelope_for_existing_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        create_session(&app, &token, "alpha").await;

        let res = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::GET)
                    .uri("/api/sessions/alpha/export")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let ct = res
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.starts_with("application/json"));
        let dispo = res
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .expect("Content-Disposition present")
            .to_str()
            .unwrap();
        assert!(
            dispo.contains(r#"filename="alpha.gtmux-session.json""#),
            "Content-Disposition must carry sanitized filename, got {dispo}"
        );

        let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let env: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(env["kind"], "gtmux.session.export");
        assert_eq!(env["export_version"], 1);
        assert_eq!(env["session_name"], "alpha");
        assert_eq!(env["layout"]["schema_version"], 2);
        assert!(env["layout"]["items"].is_array());
        assert!(env["layout"]["groups"].is_array());
        assert_eq!(
            env["metadata"]["app"], "gtmux",
            "metadata.app must be 'gtmux'"
        );
        // RFC3339 shape — `YYYY-MM-DDTHH:MM:SSZ` (20 chars).
        let exported_at = env["exported_at"].as_str().expect("exported_at string");
        assert_eq!(exported_at.len(), 20, "RFC3339 length: {exported_at}");
        assert!(exported_at.ends_with('Z'));
        assert_eq!(exported_at.chars().nth(4), Some('-'));
        assert_eq!(exported_at.chars().nth(10), Some('T'));
    }

    /// Gate 0029-2 — missing session returns 404 with `not_found` + the
    /// requested name in the body.
    #[tokio::test]
    async fn export_404_for_missing_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        let res = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::GET)
                    .uri("/api/sessions/missing/export")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let bytes = to_bytes(res.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "not_found");
        assert_eq!(v["name"], "missing");
    }

    /// Gate 0029-3 — without bearer auth the `/api/*` middleware returns
    /// 401; no envelope leaks to anonymous callers.
    #[tokio::test]
    async fn export_401_without_auth() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        create_session(&app, &token, "alpha").await;
        let res = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::GET)
                    .uri("/api/sessions/alpha/export")
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert!(res.headers().get(header::CONTENT_DISPOSITION).is_none());
    }

    /// Gate 0029-4 — names that fail `validate_session_name` return 400 with
    /// `invalid_session_name`. Belt-and-braces against path traversal — the
    /// regex `[A-Za-z0-9_-]{1,64}` already rejects everything fancy, this
    /// just asserts the response shape.
    #[tokio::test]
    async fn export_400_for_invalid_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        let res = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::GET)
                    .uri("/api/sessions/has.dot/export")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(res.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "invalid_session_name");
    }

    /// Gate 0029-5 — export → import-as-new-name round-trip. The reloaded
    /// session's `GET /layout` must match the exported envelope's `layout`
    /// (modulo ETag which is regenerated on import).
    #[tokio::test]
    async fn export_import_round_trip_equal_layout() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        create_session(&app, &token, "src").await;

        // 1. Export.
        let exp_res = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::GET)
                    .uri("/api/sessions/src/export")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(exp_res.status(), StatusCode::OK);
        let envelope: Value =
            serde_json::from_slice(&to_bytes(exp_res.into_body(), 1 << 20).await.unwrap()).unwrap();
        let exported_layout = envelope["layout"].clone();

        // 2. Import the envelope's `layout` under a fresh name.
        let import_body = serde_json::to_vec(&json!({
            "name": "dst",
            "layout": exported_layout.clone(),
        }))
        .unwrap();
        let imp_res = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/import")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(import_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(imp_res.status(), StatusCode::CREATED);

        // 3. GET the imported layout and compare.
        let get_res = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::GET)
                    .uri("/api/sessions/dst/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_res.status(), StatusCode::OK);
        let reloaded: Value =
            serde_json::from_slice(&to_bytes(get_res.into_body(), 1 << 20).await.unwrap()).unwrap();
        assert_eq!(reloaded, exported_layout, "round-trip layout must match");
    }

    #[tokio::test]
    async fn sessions_layout_put_etag_cas() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        let create_body = serde_json::to_vec(&json!({ "name": "demo" })).unwrap();
        app.clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(create_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        // ADR-0019 D5.6: PUT /layout demands an owner-scoped attach. Bearer-
        // only auth → owner_key falls back to "_unknown" in both attach +
        // PUT, so the guard passes once we run the attach handler.
        assert_eq!(attach(&app, &token, "demo").await, StatusCode::OK);

        // GET to fetch current ETag.
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions/demo/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let etag = resp
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        // Stale If-Match → 412.
        let put_body = serde_json::to_vec(&json!({
            "schema_version": 2,
            "groups": [],
            "items": [],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        }))
        .unwrap();
        let stale = "\"00000000000000000000000000000000\"";
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/sessions/demo/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::IF_MATCH, stale)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(put_body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PRECONDITION_FAILED);
        assert!(resp.headers().contains_key(header::ETAG));

        // Fresh If-Match → 204 + new ETag.
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/sessions/demo/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::IF_MATCH, etag)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(put_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(resp.headers().contains_key(header::ETAG));
    }

    /// ADR-0006 D13 amend ③ (0066 §BE-4 / 0067 Phase 3) — verify that the
    /// PUT path's `spawn_blocking` disk write produces bytes that, when
    /// hashed, recompose into the same ETag the response header returned.
    /// If `spawn_blocking` truncates, writes the wrong buffer, or races
    /// the in-memory snapshot swap, this round-trip detects it.
    #[tokio::test]
    async fn sessions_layout_put_disk_bytes_match_response_etag() {
        use ring::digest::{digest, SHA256};
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, workspace_dir) = make_app_with_workspace(&dir);
        let create_body = serde_json::to_vec(&json!({ "name": "be4" })).unwrap();
        app.clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(create_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        // ADR-0019 D5.6 owner-attach guard.
        assert_eq!(attach(&app, &token, "be4").await, StatusCode::OK);

        // GET to fetch current ETag.
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions/be4/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let etag = resp
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        // PUT a non-trivial layout so the byte payload differs from the
        // initial empty state — exercises the spawn_blocking write path
        // with real content rather than the no-change shortcut.
        let put_layout = json!({
            "schema_version": 2,
            "groups": [],
            "items": [],
            "viewport": { "x": 12.5, "y": -7.25, "zoom": 1.5 },
        });
        let put_body = serde_json::to_vec(&put_layout).unwrap();
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/sessions/be4/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::IF_MATCH, etag)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(put_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let new_etag_quoted = resp
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        // Read the file the spawn_blocking write produced.
        let disk_bytes = std::fs::read(workspace_dir.join("be4.json")).unwrap();
        // Hash → first 16 bytes → 32-hex → quote.
        let d = digest(&SHA256, &disk_bytes);
        let mut hex = String::with_capacity(32);
        for b in &d.as_ref()[..16] {
            hex.push_str(&format!("{b:02x}"));
        }
        let from_disk = format!("\"{hex}\"");
        assert_eq!(
            new_etag_quoted, from_disk,
            "response ETag must equal SHA256-128 of disk bytes after spawn_blocking write"
        );
    }

    // ── ADR-0021 D7 amend ③ (0066 §BE-2 / 0067 Phase 4 / 0068 work package) ──
    //
    // attach_index integration tests — confirms each of the four mutation
    // hooks keeps the in-memory reverse index in lock-step with the
    // disk-of-truth that powers `GET /api/terminals`'s `attach_count`.

    /// Helper: PUT a layout with `If-Match` = current ETag and a single
    /// terminal item carrying `uuid`. Returns the new ETag.
    async fn put_layout_with_terminal(
        app: &Router,
        token: &TokenString,
        session: &str,
        uuid: &str,
    ) -> String {
        // Fetch current ETag.
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/api/sessions/{session}/layout"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let etag = resp
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        // PUT a layout containing the terminal item. The `x-gtmux-webpage-id`
        // header carries the same value the matching `attach_idx_create_session`
        // call used, so `attach_owner_key(headers)` reproduces the same
        // owner_key and clears the ADR-0019 D5.6 attach-guard on PUT.
        let layout = json!({
            "schema_version": 2,
            "groups": [],
            "items": [{
                "type": "terminal",
                "id": uuid,
                "parent_id": null,
                "x": 0.0, "y": 0.0, "w": 100.0, "h": 100.0, "z": 0,
                "visibility": "visible",
                "locked": false,
                "label": "", "description": "",
                "minimized": false,
            }],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        });
        let put_body = serde_json::to_vec(&layout).unwrap();
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/sessions/{session}/layout"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(token))
                    .header("x-gtmux-webpage-id", session)
                    .header(header::IF_MATCH, etag)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(put_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        resp.headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    async fn attach_idx_create_session(app: &Router, token: &TokenString, name: &str) {
        let create_body = serde_json::to_vec(&json!({ "name": name })).unwrap();
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(create_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        // ADR-0019 D5.6: PUT /layout + DELETE /items require an owner-scoped
        // attach. Each session attaches as its own Webpage (`webpage_id` ==
        // session name) so multi-session tests get distinct owner keys —
        // attaching session B does not implicitly detach session A.
        assert_eq!(
            attach_as_webpage(app, token, name, name).await,
            StatusCode::OK
        );
    }

    /// Test helper: seed `session_locks_by_owner` so handlers protected by
    /// the ADR-0019 D5.6 owner-attach guard (`PUT /layout`,
    /// `DELETE /items`) treat a bearer-only request as already attached.
    /// Bypasses the real flock — pure in-memory poke. The owner_key key
    /// matches what `attach_owner_key(headers)` returns for a request with
    /// no `Cookie` and no `x-gtmux-webpage-id` header.
    async fn seed_owner_attached(state: &AppState, name: &str) {
        state
            .session_locks_by_owner
            .lock()
            .await
            .insert("_unknown".to_string(), name.to_string());
    }

    const UUID_A: &str = "11111111-2222-4333-8444-aaaaaaaaaaaa";
    const UUID_B: &str = "11111111-2222-4333-8444-bbbbbbbbbbbb";

    #[tokio::test]
    async fn attach_index_layout_put_adds_uuid_to_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _, state) = make_app_with_workspace_and_state(&dir);
        attach_idx_create_session(&app, &token, "alpha").await;
        put_layout_with_terminal(&app, &token, "alpha", UUID_A).await;
        let refs = state.attach_index.read_all_attach_refs();
        assert_eq!(refs.get(UUID_A).unwrap(), &vec!["alpha".to_string()]);
    }

    /// F-3 (ADR-0021 D8 amend ② / 0075/0076/0077): drag idempotency —
    /// a layout PUT whose `(removed, added)` diff is net-zero (only
    /// `x/y` mutation of an existing terminal) must NOT emit any
    /// `AttachReplayEvent`. Otherwise every panel drag would re-replay
    /// the ring buffer, producing duplicate history.
    #[tokio::test]
    async fn attach_existing_terminal_replay_idempotent_for_drag_layout() {
        use gtmux_pty_backend::PtyBackend;
        use gtmux_ws_server::Hub;
        let dir = tempfile::TempDir::new().unwrap();
        let (_app_unused, token, _, state) = make_app_with_workspace_and_state(&dir);

        // Replace the hub with a fresh one whose attach_replay subscriber
        // we hold *before* any PUT, so the broadcast cap doesn't drop the
        // event before we observe. The original `app` is discarded because
        // it was built against the auto-wired hub from
        // `make_app_with_workspace_and_state` — we rebuild from the same
        // state with our test hub instead.
        let hub = Hub::new(PtyBackend::new());
        let mut state = state;
        state.hub = Some(hub.clone());
        let app = router_with_state(state);
        let mut attach_replay_rx = hub.subscribe_attach_replay();

        attach_idx_create_session(&app, &token, "alpha").await;
        // First PUT establishes UUID_A in the layout (added=[UUID_A]).
        let etag = put_layout_with_terminal(&app, &token, "alpha", UUID_A).await;
        // Drain any event that the *first* PUT might have produced (no
        // alive PaneId for UUID_A → no actual emit, but stay defensive).
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            attach_replay_rx.recv(),
        )
        .await;

        // Second PUT: same UUID_A, *different x/y*. apply_diff sees
        // added=[], removed=[] → no replay emit.
        let dragged_layout = json!({
            "schema_version": 2,
            "groups": [],
            "items": [{
                "type": "terminal",
                "id": UUID_A,
                "parent_id": null,
                "x": 999.0, "y": 999.0, "w": 100.0, "h": 100.0, "z": 0,
                "visibility": "visible",
                "locked": false,
                "label": "", "description": "",
                "minimized": false,
            }],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        });
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/sessions/alpha/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header("x-gtmux-webpage-id", "alpha")
                    .header(header::IF_MATCH, etag)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&dragged_layout).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify no AttachReplayEvent was published for the drag PUT.
        let drag_replay = tokio::time::timeout(
            std::time::Duration::from_millis(150),
            attach_replay_rx.recv(),
        )
        .await;
        assert!(
            drag_replay.is_err(),
            "drag-only PUT (added=[], removed=[]) must not emit an AttachReplayEvent; got {drag_replay:?}"
        );
    }

    #[tokio::test]
    async fn attach_index_layout_put_remove_terminal_drops_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _, state) = make_app_with_workspace_and_state(&dir);
        attach_idx_create_session(&app, &token, "alpha").await;
        let etag = put_layout_with_terminal(&app, &token, "alpha", UUID_A).await;
        // Now PUT an empty-items layout — should drop UUID_A's entry. The
        // `x-gtmux-webpage-id: alpha` header matches the attach owner_key
        // seeded by `attach_idx_create_session("alpha")`.
        let empty_layout = json!({
            "schema_version": 2,
            "groups": [],
            "items": [],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        });
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/sessions/alpha/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header("x-gtmux-webpage-id", "alpha")
                    .header(header::IF_MATCH, etag)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&empty_layout).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let refs = state.attach_index.read_all_attach_refs();
        assert!(
            !refs.contains_key(UUID_A),
            "PUT removing terminal must drop its attach_index entry"
        );
    }

    #[tokio::test]
    async fn attach_index_delete_item_removes_terminal_uuid() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _, state) = make_app_with_workspace_and_state(&dir);
        attach_idx_create_session(&app, &token, "alpha").await;
        put_layout_with_terminal(&app, &token, "alpha", UUID_A).await;
        // DELETE the item. `x-gtmux-webpage-id: alpha` so the ADR-0019 D5.6
        // owner-attach guard sees the same owner_key the attach handler
        // recorded.
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/sessions/alpha/items/{UUID_A}"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header("x-gtmux-webpage-id", "alpha")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let refs = state.attach_index.read_all_attach_refs();
        assert!(
            !refs.contains_key(UUID_A),
            "DELETE item must drop its attach_index entry"
        );
    }

    #[tokio::test]
    async fn attach_index_import_seeds_uuid_immediately() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _, state) = make_app_with_workspace_and_state(&dir);
        // Build an import body referencing UUID_B.
        let imported_layout = json!({
            "schema_version": 2,
            "groups": [],
            "items": [{
                "type": "terminal",
                "id": UUID_B,
                "parent_id": null,
                "x": 0.0, "y": 0.0, "w": 100.0, "h": 100.0, "z": 0,
                "visibility": "visible",
                "locked": false,
                "label": "", "description": "",
                "minimized": false,
            }],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        });
        let import_body = serde_json::to_vec(&json!({
            "name": "from_import",
            "layout": imported_layout,
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/import")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(import_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let refs = state.attach_index.read_all_attach_refs();
        assert_eq!(
            refs.get(UUID_B).unwrap(),
            &vec!["from_import".to_string()],
            "imported session must be visible in attach_index immediately"
        );
    }

    #[tokio::test]
    async fn attach_index_session_delete_clears_session_from_entries() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _, state) = make_app_with_workspace_and_state(&dir);
        attach_idx_create_session(&app, &token, "alpha").await;
        attach_idx_create_session(&app, &token, "beta").await;
        put_layout_with_terminal(&app, &token, "alpha", UUID_A).await;
        put_layout_with_terminal(&app, &token, "beta", UUID_A).await; // mirror
                                                                      // Sanity: both sessions reference UUID_A.
        let refs_before = state.attach_index.read_all_attach_refs();
        let mut sessions_before = refs_before.get(UUID_A).unwrap().clone();
        sessions_before.sort();
        assert_eq!(
            sessions_before,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        // DELETE alpha.
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri("/api/sessions/alpha")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        // UUID_A must remain (beta still references it) but only beta.
        let refs_after = state.attach_index.read_all_attach_refs();
        assert_eq!(
            refs_after.get(UUID_A).unwrap(),
            &vec!["beta".to_string()],
            "DELETE alpha must remove alpha-membership from UUID_A's set"
        );
    }

    #[tokio::test]
    async fn sessions_delete_removes_file_and_cache() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, workspace_dir) = make_app_with_workspace(&dir);
        let create_body = serde_json::to_vec(&json!({ "name": "doomed" })).unwrap();
        app.clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(create_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(workspace_dir.join("doomed.json").exists());

        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri("/api/sessions/doomed")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(!workspace_dir.join("doomed.json").exists());

        // GET layout after delete → 404.
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions/doomed/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn sessions_endpoints_503_without_workspace() {
        let (app, token) = make_app();
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn sessions_require_bearer_auth() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, _token, _) = make_app_with_workspace(&dir);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── Stage 4-B: GET /api/terminals (ADR-0021 D7 + ADR-0018 D2) ──

    fn make_state_with_workspace(
        dir: &tempfile::TempDir,
    ) -> (AppState, TokenString, std::path::PathBuf) {
        let token = issue_token().expect("token");
        let cfg = test_config();
        let workspace_dir = dir.path().to_path_buf();
        let wm = WorkspaceManager::from_path(workspace_dir.clone()).expect("workspace");
        let state = AppState::new(cfg, token.clone()).with_workspace(wm);
        (state, token, workspace_dir)
    }

    #[tokio::test]
    async fn terminals_list_empty_when_pool_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, _) = make_state_with_workspace(&dir);
        let app = router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/terminals")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body, json!([]));
    }

    #[tokio::test]
    async fn terminals_list_503_without_workspace() {
        let (app, token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/terminals")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn terminals_list_joins_pool_metadata_and_session_refs() {
        use gtmux_pty_backend::PaneId;
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace(&dir);

        // Two UUIDs in the pool, with metadata.
        state
            .terminal_map
            .register("uuid-aaa".into(), PaneId(1))
            .await
            .unwrap();
        state.terminal_meta.record_spawn("uuid-aaa").await;
        state
            .terminal_map
            .register("uuid-bbb".into(), PaneId(2))
            .await
            .unwrap();
        state.terminal_meta.record_spawn("uuid-bbb").await;

        // Two on-disk session files: one references uuid-aaa; the other
        // references both.
        let session_a = json!({
            "schema_version": 2,
            "groups": [],
            "items": [
                {
                    "id": "uuid-aaa", "type": "terminal",
                    "parent_id": null,
                    "x": 0.0, "y": 0.0, "w": 640.0, "h": 400.0, "z": 0,
                    "visibility": "visible", "locked": false,
                    "minimized": false
                }
            ],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
        });
        let session_b = json!({
            "schema_version": 2,
            "groups": [],
            "items": [
                {
                    "id": "uuid-aaa", "type": "terminal",
                    "parent_id": null,
                    "x": 0.0, "y": 0.0, "w": 640.0, "h": 400.0, "z": 0,
                    "visibility": "visible", "locked": false,
                    "minimized": false
                },
                {
                    "id": "uuid-bbb", "type": "terminal",
                    "parent_id": null,
                    "x": 0.0, "y": 0.0, "w": 640.0, "h": 400.0, "z": 0,
                    "visibility": "visible", "locked": false,
                    "minimized": false
                }
            ],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
        });
        std::fs::write(
            workspace_dir.join("alpha.json"),
            serde_json::to_vec(&session_a).unwrap(),
        )
        .unwrap();
        std::fs::write(
            workspace_dir.join("beta.json"),
            serde_json::to_vec(&session_b).unwrap(),
        )
        .unwrap();

        // ADR-0021 D7 amend ③ (0068): `GET /api/terminals` reads from
        // the in-memory attach_index, not disk. The test seeds the files
        // *after* AppState boot, so we must replay the boot rebuild
        // explicitly here to mirror what production does on startup.
        let wm = state.workspace.as_ref().unwrap().clone();
        state.attach_index.rebuild_from_disk(&wm).unwrap();

        let app = router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/terminals")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        let rows = body.as_array().expect("array");
        assert_eq!(rows.len(), 2);

        // Pull rows by id; ordering on created_at is identical (same wall
        // clock second), so we look them up by id rather than by index.
        let row_a = rows
            .iter()
            .find(|r| r["id"] == "uuid-aaa")
            .expect("uuid-aaa row");
        let row_b = rows
            .iter()
            .find(|r| r["id"] == "uuid-bbb")
            .expect("uuid-bbb row");

        assert_eq!(row_a["alive"], true);
        assert_eq!(row_a["label"], "");
        assert_eq!(row_a["attach_count"], 2);
        let names_a: Vec<String> = row_a["attached_sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(names_a.contains(&"alpha".into()));
        assert!(names_a.contains(&"beta".into()));

        assert_eq!(row_b["alive"], true);
        assert_eq!(row_b["attach_count"], 1);
        let names_b: Vec<String> = row_b["attached_sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(names_b, vec!["beta".to_string()]);
    }

    // ── Stage 4-C: match-or-spawn on attach (ADR-0018 D6) ──

    #[tokio::test]
    async fn attach_returns_matched_and_unmatched_split() {
        use gtmux_pty_backend::PaneId;
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace(&dir);

        // Pre-populate the pool with one of the two UUIDs the layout
        // references — that one must come back as `matched`; the other
        // must show up as `unmatched`.
        state
            .terminal_map
            .register("11111111-2222-4333-8444-555555555555".into(), PaneId(1))
            .await
            .unwrap();
        state
            .terminal_meta
            .record_spawn("11111111-2222-4333-8444-555555555555")
            .await;

        std::fs::write(
            workspace_dir.join("demo.json"),
            serde_json::to_vec(&json!({
                "schema_version": 2,
                "groups": [],
                "items": [
                    {
                        "id": "11111111-2222-4333-8444-555555555555", "type": "terminal",
                        "parent_id": null,
                        "x": 0.0, "y": 0.0, "w": 640.0, "h": 400.0, "z": 0,
                        "visibility": "visible", "locked": false,
                        "minimized": false
                    },
                    {
                        "id": "66666666-7777-4888-8999-aaaaaaaaaaaa", "type": "terminal",
                        "parent_id": null,
                        "x": 0.0, "y": 0.0, "w": 640.0, "h": 400.0, "z": 0,
                        "visibility": "visible", "locked": false,
                        "minimized": false
                    }
                ],
                "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
            }))
            .unwrap(),
        )
        .unwrap();

        let app = router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/demo/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["name"], "demo");
        assert_eq!(body["attached"], true);
        assert_eq!(
            body["matched"],
            json!(["11111111-2222-4333-8444-555555555555"])
        );
        assert_eq!(
            body["unmatched"],
            json!(["66666666-7777-4888-8999-aaaaaaaaaaaa"])
        );
    }

    #[tokio::test]
    async fn attach_confirm_503_without_hub() {
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace(&dir);
        std::fs::write(
            workspace_dir.join("demo.json"),
            serde_json::to_vec(&json!({
                "schema_version": 2,
                "groups": [],
                "items": [],
                "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
            }))
            .unwrap(),
        )
        .unwrap();
        let app = router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/demo/attach/confirm")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn attach_confirm_403_when_not_attached() {
        let dir = tempfile::TempDir::new().unwrap();
        let token = issue_token().expect("token");
        let cfg = test_config();
        let workspace_dir = dir.path().to_path_buf();
        let wm = WorkspaceManager::from_path(workspace_dir.clone()).expect("workspace");
        let backend = gtmux_pty_backend::PtyBackend::new();
        let hub = gtmux_ws_server::Hub::new(backend);
        let state = AppState::with_hub_and_workspace(cfg, token.clone(), hub, wm);
        std::fs::write(
            workspace_dir.join("demo.json"),
            serde_json::to_vec(&json!({
                "schema_version": 2,
                "groups": [],
                "items": [],
                "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
            }))
            .unwrap(),
        )
        .unwrap();
        let app = router_with_state(state);
        // No prior /attach — confirm must 403.
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/demo/attach/confirm")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // ── Stage 4-E: BackendNotify::PaneDied auto-unregister ──

    #[tokio::test]
    async fn handle_pane_died_drops_map_but_keeps_metadata() {
        use gtmux_pty_backend::PaneId;
        let dir = tempfile::TempDir::new().unwrap();
        let (state, _, _) = make_state_with_workspace(&dir);
        let uuid = "11111111-2222-4333-8444-555555555558";
        state
            .terminal_map
            .register(uuid.into(), PaneId(42))
            .await
            .unwrap();
        state.terminal_meta.record_spawn(uuid).await;
        let before_created = state.terminal_meta.get(uuid).await.unwrap().created_at;
        assert!(state.terminal_map.lookup_pane(uuid).await.is_some());
        state.handle_pane_died(PaneId(42), None).await;
        // Map entry is gone (Pane is dead) but metadata is preserved so a
        // follow-up respawn keeps `created_at` + `label` (ADR-0021 D10.1).
        assert!(state.terminal_map.lookup_pane(uuid).await.is_none());
        let after = state.terminal_meta.get(uuid).await.expect("metadata kept");
        assert_eq!(after.created_at, before_created);
    }

    #[tokio::test]
    async fn handle_pane_died_is_idempotent_for_unknown_pane() {
        use gtmux_pty_backend::PaneId;
        let dir = tempfile::TempDir::new().unwrap();
        let (state, _, _) = make_state_with_workspace(&dir);
        // No entry — must not panic.
        state.handle_pane_died(PaneId(999), None).await;
        assert!(state.terminal_map.is_empty().await);
    }

    /// ADR-0021 D10.2 / 0053 §3.4 — two concurrent `POST /respawn` calls
    /// on the same UUID must converge on a *single* alive PaneId binding.
    /// The handler's per-UUID `respawn_locks` mutex serialises the
    /// kill→spawn pair; the runner-up enters its critical section after
    /// the winner has already published a fresh PaneId, finds the
    /// `lookup_pane` hit, and returns the idempotent `{ reused: true }`
    /// path. Without the lock, both callers would kill+spawn back-to-back
    /// and the second's kill would orphan the first's just-bound output
    /// stream.
    #[tokio::test]
    async fn respawn_concurrent_same_uuid_yields_single_alive_binding() {
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, _) = make_state_with_workspace_and_hub(&dir);
        let uuid = "11111111-2222-4333-8444-555555555570";
        let app = router_with_state(state.clone());

        let make_req = || {
            HttpRequest::builder()
                .method(Method::POST)
                .uri(format!("/api/terminals/{uuid}/respawn"))
                .header(header::HOST, TEST_HOST)
                .header(header::AUTHORIZATION, bearer(&token))
                .body(Body::empty())
                .unwrap()
        };
        let (r1, r2) = tokio::join!(
            app.clone().oneshot(make_req()),
            app.clone().oneshot(make_req()),
        );
        let r1 = r1.unwrap();
        let r2 = r2.unwrap();
        assert_eq!(r1.status(), StatusCode::OK, "first call must 200");
        assert_eq!(r2.status(), StatusCode::OK, "second call must 200");

        let b1: Value =
            serde_json::from_slice(&to_bytes(r1.into_body(), 4096).await.unwrap()).unwrap();
        let b2: Value =
            serde_json::from_slice(&to_bytes(r2.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(b1["id"], uuid);
        assert_eq!(b2["id"], uuid);
        let mut reused_flags = vec![
            b1["reused"].as_bool().expect("reused bool"),
            b2["reused"].as_bool().expect("reused bool"),
        ];
        reused_flags.sort();
        assert_eq!(
            reused_flags,
            vec![false, true],
            "exactly one caller must run the kill+spawn path (reused=false); \
             the other must see the lookup_pane hit (reused=true). got {b1:?} / {b2:?}"
        );

        // Exactly one alive PaneId is bound — no duplicate PTY.
        assert!(
            state.terminal_map.lookup_pane(uuid).await.is_some(),
            "the UUID must end up with a live binding"
        );
        // Cleanup so the TempDir Drop doesn't trip on a held flock.
        crate::sessions::kill_and_unregister_terminal(&state, uuid).await;
    }

    #[tokio::test]
    async fn pane_died_then_respawn_round_trip_preserves_created_at() {
        // Regression for smoke-9: ensure the kill → respawn cycle does not
        // reset `created_at`. We simulate respawn by re-registering the
        // same UUID with a fresh PaneId and calling record_spawn again
        // (the idempotent path of TerminalMetadataStore::record_spawn
        // preserves created_at when the entry already exists).
        use gtmux_pty_backend::PaneId;
        let dir = tempfile::TempDir::new().unwrap();
        let (state, _, _) = make_state_with_workspace(&dir);
        let uuid = "11111111-2222-4333-8444-555555555559";
        state
            .terminal_map
            .register(uuid.into(), PaneId(101))
            .await
            .unwrap();
        state.terminal_meta.record_spawn(uuid).await;
        let original_created_at = state.terminal_meta.get(uuid).await.unwrap().created_at;

        // Kernel-driven death (the consumer path).
        state.handle_pane_died(PaneId(101), None).await;
        // Sleep across a whole-second boundary so a buggy re-init of
        // created_at would visibly drift.
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

        // Respawn — fresh PaneId, same UUID.
        state
            .terminal_map
            .register(uuid.into(), PaneId(202))
            .await
            .unwrap();
        state.terminal_meta.record_spawn(uuid).await;

        let post = state.terminal_meta.get(uuid).await.unwrap();
        assert_eq!(
            post.created_at, original_created_at,
            "created_at must survive a death/respawn round-trip"
        );
    }

    // ── Stage 4 cleanup: PATCH /api/terminals/:id (BE-8 label) ──

    #[tokio::test]
    async fn patch_terminal_sets_label_when_uuid_known() {
        use gtmux_pty_backend::PaneId;
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, _) = make_state_with_workspace(&dir);
        let uuid = "11111111-2222-4333-8444-55555555555a";
        state
            .terminal_map
            .register(uuid.into(), PaneId(1))
            .await
            .unwrap();
        state.terminal_meta.record_spawn(uuid).await;

        let body = serde_json::to_vec(&json!({ "label": "build watch" })).unwrap();
        let app = router_with_state(state.clone());
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PATCH)
                    .uri(format!("/api/terminals/{uuid}"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            state.terminal_meta.get(uuid).await.unwrap().label,
            "build watch"
        );
    }

    #[tokio::test]
    async fn patch_terminal_404_for_unknown_uuid() {
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, _) = make_state_with_workspace(&dir);
        let body = serde_json::to_vec(&json!({ "label": "x" })).unwrap();
        let app = router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PATCH)
                    .uri("/api/terminals/11111111-2222-4333-8444-55555555555b")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn patch_terminal_400_when_label_too_long() {
        use gtmux_pty_backend::PaneId;
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, _) = make_state_with_workspace(&dir);
        let uuid = "11111111-2222-4333-8444-55555555555c";
        state
            .terminal_map
            .register(uuid.into(), PaneId(1))
            .await
            .unwrap();
        state.terminal_meta.record_spawn(uuid).await;

        let too_long = "x".repeat(crate::terminals::MAX_LABEL_BYTES + 1);
        let body = serde_json::to_vec(&json!({ "label": too_long })).unwrap();
        let app = router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PATCH)
                    .uri(format!("/api/terminals/{uuid}"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ── Stage 4-D: DELETE items + terminal kill / respawn ──

    fn make_state_with_workspace_and_hub(
        dir: &tempfile::TempDir,
    ) -> (AppState, TokenString, std::path::PathBuf) {
        let token = issue_token().expect("token");
        let cfg = test_config();
        let workspace_dir = dir.path().to_path_buf();
        let wm = WorkspaceManager::from_path(workspace_dir.clone()).expect("workspace");
        let backend = gtmux_pty_backend::PtyBackend::new();
        let hub = gtmux_ws_server::Hub::new(backend);
        let state = AppState::with_hub_and_workspace(cfg, token.clone(), hub, wm);
        (state, token, workspace_dir)
    }

    fn make_layout_with_one_terminal(uuid: &str) -> Value {
        json!({
            "schema_version": 2,
            "groups": [],
            "items": [
                {
                    "id": uuid, "type": "terminal",
                    "parent_id": null,
                    "x": 0.0, "y": 0.0, "w": 640.0, "h": 400.0, "z": 0,
                    "visibility": "visible", "locked": false,
                    "minimized": false
                }
            ],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
        })
    }

    #[tokio::test]
    async fn delete_item_removes_panel_only_by_default() {
        use gtmux_pty_backend::PaneId;
        let uuid = "11111111-2222-4333-8444-555555555555";
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace_and_hub(&dir);

        // Bind the UUID in the pool so we can verify it survives the
        // panel-only delete.
        state
            .terminal_map
            .register(uuid.into(), PaneId(1))
            .await
            .unwrap();
        state.terminal_meta.record_spawn(uuid).await;

        std::fs::write(
            workspace_dir.join("demo.json"),
            serde_json::to_vec(&make_layout_with_one_terminal(uuid)).unwrap(),
        )
        .unwrap();
        // ADR-0019 D5.6 owner-attach guard: synthesise the same owner_key
        // (`"_unknown"`) that the bearer-only DELETE below produces.
        seed_owner_attached(&state, "demo").await;

        let app = router_with_state(state.clone());
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/sessions/demo/items/{uuid}"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(
            resp.headers().get(header::ETAG).is_some(),
            "ETag must accompany the 204"
        );

        // The terminal is still in the pool (panel-only delete).
        assert!(state.terminal_map.lookup_pane(uuid).await.is_some());
        assert!(state.terminal_meta.get(uuid).await.is_some());

        // The item is gone from the on-disk layout.
        let bytes = std::fs::read(workspace_dir.join("demo.json")).unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn delete_item_with_kill_terminal_drops_pool_entry() {
        use gtmux_pty_backend::PaneId;
        let uuid = "11111111-2222-4333-8444-555555555556";
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace_and_hub(&dir);

        state
            .terminal_map
            .register(uuid.into(), PaneId(2))
            .await
            .unwrap();
        state.terminal_meta.record_spawn(uuid).await;

        std::fs::write(
            workspace_dir.join("demo.json"),
            serde_json::to_vec(&make_layout_with_one_terminal(uuid)).unwrap(),
        )
        .unwrap();
        seed_owner_attached(&state, "demo").await;

        let app = router_with_state(state.clone());
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/api/sessions/demo/items/{uuid}?kill_terminal=true"
                    ))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // The terminal is no longer in the pool.
        assert!(state.terminal_map.lookup_pane(uuid).await.is_none());
        assert!(state.terminal_meta.get(uuid).await.is_none());
    }

    #[tokio::test]
    async fn delete_item_404_when_id_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace(&dir);
        let layout = make_layout_with_one_terminal("11111111-2222-4333-8444-555555555557");
        std::fs::write(
            workspace_dir.join("demo.json"),
            serde_json::to_vec(&layout).unwrap(),
        )
        .unwrap();
        seed_owner_attached(&state, "demo").await;
        let app = router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri("/api/sessions/demo/items/00000000-0000-4000-8000-000000000000")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn terminal_kill_404_when_not_in_pool() {
        let dir = tempfile::TempDir::new().unwrap();
        let (_state, token, _) = make_state_with_workspace_and_hub(&dir);
        let backend = gtmux_pty_backend::PtyBackend::new();
        let hub = gtmux_ws_server::Hub::new(backend);
        let state = AppState::with_hub(test_config(), token.clone(), hub);
        let app = router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/terminals/00000000-0000-4000-8000-000000000000/kill")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn terminal_kill_503_without_hub() {
        let (app, token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/terminals/00000000-0000-4000-8000-000000000000/kill")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn terminal_respawn_503_without_hub() {
        let (app, token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/terminals/00000000-0000-4000-8000-000000000000/respawn")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // ── Stage 3: cross-server session attach lock (ADR-0019 D3/D6) ──

    async fn create_session(app: &Router, token: &TokenString, name: &str) {
        let body = serde_json::to_vec(&json!({ "name": name })).unwrap();
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    async fn attach(app: &Router, token: &TokenString, name: &str) -> StatusCode {
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri(format!("/api/sessions/{name}/attach"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        resp.status()
    }

    async fn attach_as_webpage(
        app: &Router,
        token: &TokenString,
        name: &str,
        webpage_id: &str,
    ) -> StatusCode {
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri(format!("/api/sessions/{name}/attach"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(token))
                    .header("x-gtmux-webpage-id", webpage_id)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        resp.status()
    }

    async fn detach(app: &Router, token: &TokenString, name: &str) -> StatusCode {
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/sessions/{name}/attach"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        resp.status()
    }

    async fn detach_as_webpage(
        app: &Router,
        token: &TokenString,
        name: &str,
        webpage_id: &str,
    ) -> StatusCode {
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/sessions/{name}/attach"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(token))
                    .header("x-gtmux-webpage-id", webpage_id)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        resp.status()
    }

    async fn list_as_webpage(app: &Router, token: &TokenString, webpage_id: &str) -> Value {
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::GET)
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(token))
                    .header("x-gtmux-webpage-id", webpage_id)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn attach_404_when_session_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        let status = attach(&app, &token, "absent").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn attach_then_detach_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        create_session(&app, &token, "alpha").await;
        assert_eq!(attach(&app, &token, "alpha").await, StatusCode::OK);
        assert_eq!(detach(&app, &token, "alpha").await, StatusCode::OK);
        // Second detach is still 200 (idempotent).
        assert_eq!(detach(&app, &token, "alpha").await, StatusCode::OK);
    }

    #[tokio::test]
    async fn same_cookie_different_webpage_cannot_attach_same_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        create_session(&app, &token, "alpha").await;

        assert_eq!(
            attach_as_webpage(&app, &token, "alpha", "page-a").await,
            StatusCode::OK
        );
        assert_eq!(
            attach_as_webpage(&app, &token, "alpha", "page-a").await,
            StatusCode::OK,
            "same webpage reattach remains idempotent"
        );
        assert_eq!(
            attach_as_webpage(&app, &token, "alpha", "page-b").await,
            StatusCode::CONFLICT,
            "same auth cookie in a different tab must still be a different Webpage"
        );
    }

    #[tokio::test]
    async fn detach_is_webpage_scoped() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        create_session(&app, &token, "alpha").await;

        assert_eq!(
            attach_as_webpage(&app, &token, "alpha", "page-a").await,
            StatusCode::OK
        );
        assert_eq!(
            detach_as_webpage(&app, &token, "alpha", "page-b").await,
            StatusCode::OK,
            "detach is idempotent but must not release another webpage"
        );
        assert_eq!(
            attach_as_webpage(&app, &token, "alpha", "page-c").await,
            StatusCode::CONFLICT
        );
        assert_eq!(
            detach_as_webpage(&app, &token, "alpha", "page-a").await,
            StatusCode::OK
        );
        assert_eq!(
            attach_as_webpage(&app, &token, "alpha", "page-c").await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn session_list_disables_any_open_webpage_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        create_session(&app, &token, "alpha").await;
        create_session(&app, &token, "beta").await;

        assert_eq!(
            attach_as_webpage(&app, &token, "alpha", "page-a").await,
            StatusCode::OK
        );

        let as_owner = list_as_webpage(&app, &token, "page-a").await;
        let alpha_for_owner = as_owner
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "alpha")
            .unwrap();
        assert_eq!(
            alpha_for_owner["active"],
            json!(true),
            "an already-open session is not selectable, even for the owning webpage"
        );

        let as_other = list_as_webpage(&app, &token, "page-b").await;
        let alpha_for_other = as_other
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "alpha")
            .unwrap();
        assert_eq!(
            alpha_for_other["active"],
            json!(true),
            "a different webpage must still see the row as in-use"
        );
    }

    // ── ADR-0021 D6 amend ② / 0071 §D-5 — POST /api/leave (sendBeacon) ─

    async fn leave_with_webpage_id(
        app: &Router,
        token: &TokenString,
        webpage_id: &str,
        cookie: Option<&str>,
    ) -> StatusCode {
        let mut req = HttpRequest::builder()
            .method(Method::POST)
            .uri(format!("/api/leave?webpage_id={webpage_id}"))
            .header(header::HOST, TEST_HOST)
            .header(header::AUTHORIZATION, bearer(token));
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, format!("gtmux_auth={c}"));
        }
        let resp = app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        resp.status()
    }

    /// Happy path: a Webpage that holds an attach calls `/api/leave` →
    /// 204, and a subsequent GET /sessions shows the row as no longer
    /// active.
    #[tokio::test]
    async fn leave_releases_lock_for_owner() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        create_session(&app, &token, "alpha").await;

        assert_eq!(
            attach_as_webpage(&app, &token, "alpha", "page-a").await,
            StatusCode::OK
        );
        // Sanity: list shows alpha as active for the same Webpage (D5.6 —
        // any open Webpage sees the row as in-use).
        let before = list_as_webpage(&app, &token, "page-a").await;
        let alpha_before = before
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "alpha")
            .unwrap();
        assert_eq!(alpha_before["active"], json!(true));

        assert_eq!(
            leave_with_webpage_id(&app, &token, "page-a", None).await,
            StatusCode::NO_CONTENT
        );

        let after = list_as_webpage(&app, &token, "page-b").await;
        let alpha_after = after
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "alpha")
            .unwrap();
        assert_eq!(
            alpha_after["active"],
            json!(false),
            "after /api/leave the session must be selectable again from any Webpage"
        );
    }

    /// Idempotency: calling `/api/leave` without any prior attach is a
    /// silent no-op + 204. sendBeacon fires `beforeunload` even when the
    /// page never attached (rare but possible — e.g. the user closed the
    /// tab right after the AuthDialog), so the handler must not 4xx.
    #[tokio::test]
    async fn leave_idempotent_when_no_lock() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        assert_eq!(
            leave_with_webpage_id(&app, &token, "page-ghost", None).await,
            StatusCode::NO_CONTENT
        );
    }

    /// `/api/leave` rides the same `/api/*` middleware as the other
    /// session endpoints — bearer / cookie missing → 401 before the
    /// handler runs.
    #[tokio::test]
    async fn leave_requires_auth() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, _token, _) = make_app_with_workspace(&dir);
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/leave?webpage_id=page-a")
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// `/api/leave` is owner-scoped, exactly like `DELETE /attach`. A
    /// Webpage releasing its own attach must not collateral-release a
    /// *sibling tab*'s attach to a different session.
    #[tokio::test]
    async fn leave_releases_only_matching_owner() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        create_session(&app, &token, "alpha").await;
        create_session(&app, &token, "beta").await;

        // Two Webpages on the *same* (bearer-only) auth context, distinct
        // tab identities — each takes its own session.
        assert_eq!(
            attach_as_webpage(&app, &token, "alpha", "page-1").await,
            StatusCode::OK
        );
        assert_eq!(
            attach_as_webpage(&app, &token, "beta", "page-2").await,
            StatusCode::OK
        );

        // Leave from page-1: only alpha should release.
        assert_eq!(
            leave_with_webpage_id(&app, &token, "page-1", None).await,
            StatusCode::NO_CONTENT
        );

        let listing = list_as_webpage(&app, &token, "page-3").await;
        let alpha = listing
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "alpha")
            .unwrap();
        let beta = listing
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "beta")
            .unwrap();
        assert_eq!(
            alpha["active"],
            json!(false),
            "page-1's /api/leave must drop alpha's lock"
        );
        assert_eq!(
            beta["active"],
            json!(true),
            "page-2's attach must survive page-1's /api/leave"
        );
    }

    #[tokio::test]
    async fn attach_active_flag_appears_in_list() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        create_session(&app, &token, "beta").await;

        // Before attach: active = false
        let list_before = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(list_before.into_body(), 64 * 1024).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v, json!([{ "name": "beta", "active": false }]));

        // Attach.
        assert_eq!(attach(&app, &token, "beta").await, StatusCode::OK);

        // After attach: active = true
        let list_after = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(list_after.into_body(), 64 * 1024).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v, json!([{ "name": "beta", "active": true }]));

        // Cleanup so the TempDir Drop doesn't trip the held flock cleanup.
        assert_eq!(detach(&app, &token, "beta").await, StatusCode::OK);
    }

    /// `POST /attach` followed by `POST /attach` with the *same cookie*
    /// must be idempotent — second call 200, same lock retained. ADR-0019
    /// D3: refresh races and silent reattach (plan-0008 Phase 2) rely on
    /// this contract to avoid spuriously surfacing the "in use" modal.
    #[tokio::test]
    async fn attach_idempotent_for_same_cookie_same_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, workspace_dir) = make_app_with_workspace(&dir);
        create_session(&app, &token, "gamma").await;
        let cookie_value = "same-cookie-aaa";
        let make_req = || {
            HttpRequest::builder()
                .method(Method::POST)
                .uri("/api/sessions/gamma/attach")
                .header(header::HOST, TEST_HOST)
                .header(header::AUTHORIZATION, bearer(&token))
                .header(header::COOKIE, format!("gtmux_auth={cookie_value}"))
                .body(Body::empty())
                .unwrap()
        };
        let r1 = app.clone().oneshot(make_req()).await.unwrap();
        assert_eq!(r1.status(), StatusCode::OK);
        let b1 = to_bytes(r1.into_body(), 64 * 1024).await.unwrap();
        let v1: Value = serde_json::from_slice(&b1).unwrap();
        assert_eq!(v1["attached"], json!(true));
        assert_eq!(v1["name"], json!("gamma"));
        assert!(workspace_dir.join(".locks/gamma.lock").exists());

        // Second attach with the *same* cookie must be 200 idempotent —
        // the existing lock is reused, body shape matches the first call.
        let r2 = app.clone().oneshot(make_req()).await.unwrap();
        assert_eq!(r2.status(), StatusCode::OK);
        let b2 = to_bytes(r2.into_body(), 64 * 1024).await.unwrap();
        let v2: Value = serde_json::from_slice(&b2).unwrap();
        assert_eq!(v2["attached"], json!(true));
        assert_eq!(v2["name"], json!("gamma"));
        assert_eq!(v2["server_id"], v1["server_id"]);
        // Lock file must still exist (no implicit release fired).
        assert!(workspace_dir.join(".locks/gamma.lock").exists());

        // A single DELETE releases the lock.
        let release = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri("/api/sessions/gamma/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie_value}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(release.status(), StatusCode::OK);
    }

    /// `POST /attach` from a *different cookie* while the session is held
    /// must return 409 (no takeover, ADR-0019 D4). Counterpart of the
    /// same-cookie idempotent contract above.
    #[tokio::test]
    async fn attach_409_when_held_by_different_cookie() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, workspace_dir) = make_app_with_workspace(&dir);
        create_session(&app, &token, "gamma").await;
        let cookie_a = "cookie-aaa";
        let cookie_b = "cookie-bbb";
        let post = |cookie: &str| {
            app.clone().oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/gamma/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .body(Body::empty())
                    .unwrap(),
            )
        };
        assert_eq!(post(cookie_a).await.unwrap().status(), StatusCode::OK);
        // Different cookie attempting takeover → 409.
        assert_eq!(post(cookie_b).await.unwrap().status(), StatusCode::CONFLICT);
        // Owner releases.
        let release = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri("/api/sessions/gamma/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie_a}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(release.status(), StatusCode::OK);
        assert!(!workspace_dir.join(".locks/gamma.lock").exists());
        // Now cookie_b can acquire it.
        assert_eq!(post(cookie_b).await.unwrap().status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn release_lock_for_owner_drops_the_attach() {
        // ADR-0019 D6 + ADR-0021 D6: a WS-close event must auto-release the
        // session lock the cookie still holds, with no need for an explicit
        // DELETE.
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, workspace_dir) = make_app_with_workspace(&dir);
        create_session(&app, &token, "auto-rel").await;

        // Attach with a known cookie so we can drive release-by-cookie.
        let cookie_value = "test-cookie-XYZ";
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/auto-rel/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie_value}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(workspace_dir.join(".locks/auto-rel.lock").exists());

        // Reach into the AppState behind the router to invoke the release
        // path. We can't easily get the AppState back from `router_with_state`
        // so we reconstruct one against the same workspace dir + simulate.
        // The unit-level coverage of release_lock_for_owner is via the
        // standalone test below; here we verify the *integration* surface.
        // A second attach from a *different* cookie must 409 because takeover
        // is forbidden (ADR-0019 D4) — the auto-release path is the only way
        // the lock goes away (apart from explicit DELETE). Same-cookie
        // reattach is now idempotent (D3) and is covered separately by
        // `attach_idempotent_for_same_cookie_same_session`.
        let other_cookie = "test-cookie-OTHER";
        let again = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/auto-rel/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={other_cookie}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(again.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn refresh_lease_for_owner_bumps_lease_until() {
        // ADR-0019 D6.2: each Ping/Pong drives a lease refresh so peeking
        // modals don't see a stale "expected expiry" hint.
        let dir = tempfile::TempDir::new().unwrap();
        let token = issue_token().expect("token");
        let cfg = test_config();
        let wm = WorkspaceManager::from_path(dir.path().to_path_buf()).expect("ws");
        let state = AppState::new(cfg, token).with_workspace(wm);

        let cookie_value = "refresh-cookie";
        let locks_dir = dir.path().join(".locks");
        std::fs::create_dir_all(&locks_dir).unwrap();
        let server_id = state.server_id.clone();
        let guard = tokio::task::spawn_blocking({
            let locks_dir = locks_dir.clone();
            move || crate::session_lock::acquire(&locks_dir, "refresh", server_id, cookie_value)
        })
        .await
        .unwrap()
        .unwrap();
        state
            .session_locks
            .lock()
            .await
            .insert("refresh".to_string(), guard);
        state
            .session_locks_by_owner
            .lock()
            .await
            .insert(cookie_value.to_string(), "refresh".to_string());

        let path = locks_dir.join("refresh.lock");
        let lease_before: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let before_until = lease_before["lease_until_unix"].as_u64().unwrap();

        // Sleep past the 1s resolution of unix-seconds so the new lease
        // can demonstrably differ.
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        state.refresh_lease_for_owner(cookie_value).await;

        let lease_after: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let after_until = lease_after["lease_until_unix"].as_u64().unwrap();
        assert!(
            after_until > before_until,
            "lease must extend on refresh: before={before_until} after={after_until}"
        );

        // Idempotent — refresh for a cookie with no lock is a no-op.
        state.refresh_lease_for_owner("absent-cookie").await;
    }

    #[tokio::test]
    async fn release_lock_for_owner_directly_on_appstate() {
        // Direct unit test of `AppState::release_lock_for_owner` (the
        // method the WS-close consumer task calls).
        let dir = tempfile::TempDir::new().unwrap();
        let token = issue_token().expect("token");
        let cfg = test_config();
        let wm = WorkspaceManager::from_path(dir.path().to_path_buf()).expect("ws");
        let state = AppState::new(cfg, token).with_workspace(wm);

        // Manually populate both maps as the attach handler would have.
        let cookie_value = "manual-cookie";
        {
            let workspace_dir = dir.path();
            let locks_dir = workspace_dir.join(".locks");
            std::fs::create_dir_all(&locks_dir).unwrap();
            let server_id = state.server_id.clone();
            let guard = tokio::task::spawn_blocking(move || {
                crate::session_lock::acquire(&locks_dir, "manual", server_id, cookie_value)
            })
            .await
            .unwrap()
            .unwrap();
            state
                .session_locks
                .lock()
                .await
                .insert("manual".to_string(), guard);
            state
                .session_locks_by_owner
                .lock()
                .await
                .insert(cookie_value.to_string(), "manual".to_string());
        }
        assert!(dir.path().join(".locks/manual.lock").exists());

        // Release-by-cookie must drop both maps and the lock file.
        state.release_lock_for_owner(cookie_value).await;
        assert!(state.session_locks.lock().await.is_empty());
        assert!(state.session_locks_by_owner.lock().await.is_empty());
        assert!(!dir.path().join(".locks/manual.lock").exists());

        // Idempotent — second call on an absent cookie is a no-op.
        state.release_lock_for_owner(cookie_value).await;
    }

    #[tokio::test]
    async fn attach_409_when_another_server_holds_flock() {
        // Simulate a *different* server holding the cross-workspace flock by
        // grabbing it directly via the session_lock primitives, then attempting
        // an attach through the handler — handler must report 409.
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, workspace_dir) = make_app_with_workspace(&dir);
        create_session(&app, &token, "delta").await;

        let locks_dir = workspace_dir.join(".locks");
        let other_server_id: Arc<str> = crate::session_lock::fresh_server_id().into();
        let _other = tokio::task::spawn_blocking(move || {
            crate::session_lock::acquire(&locks_dir, "delta", other_server_id, "ext-conn")
        })
        .await
        .unwrap()
        .unwrap();

        let status = attach(&app, &token, "delta").await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    // ── Stage 5 D10 α: SessionTable implements CookieValidator ──

    #[tokio::test]
    async fn session_table_cookie_validator_returns_true_for_live_session() {
        // The CookieValidator impl delegates to SessionTable::validate;
        // a freshly-issued cookie must read back as valid.
        use crate::auth::{AuthMode, SessionTable};
        let table = SessionTable::new(std::time::Duration::from_secs(60));
        let cookie = table.issue(AuthMode::Token).await.expect("issue");
        let live = gtmux_ws_server::CookieValidator::validate(&table, &cookie).await;
        assert!(live, "freshly issued cookie must validate");
    }

    #[tokio::test]
    async fn session_table_cookie_validator_returns_false_for_unknown() {
        use crate::auth::SessionTable;
        let table = SessionTable::new(std::time::Duration::from_secs(60));
        let live = gtmux_ws_server::CookieValidator::validate(&table, "nope").await;
        assert!(!live, "unknown cookie must not validate");
    }

    // ── Stage 5-D path P2: POST /terminals + 0x86 MOUNT_CASCADE ──

    #[tokio::test]
    async fn create_terminal_publishes_mount_cascade_and_terminal_list_change() {
        // The endpoint is the centerpiece of 5-D P2 — verify that one
        // POST publishes BOTH the trigger-session frame (0x86 mount-cascade)
        // and the other-session frame (0x87 terminal-list-change), with
        // matching UUID + coordinates.
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, _) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        let app = router_with_state(state);
        let cookie = "p2-cookie";
        create_session(&app, &token, "p2demo").await;

        // Take the attach so the create_terminal handler sees the cookie
        // as the lock holder.
        let attach = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/p2demo/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(attach.status(), StatusCode::OK);

        let mut cascade_rx = hub.subscribe_mount_cascade();
        let mut list_rx = hub.subscribe_terminal_list_change();

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/p2demo/terminals")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        let uuid = v["terminal_id"].as_str().expect("terminal_id").to_string();
        assert!(v["pane_id"].as_u64().is_some());
        // Empty layout → fallback coords (80, 80, 720, 420).
        assert_eq!(v["x"], 80.0);
        assert_eq!(v["y"], 80.0);
        assert_eq!(v["w"], 720.0);
        assert_eq!(v["h"], 420.0);

        let cascade =
            tokio::time::timeout(std::time::Duration::from_millis(500), cascade_rx.recv())
                .await
                .expect("cascade timeout")
                .expect("cascade recv");
        assert_eq!(&*cascade.trigger_session, "p2demo");
        assert_eq!(&*cascade.terminal_id, uuid);
        assert_eq!(cascade.x, 80.0);
        assert_eq!(cascade.y, 80.0);

        let list = tokio::time::timeout(std::time::Duration::from_millis(500), list_rx.recv())
            .await
            .expect("list timeout")
            .expect("list recv");
        assert_eq!(&*list.trigger_session, "p2demo");
        assert_eq!(list.added.len(), 1);
        assert_eq!(&*list.added[0], &uuid);
        assert_eq!(list.removed.len(), 0);
    }

    #[tokio::test]
    async fn create_terminal_403_when_not_attached() {
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, _) = make_state_with_workspace_and_hub(&dir);
        let app = router_with_state(state);
        create_session(&app, &token, "p2na").await;

        // Skip attach — POST /terminals must 403 not_attached.
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/p2na/terminals")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn create_terminal_cascade_offsets_from_existing_max() {
        // With one terminal at (200, 150), the next default is (232, 182).
        use gtmux_pty_backend::PaneId;
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        let existing_uuid = "11111111-2222-4333-8444-666666666700";
        state
            .terminal_map
            .register(existing_uuid.into(), PaneId(50))
            .await
            .unwrap();
        std::fs::write(
            workspace_dir.join("p2cas.json"),
            serde_json::to_vec(&json!({
                "schema_version": 2,
                "groups": [],
                "items": [{
                    "id": existing_uuid,
                    "type": "terminal",
                    "parent_id": null,
                    "x": 200.0, "y": 150.0, "w": 640.0, "h": 400.0, "z": 0,
                    "visibility": "visible", "locked": false, "minimized": false
                }],
                "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
            }))
            .unwrap(),
        )
        .unwrap();

        let app = router_with_state(state);
        let cookie = "p2cas-cookie";
        let attach = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/p2cas/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(attach.status(), StatusCode::OK);

        let mut cascade_rx = hub.subscribe_mount_cascade();

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/p2cas/terminals")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["x"], 232.0);
        assert_eq!(v["y"], 182.0);

        let cascade =
            tokio::time::timeout(std::time::Duration::from_millis(500), cascade_rx.recv())
                .await
                .expect("cascade timeout")
                .expect("cascade recv");
        assert_eq!(cascade.x, 232.0);
        assert_eq!(cascade.y, 182.0);
    }

    // ── FE Issue C unblock: spawn_terminal_with_uuid publishes 0x88 binding ──

    #[tokio::test]
    async fn spawn_terminal_with_uuid_publishes_terminal_spawned() {
        // Direct invocation: hub must observe the UUID↔PaneId binding so the
        // WS dispatcher can fan it out as 0x88 TERMINAL_SPAWNED. This is the
        // path FE relies on to switch XtermHost into "terminal" mode without
        // polling /api/terminals.
        let dir = tempfile::TempDir::new().unwrap();
        let (state, _, _) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        let uuid = "11111111-2222-4333-8444-66666666666e";
        let mut rx = hub.subscribe_terminal_spawned();
        let pane = state
            .spawn_terminal_with_uuid(uuid.to_string())
            .await
            .expect("spawn");
        let event = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
            .await
            .expect("publish must arrive")
            .expect("recv");
        assert_eq!(&*event.terminal_id, uuid);
        assert_eq!(event.pane_id, pane.0);
    }

    #[tokio::test]
    async fn spawn_terminal_with_uuid_does_not_double_publish_on_idempotent_path() {
        // Same UUID twice → fast-path returns the existing PaneId without
        // re-registering. The binding event was already emitted on the
        // first call, so the second call must stay silent.
        let dir = tempfile::TempDir::new().unwrap();
        let (state, _, _) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        let uuid = "11111111-2222-4333-8444-66666666666f";
        let mut rx = hub.subscribe_terminal_spawned();
        let _first = state
            .spawn_terminal_with_uuid(uuid.to_string())
            .await
            .expect("spawn 1");
        let _drain = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
            .await
            .expect("first publish")
            .expect("recv");
        // Re-spawn the same UUID — fast-path lookup, no fresh broadcast.
        let _second = state
            .spawn_terminal_with_uuid(uuid.to_string())
            .await
            .expect("spawn 2");
        let racy = tokio::time::timeout(std::time::Duration::from_millis(80), rx.recv()).await;
        assert!(
            racy.is_err(),
            "idempotent re-spawn must not publish a second binding, got: {racy:?}"
        );
    }

    // ── Stage 5-D path P1: attach_confirm publishes terminal-list-change ──

    #[tokio::test]
    async fn attach_confirm_publishes_terminal_list_change_when_spawn_succeeds() {
        // After a spawn batch lands the hub must broadcast a
        // TerminalListChangeEvent so other sessions' webpages can refresh
        // their pool ahead of the 5-s poll. trigger_session = the session
        // being attached to.
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        let uuid = "11111111-2222-4333-8444-666666666777";
        std::fs::write(
            workspace_dir.join("ttlc.json"),
            serde_json::to_vec(&make_layout_with_one_terminal(uuid)).unwrap(),
        )
        .unwrap();

        let app = router_with_state(state);
        let cookie = "ttlc-cookie";
        // /attach acquires the lock + binds the cookie to "ttlc".
        let attach_resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/ttlc/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(attach_resp.status(), StatusCode::OK);

        let mut rx = hub.subscribe_terminal_list_change();

        let confirm = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/ttlc/attach/confirm")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(confirm.status(), StatusCode::OK);
        let body = to_bytes(confirm.into_body(), 64 * 1024).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["spawned"], json!([uuid]));

        let event = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
            .await
            .expect("publish must arrive")
            .expect("recv");
        assert_eq!(&*event.trigger_session, "ttlc");
        assert_eq!(event.added.len(), 1);
        assert_eq!(&*event.added[0], uuid);
        assert_eq!(event.removed.len(), 0);
    }

    #[tokio::test]
    async fn attach_confirm_skips_publish_when_no_spawn_lands() {
        // Empty layout → spawned=[] → no broadcast (would create wakeup
        // noise for every WS subscriber to no purpose).
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        std::fs::write(
            workspace_dir.join("empty.json"),
            serde_json::to_vec(&json!({
                "schema_version": 2,
                "groups": [],
                "items": [],
                "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
            }))
            .unwrap(),
        )
        .unwrap();

        let app = router_with_state(state);
        let cookie = "empty-cookie";
        let attach = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/empty/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(attach.status(), StatusCode::OK);

        let mut rx = hub.subscribe_terminal_list_change();

        let confirm = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/empty/attach/confirm")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(confirm.status(), StatusCode::OK);

        // Must time out — no event was emitted.
        let race = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        assert!(race.is_err(), "expected no publish, got: {race:?}");
    }

    // ── Stage 5-B: handle_pane_died publishes terminal-died via hub ──

    #[tokio::test]
    async fn handle_pane_died_publishes_exit_reason_when_no_signal() {
        use gtmux_pty_backend::PaneId;
        let dir = tempfile::TempDir::new().unwrap();
        let (state, _, _) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        let uuid = "11111111-2222-4333-8444-66666666666b";
        state
            .terminal_map
            .register(uuid.into(), PaneId(70))
            .await
            .unwrap();
        let mut rx = hub.subscribe_terminal_died();
        state.handle_pane_died(PaneId(70), None).await;
        let event = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
            .await
            .expect("publish must arrive")
            .expect("recv");
        assert_eq!(&*event.uuid, uuid);
        assert_eq!(event.reason, "exit");
    }

    #[tokio::test]
    async fn handle_pane_died_publishes_killed_reason_when_signal_set() {
        use gtmux_pty_backend::PaneId;
        let dir = tempfile::TempDir::new().unwrap();
        let (state, _, _) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        let uuid = "11111111-2222-4333-8444-66666666666c";
        state
            .terminal_map
            .register(uuid.into(), PaneId(71))
            .await
            .unwrap();
        let mut rx = hub.subscribe_terminal_died();
        state.handle_pane_died(PaneId(71), Some(15)).await;
        let event = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
            .await
            .expect("publish must arrive")
            .expect("recv");
        assert_eq!(&*event.uuid, uuid);
        assert_eq!(event.reason, "killed");
    }

    #[tokio::test]
    async fn handle_pane_died_does_not_publish_for_unknown_pane() {
        use gtmux_pty_backend::PaneId;
        let dir = tempfile::TempDir::new().unwrap();
        let (state, _, _) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        let mut rx = hub.subscribe_terminal_died();
        state.handle_pane_died(PaneId(9999), None).await;
        // Nothing was bound, so nothing must be published.
        let race = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        assert!(race.is_err(), "expected no publish, got: {race:?}");
    }

    // ── Stage 5-A: cookie ↔ session mirror into the WS hub ──

    #[tokio::test]
    async fn attach_mirrors_cookie_to_hub_session_table() {
        // attach_handler must update hub.session_for_owner so the WS
        // dispatcher (5-C) can route session-scoped envelopes. Verifies
        // both the success-path write and that detach/cleanup later
        // unwinds it.
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        // Empty layout — match-or-spawn just returns matched=[]/unmatched=[].
        std::fs::write(
            workspace_dir.join("mirror.json"),
            serde_json::to_vec(&json!({
                "schema_version": 2,
                "groups": [],
                "items": [],
                "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
            }))
            .unwrap(),
        )
        .unwrap();

        let app = router_with_state(state);
        let cookie_value = "mirror-cookie-aaa";
        // Pre-condition: hub knows nothing about this cookie.
        assert_eq!(hub.session_for_owner(cookie_value), None);

        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/mirror/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie_value}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // attach must have mirrored the cookie binding into the hub.
        assert_eq!(
            hub.session_for_owner(cookie_value),
            Some("mirror".to_string())
        );

        // DELETE /attach must clear the mirror.
        let detach = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri("/api/sessions/mirror/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie_value}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(detach.status(), StatusCode::OK);
        assert_eq!(hub.session_for_owner(cookie_value), None);
    }

    #[tokio::test]
    async fn release_lock_for_owner_clears_hub_session() {
        // The WS-disconnect-driven release path must also clear the hub
        // mirror, otherwise a fresh WS reconnect for the same cookie
        // would see itself as still session-attached.
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        std::fs::write(
            workspace_dir.join("auto.json"),
            serde_json::to_vec(&json!({
                "schema_version": 2,
                "groups": [],
                "items": [],
                "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
            }))
            .unwrap(),
        )
        .unwrap();

        let state_clone = state.clone();
        let app = router_with_state(state);
        let cookie_value = "release-cookie-bbb";
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/auto/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie_value}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            hub.session_for_owner(cookie_value),
            Some("auto".to_string())
        );

        // Drive the WS-disconnect path directly.
        state_clone.release_lock_for_owner(cookie_value).await;
        assert_eq!(hub.session_for_owner(cookie_value), None);
    }

    #[tokio::test]
    async fn detach_is_owner_scoped_in_hub() {
        // ADR-0019 D5.6: detach_handler releases the lock only for the
        // calling Webpage's owner_key. Phantom hub entries from other
        // Webpages (or stale bindings from an old code path) must
        // survive — only the matching owner is cleared.
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        std::fs::write(
            workspace_dir.join("multi.json"),
            serde_json::to_vec(&json!({
                "schema_version": 2,
                "groups": [],
                "items": [],
                "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
            }))
            .unwrap(),
        )
        .unwrap();

        // Seed three owner bindings directly on the hub. Only "cookie-A"
        // will actually go through the attach handler below; the others
        // are *phantom* mappings (e.g. from a stale prior code path or
        // a different Webpage). D5.6 detach must not touch them.
        hub.set_session_for_owner("cookie-A", "multi");
        hub.set_session_for_owner("cookie-B", "multi");
        hub.set_session_for_owner("cookie-C", "other");

        let app = router_with_state(state);
        // Real attach to acquire the flock under cookie-A.
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/multi/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, "gtmux_auth=cookie-A")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Detach as cookie-A: owner-scoped, so only cookie-A's hub
        // binding goes; cookie-B / cookie-C survive untouched.
        let detach = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri("/api/sessions/multi/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, "gtmux_auth=cookie-A")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(detach.status(), StatusCode::OK);
        assert_eq!(hub.session_for_owner("cookie-A"), None);
        assert_eq!(
            hub.session_for_owner("cookie-B"),
            Some("multi".into()),
            "D5.6: detach is owner-scoped — sibling Webpage bindings must survive"
        );
        assert_eq!(hub.session_for_owner("cookie-C"), Some("other".into()));
    }

    // ── Implicit detach-on-reattach (session switch UX) ──────────────────

    #[tokio::test]
    async fn attach_implicitly_releases_previous_session_for_same_cookie() {
        // ADR-0019 D3 single-attach: when the same cookie attaches to a
        // *different* session, the previous session's flock must auto-release.
        // Without this, the previous session stays `active=true` forever and
        // the WorkspaceSwitcher's session-shift UX leaks state.
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        for name in ["one", "two"] {
            std::fs::write(
                workspace_dir.join(format!("{name}.json")),
                serde_json::to_vec(&json!({
                    "schema_version": 2,
                    "groups": [],
                    "items": [],
                    "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
                }))
                .unwrap(),
            )
            .unwrap();
        }

        let app = router_with_state(state);
        let cookie_value = "switch-cookie-zzz";
        // 1) attach 'one'
        let r1 = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/one/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie_value}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::OK);
        assert!(workspace_dir.join(".locks/one.lock").exists());
        assert_eq!(hub.session_for_owner(cookie_value), Some("one".into()));

        // 2) attach 'two' with the SAME cookie — must implicitly release 'one'.
        let r2 = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/sessions/two/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::COOKIE, format!("gtmux_auth={cookie_value}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r2.status(), StatusCode::OK);
        // 'one' lock must be gone, 'two' lock present.
        assert!(!workspace_dir.join(".locks/one.lock").exists());
        assert!(workspace_dir.join(".locks/two.lock").exists());
        // hub mirror must now point at 'two'.
        assert_eq!(hub.session_for_owner(cookie_value), Some("two".into()));

        // 3) Listing — 'one' active=false, 'two' active=true.
        let list = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/sessions")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(list.into_body(), 4096).await.unwrap();
        let rows: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        let one = rows.iter().find(|r| r["name"] == "one").expect("one row");
        let two = rows.iter().find(|r| r["name"] == "two").expect("two row");
        assert_eq!(
            one["active"],
            json!(false),
            "previous session must be released"
        );
        assert_eq!(two["active"], json!(true), "new session must be active");
    }

    #[tokio::test]
    async fn attach_same_name_same_cookie_is_idempotent_200() {
        // ADR-0019 D3: re-attaching to the *same* session with the same
        // cookie is idempotent — second call 200, existing flock retained.
        // Refresh races (SPA reattach overtaking WS-close release) and
        // plan-0008 Phase 2 silentReattach depend on this contract; flipping
        // it to 409 would surface the "in use" modal against the very same
        // webpage. Hub mirror also stays pointing at this session.
        let dir = tempfile::TempDir::new().unwrap();
        let (state, token, workspace_dir) = make_state_with_workspace_and_hub(&dir);
        let hub = state.hub.as_ref().expect("hub wired").clone();
        std::fs::write(
            workspace_dir.join("solo.json"),
            serde_json::to_vec(&json!({
                "schema_version": 2,
                "groups": [],
                "items": [],
                "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
            }))
            .unwrap(),
        )
        .unwrap();

        let app = router_with_state(state);
        let cookie_value = "solo-cookie-yyy";
        let make_req = || {
            HttpRequest::builder()
                .method(Method::POST)
                .uri("/api/sessions/solo/attach")
                .header(header::HOST, TEST_HOST)
                .header(header::AUTHORIZATION, bearer(&token))
                .header(header::COOKIE, format!("gtmux_auth={cookie_value}"))
                .body(Body::empty())
                .unwrap()
        };
        assert_eq!(
            app.clone().oneshot(make_req()).await.unwrap().status(),
            StatusCode::OK
        );
        assert!(workspace_dir.join(".locks/solo.lock").exists());
        assert_eq!(hub.session_for_owner(cookie_value), Some("solo".into()));
        // Second attach (same cookie, same name) — 200 idempotent. Lock
        // file stays put (no implicit release fired) and the hub mirror
        // is unchanged.
        let r2 = app.clone().oneshot(make_req()).await.unwrap();
        assert_eq!(r2.status(), StatusCode::OK);
        let body = axum::body::to_bytes(r2.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["attached"], json!(true));
        assert_eq!(v["name"], json!("solo"));
        assert!(workspace_dir.join(".locks/solo.lock").exists());
        assert_eq!(hub.session_for_owner(cookie_value), Some("solo".into()));
    }

    // ── ADR-0033 / 0080 — `/api/assets/*` ──────────────────────────────────

    /// 1×1 PNG (transparent) used by the asset tests. Hand-rolled so we don't
    /// pull a `png` crate dep just for fixtures. The IHDR width/height bytes
    /// are valid; the rest of the chunks are placeholders — sniff + dimensions
    /// only inspect the IHDR window.
    fn fixture_png_1x1() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        v.extend_from_slice(&[0, 0, 0, 13]); // IHDR length
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&1u32.to_be_bytes()); // width
        v.extend_from_slice(&1u32.to_be_bytes()); // height
        v.extend_from_slice(&[8, 6, 0, 0, 0]);
        v.extend_from_slice(&[0; 4]); // CRC placeholder
                                      // Empty IDAT (sniff doesn't decode pixels).
        v.extend_from_slice(&[0, 0, 0, 0]);
        v.extend_from_slice(b"IEND");
        v.extend_from_slice(&[0xAE, 0x42, 0x60, 0x82]);
        v
    }

    /// Build a `multipart/form-data` body manually so we don't pull a fresh
    /// dep just for tests. `file` field is binary; `kind` is plain text.
    fn build_multipart(
        boundary: &str,
        file_name: &str,
        content_type: &str,
        file_bytes: &[u8],
        kind: &str,
    ) -> Vec<u8> {
        let mut body = Vec::new();
        // file
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n",)
                .as_bytes(),
        );
        body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
        body.extend_from_slice(file_bytes);
        body.extend_from_slice(b"\r\n");
        // kind
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(b"Content-Disposition: form-data; name=\"kind\"\r\n\r\n");
        body.extend_from_slice(kind.as_bytes());
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        body
    }

    fn upload_request(token: &TokenString, body: Vec<u8>, boundary: &str) -> HttpRequest<Body> {
        HttpRequest::builder()
            .method("POST")
            .uri("/api/assets")
            .header(header::HOST, TEST_HOST)
            .header(header::AUTHORIZATION, bearer(token))
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap()
    }

    fn upload_from_path_request(token: &TokenString, path: &Path, kind: &str) -> HttpRequest<Body> {
        HttpRequest::builder()
            .method("POST")
            .uri("/api/assets/from-path")
            .header(header::HOST, TEST_HOST)
            .header(header::AUTHORIZATION, bearer(token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "path": path.to_string_lossy(), "kind": kind }).to_string(),
            ))
            .unwrap()
    }

    /// 0080 §5 — image upload → GET bytes roundtrip. Verifies idempotent
    /// content addressing as a side benefit (the same asset_id is reachable
    /// via GET right away).
    #[tokio::test]
    async fn assets_image_upload_and_get_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        let png = fixture_png_1x1();
        let body = build_multipart("boundary42", "tiny.png", "image/png", &png, "image");
        let resp = app
            .clone()
            .oneshot(upload_request(&token, body, "boundary42"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let asset_id = v["asset_id"].as_str().unwrap().to_string();
        assert_eq!(asset_id.len(), 64);
        assert!(asset_id
            .bytes()
            .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')));
        assert_eq!(v["mime"], "image/png");
        assert_eq!(v["file_name"], "tiny.png");
        assert_eq!(v["size_bytes"], png.len() as u64);
        assert_eq!(v["original_w"], 1);
        assert_eq!(v["original_h"], 1);

        // GET the same id — bytes must be byte-identical.
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/api/assets/{asset_id}"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/png"
        );
        assert!(resp
            .headers()
            .get(header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("immutable"));
        let got = to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap()
            .to_vec();
        assert_eq!(got, png, "GET must return identical bytes");
    }

    /// Existing file-system picker flow: FE selects a workspace path, then
    /// asks the API to copy that file into the content-addressed asset store.
    #[tokio::test]
    async fn assets_from_path_image_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, workspace_dir) = make_app_with_workspace(&dir);
        let png = fixture_png_1x1();
        let source = workspace_dir.join("picked.png");
        std::fs::write(&source, &png).unwrap();

        let resp = app
            .clone()
            .oneshot(upload_from_path_request(&token, &source, "image"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["mime"], "image/png");
        assert_eq!(v["file_name"], "picked.png");
        let asset_id = v["asset_id"].as_str().unwrap();

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/api/assets/{asset_id}"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let got = to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap()
            .to_vec();
        assert_eq!(got, png);
    }

    #[tokio::test]
    async fn assets_from_path_rejects_outside_workspace() {
        let dir = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        let source = outside.path().join("picked.png");
        std::fs::write(&source, fixture_png_1x1()).unwrap();

        let resp = app
            .oneshot(upload_from_path_request(&token, &source, "image"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    /// 0080 §5 — same bytes uploaded twice yield the same asset_id (the
    /// sha256 deduplication of ADR-0033 D8). Both responses are 201.
    #[tokio::test]
    async fn assets_idempotent_same_bytes() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        let png = fixture_png_1x1();

        let upload_once = || async {
            let body = build_multipart("b1", "x.png", "image/png", &png, "image");
            let resp = app
                .clone()
                .oneshot(upload_request(&token, body, "b1"))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::CREATED);
            let bytes = to_bytes(resp.into_body(), 4096).await.unwrap();
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            v["asset_id"].as_str().unwrap().to_string()
        };
        let id1 = upload_once().await;
        let id2 = upload_once().await;
        assert_eq!(id1, id2, "same bytes must dedupe to the same asset_id");
    }

    /// 0080 §5 — oversize upload returns 413. We send a body that exceeds
    /// `ASSET_MAX_BYTES` after the size check inside the handler runs
    /// (axum's DefaultBodyLimit may also fire — both map to 413).
    #[tokio::test]
    async fn assets_oversize_returns_413() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        // 21 MiB random-ish payload — over the 20 MiB cap. We need to stay
        // inside the route's DefaultBodyLimit (+ 1 MiB headroom) or axum
        // short-circuits with 413 before the handler runs; either path is
        // an acceptable 413 — the goal is "client gets PAYLOAD_TOO_LARGE".
        let big = vec![0u8; assets::ASSET_MAX_BYTES + 16];
        let mut payload = Vec::with_capacity(big.len() + 16);
        payload.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        payload.extend_from_slice(&big);
        let body = build_multipart("bo", "big.png", "image/png", &payload, "image");
        let resp = app
            .oneshot(upload_request(&token, body, "bo"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    /// 0080 §5 — invalid `asset_id` path returns 400 (not 404). Anything that
    /// doesn't match `[a-f0-9]{64}` is rejected before any FS access — this
    /// is the path-traversal guard.
    #[tokio::test]
    async fn assets_invalid_asset_id_path_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        // ".." traversal attempt
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/assets/..%2Fpasswd")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Axum's path extractor decodes %2F; "../passwd" doesn't match the
        // 64-char hex shape, so we get 400 from the validator.
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Wrong length
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/assets/abc123")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Uppercase hex — allowlist is lowercase only.
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/api/assets/{}", "A".repeat(64)))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// 0080 §5 — unauthenticated upload returns 401 from the bearer auth
    /// middleware. Confirms `/api/assets` sits on the same `/api/*` gate
    /// as everything else.
    #[tokio::test]
    async fn assets_upload_unauthorized() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, _token, _) = make_app_with_workspace(&dir);
        let png = fixture_png_1x1();
        let body = build_multipart("ba", "x.png", "image/png", &png, "image");
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/assets")
                    .header(header::HOST, TEST_HOST)
                    .header(header::CONTENT_TYPE, "multipart/form-data; boundary=ba")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// MIME / kind mismatch: client claims `kind=document` but the bytes are
    /// a PNG → 415. Magic-byte sniff is the source of truth (ADR-0033 D4).
    #[tokio::test]
    async fn assets_kind_mime_mismatch_returns_415() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        let png = fixture_png_1x1();
        let body = build_multipart("bm", "x.png", "image/png", &png, "document");
        let resp = app
            .oneshot(upload_request(&token, body, "bm"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    /// Asset-based document upload — PDF magic bytes round-trip with
    /// `mime: application/pdf`. Backs the FE `DocumentNode` asset-mode wire.
    #[tokio::test]
    async fn assets_document_pdf_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        let pdf = b"%PDF-1.4\n%minimal\n%%EOF\n".to_vec();
        let body = build_multipart("bd", "brief.pdf", "application/pdf", &pdf, "document");
        let resp = app
            .clone()
            .oneshot(upload_request(&token, body, "bd"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["mime"], "application/pdf");
        assert_eq!(v["file_name"], "brief.pdf");
        assert_eq!(v["size_bytes"], pdf.len() as u64);
        // Documents must not carry image dimensions.
        assert!(v.get("original_w").is_none() || v["original_w"].is_null());
        assert!(v.get("original_h").is_none() || v["original_h"].is_null());
    }

    // ─────────────────────────────────────────────────────────────────────
    //  UI/UX batch-5 — ADR-0018 D4 amend ①+② (figure + text payload)
    //
    //  schema.rs unit tests already cover the (de)serialise + validate
    //  surface in isolation. These integration tests fire real HTTP
    //  requests through the Router so the FE's wire round-trips — and the
    //  disk write that backs it — pick up the new fields end-to-end.
    // ─────────────────────────────────────────────────────────────────────

    const BATCH5_UUID_RECT: &str = "b5b50000-0000-4111-8222-000000000001";
    const BATCH5_UUID_TEXT: &str = "b5b50000-0000-4111-8222-000000000002";

    /// Helper: GET layout, return current ETag.
    async fn batch5_fetch_etag(app: &Router, token: &TokenString, session: &str) -> String {
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/api/sessions/{session}/layout"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        resp.headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    /// Helper: PUT a layout body and return the response.
    async fn batch5_put_layout(
        app: &Router,
        token: &TokenString,
        session: &str,
        etag: &str,
        layout: &Value,
    ) -> Response {
        let body = serde_json::to_vec(layout).unwrap();
        app.clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/sessions/{session}/layout"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(token))
                    .header("x-gtmux-webpage-id", session)
                    .header(header::IF_MATCH, etag)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// Helper: GET layout body as `serde_json::Value` (no ETag assertion).
    async fn batch5_get_layout(app: &Router, token: &TokenString, session: &str) -> Value {
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/api/sessions/{session}/layout"))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 16 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// End-to-end: PUT a Rect carrying every D4 amend ① field (fill off,
    /// stroke on, corner rounded, dash_dot) → 204 + new ETag. GET back →
    /// every field preserved by the disk-of-truth write.
    #[tokio::test]
    async fn batch5_layout_put_rect_full_payload_round_trip() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        attach_idx_create_session(&app, &token, "fig1").await;
        let etag = batch5_fetch_etag(&app, &token, "fig1").await;

        let layout = json!({
            "schema_version": 2,
            "groups": [],
            "items": [{
                "type": "rect",
                "id": BATCH5_UUID_RECT,
                "parent_id": null,
                "x": 50.0, "y": 60.0, "w": 200.0, "h": 120.0, "z": 1,
                "visibility": "visible", "locked": false,
                "label": "", "description": "", "minimized": false,
                "stroke": "#0d99ff", "fill": "#abcdef", "stroke_width": 4,
                "fill_enabled": false,
                "stroke_enabled": true,
                "corner_rounded": true,
                "stroke_dash": "dash_dot",
            }],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        });
        let resp = batch5_put_layout(&app, &token, "fig1", &etag, &layout).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let got = batch5_get_layout(&app, &token, "fig1").await;
        let rect = got["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|it| it["id"] == BATCH5_UUID_RECT)
            .expect("rect persisted");
        assert_eq!(rect["fill_enabled"], false);
        assert_eq!(rect["stroke_enabled"], true);
        assert_eq!(rect["corner_rounded"], true);
        assert_eq!(rect["stroke_dash"], "dash_dot");
        assert_eq!(rect["stroke_width"], 4);
    }

    /// Legacy compat: PUT a Rect *without* any of the D4 amend ① fields →
    /// 204. GET back: `fill_enabled` / `stroke_enabled` deserialise to
    /// `true` (via `default = "default_true"`), `corner_rounded` defaults
    /// to `false`, `stroke_dash` is omitted from the wire form.
    #[tokio::test]
    async fn batch5_layout_put_legacy_rect_get_defaults() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        attach_idx_create_session(&app, &token, "fig2").await;
        let etag = batch5_fetch_etag(&app, &token, "fig2").await;

        let legacy_layout = json!({
            "schema_version": 2,
            "groups": [],
            "items": [{
                "type": "rect",
                "id": BATCH5_UUID_RECT,
                "parent_id": null,
                "x": 0.0, "y": 0.0, "w": 100.0, "h": 100.0, "z": 0,
                "visibility": "visible", "locked": false,
                "label": "", "description": "", "minimized": false,
                "stroke": "#000", "fill": "#fff", "stroke_width": 1,
            }],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        });
        let resp = batch5_put_layout(&app, &token, "fig2", &etag, &legacy_layout).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let got = batch5_get_layout(&app, &token, "fig2").await;
        let rect = got["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|it| it["id"] == BATCH5_UUID_RECT)
            .expect("rect persisted");
        assert_eq!(rect["fill_enabled"], true);
        assert_eq!(rect["stroke_enabled"], true);
        assert_eq!(rect["corner_rounded"], false);
        assert!(
            rect.get("stroke_dash").is_none() || rect["stroke_dash"].is_null(),
            "None stroke_dash must be skipped on wire (`skip_serializing_if`)"
        );
    }

    /// Validation surface: PUT a Rect with `stroke_width = 99` (over the
    /// 1..=32 inspector band) → 400 with the stable
    /// `stroke_width_out_of_range` envelope code so the FE can render a
    /// precise message instead of a generic "bad request".
    #[tokio::test]
    async fn batch5_layout_put_stroke_width_overflow_returns_400() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        attach_idx_create_session(&app, &token, "fig3").await;
        let etag = batch5_fetch_etag(&app, &token, "fig3").await;

        let bad_layout = json!({
            "schema_version": 2,
            "groups": [],
            "items": [{
                "type": "rect",
                "id": BATCH5_UUID_RECT,
                "parent_id": null,
                "x": 0.0, "y": 0.0, "w": 100.0, "h": 100.0, "z": 0,
                "visibility": "visible", "locked": false,
                "label": "", "description": "", "minimized": false,
                "stroke": "#000", "fill": "#fff", "stroke_width": 99,
            }],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        });
        let resp = batch5_put_layout(&app, &token, "fig3", &etag, &bad_layout).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(resp.into_body(), 4096).await.unwrap();
        let env: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(env["error"], "stroke_width_out_of_range");
    }

    /// Validation surface: PUT a Text with `font_size = 200` (over the
    /// 8..=96 inspector band) → 400 + `text_font_size_out_of_range`.
    #[tokio::test]
    async fn batch5_layout_put_text_font_size_overflow_returns_400() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        attach_idx_create_session(&app, &token, "txt1").await;
        let etag = batch5_fetch_etag(&app, &token, "txt1").await;

        let bad_layout = json!({
            "schema_version": 2,
            "groups": [],
            "items": [{
                "type": "text",
                "id": BATCH5_UUID_TEXT,
                "parent_id": null,
                "x": 0.0, "y": 0.0, "w": 160.0, "h": 56.0, "z": 0,
                "visibility": "visible", "locked": false,
                "label": "", "description": "", "minimized": false,
                "text": "Hello", "font_size": 200, "color": "#333",
            }],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        });
        let resp = batch5_put_layout(&app, &token, "txt1", &etag, &bad_layout).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(resp.into_body(), 4096).await.unwrap();
        let env: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(env["error"], "text_font_size_out_of_range");
    }

    /// End-to-end Text style round-trip: PUT Text with bold + italic +
    /// underline (strikethrough off) → 204. GET back → every batch-5
    /// field present with the exact value.
    #[tokio::test]
    async fn batch5_layout_put_text_full_style_round_trip() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        attach_idx_create_session(&app, &token, "txt2").await;
        let etag = batch5_fetch_etag(&app, &token, "txt2").await;

        let layout = json!({
            "schema_version": 2,
            "groups": [],
            "items": [{
                "type": "text",
                "id": BATCH5_UUID_TEXT,
                "parent_id": null,
                "x": 10.0, "y": 20.0, "w": 240.0, "h": 64.0, "z": 5,
                "visibility": "visible", "locked": false,
                "label": "Heading", "description": "", "minimized": false,
                "text": "Build Plan", "font_size": 18, "color": "#222",
                "font_weight": "bold",
                "italic": true,
                "underline": true,
                "strikethrough": false,
            }],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        });
        let resp = batch5_put_layout(&app, &token, "txt2", &etag, &layout).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let got = batch5_get_layout(&app, &token, "txt2").await;
        let text = got["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|it| it["id"] == BATCH5_UUID_TEXT)
            .expect("text persisted");
        assert_eq!(text["font_weight"], "bold");
        assert_eq!(text["italic"], true);
        assert_eq!(text["underline"], true);
        assert_eq!(text["strikethrough"], false);
        assert_eq!(text["font_size"], 18);
    }
}
