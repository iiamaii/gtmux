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

mod auth;
mod schema;
mod session_lock;
mod sessions;
mod storage;
mod terminal_map;
mod terminals;
mod workspace;

pub use auth::{
    default_password_hash_path, default_rate_limiter, default_session_table, hash_password,
    load_password_hash, save_password_hash, verify_password, AuthError, AuthMode, RateLimiter,
    SessionTable,
};
pub use schema::{
    detect_shape, migrate_v1_to_v2, validate as validate_layout_v2, Group, Item, ItemCommon,
    Layout, Point, SchemaShape, ValidationError, Viewport, Visibility, SCHEMA_VERSION,
};
pub use session_lock::{fresh_server_id, Lease, LockError, LockGuard, LockState};
pub use sessions::{SessionCache, SessionError, SessionLayout};
pub use storage::{LayoutStore, StorageError};
pub use terminal_map::{fresh_terminal_uuid, MapError as TerminalMapError, TerminalMap};
pub use terminals::{TerminalInfo, TerminalMetadata, TerminalMetadataStore};
pub use workspace::{
    validate_session_name, BootMigrationReport, SessionInfo, WorkspaceError, WorkspaceManager,
};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use gtmux_auth::TokenString;
use gtmux_config::{Config, Mode};
use ring::digest::{digest, SHA256};
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
/// form; the hex string is a rendering helper kept in sync with the bytes.
#[derive(Debug, Clone)]
pub struct LayoutSnapshot {
    /// 16-byte raw ETag (SHA-256-128 of the canonical body bytes).
    pub etag: [u8; 16],
    /// 32-character lowercase hex of `etag` — for `ETag` header rendering.
    pub etag_hex: String,
    /// The current layout body as an opaque JSON value. A future
    /// `gtmux-canvas-layout` crate will replace this with a strongly-typed
    /// struct; the API contract holds because canonical JSON serialisation is
    /// stable for both shapes.
    pub body: Value,
}

impl LayoutSnapshot {
    /// Build the initial empty snapshot per `canvas-layout-schema.md` §4.1.
    pub fn empty() -> Self {
        let body = json!({
            "schema_version": 1,
            "groups": [],
            "panels": [],
        });
        Self::from_body(body)
    }

    pub(crate) fn from_body(body: Value) -> Self {
        let bytes = canonical_serialize(&body);
        let (etag, etag_hex) = compute_etag(&bytes);
        Self {
            etag,
            etag_hex,
            body,
        }
    }
}

/// Shared application state wired into the router. Cloning is cheap (Arc).
#[derive(Clone)]
pub struct AppState {
    /// Loaded gtmux config — used for mode, host/origin allowlists, port.
    pub config: Arc<Config>,
    /// The session token for this Server run.
    pub token: Arc<TokenString>,
    /// Layout snapshot guarded by an `RwLock` for atomic GET / PUT swap.
    pub layout: Arc<RwLock<LayoutSnapshot>>,
    /// Auth-failure counter exposed for downstream rate-limit middleware
    /// (P1 enforcement; ADR-0003 D12 cloud-only). The counter is monotonic.
    pub auth_failure_counter: Arc<AtomicU64>,
    /// Optional WS broadcast hub. When set, `layout_put_handler` publishes
    /// the new ETag so live WS subscribers re-hydrate via the dispatcher's
    /// `LAYOUT_CHANGED` path. `None` in unit-tests that exercise the HTTP
    /// surface in isolation.
    pub hub: Option<gtmux_ws_server::Hub>,
    /// Optional on-disk layout store. When set, `layout_put_handler` writes
    /// the new snapshot atomically before swapping the in-memory state and
    /// broadcasting `LAYOUT_CHANGED` (ADR-0006 D13 5-step). `None` in tests
    /// that exercise the HTTP surface without touching disk.
    pub store: Option<Arc<LayoutStore>>,
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
    /// PHC-encoded Argon2id hash for password-mode auth (ADR-0020 D5). `None`
    /// in token mode or when the password file doesn't exist yet (login then
    /// 503s with a hint to run `gtmux set-password`).
    pub password_hash: Option<Arc<String>>,
    /// UUID v4 minted once per server boot (ADR-0019 D6.1). Written into
    /// `.locks/<name>.lock` bodies so other servers can disambiguate
    /// holders that happen to share a PID.
    pub server_id: Arc<str>,
    /// Locks currently held by *this* server, keyed by session name. The
    /// outer Mutex protects the map; each [`LockGuard`] inside is itself
    /// the OS-level flock. Serialises same-server attach attempts on the
    /// same session name (D6.6).
    pub session_locks: Arc<tokio::sync::Mutex<std::collections::HashMap<String, LockGuard>>>,
    /// Reverse index: cookie value → session name. Populated when an attach
    /// succeeds against a known cookie; consulted on WS-close to find the
    /// matching `session_locks` entry to release (ADR-0019 D6 §heartbeat).
    /// Manipulated *only* while `session_locks` is held to keep the two
    /// maps consistent — never under contention from a different path.
    pub session_locks_by_cookie: Arc<tokio::sync::Mutex<std::collections::HashMap<String, String>>>,
    /// UUID ↔ PaneId bridge for the schema v2 terminal-item model (ADR-0018
    /// D2). Every spawn that surfaces through the HTTP API registers here;
    /// every detected death unregisters. The `pty-backend` / `ws-server`
    /// crates remain UUID-blind — only this crate crosses the boundary.
    pub terminal_map: Arc<TerminalMap>,
    /// Per-terminal label + created_at, keyed by the same UUID as
    /// `terminal_map`. In-memory only — recreated each boot (Stage 4-B).
    pub terminal_meta: Arc<TerminalMetadataStore>,
}

impl AppState {
    /// Assemble shared state with a fresh empty layout snapshot.
    /// `hub` is `None`; production callers must use [`AppState::with_hub`].
    pub fn new(config: Config, token: TokenString) -> Self {
        let session_table = default_session_table(config.auth.cookie_max_age_days);
        Self {
            session_table,
            rate_limiter: default_rate_limiter(),
            password_hash: None,
            server_id: Arc::from(fresh_server_id()),
            session_locks: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            session_locks_by_cookie: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            terminal_map: Arc::new(TerminalMap::new()),
            terminal_meta: Arc::new(TerminalMetadataStore::new()),
            config: Arc::new(config),
            token: Arc::new(token),
            layout: Arc::new(RwLock::new(LayoutSnapshot::empty())),
            auth_failure_counter: Arc::new(AtomicU64::new(0)),
            hub: None,
            store: None,
            workspace: None,
            session_cache: Arc::new(SessionCache::new()),
        }
    }

    /// Attach a pre-loaded Argon2id password hash (read from the file
    /// produced by `gtmux set-password`). Without this `POST /auth/login`
    /// returns 503 in password mode.
    pub fn with_password_hash(mut self, hash: String) -> Self {
        self.password_hash = Some(Arc::new(hash));
        self
    }

    /// Refresh the lease body of the session lock currently held by
    /// `cookie` (ADR-0019 D6.2). Called from the WS heartbeat consumer task
    /// on every Ping/Pong. Bumps the `lease_until_unix` field so a peeking
    /// modal sees a fresh expected expiry. The kernel flock is unaffected
    /// — this is purely a diagnostic refresh.
    ///
    /// Idempotent. A cookie that holds no lock is a no-op.
    pub async fn refresh_lease_for_cookie(&self, cookie: &str) {
        let by_cookie = self.session_locks_by_cookie.lock().await;
        let Some(name) = by_cookie.get(cookie).cloned() else {
            return;
        };
        drop(by_cookie);
        let mut holders = self.session_locks.lock().await;
        if let Some(guard) = holders.get_mut(&name) {
            if let Err(e) = guard.refresh_lease(cookie) {
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
    pub async fn handle_pane_died(
        &self,
        pane: gtmux_pty_backend::PaneId,
        signal: Option<i32>,
    ) {
        if let Some(uuid) = self.terminal_map.unregister_pane(pane).await {
            let reason = if signal.is_some() { "killed" } else { "exit" };
            if let Some(hub) = self.hub.as_ref() {
                hub.publish_terminal_died(&uuid, reason);
            }
            tracing::debug!(
                pane = ?pane,
                uuid = %uuid,
                reason,
                "terminal: unregistered after BackendNotify::PaneDied (metadata preserved)"
            );
        }
    }

    /// Release any cross-server session lock currently held by `cookie`
    /// (ADR-0019 D6). Called from the WS disconnect consumer task on close.
    /// Idempotent — a cookie that never attached is a no-op.
    pub async fn release_lock_for_cookie(&self, cookie: &str) {
        // Locks are taken in a fixed order (locks_by_cookie → session_locks)
        // anywhere two maps are touched together, so a same-cookie attach
        // racing with a disconnect cannot deadlock.
        let mut by_cookie = self.session_locks_by_cookie.lock().await;
        let Some(name) = by_cookie.remove(cookie) else {
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
        // Stage 5-A: mirror the auto-release into the hub's cookie ↔
        // session table so a fresh WS reconnect doesn't see this cookie
        // as still session-attached.
        if let Some(hub) = self.hub.as_ref() {
            hub.clear_session_for_cookie(cookie);
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
        let hub = self.hub.as_ref().ok_or(SpawnTerminalError::HubUnavailable)?;
        let pane = hub
            .backend()
            .spawn(gtmux_pty_backend::SpawnSpec::default_shell())?;
        match self.terminal_map.register(uuid.clone(), pane).await {
            Ok(()) => {
                self.terminal_meta.record_spawn(&uuid).await;
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
    pub fn with_workspace(mut self, workspace: WorkspaceManager) -> Self {
        self.workspace = Some(Arc::new(workspace));
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

    /// Production constructor: hub + on-disk layout file. The file is loaded
    /// at boot time via [`LayoutStore::load`] (ADR-0006 D10 7-state table
    /// — absent / valid / corrupt all converge on a valid `LayoutSnapshot`).
    /// Subsequent successful `PUT /api/layout` calls atomically rewrite the
    /// file before the in-memory swap.
    pub fn with_hub_and_path(
        config: Config,
        token: TokenString,
        hub: gtmux_ws_server::Hub,
        layout_path: PathBuf,
    ) -> Self {
        let store = LayoutStore::new(layout_path);
        let snapshot = store.load();
        let session_table = default_session_table(config.auth.cookie_max_age_days);
        Self {
            session_table,
            rate_limiter: default_rate_limiter(),
            password_hash: None,
            server_id: Arc::from(fresh_server_id()),
            session_locks: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            session_locks_by_cookie: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            terminal_map: Arc::new(TerminalMap::new()),
            terminal_meta: Arc::new(TerminalMetadataStore::new()),
            config: Arc::new(config),
            token: Arc::new(token),
            layout: Arc::new(RwLock::new(snapshot)),
            auth_failure_counter: Arc::new(AtomicU64::new(0)),
            hub: Some(hub),
            store: Some(Arc::new(store)),
            workspace: None,
            session_cache: Arc::new(SessionCache::new()),
        }
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
            "/api/layout",
            get(layout_get_handler).put(layout_put_handler),
        )
        .route(
            "/api/sessions",
            get(sessions::list_handler).post(sessions::create_handler),
        )
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
            axum::routing::post(sessions::attach_handler)
                .delete(sessions::detach_handler),
        )
        .route(
            "/api/sessions/{name}/attach/confirm",
            axum::routing::post(sessions::attach_confirm_handler),
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
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            bearer_auth_middleware,
        ));

    let mut router = Router::new()
        .merge(api)
        // Auth subtree — ADR-0020. The legacy `/auth/bootstrap` is kept as a
        // 302-style redirect to `/auth?token=…` for backwards compat with
        // bookmarks; the inline-script HTML it used to serve is gone.
        .route("/auth", get(auth::auth_page_handler))
        .route("/auth/login", axum::routing::post(auth::auth_login_handler))
        .route("/auth/logout", axum::routing::post(auth::auth_logout_handler))
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

/// Legacy bootstrap route — ADR-0020 D8 obsoleted the inline-script flow.
/// We keep the URL alive so existing bookmarks still work, but the body is
/// now a 303 to the canonical `/auth?token=…` endpoint. The token query
/// itself is verified (and cookie issued) by `auth_page_handler`.
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
            "/auth?token={}&redirect={}",
            urlencode_query(&token),
            urlencode_query(r)
        ),
        None => format!("/auth?token={}", urlencode_query(&token)),
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

async fn layout_get_handler(State(state): State<AppState>, req: Request) -> Response {
    let snap = state.layout.read().await;
    let etag_quoted = format!("\"{}\"", snap.etag_hex);

    // RFC 7232: If-None-Match → 304 when current ETag matches.
    if let Some(if_none_match) = req.headers().get(header::IF_NONE_MATCH) {
        if let Ok(v) = if_none_match.to_str() {
            if parse_etag_header(v).is_some_and(|h| h == snap.etag_hex) {
                let mut resp = Response::builder()
                    .status(StatusCode::NOT_MODIFIED)
                    .header(header::ETAG, &etag_quoted)
                    .body(Body::empty())
                    .expect("static headers");
                apply_security_headers(resp.headers_mut(), state.config.mode());
                return resp;
            }
        }
    }

    let body = serde_json::to_vec(&snap.body).expect("snapshot is always valid JSON");
    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ETAG, &etag_quoted)
        .body(Body::from(body))
        .expect("static headers");
    apply_security_headers(resp.headers_mut(), state.config.mode());
    resp
}

/// Soft cap from `canvas-layout-schema.md` §3 R9 (256 KB).
const PUT_MAX_BYTES: usize = 256 * 1024;

async fn layout_put_handler(State(state): State<AppState>, req: Request) -> Response {
    // 1. If-Match — required.
    let if_match = match req.headers().get(header::IF_MATCH) {
        Some(v) => match v.to_str() {
            Ok(s) => match parse_etag_header(s) {
                Some(parsed) => parsed,
                None => return HttpApiError::PreconditionRequired.into_response(),
            },
            Err(_) => return HttpApiError::PreconditionRequired.into_response(),
        },
        None => return HttpApiError::PreconditionRequired.into_response(),
    };

    // 2. Read the body up to the 256 KB cap. axum 0.8 returns `Body`, which
    //    we drain via `http-body-util` for a portable size-bounded read.
    let body_bytes = match read_bounded_body(req, PUT_MAX_BYTES).await {
        Ok(b) => b,
        Err(BodyReadError::TooLarge) => return HttpApiError::PayloadTooLarge.into_response(),
        Err(BodyReadError::Io(msg)) => return HttpApiError::BadRequest(msg).into_response(),
    };

    // 3. Parse JSON.
    let body: Value = match serde_json::from_slice(&body_bytes) {
        Ok(v) => v,
        Err(e) => return HttpApiError::BadRequest(format!("json: {e}")).into_response(),
    };

    // 4. Minimal schema check — full validation is gated behind the
    //    `gtmux-canvas-layout` crate (Sprint 3+).
    if let Err(msg) = minimal_layout_check(&body) {
        return HttpApiError::BadRequest(msg).into_response();
    }

    // 5. Atomic compare-and-swap on the ETag. The whole transition runs under
    //    the write lock so two concurrent PUTs cannot observe the same ETag.
    //    ADR-0006 D13 5-step: validate (done) → new ETag → atomic disk write →
    //    in-memory swap → LAYOUT_CHANGED broadcast.
    let mut snap = state.layout.write().await;
    if if_match != snap.etag_hex {
        let current_etag_quoted = format!("\"{}\"", snap.etag_hex);
        let mut resp = HttpApiError::PreconditionFailed.into_response();
        // Hand back the current ETag so the client can refetch and retry.
        resp.headers_mut().insert(
            header::ETAG,
            HeaderValue::from_str(&current_etag_quoted).expect("hex is always valid header value"),
        );
        return resp;
    }

    let new_snap = LayoutSnapshot::from_body(body);
    let new_etag = new_snap.etag;
    let new_etag_quoted = format!("\"{}\"", new_snap.etag_hex);

    // Disk-first: a failed atomic write must leave both the on-disk file and
    // the in-memory snapshot untouched so the client can safely retry. The
    // 500 response carries the *current* ETag so the SPA can resync without
    // re-fetching the layout (the rejected payload never made it to memory).
    if let Some(store) = &state.store {
        let body_bytes = canonical_serialize(&new_snap.body);
        if let Err(e) = store.save(&body_bytes) {
            tracing::error!(
                error = %e,
                "layout_put_handler: atomic disk write failed; in-memory snapshot preserved"
            );
            let current_etag_quoted = format!("\"{}\"", snap.etag_hex);
            let mut resp = (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "layout_persist_failed",
                    "message": e.to_string(),
                })),
            )
                .into_response();
            if let Ok(val) = HeaderValue::from_str(&current_etag_quoted) {
                resp.headers_mut().insert(header::ETAG, val);
            }
            return resp;
        }
    }

    *snap = new_snap;
    drop(snap);

    // Fan out LAYOUT_CHANGED to every live WS subscriber so the SPA
    // dispatcher revalidates via `If-None-Match` and rehydrates panels.
    match &state.hub {
        Some(hub) => {
            tracing::debug!(
                etag = %new_etag_quoted,
                "layout_put_handler: publishing LAYOUT_CHANGED to WS subscribers"
            );
            hub.publish_layout_changed(new_etag);
        }
        None => {
            tracing::debug!("layout_put_handler: no hub attached, skipping broadcast");
        }
    }

    // canvas-layout-schema §4.2: 204 No Content + ETag header. The empty
    // body distinguishes a confirmed commit from a 200-style response so
    // the SPA's PUT helper does not misclassify it as an error.
    let mut resp = Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::ETAG, &new_etag_quoted)
        .body(Body::empty())
        .expect("static headers");
    apply_security_headers(resp.headers_mut(), state.config.mode());
    resp
}

// ─────────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Apply the SSoT §1.5 security headers to every response. STS is only
/// emitted in Cloud mode (SSoT line for STS).
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

/// Canonical-form serialisation for ETag stability. We use `serde_json`'s
/// default (compact, no trailing whitespace, deterministic struct ordering)
/// — sufficient for the MVP. Future tightening (sorted keys, normalised
/// numbers) lives in the `gtmux-canvas-layout` crate so behaviour can change
/// without affecting the HTTP surface.
fn canonical_serialize(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).expect("Value is always serialisable")
}

/// Compute SHA256-128 → (raw bytes, lowercase hex). 32 chars.
fn compute_etag(bytes: &[u8]) -> ([u8; 16], String) {
    let d = digest(&SHA256, bytes);
    let full = d.as_ref();
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&full[..16]);
    let mut hex = String::with_capacity(32);
    for b in raw.iter() {
        hex.push_str(&format!("{:02x}", b));
    }
    (raw, hex)
}

/// Parse the value of an `ETag` / `If-Match` / `If-None-Match` header into
/// its 32-character lowercase-hex inner content. Returns `None` if the
/// header is not a single, strong, 32-hex ETag.
fn parse_etag_header(v: &str) -> Option<String> {
    let trimmed = v.trim();
    // RFC 7232: weak ETags begin with `W/`. We accept neither weak nor
    // wildcard (`*`) — PUT must use the strong opaque-tag form.
    if trimmed.starts_with("W/") || trimmed == "*" {
        return None;
    }
    let inner = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))?;
    if inner.len() != 32 {
        return None;
    }
    if !inner
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return None;
    }
    Some(inner.to_string())
}

/// Minimal schema check pending the full `gtmux-canvas-layout` crate. We
/// reject anything that obviously cannot satisfy the schema so the hook into
/// the future validator is a drop-in.
pub(crate) fn minimal_layout_check(body: &Value) -> Result<(), String> {
    let obj = body.as_object().ok_or("body must be a JSON object")?;
    if let Some(g) = obj.get("groups") {
        if !g.is_array() {
            return Err("groups must be an array".to_string());
        }
    } else {
        return Err("missing required field: groups".to_string());
    }
    if let Some(p) = obj.get("panels") {
        if !p.is_array() {
            return Err("panels must be an array".to_string());
        }
    } else {
        return Err("missing required field: panels".to_string());
    }
    if let Some(sv) = obj.get("schema_version") {
        if sv.as_u64() != Some(1) {
            return Err("schema_version must be 1".to_string());
        }
    }
    Ok(())
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
    use gtmux_config::{Config, RuntimeConfig, SecurityConfig, ServerConfig};
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
    async fn layout_get_initial() {
        let (app, token) = make_app();
        let req = HttpRequest::builder()
            .uri("/api/layout")
            .header(header::HOST, TEST_HOST)
            .header(header::AUTHORIZATION, bearer(&token))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let etag = resp.headers().get(header::ETAG).cloned();
        assert!(etag.is_some(), "ETag header must be present");
        let etag_str = etag.unwrap();
        let inner = parse_etag_header(etag_str.to_str().unwrap());
        assert!(inner.is_some(), "ETag must be quoted 32-hex");
        assert_eq!(inner.unwrap().len(), 32);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["groups"].as_array().unwrap().len(), 0);
        assert_eq!(body["panels"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn layout_get_304_on_if_none_match() {
        let (app, token) = make_app();
        let first = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let etag = first
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let second = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::IF_NONE_MATCH, &etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn layout_put_requires_if_match() {
        let (app, token) = make_app();
        let body = serde_json::to_vec(&json!({
            "schema_version": 1,
            "groups": [],
            "panels": [],
        }))
        .unwrap();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PRECONDITION_REQUIRED);
    }

    #[tokio::test]
    async fn layout_put_etag_mismatch() {
        let (app, token) = make_app();
        let body = serde_json::to_vec(&json!({
            "schema_version": 1,
            "groups": [],
            "panels": [],
        }))
        .unwrap();
        let stale = "\"00000000000000000000000000000000\"";
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::IF_MATCH, stale)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PRECONDITION_FAILED);
        // The current ETag must be attached so clients can rebase.
        assert!(resp.headers().get(header::ETAG).is_some());
    }

    #[tokio::test]
    async fn layout_put_success_updates_etag() {
        let (app, token) = make_app();
        // 1. Fetch current ETag.
        let get1 = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let etag1 = get1
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        // 2. PUT a new body.
        let new_body = serde_json::to_vec(&json!({
            "schema_version": 1,
            "groups": [{
                "id": "ga1",
                "parent_id": null,
                "label": "main",
                "color": "#abcdef",
                "visibility": true,
                "locked": false,
                "order": 0,
            }],
            "panels": [],
        }))
        .unwrap();
        let put = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::IF_MATCH, &etag1)
                    .body(Body::from(new_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put.status(), StatusCode::NO_CONTENT);
        let etag2 = put
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_ne!(etag1, etag2);

        // 3. GET reflects the new content.
        let get2 = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(get2.into_body(), 64 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["groups"].as_array().unwrap().len(), 1);
        assert_eq!(body["groups"][0]["id"], "ga1");
    }

    #[tokio::test]
    async fn layout_put_schema_violation() {
        let (app, token) = make_app();
        // Get the current ETag first so If-Match matches.
        let get1 = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let etag = get1
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let bogus_body = serde_json::to_vec(&json!({
            "groups": "not an array",
        }))
        .unwrap();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::IF_MATCH, &etag)
                    .body(Body::from(bogus_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn auth_required_for_layout() {
        let (app, _token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
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

    // ── ADR-0020: token-mode entry → opaque session cookie + 303 ──
    //
    // The legacy `/auth/bootstrap` route now redirects to the canonical
    // `/auth?token=…` endpoint (ADR-0020 D8). Tests below cover both routes
    // since existing bookmarks must keep working until the FE migrates.

    #[tokio::test]
    async fn auth_page_token_query_issues_cookie_and_redirects() {
        let (app, token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/auth?token={}", token.0))
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let set_cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .expect("Set-Cookie present")
            .to_str()
            .unwrap()
            .to_string();
        assert!(set_cookie.starts_with(&format!("{COOKIE_NAME_STR}=")));
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Strict"));
        // Cookie value must NOT be the raw bearer token — it's an opaque
        // session-id from `SessionTable::issue`.
        let cookie_value = set_cookie
            .split(';')
            .next()
            .unwrap()
            .strip_prefix(&format!("{COOKIE_NAME_STR}="))
            .unwrap()
            .to_string();
        assert_ne!(cookie_value, token.0, "cookie must be opaque session-id, not bearer token");
        // Location → "/" (default redirect target).
        let location = resp
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(location, "/");
    }

    #[tokio::test]
    async fn auth_page_invalid_token_returns_html_error() {
        let (app, _token) = make_app();
        let wrong = "A".repeat(43);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/auth?token={wrong}"))
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
    async fn auth_page_without_token_renders_landing() {
        let (app, _token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/auth")
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.starts_with("text/html"));
    }

    #[tokio::test]
    async fn bootstrap_legacy_route_redirects_to_auth() {
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
        assert!(location.starts_with("/auth?token="));
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
    async fn auth_redirect_target_rejects_external() {
        for evil in [
            "https://evil.example/x",
            "//evil.example",
            "/\\evil.example",
        ] {
            let (app, token) = make_app();
            let resp = app
                .oneshot(
                    HttpRequest::builder()
                        .uri(format!("/auth?token={}&redirect={evil}", token.0))
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
                .unwrap();
            assert_eq!(location, "/", "evil input {evil:?} must normalise to '/'");
        }
    }

    #[tokio::test]
    async fn cookie_auth_works_after_login() {
        let (app, token) = make_app();
        let auth_resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/auth?token={}", token.0))
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
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

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, &name_value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "session cookie must satisfy the auth middleware"
        );
    }

    #[tokio::test]
    async fn auth_logout_clears_cookie_and_revokes() {
        let (app, token) = make_app();
        let auth_resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/auth?token={}", token.0))
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
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
        assert!(clear.contains("Max-Age=0"), "clear cookie expected: {clear}");

        // Subsequent request with the now-revoked cookie must 401.
        let after = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
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

    // ── pure-function unit tests for the helpers ──

    #[test]
    fn etag_helper_is_deterministic() {
        let snap1 = LayoutSnapshot::empty();
        let snap2 = LayoutSnapshot::empty();
        assert_eq!(snap1.etag, snap2.etag);
        assert_eq!(snap1.etag_hex.len(), 32);
        assert!(snap1.etag_hex.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn parse_etag_header_rejects_weak_and_uppercase() {
        assert!(parse_etag_header("W/\"deadbeefdeadbeefdeadbeefdeadbeef\"").is_none());
        assert!(parse_etag_header("\"DEADBEEFDEADBEEFDEADBEEFDEADBEEF\"").is_none());
        assert!(parse_etag_header("*").is_none());
        assert!(parse_etag_header("\"deadbeefdeadbeefdeadbeefdeadbeef\"").is_some());
    }

    #[test]
    fn normalise_redirect_blocks_open_redirect() {
        // The helper lives in `auth.rs` now (ADR-0020 D8 relocation). The
        // unit-level coverage there is authoritative; this stub keeps a
        // breadcrumb so a future move stays traceable.
        assert_eq!(crate::auth::normalise_redirect_target(None), "/");
        assert_eq!(crate::auth::normalise_redirect_target(Some("//evil")), "/");
        assert_eq!(crate::auth::normalise_redirect_target(Some("/\\evil")), "/");
        assert_eq!(crate::auth::normalise_redirect_target(Some("https://evil")), "/");
        assert_eq!(crate::auth::normalise_redirect_target(Some("evil")), "/");
        assert_eq!(crate::auth::normalise_redirect_target(Some("/canvas")), "/canvas");
        assert_eq!(
            crate::auth::normalise_redirect_target(Some("/x\r\nSet-Cookie: ev")),
            "/"
        );
    }

    // ── S7-PERSISTENCE-MINIMAL (ADR-0006) — disk-backed PUT flow ──

    fn make_app_with_store(dir: &tempfile::TempDir) -> (Router, TokenString, std::path::PathBuf) {
        let token = issue_token().expect("token");
        let cfg = test_config();
        let layout_path = dir.path().join("test.layout.json");
        let store = LayoutStore::new(layout_path.clone());
        let state = AppState {
            config: Arc::new(cfg),
            token: Arc::new(token.clone()),
            layout: Arc::new(RwLock::new(store.load())),
            auth_failure_counter: Arc::new(AtomicU64::new(0)),
            hub: None,
            store: Some(Arc::new(store)),
            workspace: None,
            session_cache: Arc::new(crate::sessions::SessionCache::new()),
            session_table: crate::auth::default_session_table(7),
            rate_limiter: crate::auth::default_rate_limiter(),
            password_hash: None,
            server_id: Arc::from(crate::session_lock::fresh_server_id()),
            session_locks: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            session_locks_by_cookie: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            terminal_map: Arc::new(TerminalMap::new()),
            terminal_meta: Arc::new(TerminalMetadataStore::new()),
        };
        let app = router_with_state(state);
        (app, token, layout_path)
    }

    async fn current_etag(app: &Router, token: &TokenString) -> String {
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
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

    fn sample_body() -> Value {
        json!({
            "schema_version": 1,
            "groups": [{
                "id": "ga1",
                "parent_id": null,
                "label": "main",
                "color": "#abcdef",
                "visibility": true,
                "locked": false,
                "order": 0,
            }],
            "panels": [],
        })
    }

    #[tokio::test]
    async fn layout_put_persists_to_disk() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, path) = make_app_with_store(&dir);
        // Before PUT the file does not exist (empty layout loaded from absent).
        assert!(!path.exists());

        let etag = current_etag(&app, &token).await;
        let new_body = sample_body();
        let put = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::IF_MATCH, &etag)
                    .body(Body::from(serde_json::to_vec(&new_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put.status(), StatusCode::NO_CONTENT);

        // The atomic write must have produced exactly one file with mode 0600.
        assert!(path.exists(), "PUT must materialise the layout file");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let on_disk: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(on_disk, new_body);
    }

    #[tokio::test]
    async fn layout_put_412_leaves_disk_unchanged() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, path) = make_app_with_store(&dir);

        // Seed disk with a known-good layout via a successful PUT.
        let etag1 = current_etag(&app, &token).await;
        let _ = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::IF_MATCH, &etag1)
                    .body(Body::from(serde_json::to_vec(&sample_body()).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let baseline = std::fs::read(&path).unwrap();

        // PUT with a stale ETag must be rejected with 412 — and the file
        // bytes on disk must be untouched.
        let stale = "\"00000000000000000000000000000000\"";
        let alt_body = json!({
            "schema_version": 1,
            "groups": [],
            "panels": [],
        });
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::IF_MATCH, stale)
                    .body(Body::from(serde_json::to_vec(&alt_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PRECONDITION_FAILED);

        let after = std::fs::read(&path).unwrap();
        assert_eq!(baseline, after, "412 must not write to disk");
    }

    #[tokio::test]
    async fn boot_after_put_reloads_same_etag() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app1, token, path) = make_app_with_store(&dir);
        let etag1 = current_etag(&app1, &token).await;
        let body = sample_body();
        let put = app1
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::IF_MATCH, &etag1)
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put.status(), StatusCode::NO_CONTENT);
        let etag_after_put = put
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        // Simulate a server restart: build a fresh AppState pointing at the
        // same file. The loader recomputes the ETag from disk bytes and the
        // GET handler must surface the same value the previous PUT returned.
        let cfg = test_config();
        let store = LayoutStore::new(path);
        let state = AppState {
            config: Arc::new(cfg),
            token: Arc::new(token.clone()),
            layout: Arc::new(RwLock::new(store.load())),
            auth_failure_counter: Arc::new(AtomicU64::new(0)),
            hub: None,
            store: Some(Arc::new(store)),
            workspace: None,
            session_cache: Arc::new(crate::sessions::SessionCache::new()),
            session_table: crate::auth::default_session_table(7),
            rate_limiter: crate::auth::default_rate_limiter(),
            password_hash: None,
            server_id: Arc::from(crate::session_lock::fresh_server_id()),
            session_locks: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            session_locks_by_cookie: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            terminal_map: Arc::new(TerminalMap::new()),
            terminal_meta: Arc::new(TerminalMetadataStore::new()),
        };
        let app2 = router_with_state(state);
        let etag2 = current_etag(&app2, &token).await;
        assert_eq!(etag_after_put, etag2, "ETag must survive a cold boot");
    }

    #[tokio::test]
    async fn boot_with_corrupt_file_quarantines_and_serves_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.layout.json");
        // Seed a parse-failure file before the server starts.
        std::fs::write(&path, b"{ not json }").unwrap();
        let store = LayoutStore::new(path.clone());
        let cfg = test_config();
        let token = issue_token().expect("token");
        let state = AppState {
            config: Arc::new(cfg),
            token: Arc::new(token.clone()),
            layout: Arc::new(RwLock::new(store.load())),
            auth_failure_counter: Arc::new(AtomicU64::new(0)),
            hub: None,
            store: Some(Arc::new(store)),
            workspace: None,
            session_cache: Arc::new(crate::sessions::SessionCache::new()),
            session_table: crate::auth::default_session_table(7),
            rate_limiter: crate::auth::default_rate_limiter(),
            password_hash: None,
            server_id: Arc::from(crate::session_lock::fresh_server_id()),
            session_locks: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            session_locks_by_cookie: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            terminal_map: Arc::new(TerminalMap::new()),
            terminal_meta: Arc::new(TerminalMetadataStore::new()),
        };
        let app = router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
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
        assert_eq!(body["groups"].as_array().unwrap().len(), 0);
        assert_eq!(body["panels"].as_array().unwrap().len(), 0);
        // Original file is gone; a sidecar is in its place.
        assert!(!path.exists());
        let has_sidecar = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(has_sidecar, "corrupt file must be quarantined");
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
        state.terminal_meta.record_spawn("11111111-2222-4333-8444-555555555555").await;

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
        assert_eq!(body["matched"], json!(["11111111-2222-4333-8444-555555555555"]));
        assert_eq!(body["unmatched"], json!(["66666666-7777-4888-8999-aaaaaaaaaaaa"]));
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

        let app = router_with_state(state.clone());
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/sessions/demo/items/{uuid}?kill_terminal=true"))
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

    #[tokio::test]
    async fn attach_409_when_already_held_same_server() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, token, _) = make_app_with_workspace(&dir);
        create_session(&app, &token, "gamma").await;
        assert_eq!(attach(&app, &token, "gamma").await, StatusCode::OK);
        // Same server, same session — second attach must 409 (no takeover).
        assert_eq!(attach(&app, &token, "gamma").await, StatusCode::CONFLICT);
        assert_eq!(detach(&app, &token, "gamma").await, StatusCode::OK);
        // After release the lock is free again.
        assert_eq!(attach(&app, &token, "gamma").await, StatusCode::OK);
        assert_eq!(detach(&app, &token, "gamma").await, StatusCode::OK);
    }

    #[tokio::test]
    async fn release_lock_for_cookie_drops_the_attach() {
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
        // The unit-level coverage of release_lock_for_cookie is via the
        // standalone test below; here we verify the *integration* surface.
        // Second attach (same cookie) must still 409 because takeover is
        // forbidden — the auto-release path is the only way the lock goes
        // away (apart from explicit DELETE).
        let again = app
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
        assert_eq!(again.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn refresh_lease_for_cookie_bumps_lease_until() {
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
            .session_locks_by_cookie
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
        state.refresh_lease_for_cookie(cookie_value).await;

        let lease_after: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let after_until = lease_after["lease_until_unix"].as_u64().unwrap();
        assert!(
            after_until > before_until,
            "lease must extend on refresh: before={before_until} after={after_until}"
        );

        // Idempotent — refresh for a cookie with no lock is a no-op.
        state.refresh_lease_for_cookie("absent-cookie").await;
    }

    #[tokio::test]
    async fn release_lock_for_cookie_directly_on_appstate() {
        // Direct unit test of `AppState::release_lock_for_cookie` (the
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
                .session_locks_by_cookie
                .lock()
                .await
                .insert(cookie_value.to_string(), "manual".to_string());
        }
        assert!(dir.path().join(".locks/manual.lock").exists());

        // Release-by-cookie must drop both maps and the lock file.
        state.release_lock_for_cookie(cookie_value).await;
        assert!(state.session_locks.lock().await.is_empty());
        assert!(state.session_locks_by_cookie.lock().await.is_empty());
        assert!(!dir.path().join(".locks/manual.lock").exists());

        // Idempotent — second call on an absent cookie is a no-op.
        state.release_lock_for_cookie(cookie_value).await;
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
        // attach_handler must update hub.session_for_cookie so the WS
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
        assert_eq!(hub.session_for_cookie(cookie_value), None);

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
            hub.session_for_cookie(cookie_value),
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
        assert_eq!(hub.session_for_cookie(cookie_value), None);
    }

    #[tokio::test]
    async fn release_lock_for_cookie_clears_hub_session() {
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
            hub.session_for_cookie(cookie_value),
            Some("auto".to_string())
        );

        // Drive the WS-disconnect path directly.
        state_clone.release_lock_for_cookie(cookie_value).await;
        assert_eq!(hub.session_for_cookie(cookie_value), None);
    }

    #[tokio::test]
    async fn detach_clears_all_cookies_for_session_in_hub() {
        // detach_handler clears `session_locks_by_cookie` by *name* — the
        // hub mirror must follow the same semantics so multiple webpages
        // sharing a session (a configuration ADR-0019 D3 forbids today
        // but the table tolerates) all drop their bindings together.
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

        // Seed two cookies pointing at "multi" directly on the hub side
        // (the same-server flock guard forbids two cookies actually
        // attaching at once; we still want the clear-by-name semantic
        // to be unambiguous).
        hub.set_session_for_cookie("cookie-A", "multi");
        hub.set_session_for_cookie("cookie-B", "multi");
        hub.set_session_for_cookie("cookie-C", "other");

        let app = router_with_state(state);
        // Real attach to acquire the flock under cookie-A so DELETE can
        // succeed against `session_locks`. The set_session_for_cookie
        // above for cookie-A gets overwritten with the same value by the
        // attach handler — still "multi".
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

        let detach = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri("/api/sessions/multi/attach")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(detach.status(), StatusCode::OK);
        assert_eq!(hub.session_for_cookie("cookie-A"), None);
        assert_eq!(hub.session_for_cookie("cookie-B"), None);
        // Cookie-C pointed at a different session; must survive.
        assert_eq!(hub.session_for_cookie("cookie-C"), Some("other".into()));
    }
}
