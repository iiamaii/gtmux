//! ADR-0035 / 0061 — `GET /api/fs/list?dir=<percent-encoded>`.
//!
//! Returns `{ dir, parent, entries[], total, truncated }` for one directory
//! at a time (lazy per-dir navigation — ADR-0035 D3). MVP scope = workspace
//! root only (implicit allow). External roots (ADR-0035 D2 / D2.1) land in
//! Stage 3 after the `[picker.roots]` toml schema mutation is ready.
//!
//! Implementation notes:
//!   * Path resolve = `canonicalize` then `starts_with(workspace_dir)`.
//!     Symlinks following the canonical resolve are honoured but the
//!     resolved real path must still sit inside the workspace.
//!   * Dot-prefixed entries are skipped (D7). A future `picker.show_hidden`
//!     setting (D7) flips this; for now we hard-code skip.
//!   * Cap = [`MAX_ENTRIES`] (500). When exceeded the response reports
//!     `truncated: true` and the slice of 500 — pagination is P3+ (D8).

use std::path::{Path, PathBuf};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::AppState;

/// ADR-0035 D3 — entries per request cap.
const MAX_ENTRIES: usize = 500;

#[derive(Debug, Deserialize)]
pub struct FsListQuery {
    /// Absolute path of the directory to list. Empty string = workspace root
    /// (convenience — the picker modal opens here by default).
    pub dir: String,
    /// Per-request override of `BehaviorSettings.picker_show_hidden`. When
    /// `Some`, the request value wins; when `None`, the Settings value is
    /// used. Lets the FE picker modal toggle hidden entries without
    /// touching the persistent Settings.
    #[serde(default)]
    pub show_hidden: Option<bool>,
}

#[derive(Debug, Serialize)]
struct FsListResponse {
    dir: String,
    parent: Option<String>,
    entries: Vec<FsEntry>,
    total: usize,
    truncated: bool,
}

#[derive(Debug, Serialize)]
struct FsEntry {
    name: String,
    /// `"file"` or `"directory"`.
    kind: &'static str,
    /// `None` for directories.
    size_bytes: Option<u64>,
    /// Unix seconds since epoch; `None` if the modified time is unavailable.
    mtime_unix: Option<u64>,
}

/// `GET /api/fs/list?dir=<percent-encoded>` — ADR-0035 D3.
pub async fn fs_list_handler(
    State(state): State<AppState>,
    Query(q): Query<FsListQuery>,
) -> Response {
    let Some(wm) = state.workspace.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "workspace_not_configured" })),
        )
            .into_response();
    };
    let workspace_root = wm.path();

    // Resolve target dir. Empty = workspace root (default open).
    let target: PathBuf = if q.dir.is_empty() {
        workspace_root.to_path_buf()
    } else {
        PathBuf::from(&q.dir)
    };

    // Canonicalize — also confirms the path exists.
    let canonical = match target.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "dir_not_found" })),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_dir" })),
            )
                .into_response();
        }
    };

    // MVP scope — workspace root only. Stage 3 expands to ADR-0035 D2.1
    // picker.roots after the toml schema mutation lands.
    let workspace_canonical = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(_) => return internal_500("workspace_canonicalize_failed"),
    };
    if !canonical.starts_with(&workspace_canonical) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "dir_not_allowed" })),
        )
            .into_response();
    }

    if !canonical.is_dir() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "not_a_directory" })),
        )
            .into_response();
    }

    // ADR-0035 D7 — Settings.picker_show_hidden default 또는 query param
    // 의 per-request override. true → dot-prefixed (`.git`, `.env`) 도 표시.
    let show_hidden = match q.show_hidden {
        Some(v) => v,
        None => state.behavior_settings.read().await.picker_show_hidden,
    };

    // Read entries. Skip dot-prefixed by default (Settings 의 toggle) and
    // any IO-flaky stragglers.
    let mut entries: Vec<FsEntry> = match std::fs::read_dir(&canonical) {
        Ok(it) => it,
        Err(_) => return internal_500("read_dir_failed"),
    }
    .filter_map(|res| res.ok())
    .filter(|e| {
        if show_hidden {
            return true;
        }
        e.file_name()
            .to_str()
            .map(|n| !n.starts_with('.'))
            .unwrap_or(false)
    })
    .filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        let meta = e.metadata().ok()?;
        let kind = if meta.is_dir() { "directory" } else { "file" };
        let size_bytes = if meta.is_file() {
            Some(meta.len())
        } else {
            None
        };
        let mtime_unix = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        Some(FsEntry {
            name,
            kind,
            size_bytes,
            mtime_unix,
        })
    })
    .collect();

    // Sort — directories first, then by name (case-insensitive).
    entries.sort_by(|a, b| match (a.kind, b.kind) {
        ("directory", "file") => std::cmp::Ordering::Less,
        ("file", "directory") => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    let total = entries.len();
    let truncated = total > MAX_ENTRIES;
    if truncated {
        entries.truncate(MAX_ENTRIES);
    }

    // Parent — workspace root has no parent for the picker (don't escape).
    let parent: Option<String> = if canonical == workspace_canonical {
        None
    } else {
        canonical
            .parent()
            .filter(|p| p.starts_with(&workspace_canonical))
            .map(|p| p.to_string_lossy().into_owned())
    };

    let body = FsListResponse {
        dir: canonical.to_string_lossy().into_owned(),
        parent,
        entries,
        total,
        truncated,
    };

    (StatusCode::OK, Json(body)).into_response()
}

fn internal_500(reason: &'static str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "internal", "reason": reason })),
    )
        .into_response()
}

/// Test-only — ensures the workspace canonical resolve matches future usage.
#[cfg(test)]
pub(crate) fn _max_entries() -> usize {
    MAX_ENTRIES
}

#[cfg(test)]
fn _force_compile<P: AsRef<Path>>(_p: P) {}
