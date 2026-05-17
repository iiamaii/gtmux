//! Terminal metadata + `GET /api/terminals` (Stage 4-B / BE-NEW-10 / BE-8).
//!
//! The server-wide Terminal *pool* — the set of alive PTY child processes —
//! is owned by [`crate::TerminalMap`]. This module adds the *metadata* layer
//! (label / created_at) and exposes a single read endpoint that joins three
//! sources to produce the Sidebar Terminals list:
//!
//!   1. `terminal_map`              — UUID ↔ PaneId bridge (alive pool).
//!   2. `terminal_meta`             — per-UUID label + created_at (this file).
//!   3. `attach_index`              — every terminal item across every
//!                                    session file (attach_count + names).
//!
//! Source (3) used to be read by scanning every session file on every
//! `GET /api/terminals` request. ADR-0021 D7 amend ③ (0068 / 0067 Phase 4
//! / 0066 §BE-2) replaced that with an in-memory reverse index that is
//! cold-rebuilt at boot and updated by the layout-mutating handlers
//! (`PUT /layout`, `DELETE /items/:id`, `POST /import`, `DELETE` session).
//! Per-request cost on the hot path is now O(N_terminals_in_pool), all
//! in-memory.
//!
//! Metadata is *in-memory only* — it is recreated whenever the server boots,
//! since both the `terminal_map` and the alive PTY pool are themselves
//! ephemeral. Persistence would buy nothing the schema doesn't already give
//! us (the UUID itself survives in session files, ADR-0018 D2).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;

/// Hard cap on label byte length — matches `ItemCommon.label` (4 KiB,
/// ADR-0018 D8). Enforced by [`patch_handler`] before the label hits
/// the metadata store.
pub const MAX_LABEL_BYTES: usize = 4096;

/// Per-terminal metadata stored alongside the [`crate::TerminalMap`].
/// Created when a spawn registers a UUID, dropped when the same UUID
/// unregisters (terminal death or explicit kill).
#[derive(Debug, Clone)]
pub struct TerminalMetadata {
    /// User-supplied free-form label. Empty by default; populated via PATCH
    /// (P1+). Bound by [`MAX_LABEL_BYTES`].
    pub label: String,
    /// Unix epoch seconds at which this UUID was first registered with the
    /// store. Stable across re-spawns of the same UUID (e.g. dangling →
    /// fresh spawn) so the user sees the "originally created at" timestamp.
    pub created_at: u64,
}

/// In-memory metadata store. Keyed by the same UUID string that
/// [`crate::TerminalMap`] uses.
#[derive(Default, Debug)]
pub struct TerminalMetadataStore {
    inner: RwLock<HashMap<String, TerminalMetadata>>,
}

impl TerminalMetadataStore {
    /// Empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a fresh spawn. Idempotent on UUID — re-recording an existing
    /// UUID preserves the original `created_at` (matches the "stable across
    /// re-spawn" rule in [`TerminalMetadata`]).
    pub async fn record_spawn(&self, uuid: &str) {
        let mut g = self.inner.write().await;
        g.entry(uuid.to_string())
            .or_insert_with(|| TerminalMetadata {
                label: String::new(),
                created_at: now_unix(),
            });
    }

    /// Drop metadata when the corresponding UUID is gone from the pool.
    /// Idempotent — removing an absent UUID is a no-op.
    pub async fn forget(&self, uuid: &str) {
        let mut g = self.inner.write().await;
        g.remove(uuid);
    }

    /// Read-only snapshot of every UUID's metadata. Allocates a copy.
    pub async fn snapshot(&self) -> HashMap<String, TerminalMetadata> {
        self.inner.read().await.clone()
    }

    /// Read one entry. `None` if absent.
    pub async fn get(&self, uuid: &str) -> Option<TerminalMetadata> {
        self.inner.read().await.get(uuid).cloned()
    }

    /// Set the label on an existing entry. Returns `false` when the UUID
    /// is unknown — callers map that to 404 so the FE does not silently
    /// race with a terminal deletion.
    pub async fn set_label(&self, uuid: &str, label: String) -> bool {
        let mut g = self.inner.write().await;
        match g.get_mut(uuid) {
            Some(m) => {
                m.label = label;
                true
            }
            None => false,
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ─────────────────────────────────────────────────────────────────────────────
//  GET /api/terminals — response shape + handler
// ─────────────────────────────────────────────────────────────────────────────

/// One row in the `GET /api/terminals` response.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TerminalInfo {
    /// Schema-side UUID; same value rendered into session files.
    pub id: String,
    /// Liveness from the bridge map. Currently always `true` because dead
    /// terminals are unregistered, but kept as a field for forward compat
    /// with the dangling state coming in Batch 4-D.
    pub alive: bool,
    /// User label, empty by default.
    pub label: String,
    /// Unix seconds; stable across re-spawns of the same UUID.
    pub created_at: u64,
    /// Number of session-layout terminal items that reference `id` across
    /// the whole workspace. `0` for terminals that exist in the pool but
    /// are not yet placed on any canvas.
    pub attach_count: u32,
    /// Names of sessions whose layout files reference `id`. Same ordering
    /// as `workspace.enumerate_sessions()` (lexicographic).
    pub attached_sessions: Vec<String>,
}

/// `GET /api/terminals` — server-wide alive Terminal pool with metadata
/// and cross-session attach references. Empty list if no terminals exist.
/// Returns 503 when no workspace is configured (matches `/api/sessions`).
pub async fn list_handler(State(state): State<crate::AppState>) -> Response {
    if state.workspace.is_none() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "workspace_not_configured" })),
        )
            .into_response();
    };

    let pool = state.terminal_map.snapshot().await;
    let meta = state.terminal_meta.snapshot().await;

    // ADR-0021 D7 amend ③ (0068) — in-memory reverse index replaces the
    // per-request workspace scan. Cold-built at boot from disk; kept
    // fresh by the layout-mutating handlers in `sessions.rs`.
    let session_refs = state.attach_index.read_all_attach_refs();

    let mut rows: Vec<TerminalInfo> = pool
        .into_iter()
        .map(|(uuid, _pane)| {
            let m = meta.get(&uuid);
            let label = m.map(|x| x.label.clone()).unwrap_or_default();
            let created_at = m.map(|x| x.created_at).unwrap_or(0);
            let attached_sessions = session_refs.get(&uuid).cloned().unwrap_or_default();
            let attach_count = u32::try_from(attached_sessions.len()).unwrap_or(u32::MAX);
            TerminalInfo {
                id: uuid,
                alive: true,
                label,
                created_at,
                attach_count,
                attached_sessions,
            }
        })
        .collect();

    // Stable ordering: created_at ASC, then id ASC. Makes the sidebar list
    // not jump around between polls.
    rows.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });

    Json(rows).into_response()
}

// ─────────────────────────────────────────────────────────────────────────────
//  POST /api/terminals/:id/kill — SIGTERM only, panel(s) survive
//  POST /api/terminals/:id/respawn — kill (if alive) + fresh spawn, same UUID
// ─────────────────────────────────────────────────────────────────────────────

/// `POST /api/terminals/:id/kill` — SIGTERM the Terminal bound to `id`,
/// drop it from the bridge map and metadata store, and leave every panel
/// that references this UUID in a *dangling* state (ADR-0021 D9.4 explicit
/// `[Kill terminal]` action). Returns:
///   * 204 on success (the terminal was alive and was killed)
///   * 404 when the UUID is not currently in the pool
///   * 503 when no hub is configured
pub async fn kill_handler(
    State(state): State<crate::AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if state.hub.is_none() {
        return service_unavailable("hub_not_configured");
    }
    if state.terminal_map.lookup_pane(&id).await.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "terminal_not_found",
                "message": format!("terminal '{id}' is not in the alive pool"),
            })),
        )
            .into_response();
    }
    crate::sessions::kill_and_unregister_terminal(&state, &id).await;
    // Explicit user kill — drop metadata as well so the row disappears
    // from `GET /api/terminals`. Compare to `respawn_handler`, which
    // keeps metadata to preserve `created_at` + `label` across the
    // transient death (ADR-0021 D10.1).
    state.terminal_meta.forget(&id).await;
    StatusCode::NO_CONTENT.into_response()
}

/// `POST /api/terminals/:id/respawn` — drop any alive PaneId currently
/// bound to `id`, then spawn a fresh one and bind it to the same UUID
/// (ADR-0021 D10.1 lazy fresh-spawn arm, but invoked explicitly here).
/// The panels that reference this UUID re-attach automatically once the
/// new PaneId broadcasts its first output.
///
/// **Concurrency** (ADR-0021 D10.2, 0053 §3.4): the handler holds a
/// per-UUID lock from [`AppState::respawn_locks`] across the kill→spawn
/// pair so two simultaneous requests on the same UUID don't churn the
/// PaneId binding. After acquiring the lock the second caller sees the
/// first call's fresh PaneId already bound and short-circuits to an
/// idempotent 200 (`reused: true`) — no kill, no fresh spawn. This is
/// the multi-webpage `PanelDanglingOverlay` auto-respawn safety net.
///
/// Returns:
///   * 200 + `{ id, reused: false }` — kill+spawn ran, fresh PaneId bound.
///   * 200 + `{ id, reused: true }`  — another caller already published a
///     fresh PaneId while we held the lock; their binding is returned.
///   * 503 when no hub is configured.
///   * 500 with the spawn error on backend failure.
pub async fn respawn_handler(
    State(state): State<crate::AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if state.hub.is_none() {
        return service_unavailable("hub_not_configured");
    }

    // Per-UUID serialisation — fetch or create the lock for this UUID.
    let per_uuid_lock = {
        let mut map = state.respawn_locks.lock().await;
        map.entry(id.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    let _guard = per_uuid_lock.lock().await;

    // Lost-the-race idempotent arm: if another concurrent respawn already
    // published a fresh PaneId for this UUID, treat ours as a no-op so we
    // don't kill the just-bound Pane and orphan its output stream.
    if state.terminal_map.lookup_pane(&id).await.is_some() {
        return (
            StatusCode::OK,
            Json(json!({ "id": id, "reused": true })),
        )
            .into_response();
    }

    // Best-effort kill of the existing pane. A UUID with no current binding
    // (dangling) just gets a fresh spawn; the kill_and_unregister is a no-op
    // in that case.
    crate::sessions::kill_and_unregister_terminal(&state, &id).await;
    match state.spawn_terminal_with_uuid(id.clone()).await {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({ "id": id, "reused": false })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "respawn_failed",
                "message": e.to_string(),
            })),
        )
            .into_response(),
    }
}

/// Body for [`patch_handler`].
#[derive(Debug, Deserialize)]
pub struct PatchTerminalBody {
    /// New free-form label. Cap = [`MAX_LABEL_BYTES`].
    pub label: String,
}

/// `PATCH /api/terminals/:id` — update the user-supplied label on an
/// existing Terminal metadata entry (BE-8). Returns:
///   * 204 on success
///   * 400 when the label exceeds [`MAX_LABEL_BYTES`]
///   * 404 when the UUID is not in the metadata store (either never
///     spawned or already removed via `/kill` / `DELETE item`)
pub async fn patch_handler(
    State(state): State<crate::AppState>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<PatchTerminalBody>,
) -> Response {
    if body.label.len() > MAX_LABEL_BYTES {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "label_too_long",
                "message": format!(
                    "label length {} exceeds cap {MAX_LABEL_BYTES}",
                    body.label.len()
                ),
            })),
        )
            .into_response();
    }
    if !state.terminal_meta.set_label(&id, body.label).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "terminal_not_found",
                "message": format!("terminal '{id}' has no metadata entry"),
            })),
        )
            .into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn record_spawn_is_idempotent_preserving_created_at() {
        let store = TerminalMetadataStore::new();
        store.record_spawn("uuid-a").await;
        let first = store.get("uuid-a").await.unwrap();
        // Sleep a few ms then re-record; created_at must not change.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        store.record_spawn("uuid-a").await;
        let second = store.get("uuid-a").await.unwrap();
        assert_eq!(first.created_at, second.created_at);
    }

    #[tokio::test]
    async fn forget_drops_entry() {
        let store = TerminalMetadataStore::new();
        store.record_spawn("uuid-a").await;
        store.forget("uuid-a").await;
        assert!(store.get("uuid-a").await.is_none());
    }

    #[tokio::test]
    async fn set_label_updates_only_existing_uuid() {
        let store = TerminalMetadataStore::new();
        // Unknown UUID — false (handler maps to 404).
        assert!(!store.set_label("missing", "x".into()).await);
        store.record_spawn("uuid-a").await;
        assert!(store.set_label("uuid-a", "build watch".into()).await);
        assert_eq!(
            store.get("uuid-a").await.unwrap().label,
            "build watch"
        );
    }

    #[tokio::test]
    async fn snapshot_is_a_copy() {
        let store = TerminalMetadataStore::new();
        store.record_spawn("uuid-a").await;
        let snap = store.snapshot().await;
        store.forget("uuid-a").await;
        assert_eq!(snap.len(), 1);
    }
}
