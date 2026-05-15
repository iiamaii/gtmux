//! Session record I/O + HTTP handlers (`/api/sessions[/<name>[/layout]]`).
//!
//! Source-of-truth:
//! - `docs/adr/0019-session-and-workspace-model.md` D1, D5, D7, D10
//! - `docs/adr/0018-canvas-item-data-model.md` D1, D5, D6, D8
//! - `docs/adr/0006-persistence-storage.md` D3 (atomic write), D5 (ETag),
//!   D10 (D10 7-state table — borrowed for session record reads),
//!   D13 (write order: validate → new ETag → disk → memory → broadcast)
//!
//! Wire shape:
//! ```text
//!   GET    /api/sessions                     → 200 [{ name, active }]
//!   POST   /api/sessions   {name}            → 201 { name } | 409 already_exists | 400 invalid_name
//!   DELETE /api/sessions/:name               → 204
//!   GET    /api/sessions/:name/layout        → 200 Layout JSON + ETag
//!   PUT    /api/sessions/:name/layout        → 204 + ETag (If-Match required, 412 on stale)
//! ```
//!
//! Concurrency: single in-memory snapshot per session is held in
//! [`AppState::session_layouts`]. The first read of a session lazily loads
//! and caches; PUT does compare-and-swap on the cached ETag under a write
//! lock, exactly like `/api/layout` does for the legacy v1 store.

// Public fields on `SessionLayout` / `SessionError` variants are documented at
// the type/variant level. Suppress per-field missing-docs to keep the wire
// types compact (and consistent with `schema.rs`).
#![allow(missing_docs)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::http::{header, HeaderMap, HeaderValue, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use ring::digest::{digest, SHA256};
use serde::Deserialize;
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::schema::{self, Layout};
use crate::workspace::{
    atomic_write_session, validate_session_name, WorkspaceError, WorkspaceManager,
};

/// Soft cap on session-record PUT bodies. Matches ADR-0018 D8 §"전체 file size
/// cap: 16 MB (P0)" — the existing legacy `/api/layout` cap (256 KiB) is too
/// tight for the v2 schema once free-draw / images land. We still enforce a
/// per-field validation (4 KiB label / 64 KiB text) inside `schema::validate`.
const SESSION_PUT_MAX_BYTES: usize = 16 * 1024 * 1024;

// ─────────────────────────────────────────────────────────────────────────────
//  SessionLayout — in-memory cache entry
// ─────────────────────────────────────────────────────────────────────────────

/// One in-memory session record (parsed + canonicalised) and its ETag.
#[derive(Debug, Clone)]
pub struct SessionLayout {
    pub etag: [u8; 16],
    pub etag_hex: String,
    pub layout: Layout,
}

impl SessionLayout {
    pub fn new(layout: Layout) -> Self {
        let bytes = canonical_bytes(&layout);
        let (etag, etag_hex) = sha256_128(&bytes);
        Self {
            etag,
            etag_hex,
            layout,
        }
    }

    fn from_disk(layout: Layout) -> Self {
        Self::new(layout)
    }
}

fn canonical_bytes(layout: &Layout) -> Vec<u8> {
    serde_json::to_vec(layout).expect("Layout is always JSON-serialisable")
}

fn sha256_128(bytes: &[u8]) -> ([u8; 16], String) {
    let d = digest(&SHA256, bytes);
    let full = d.as_ref();
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&full[..16]);
    let mut hex = String::with_capacity(32);
    for b in raw.iter() {
        hex.push_str(&format!("{b:02x}"));
    }
    (raw, hex)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Cache — keyed by session name
// ─────────────────────────────────────────────────────────────────────────────

/// In-memory cache of loaded session layouts, keyed by session name.
/// Per-session `RwLock` is acquired only briefly for read or CAS PUT; the
/// outer cache map uses a single `RwLock` for the rare insert path.
#[derive(Default, Debug)]
pub struct SessionCache {
    entries: RwLock<HashMap<String, Arc<RwLock<SessionLayout>>>>,
}

impl SessionCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fetch or lazily load the cached layout for `name`.
    pub async fn get_or_load(
        &self,
        wm: &WorkspaceManager,
        name: &str,
    ) -> Result<Arc<RwLock<SessionLayout>>, SessionError> {
        {
            let read = self.entries.read().await;
            if let Some(arc) = read.get(name) {
                return Ok(Arc::clone(arc));
            }
        }
        // Slow path — load from disk, then insert. Re-check under the write
        // lock in case another task raced us.
        let loaded = load_from_disk(wm, name)?;
        let arc = Arc::new(RwLock::new(loaded));
        let mut write = self.entries.write().await;
        if let Some(existing) = write.get(name) {
            return Ok(Arc::clone(existing));
        }
        write.insert(name.to_string(), Arc::clone(&arc));
        Ok(arc)
    }

    /// Drop a session from the cache (used by DELETE).
    pub async fn evict(&self, name: &str) {
        let mut write = self.entries.write().await;
        write.remove(name);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Disk I/O
// ─────────────────────────────────────────────────────────────────────────────

/// Load a session record from disk into the cache shape. Missing file is a
/// hard error (handler maps to 404) so the cache never holds a "phantom"
/// empty layout for a name that the user has not created via `POST`.
fn load_from_disk(wm: &WorkspaceManager, name: &str) -> Result<SessionLayout, SessionError> {
    let path = wm.session_path(name)?;
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(SessionError::NotFound(name.to_string()));
        }
        Err(e) => return Err(SessionError::Io(e)),
    };
    let layout: Layout = match serde_json::from_slice::<Layout>(&bytes) {
        Ok(l) => l,
        Err(e) => {
            // Treat as corrupt — quarantine and bubble up as 500 so the
            // operator sees the sidecar in the workspace dir.
            sidecar_quarantine(&path, "session-parse-fail");
            return Err(SessionError::Corrupt(format!(
                "{}: {e}",
                path.display()
            )));
        }
    };
    if let Err(e) = schema::validate(&layout) {
        sidecar_quarantine(&path, e.code());
        return Err(SessionError::Corrupt(format!(
            "{} failed validation: {e}",
            path.display()
        )));
    }
    Ok(SessionLayout::from_disk(layout))
}

fn sidecar_quarantine(path: &Path, reason: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut filename = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    filename.push(format!(".corrupt-{ts}"));
    let sidecar = match path.parent() {
        Some(d) => d.join(filename),
        None => std::path::PathBuf::from(filename),
    };
    if let Err(e) = std::fs::rename(path, &sidecar) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                original = %path.display(),
                error = %e,
                reason,
                "sessions: quarantine rename failed",
            );
        }
    } else {
        tracing::error!(
            original = %path.display(),
            quarantine = %sidecar.display(),
            reason,
            "sessions: corrupt session record quarantined"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Errors
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session not found: {0:?}")]
    NotFound(String),
    #[error("session already exists: {0:?}")]
    AlreadyExists(String),
    #[error("validation: {0}")]
    Validation(#[from] schema::ValidationError),
    #[error("workspace: {0}")]
    Workspace(#[from] WorkspaceError),
    #[error("corrupt session record: {0}")]
    Corrupt(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("payload too large")]
    PayloadTooLarge,
    #[error("If-Match required")]
    PreconditionRequired,
    #[error("If-Match mismatch")]
    PreconditionFailed,
    #[error("bad json: {0}")]
    BadJson(String),
}

impl SessionError {
    fn status(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::AlreadyExists(_) => StatusCode::CONFLICT,
            Self::Validation(_) | Self::BadJson(_) => StatusCode::BAD_REQUEST,
            Self::Workspace(WorkspaceError::InvalidSessionName(_)) => StatusCode::BAD_REQUEST,
            Self::Workspace(_) | Self::Io(_) | Self::Corrupt(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::PreconditionRequired => StatusCode::PRECONDITION_REQUIRED,
            Self::PreconditionFailed => StatusCode::PRECONDITION_FAILED,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "session_not_found",
            Self::AlreadyExists(_) => "session_already_exists",
            Self::Validation(v) => v.code(),
            Self::Workspace(WorkspaceError::InvalidSessionName(_)) => "invalid_session_name",
            Self::Workspace(_) => "workspace_error",
            Self::Io(_) => "io_error",
            Self::Corrupt(_) => "session_corrupt",
            Self::PayloadTooLarge => "payload_too_large",
            Self::PreconditionRequired => "precondition_required",
            Self::PreconditionFailed => "precondition_failed",
            Self::BadJson(_) => "bad_request",
        }
    }
}

impl IntoResponse for SessionError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": self.code(),
            "message": self.to_string(),
        });
        (self.status(), Json(body)).into_response()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// `GET /api/sessions` — list workspace records. The `active` flag is
/// resolved by peeking the cross-server flock at `<workspace>/.locks/<name>.lock`
/// (ADR-0019 D6.3): EWOULDBLOCK on LOCK_SH ⇒ in use; LOCK_SH success ⇒ stale.
pub async fn list_handler(State(state): State<crate::AppState>) -> Response {
    let Some(wm) = state.workspace.as_ref() else {
        return service_unavailable("workspace_not_configured");
    };
    let infos = match wm.enumerate_sessions() {
        Ok(v) => v,
        Err(e) => return SessionError::Workspace(e).into_response(),
    };
    let locks_dir = wm.locks_dir();
    let body: Vec<Value> = infos
        .into_iter()
        .map(|i| {
            let active = matches!(
                crate::session_lock::peek(&locks_dir, &i.name),
                crate::session_lock::LockState::InUse(_)
                    | crate::session_lock::LockState::InUseRaceyBody
            );
            json!({ "name": i.name, "active": active })
        })
        .collect();
    Json(body).into_response()
}

/// `POST /api/sessions/:name/attach` — acquire the cross-server lock for
/// the session and bind it to the caller's auth cookie. Returns:
///   * 200 + `{ name }` on first-attach success
///   * 409 + body describing the holder on conflict (ADR-0019 D4 — no
///     takeover; the modal renders the row as disabled)
///   * 404 if the session record does not exist
///   * 503 when no workspace is configured
///
/// ADR-0021 D6 heartbeat is not yet plumbed; the lock survives until the
/// cookie's session is revoked (logout / token rotation) or the server
/// process exits.
pub async fn attach_handler(
    State(state): State<crate::AppState>,
    AxumPath(name): AxumPath<String>,
    req: Request<Body>,
) -> Response {
    let Some(wm) = state.workspace.as_ref() else {
        return service_unavailable("workspace_not_configured");
    };

    // Validate the name once up-front so a bad path doesn't survive
    // through the lock-acquire blocking call.
    if let Err(e) = crate::workspace::validate_session_name(&name) {
        return SessionError::Workspace(e).into_response();
    }
    let session_path = match wm.session_path(&name) {
        Ok(p) => p,
        Err(e) => return SessionError::Workspace(e).into_response(),
    };
    if !session_path.exists() {
        return SessionError::NotFound(name).into_response();
    }

    // Tie the lock to the caller's cookie so a subsequent /detach from the
    // same browser tab can release exactly this lock without ambient
    // ambiguity.
    let cookie = crate::auth::extract_session_cookie(req.headers())
        .unwrap_or_else(|| "_unknown".to_string());

    // ADR-0019 D3 single-attach invariant — implicit detach-on-reattach.
    // If this cookie already holds a *different* session's lock (e.g. user
    // switched session in WorkspaceSwitcher without an explicit DELETE
    // attach), release it before acquiring the new one. Without this,
    // the previous session stays `active=true` indefinitely (the cookie's
    // reverse-map entry would simply be overwritten further down and the
    // old flock would leak in `session_locks` until process exit).
    //
    // Same-name reattach is an idempotent no-op — the cleanup branch is
    // skipped and the rest of this handler short-circuits via the
    // `holders.contains_key(&name)` check immediately below.
    let previous_session: Option<String> = {
        let by_cookie = state.session_locks_by_cookie.lock().await;
        by_cookie
            .get(&cookie)
            .filter(|prev| prev.as_str() != name)
            .cloned()
    };
    if let Some(prev_name) = previous_session {
        let mut by_cookie = state.session_locks_by_cookie.lock().await;
        by_cookie.remove(&cookie);
        drop(by_cookie);
        let mut holders = state.session_locks.lock().await;
        if let Some(mut guard) = holders.remove(&prev_name) {
            tracing::info!(
                cookie_len = cookie.len(),
                prev_session = %prev_name,
                next_session = %name,
                "session_lock: implicit detach on cookie switch"
            );
            guard.release();
        }
        drop(holders);
        if let Some(hub) = state.hub.as_ref() {
            hub.clear_session_for_cookie(&cookie);
        }
    }

    // Same-server serialisation (D6.6) — only one attach attempt at a time
    // per session name from *this* process.
    let mut holders = state.session_locks.lock().await;
    if holders.contains_key(&name) {
        // Even from the same server, takeover is forbidden.
        return lock_conflict_response(&state, wm, &name);
    }

    let locks_dir = wm.locks_dir();
    let server_id = state.server_id.clone();
    let ws_conn_id = cookie.clone();
    let name_for_block = name.clone();
    let acquired = tokio::task::spawn_blocking(move || {
        crate::session_lock::acquire(&locks_dir, &name_for_block, server_id, &ws_conn_id)
    })
    .await;

    let guard = match acquired {
        Ok(Ok(g)) => g,
        Ok(Err(crate::session_lock::LockError::Contended)) => {
            return lock_conflict_response(&state, wm, &name);
        }
        Ok(Err(e)) => {
            tracing::error!(name, error = %e, "sessions: lock acquire failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "lock_failed", "message": e.to_string() })),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(name, error = %e, "sessions: lock task panicked");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "lock_failed", "message": "lock task failed" })),
            )
                .into_response();
        }
    };

    holders.insert(name.clone(), guard);
    // Reverse-index by cookie so a WS disconnect can find this lock to
    // release. `_unknown` (anonymous) attaches are still recorded — they
    // simply won't be auto-released since the WS will not present the
    // missing cookie.
    {
        let mut by_cookie = state.session_locks_by_cookie.lock().await;
        // If the same cookie had a stale entry for a previous attach (e.g.
        // the FE retried after a transient error), the prior session lock
        // would have been released already by the path that took it out
        // of `session_locks` — but the reverse map could lag. Overwriting
        // here is the safer choice.
        by_cookie.insert(cookie.clone(), name.clone());
    }
    // Stage 5-A: mirror the cookie ↔ session binding into the WS hub so the
    // dispatcher (5-C) can route session-scoped envelopes only to the
    // matching webpage. Skip when no hub is wired (unit-test paths).
    if let Some(hub) = state.hub.as_ref() {
        hub.set_session_for_cookie(&cookie, &name);
    }
    // Drop the same-server serialisation lock before doing layout I/O — the
    // attach itself is committed by the flock acquire above; further work
    // is a per-cookie read of the session record and a non-mutating scan
    // against `terminal_map`.
    drop(holders);

    // Match-or-spawn classification (ADR-0018 D6 read half). The FE uses
    // `unmatched` to decide whether to render the confirm modal; spawning
    // happens in a follow-up `POST /attach/confirm`.
    let (matched, unmatched) = match classify_layout_terminals(&state, wm, &name).await {
        Ok(pair) => pair,
        Err(e) => {
            // The flock is already held — release it so the user can retry
            // after fixing the underlying corrupt/missing file.
            release_attach(&state, &name, &cookie).await;
            return e.into_response();
        }
    };

    (
        StatusCode::OK,
        Json(json!({
            "name": name,
            "attached": true,
            "server_id": &*state.server_id,
            "matched": matched,
            "unmatched": unmatched,
        })),
    )
        .into_response()
}

/// `POST /api/sessions/:name/attach/confirm` — spawn fresh terminals for
/// every unmatched UUID in the session layout (ADR-0018 D6 *fresh spawn*
/// arm). Idempotent: re-running after all spawns complete returns
/// `spawned: []` with the same UUIDs in `already_present`. The caller must
/// already hold the session attach (cookie ↔ name binding from
/// `POST /attach`); otherwise responds 403.
pub async fn attach_confirm_handler(
    State(state): State<crate::AppState>,
    AxumPath(name): AxumPath<String>,
    req: Request<Body>,
) -> Response {
    let Some(wm) = state.workspace.as_ref() else {
        return service_unavailable("workspace_not_configured");
    };
    if let Err(e) = validate_session_name(&name) {
        return SessionError::Workspace(e).into_response();
    }
    if state.hub.is_none() {
        return service_unavailable("hub_not_configured");
    }

    let cookie = crate::auth::extract_session_cookie(req.headers())
        .unwrap_or_else(|| "_unknown".to_string());

    {
        let by_cookie = state.session_locks_by_cookie.lock().await;
        let owns = matches!(by_cookie.get(&cookie), Some(n) if n == &name);
        if !owns {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "not_attached",
                    "message": "cookie does not hold an attach for this session",
                })),
            )
                .into_response();
        }
    }

    let uuids = match load_terminal_uuids(&state, wm, &name).await {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };

    let mut spawned: Vec<String> = Vec::new();
    let mut already_present: Vec<String> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    for uuid in uuids {
        if state.terminal_map.lookup_pane(&uuid).await.is_some() {
            already_present.push(uuid);
            continue;
        }
        match state.spawn_terminal_with_uuid(uuid.clone()).await {
            Ok(_) => spawned.push(uuid),
            Err(e) => {
                failed.push(json!({
                    "id": uuid,
                    "error": e.to_string(),
                }));
            }
        }
    }

    // Stage 5-D path P1: hint other sessions' webpages that the alive pool
    // has grown. The trigger session itself is skipped at the WS
    // dispatcher (its layout already references the spawned UUIDs, so a
    // refresh would be redundant). Only publish when a spawn actually
    // landed — the empty case would create a wakeup for every subscriber
    // to no purpose.
    if !spawned.is_empty() {
        if let Some(hub) = state.hub.as_ref() {
            hub.publish_terminal_list_change(&name, &spawned, &[]);
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "name": name,
            "spawned": spawned,
            "already_present": already_present,
            "failed": failed,
        })),
    )
        .into_response()
}

/// `POST /api/sessions/:name/terminals` — Stage 5-D path P2. Create a
/// fresh terminal *initiated by the user* (the `[New Terminal]` button
/// path). The handler:
///   1. checks the caller's cookie still holds this session's flock
///      (403 `not_attached` otherwise — same policy as
///      `attach_confirm_handler`)
///   2. computes a default cascade coordinate from the current layout
///   3. mints a fresh UUID and calls
///      [`crate::AppState::spawn_terminal_with_uuid`] (this publishes the
///      `0x88 TERMINAL_SPAWNED` UUID↔PaneId binding to every WS)
///   4. publishes `0x86 MOUNT_CASCADE` so the *trigger session's* webpage
///      appends a fresh `TerminalItem` at the computed coordinates
///   5. publishes `0x87 TERMINAL_LIST_UPDATE` so *other sessions'*
///      webpages refresh their Terminal-list sidebar
///   6. returns 200 `{ terminal_id, pane_id, x, y, w, h }` so the caller
///      can correlate with the inbound WS frames if needed
///
/// **Persistence is FE-side.** The BE does not write the new item into
/// the session layout — the FE's `handleMountCascade` calls
/// `mutateLayout` which round-trips through `PUT /api/sessions/:name/layout`.
/// See `docs/reports/0037-backend-review-action-items.md` §6.4 for the
/// rationale (race-free for the common single-tab case; multi-tab race
/// is the same window as any concurrent PUT).
///
/// Body: ignored for MVP (`{}` is fine). A future amend can accept
/// `{label, x, y, w, h}` overrides; for now BE picks default coords.
pub async fn create_terminal_handler(
    State(state): State<crate::AppState>,
    AxumPath(name): AxumPath<String>,
    req: Request<Body>,
) -> Response {
    let Some(wm) = state.workspace.as_ref() else {
        return service_unavailable("workspace_not_configured");
    };
    if let Err(e) = validate_session_name(&name) {
        return SessionError::Workspace(e).into_response();
    }
    if state.hub.is_none() {
        return service_unavailable("hub_not_configured");
    }

    let cookie = crate::auth::extract_session_cookie(req.headers())
        .unwrap_or_else(|| "_unknown".to_string());
    {
        let by_cookie = state.session_locks_by_cookie.lock().await;
        let owns = matches!(by_cookie.get(&cookie), Some(n) if n == &name);
        if !owns {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "not_attached",
                    "message": "cookie does not hold an attach for this session",
                })),
            )
                .into_response();
        }
    }

    let (x, y, w, h) = match next_mount_cascade_coords(&state, wm, &name).await {
        Ok(coords) => coords,
        Err(e) => return e.into_response(),
    };

    let uuid = crate::terminal_map::fresh_terminal_uuid();
    let pane = match state.spawn_terminal_with_uuid(uuid.clone()).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(
                error = %e,
                "create_terminal: spawn_terminal_with_uuid failed"
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "spawn_failed", "message": e.to_string() })),
            )
                .into_response();
        }
    };

    // hub.is_none() guarded above; hub is present here.
    if let Some(hub) = state.hub.as_ref() {
        hub.publish_mount_cascade(gtmux_ws_server::MountCascadeEvent {
            trigger_session: std::sync::Arc::from(name.as_str()),
            terminal_id: std::sync::Arc::from(uuid.as_str()),
            x,
            y,
            w,
            h,
        });
        hub.publish_terminal_list_change(&name, &[uuid.clone()], &[]);
    }

    (
        StatusCode::OK,
        Json(json!({
            "terminal_id": uuid,
            "pane_id": pane.0,
            "x": x,
            "y": y,
            "w": w,
            "h": h,
        })),
    )
        .into_response()
}

/// Default coordinate policy for Stage 5-D P2 `[New Terminal]` spawns.
/// Empty layout → `(80, 80, 720, 420)`. Otherwise the bottom-right-most
/// existing terminal's `(x, y)` plus a `+32` cascade offset, with the
/// default `w/h`. Race window: if two `POST /terminals` calls land
/// concurrently before either FE has PUT the resulting layout, they may
/// pick the same coordinates — the user can move the panel. Tracking
/// that race in BE would require a write lock on the session_cache for
/// the entire spawn+publish path, which is not worth the complexity at
/// MVP scale (single-attach makes the race essentially impossible in
/// practice).
async fn next_mount_cascade_coords(
    state: &crate::AppState,
    wm: &Arc<crate::WorkspaceManager>,
    name: &str,
) -> Result<(f64, f64, f64, f64), SessionError> {
    const DEFAULT_W: f64 = 720.0;
    const DEFAULT_H: f64 = 420.0;
    const CASCADE_OFFSET: f64 = 32.0;
    const FALLBACK_X: f64 = 80.0;
    const FALLBACK_Y: f64 = 80.0;

    let entry = state.session_cache.get_or_load(wm.as_ref(), name).await?;
    let guard = entry.read().await;
    let mut max_x = 0.0_f64;
    let mut max_y = 0.0_f64;
    let mut any = false;
    for item in &guard.layout.items {
        if let crate::schema::Item::Terminal { common } = item {
            if !any {
                max_x = common.x;
                max_y = common.y;
                any = true;
            } else {
                max_x = max_x.max(common.x);
                max_y = max_y.max(common.y);
            }
        }
    }
    if any {
        Ok((
            max_x + CASCADE_OFFSET,
            max_y + CASCADE_OFFSET,
            DEFAULT_W,
            DEFAULT_H,
        ))
    } else {
        Ok((FALLBACK_X, FALLBACK_Y, DEFAULT_W, DEFAULT_H))
    }
}

async fn classify_layout_terminals(
    state: &crate::AppState,
    wm: &Arc<crate::WorkspaceManager>,
    name: &str,
) -> Result<(Vec<String>, Vec<String>), SessionError> {
    let uuids = load_terminal_uuids(state, wm, name).await?;
    let mut matched = Vec::new();
    let mut unmatched = Vec::new();
    for uuid in uuids {
        if state.terminal_map.lookup_pane(&uuid).await.is_some() {
            matched.push(uuid);
        } else {
            unmatched.push(uuid);
        }
    }
    Ok((matched, unmatched))
}

async fn load_terminal_uuids(
    state: &crate::AppState,
    wm: &Arc<crate::WorkspaceManager>,
    name: &str,
) -> Result<Vec<String>, SessionError> {
    let entry = state.session_cache.get_or_load(wm.as_ref(), name).await?;
    let guard = entry.read().await;
    Ok(guard
        .layout
        .items
        .iter()
        .filter_map(|i| match i {
            crate::schema::Item::Terminal { common } => Some(common.id.clone()),
            _ => None,
        })
        .collect())
}

async fn release_attach(state: &crate::AppState, name: &str, cookie: &str) {
    let mut holders = state.session_locks.lock().await;
    if let Some(mut guard) = holders.remove(name) {
        guard.release();
    }
    let mut by_cookie = state.session_locks_by_cookie.lock().await;
    if matches!(by_cookie.get(cookie), Some(v) if v == name) {
        by_cookie.remove(cookie);
    }
    // Stage 5-A: keep the WS hub's cookie ↔ session_name map in lock-step
    // with the http-api reverse-index. The hub method is a no-op on missing
    // entries, so a failed-attach cleanup path that never wrote anything
    // here is still safe.
    if let Some(hub) = state.hub.as_ref() {
        hub.clear_session_for_cookie(cookie);
    }
}

/// `DELETE /api/sessions/:name/attach` — release the lock held by this
/// server for `name`. Idempotent — releasing a vacant slot is a 200.
pub async fn detach_handler(
    State(state): State<crate::AppState>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    {
        let mut holders = state.session_locks.lock().await;
        if let Some(mut guard) = holders.remove(&name) {
            guard.release();
        }
    }
    // Prune the reverse map so a later WS-close for the same cookie
    // doesn't try to re-release. The cookie key may map to a stale name
    // if the FE re-attached to a different session between detach and
    // close — in that case we leave the new mapping alone.
    {
        let mut by_cookie = state.session_locks_by_cookie.lock().await;
        by_cookie.retain(|_, v| v != &name);
    }
    // Stage 5-A: mirror the prune into the WS hub so the dispatcher does
    // not keep routing session-scoped envelopes to cookies that just
    // detached from this session.
    if let Some(hub) = state.hub.as_ref() {
        hub.clear_sessions_by_name(&name);
    }
    (StatusCode::OK, Json(json!({ "name": name, "released": true }))).into_response()
}

fn lock_conflict_response(
    state: &crate::AppState,
    wm: &crate::workspace::WorkspaceManager,
    name: &str,
) -> Response {
    let locks_dir = wm.locks_dir();
    let holder = match crate::session_lock::peek(&locks_dir, name) {
        crate::session_lock::LockState::InUse(lease) => Some(lease),
        _ => None,
    };
    let body = match holder {
        Some(l) => json!({
            "error": "session_in_use",
            "message": format!("session '{name}' is held by another webpage"),
            "holder": {
                "server_id": l.server_id,
                "pid": l.pid,
                "lease_until_unix": l.lease_until_unix,
            },
            "this_server_id": &*state.server_id,
        }),
        None => json!({
            "error": "session_in_use",
            "message": format!("session '{name}' is held by another webpage"),
            "this_server_id": &*state.server_id,
        }),
    };
    (StatusCode::CONFLICT, Json(body)).into_response()
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionBody {
    pub name: String,
}

/// `POST /api/sessions { name }` — create an empty v2 record.
pub async fn create_handler(
    State(state): State<crate::AppState>,
    Json(body): Json<CreateSessionBody>,
) -> Response {
    let Some(wm) = state.workspace.as_ref() else {
        return service_unavailable("workspace_not_configured");
    };
    if let Err(e) = validate_session_name(&body.name) {
        return SessionError::Workspace(e).into_response();
    }
    let path = match wm.session_path(&body.name) {
        Ok(p) => p,
        Err(e) => return SessionError::Workspace(e).into_response(),
    };
    if path.exists() {
        return SessionError::AlreadyExists(body.name).into_response();
    }
    let layout = Layout::empty();
    let bytes = canonical_bytes(&layout);
    if let Err(e) = atomic_write_session(&path, &bytes) {
        return SessionError::Workspace(e).into_response();
    }
    // Seed the cache so the first GET /layout doesn't bounce back to disk.
    let cached = SessionLayout::new(layout);
    {
        let mut write = state.session_cache.entries.write().await;
        write.insert(body.name.clone(), Arc::new(RwLock::new(cached)));
    }
    let resp = (
        StatusCode::CREATED,
        Json(json!({ "name": body.name })),
    );
    resp.into_response()
}

/// `DELETE /api/sessions/:name` — unlink the record from disk and evict the
/// cache. ADR-0019 D10: terminal cascade-kill is *not* this handler's job —
/// we touch session storage only.
pub async fn delete_handler(
    State(state): State<crate::AppState>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let Some(wm) = state.workspace.as_ref() else {
        return service_unavailable("workspace_not_configured");
    };
    let path = match wm.session_path(&name) {
        Ok(p) => p,
        Err(e) => return SessionError::Workspace(e).into_response(),
    };
    match std::fs::remove_file(&path) {
        Ok(()) => {
            state.session_cache.evict(&name).await;
            (StatusCode::NO_CONTENT, ()).into_response()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            SessionError::NotFound(name).into_response()
        }
        Err(e) => SessionError::Io(e).into_response(),
    }
}

/// Query parameters for [`delete_item_handler`].
#[derive(Debug, Default, Deserialize)]
pub struct DeleteItemQuery {
    /// When `true` and the removed item is a terminal panel, also SIGTERM
    /// the underlying Terminal in the PTY backend (ADR-0021 D9.2
    /// `[Panel + Terminal]` option). Defaults to `false` so the user keeps
    /// the panel-only safety semantics unless they opt in.
    #[serde(default)]
    pub kill_terminal: bool,
}

/// `DELETE /api/sessions/:name/items/:id[?kill_terminal=true]` — remove a
/// single Canvas Item from the session layout (ADR-0021 D9.2). When the
/// removed item is a terminal panel and `kill_terminal=true`, the matching
/// backend Terminal is SIGTERM'd and dropped from the [`crate::TerminalMap`]
/// + metadata store. Returns 204 + the new ETag on success, 404 when the
/// item id is not present in the layout.
pub async fn delete_item_handler(
    State(state): State<crate::AppState>,
    AxumPath((name, id)): AxumPath<(String, String)>,
    axum::extract::Query(q): axum::extract::Query<DeleteItemQuery>,
) -> Response {
    let Some(wm) = state.workspace.as_ref() else {
        return service_unavailable("workspace_not_configured");
    };
    if let Err(e) = validate_session_name(&name) {
        return SessionError::Workspace(e).into_response();
    }
    let arc = match state.session_cache.get_or_load(wm, &name).await {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };

    let removed_terminal_id: Option<String>;
    let new_etag_quoted: String;
    {
        let mut snap = arc.write().await;
        let before = snap.layout.items.len();
        let removed_uuid = snap
            .layout
            .items
            .iter()
            .find_map(|item| match item {
                crate::schema::Item::Terminal { common } if common.id == id => {
                    Some(common.id.clone())
                }
                _ => None,
            });
        snap.layout.items.retain(|item| item_id(item) != id);
        if snap.layout.items.len() == before {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "item_not_found",
                    "message": format!("item '{id}' is not in session '{name}'"),
                })),
            )
                .into_response();
        }
        let new_snap = SessionLayout::new(snap.layout.clone());
        let path = match wm.session_path(&name) {
            Ok(p) => p,
            Err(e) => return SessionError::Workspace(e).into_response(),
        };
        let bytes = canonical_bytes(&new_snap.layout);
        if let Err(e) = atomic_write_session(&path, &bytes) {
            return SessionError::Workspace(e).into_response();
        }
        new_etag_quoted = format!("\"{}\"", new_snap.etag_hex);
        *snap = new_snap;
        removed_terminal_id = removed_uuid;
    }

    if q.kill_terminal {
        if let Some(uuid) = removed_terminal_id {
            kill_and_unregister_terminal(&state, &uuid).await;
            // The schema item is gone for good — drop metadata too so
            // `GET /api/terminals` does not surface a phantom row.
            state.terminal_meta.forget(&uuid).await;
        }
    }

    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::ETAG, &new_etag_quoted)
        .body(Body::empty())
        .expect("static headers")
}

fn item_id(item: &crate::schema::Item) -> &str {
    use crate::schema::Item;
    match item {
        Item::Terminal { common } => &common.id,
        Item::Text { common, .. } => &common.id,
        Item::Note { common, .. } => &common.id,
        Item::Rect { common, .. } => &common.id,
        Item::Ellipse { common, .. } => &common.id,
        Item::Line { common, .. } => &common.id,
        Item::FreeDraw { common, .. } => &common.id,
        Item::Image { common, .. } => &common.id,
        Item::Document { common, .. } => &common.id,
        Item::FilePath { common, .. } => &common.id,
    }
}

/// Best-effort: SIGTERM the Terminal bound to `uuid` and unregister it
/// from the bridge map. The metadata store is **not** touched here so
/// `created_at` and `label` survive a transient kill → respawn cycle
/// (ADR-0021 D10.1 lazy fresh-spawn). Callers that intend to forget the
/// UUID for good (DELETE item with `kill_terminal=true`, explicit
/// `POST /api/terminals/:id/kill`) follow this call with
/// `state.terminal_meta.forget(uuid)`. Idempotent — a UUID that's not in
/// the map is a no-op; a kill that fails because the Pane is already
/// dead just logs at debug.
pub(crate) async fn kill_and_unregister_terminal(state: &crate::AppState, uuid: &str) {
    let pane = match state.terminal_map.lookup_pane(uuid).await {
        Some(p) => p,
        None => return,
    };
    if let Some(hub) = state.hub.as_ref() {
        if let Err(e) = hub.backend().kill(pane) {
            tracing::debug!(
                uuid,
                pane = ?pane,
                error = %e,
                "terminal: kill returned non-fatal error (likely already dead)"
            );
        }
    }
    state.terminal_map.unregister_uuid(uuid).await;
}

/// `GET /api/sessions/:name/layout` — current snapshot + ETag.
pub async fn layout_get_handler(
    State(state): State<crate::AppState>,
    AxumPath(name): AxumPath<String>,
    req: Request<Body>,
) -> Response {
    let Some(wm) = state.workspace.as_ref() else {
        return service_unavailable("workspace_not_configured");
    };
    let arc = match state.session_cache.get_or_load(wm, &name).await {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    let snap = arc.read().await;
    let etag_quoted = format!("\"{}\"", snap.etag_hex);
    if let Some(if_none_match) = req.headers().get(header::IF_NONE_MATCH) {
        if let Ok(v) = if_none_match.to_str() {
            if parse_etag_header(v).is_some_and(|h| h == snap.etag_hex) {
                return Response::builder()
                    .status(StatusCode::NOT_MODIFIED)
                    .header(header::ETAG, &etag_quoted)
                    .body(Body::empty())
                    .expect("static headers");
            }
        }
    }
    let body = canonical_bytes(&snap.layout);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ETAG, &etag_quoted)
        .body(Body::from(body))
        .expect("static headers")
}

/// `PUT /api/sessions/:name/layout` — atomic compare-and-swap on ETag.
pub async fn layout_put_handler(
    State(state): State<crate::AppState>,
    AxumPath(name): AxumPath<String>,
    req: Request<Body>,
) -> Response {
    let Some(wm) = state.workspace.as_ref() else {
        return service_unavailable("workspace_not_configured");
    };
    // 1. If-Match — required (ADR-0006 D5).
    let if_match = match req.headers().get(header::IF_MATCH) {
        Some(v) => match v.to_str() {
            Ok(s) => match parse_etag_header(s) {
                Some(parsed) => parsed,
                None => return SessionError::PreconditionRequired.into_response(),
            },
            Err(_) => return SessionError::PreconditionRequired.into_response(),
        },
        None => return SessionError::PreconditionRequired.into_response(),
    };

    // 2. Read body.
    let body_bytes = match read_bounded_body(req, SESSION_PUT_MAX_BYTES).await {
        Ok(b) => b,
        Err(BodyReadError::TooLarge) => return SessionError::PayloadTooLarge.into_response(),
        Err(BodyReadError::Io(msg)) => return SessionError::BadJson(msg).into_response(),
    };

    // 3. Parse + validate.
    let layout: Layout = match serde_json::from_slice::<Layout>(&body_bytes) {
        Ok(l) => l,
        Err(e) => return SessionError::BadJson(e.to_string()).into_response(),
    };
    if let Err(e) = schema::validate(&layout) {
        return SessionError::Validation(e).into_response();
    }

    // 4. CAS under the per-session write lock. Disk-first.
    let arc = match state.session_cache.get_or_load(wm, &name).await {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    let mut snap = arc.write().await;
    if if_match != snap.etag_hex {
        let current_etag = format!("\"{}\"", snap.etag_hex);
        let mut resp = SessionError::PreconditionFailed.into_response();
        if let Ok(val) = HeaderValue::from_str(&current_etag) {
            resp.headers_mut().insert(header::ETAG, val);
        }
        return resp;
    }
    let new_snap = SessionLayout::new(layout);
    let path = match wm.session_path(&name) {
        Ok(p) => p,
        Err(e) => return SessionError::Workspace(e).into_response(),
    };
    let bytes = canonical_bytes(&new_snap.layout);
    if let Err(e) = atomic_write_session(&path, &bytes) {
        let current_etag = format!("\"{}\"", snap.etag_hex);
        let mut resp = SessionError::Workspace(e).into_response();
        if let Ok(val) = HeaderValue::from_str(&current_etag) {
            resp.headers_mut().insert(header::ETAG, val);
        }
        return resp;
    }
    let new_etag_quoted = format!("\"{}\"", new_snap.etag_hex);
    *snap = new_snap;
    drop(snap);
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::ETAG, &new_etag_quoted)
        .body(Body::empty())
        .expect("static headers")
}

// ─────────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn service_unavailable(code: &'static str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": code,
            "message": "the workspace subsystem is not enabled for this Server",
        })),
    )
        .into_response()
}

fn parse_etag_header(v: &str) -> Option<String> {
    let trimmed = v.trim();
    if trimmed.starts_with("W/") || trimmed == "*" {
        return None;
    }
    let inner = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"'))?;
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

enum BodyReadError {
    TooLarge,
    Io(String),
}

async fn read_bounded_body(
    req: Request<Body>,
    cap: usize,
) -> Result<Vec<u8>, BodyReadError> {
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

// Quiet axum's unused-field warning when only one handler in a module reads
// HeaderMap. The compiler should fold this away.
const _: fn(&HeaderMap) = |_| {};

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::workspace::WorkspaceManager;
    use serde_json::json;
    use tempfile::TempDir;

    fn fresh() -> (TempDir, Arc<WorkspaceManager>, Arc<SessionCache>) {
        let dir = TempDir::new().unwrap();
        let wm = Arc::new(WorkspaceManager::from_path(dir.path().to_path_buf()).unwrap());
        let cache = Arc::new(SessionCache::new());
        (dir, wm, cache)
    }

    #[tokio::test]
    async fn cache_loads_then_returns_same_arc() {
        let (dir, wm, cache) = fresh();
        // Seed a valid v2 file on disk.
        let v2 = json!({
            "schema_version": 2,
            "groups": [],
            "items": [],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        });
        std::fs::write(dir.path().join("alpha.json"), serde_json::to_vec(&v2).unwrap()).unwrap();
        let a = cache.get_or_load(&wm, "alpha").await.unwrap();
        let b = cache.get_or_load(&wm, "alpha").await.unwrap();
        assert!(Arc::ptr_eq(&a, &b), "cache must reuse the Arc");
        let snap = a.read().await;
        assert_eq!(snap.layout.schema_version, 2);
    }

    #[tokio::test]
    async fn get_or_load_returns_not_found_for_missing() {
        let (_dir, wm, cache) = fresh();
        let err = cache.get_or_load(&wm, "absent").await.unwrap_err();
        assert!(matches!(err, SessionError::NotFound(_)));
    }

    #[tokio::test]
    async fn corrupt_v2_file_quarantines_and_errors() {
        let (dir, wm, cache) = fresh();
        std::fs::write(dir.path().join("rotten.json"), b"{ not json").unwrap();
        let err = cache.get_or_load(&wm, "rotten").await.unwrap_err();
        assert!(matches!(err, SessionError::Corrupt(_)));
        // Original moved aside.
        assert!(!dir.path().join("rotten.json").exists());
    }
}
