//! ADR-0035 / 0046 D3 — Server Workspace(A)-scoped file system access:
//!   GET  /api/fs/list?dir=<percent-encoded>   — one directory listing
//!   POST /api/fs/mkdir { path }               — create a directory
//!   POST /api/fs/rmdir { path }               — remove an *empty* directory
//!
//! All three are clamped to the Server Workspace(A) sandbox and the M2
//! denylist (Store / gtmux config / gtmux state) via [`crate::fs_guard`]
//! (ADR-0045 D6). The picker is rooted at **A**, not the Store — the previous
//! `wm.path()` (Store) rooting was the bug ADR-0046 D3 fixes (the user would
//! otherwise browse gtmux's internal JSON storage).
//!
//! Implementation notes:
//!   * Path resolve = `canonicalize` then `is_path_allowed` (inside A AND
//!     outside the denylist). Symlinks following the canonical resolve are
//!     honoured but the resolved real path must still pass the guard.
//!   * The guard applies regardless of `show_hidden` (ADR-0045 D6) — hiding
//!     dotfiles is cosmetic; the denylist is the security boundary.
//!   * `list` cap = [`MAX_ENTRIES`] (500); pagination is P3+ (ADR-0035 D8).

use std::path::PathBuf;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::fs_guard;
use crate::AppState;

/// ADR-0035 D3 — entries per request cap.
const MAX_ENTRIES: usize = 500;

#[derive(Debug, Deserialize)]
pub struct FsListQuery {
    /// Absolute path of the directory to list. Empty string = Server
    /// Workspace(A) root (the picker's default open).
    pub dir: String,
    /// Per-request override of `BehaviorSettings.picker_show_hidden`. When
    /// `Some`, the request value wins; when `None`, the Settings value is
    /// used. The denylist guard applies either way (ADR-0045 D6).
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

#[derive(Debug, Deserialize)]
pub struct FsMutateBody {
    /// Absolute path of the directory to create / remove. Must resolve inside
    /// A and outside the denylist.
    pub path: String,
}

/// `GET /api/fs/list?dir=<percent-encoded>` — ADR-0035 D3 / ADR-0046 D3.
pub async fn fs_list_handler(
    State(state): State<AppState>,
    Query(q): Query<FsListQuery>,
) -> Response {
    let server_workspace = state.server_workspace.as_path();

    // Resolve target dir. Empty = Server Workspace(A) root (default open).
    let target: PathBuf = if q.dir.is_empty() {
        server_workspace.to_path_buf()
    } else {
        PathBuf::from(&q.dir)
    };

    // Canonicalize — also confirms the path exists.
    let canonical = match target.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return error(StatusCode::NOT_FOUND, "dir_not_found");
        }
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_dir"),
    };

    // A-scope + denylist (ADR-0045 D6).
    if !fs_guard::is_path_allowed(&canonical, server_workspace, &state.fs_denylist) {
        return error(StatusCode::FORBIDDEN, "dir_not_allowed");
    }

    if !canonical.is_dir() {
        return error(StatusCode::BAD_REQUEST, "not_a_directory");
    }

    // ADR-0035 D7 — Settings.picker_show_hidden default 또는 query param 의
    // per-request override. The denylist guard above already ran, so a
    // `show_hidden=true` listing still cannot *enter* a denied dir.
    let show_hidden = match q.show_hidden {
        Some(v) => v,
        None => state.behavior_settings.read().await.picker_show_hidden,
    };

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

    // Parent — the A root has no parent for the picker (don't escape A), and
    // a parent that would leave A / land in the denylist is suppressed.
    let parent: Option<String> = if canonical == server_workspace {
        None
    } else {
        canonical
            .parent()
            .filter(|p| fs_guard::is_path_allowed(p, server_workspace, &state.fs_denylist))
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

/// `POST /api/fs/mkdir { path }` — ADR-0046 D3. Create a single directory
/// inside A. The target itself must not yet exist; its parent must exist and
/// pass the A-scope + denylist guard. 400 invalid_path / 403 dir_not_allowed /
/// 409 already_exists / 500 mkdir_failed.
pub async fn fs_mkdir_handler(
    State(state): State<AppState>,
    Json(body): Json<FsMutateBody>,
) -> Response {
    let server_workspace = state.server_workspace.as_path();
    let candidate = PathBuf::from(&body.path);
    if !candidate.is_absolute() {
        return error(StatusCode::BAD_REQUEST, "invalid_path");
    }
    let Some(parent) = candidate.parent() else {
        return error(StatusCode::BAD_REQUEST, "invalid_path");
    };
    let Some(name) = candidate.file_name().and_then(|s| s.to_str()) else {
        return error(StatusCode::BAD_REQUEST, "invalid_path");
    };
    // Reject traversal / multi-component / empty names — the final segment must
    // be a plain directory name.
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return error(StatusCode::BAD_REQUEST, "invalid_path");
    }

    // Parent must exist + be inside A + outside the denylist.
    let parent_canonical = match parent.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return error(StatusCode::BAD_REQUEST, "parent_not_found");
        }
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_path"),
    };
    if !fs_guard::is_path_allowed(&parent_canonical, server_workspace, &state.fs_denylist) {
        return error(StatusCode::FORBIDDEN, "dir_not_allowed");
    }

    let target = parent_canonical.join(name);
    if target.exists() {
        return error(StatusCode::CONFLICT, "already_exists");
    }
    match std::fs::create_dir(&target) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(json!({ "path": target.to_string_lossy() })),
        )
            .into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            error(StatusCode::CONFLICT, "already_exists")
        }
        Err(_) => internal_500("mkdir_failed"),
    }
}

/// `POST /api/fs/rmdir { path }` — ADR-0046 D3. Remove an **empty** directory
/// inside A (safe by construction; a non-empty dir is rejected so the user
/// cannot wipe a populated tree via the picker). 400 invalid_path /
/// 403 dir_not_allowed / 404 dir_not_found / 409 dir_not_empty /
/// 500 rmdir_failed.
pub async fn fs_rmdir_handler(
    State(state): State<AppState>,
    Json(body): Json<FsMutateBody>,
) -> Response {
    let server_workspace = state.server_workspace.as_path();
    let candidate = PathBuf::from(&body.path);
    if !candidate.is_absolute() {
        return error(StatusCode::BAD_REQUEST, "invalid_path");
    }
    let canonical = match candidate.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return error(StatusCode::NOT_FOUND, "dir_not_found");
        }
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_path"),
    };
    if !fs_guard::is_path_allowed(&canonical, server_workspace, &state.fs_denylist) {
        return error(StatusCode::FORBIDDEN, "dir_not_allowed");
    }
    // Refuse to remove the A root itself.
    if canonical == server_workspace {
        return error(StatusCode::FORBIDDEN, "dir_not_allowed");
    }
    if !canonical.is_dir() {
        return error(StatusCode::BAD_REQUEST, "not_a_directory");
    }
    // `remove_dir` only removes empty dirs; map ENOTEMPTY → 409 explicitly.
    match std::fs::remove_dir(&canonical) {
        Ok(()) => (StatusCode::NO_CONTENT, ()).into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            error(StatusCode::NOT_FOUND, "dir_not_found")
        }
        // DirectoryNotEmpty is stable since Rust 1.83; match on the raw errno
        // as a fallback for portability.
        Err(e)
            if e.kind() == std::io::ErrorKind::DirectoryNotEmpty
                || e.raw_os_error() == Some(libc_enotempty()) =>
        {
            error(StatusCode::CONFLICT, "dir_not_empty")
        }
        Err(_) => internal_500("rmdir_failed"),
    }
}

/// ENOTEMPTY raw errno (39 on Linux, 66 on macOS/BSD). Used as a portable
/// fallback when `ErrorKind::DirectoryNotEmpty` is unavailable.
fn libc_enotempty() -> i32 {
    #[cfg(target_os = "macos")]
    {
        66
    }
    #[cfg(not(target_os = "macos"))]
    {
        39
    }
}

fn error(status: StatusCode, code: &'static str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

fn internal_500(reason: &'static str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "internal", "reason": reason })),
    )
        .into_response()
}
