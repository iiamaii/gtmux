//! ADR-0047 D10 — Server Workspace(A)-scoped file/directory **copy** (Files-tab
//! workspace clipboard paste):
//!   POST /api/fs/copy { sources, dest_dir, on_conflict }
//!
//! Copies one or more workspace entries into a destination directory. Both the
//! sources and the destination are clamped to the [`crate::fs_guard`] sandbox
//! (inside the Server Workspace(A) AND outside the Store/config/state denylist,
//! ADR-0045 D6), the **same single guard** as `fs/list`·`fs/file`·`fs/upload`·
//! `fs/rename`·`fs/remove`. Directory copy is recursive; every descendant is
//! re-checked against the guard and **symlinks are refused** (fail-closed) so a
//! link inside a copied tree can never escape A or reach the denylist. A
//! `dest_dir` that is the source itself or a descendant of it is a cycle and is
//! rejected. All paths on the wire are absolute (the canvas-item B-relative
//! form is the FE's concern). `std::fs` only — never a shell command.

use std::path::{Path, PathBuf};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::fs_file::{next_free_name, ConflictMode};
use crate::fs_guard;
use crate::AppState;

/// Upper bound on entries visited in a single copy request (files + dirs). A
/// fail-closed backstop against a pathological tree pinning a worker; over the
/// cap → `413 copy_too_large` (ADR-0047 D10).
const COPY_ENTRY_BUDGET: usize = 200_000;

#[derive(Debug, Deserialize)]
pub struct FsCopyBody {
    /// Absolute source paths (files and/or directories) inside A.
    pub sources: Vec<String>,
    /// Absolute destination directory inside A (must already exist).
    pub dest_dir: String,
    /// `reject` | `rename` | `overwrite`. Absent → `reject` (conservative); the
    /// FE sends `rename`.
    #[serde(default)]
    pub on_conflict: Option<String>,
}

#[derive(Debug, Serialize)]
struct CopiedEntry {
    /// Canonical absolute source path (request order is preserved).
    source: String,
    /// Absolute path of the copied entry under `dest_dir`.
    path: String,
    /// Final on-disk name (may be suffixed under `rename`).
    name: String,
    /// `"file"` or `"directory"`.
    kind: &'static str,
}

/// Structured copy failure → HTTP. `Symlink`/`InvalidRequest`/budget all map to
/// stable codes the FE can branch on.
#[derive(Debug)]
enum CopyError {
    InvalidRequest,
    Denied,
    NotFound,
    NameConflict,
    Cycle,
    TooLarge,
    Failed,
}

impl CopyError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::InvalidRequest => (StatusCode::BAD_REQUEST, "invalid_request"),
            Self::Denied => (StatusCode::FORBIDDEN, "dir_not_allowed"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::NameConflict => (StatusCode::CONFLICT, "name_conflict"),
            Self::Cycle => (StatusCode::CONFLICT, "copy_cycle"),
            Self::TooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "copy_too_large"),
            Self::Failed => (StatusCode::INTERNAL_SERVER_ERROR, "copy_failed"),
        };
        (status, Json(json!({ "error": code }))).into_response()
    }
}

/// One pre-validated source: canonical path + its (non-symlink) kind.
struct ValidSource {
    canonical: PathBuf,
    name: String,
    is_dir: bool,
}

/// `POST /api/fs/copy` — ADR-0047 D10. See module docs for the safety model.
/// 200 `{ entries: [{ source, path, name, kind }] }` (request order) /
/// 400 invalid_request / 403 dir_not_allowed / 404 not_found /
/// 409 name_conflict|copy_cycle / 413 copy_too_large / 500 copy_failed.
pub async fn fs_copy_handler(
    State(state): State<AppState>,
    Json(body): Json<FsCopyBody>,
) -> Response {
    match copy_request(&state, body) {
        Ok(entries) => (StatusCode::OK, Json(json!({ "entries": entries }))).into_response(),
        Err(e) => e.into_response(),
    }
}

fn copy_request(state: &AppState, body: FsCopyBody) -> Result<Vec<CopiedEntry>, CopyError> {
    let server_workspace = state.server_workspace.as_path();
    let denylist = state.fs_denylist.as_ref();

    let mode = match body.on_conflict.as_deref() {
        None => ConflictMode::Reject,
        Some(s) => ConflictMode::parse(s).ok_or(CopyError::InvalidRequest)?,
    };
    if body.sources.is_empty() {
        return Err(CopyError::InvalidRequest);
    }

    // dest_dir — absolute, canonicalize, guard, must be an existing directory.
    let dest_dir = guarded_canonical(&body.dest_dir, server_workspace, denylist)?;
    if !dest_dir.is_dir() {
        return Err(CopyError::Denied);
    }

    // Pre-validate every source before copying anything (so a bad source fails
    // the request without leaving a partial result).
    let mut sources: Vec<ValidSource> = Vec::with_capacity(body.sources.len());
    for raw in &body.sources {
        // Refuse a symlink *source* up front (its real target may sit outside
        // A; copying it would smuggle outside content in).
        if is_symlink(raw) {
            return Err(CopyError::InvalidRequest);
        }
        let canonical = guarded_canonical(raw, server_workspace, denylist)?;
        // Refuse the A root itself as a source.
        if canonical == server_workspace {
            return Err(CopyError::Denied);
        }
        // Cycle: copying X into X or into a descendant of X.
        if dest_dir == canonical || dest_dir.starts_with(&canonical) {
            return Err(CopyError::Cycle);
        }
        let name = canonical
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(CopyError::InvalidRequest)?
            .to_string();
        let is_dir = canonical.is_dir();
        sources.push(ValidSource {
            canonical,
            name,
            is_dir,
        });
    }

    // Resolve conflicts + copy, preserving request order.
    let mut entries = Vec::with_capacity(sources.len());
    let mut budget = 0usize;
    for src in sources {
        let (target, final_name) = resolve_target(&dest_dir, &src, mode)?;
        // Never copy an entry onto itself (overwrite would truncate it).
        if target == src.canonical {
            return Err(CopyError::NameConflict);
        }
        let target_preexisted = target.exists();
        if let Err(e) = copy_tree(&src.canonical, &target, server_workspace, denylist, &mut budget)
        {
            // Best-effort cleanup of a *freshly created* partial target so a
            // failed copy doesn't leave junk. Never touch a pre-existing
            // overwrite target (that is the user's own data).
            if !target_preexisted {
                if target.is_dir() {
                    let _ = std::fs::remove_dir_all(&target);
                } else {
                    let _ = std::fs::remove_file(&target);
                }
            }
            return Err(e);
        }
        entries.push(CopiedEntry {
            source: src.canonical.to_string_lossy().into_owned(),
            path: target.to_string_lossy().into_owned(),
            name: final_name,
            kind: if src.is_dir { "directory" } else { "file" },
        });
    }
    Ok(entries)
}

/// Canonicalize `raw` (absolute, no NUL) and assert it clears the A-scope +
/// denylist guard. NotFound → [`CopyError::NotFound`]; guard violation →
/// [`CopyError::Denied`].
fn guarded_canonical(
    raw: &str,
    server_workspace: &Path,
    denylist: &[PathBuf],
) -> Result<PathBuf, CopyError> {
    let candidate = PathBuf::from(raw);
    if !candidate.is_absolute() || raw.contains('\0') {
        return Err(CopyError::InvalidRequest);
    }
    let canonical = match candidate.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(CopyError::NotFound),
        Err(_) => return Err(CopyError::InvalidRequest),
    };
    if !fs_guard::is_path_allowed(&canonical, server_workspace, denylist) {
        return Err(CopyError::Denied);
    }
    Ok(canonical)
}

/// Whether the *final component* of `raw` is a symlink (lexical lstat — does
/// not follow). Used to refuse symlink sources before canonicalize resolves
/// them away.
fn is_symlink(raw: &str) -> bool {
    std::fs::symlink_metadata(raw)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// Resolve the on-disk target (and its final name) under `dest_dir` for one
/// source per the conflict mode.
fn resolve_target(
    dest_dir: &Path,
    src: &ValidSource,
    mode: ConflictMode,
) -> Result<(PathBuf, String), CopyError> {
    let initial = dest_dir.join(&src.name);
    if !initial.exists() {
        return Ok((initial, src.name.clone()));
    }
    match mode {
        ConflictMode::Reject => Err(CopyError::NameConflict),
        ConflictMode::Rename => {
            let renamed = next_free_name(dest_dir, &src.name);
            Ok((dest_dir.join(&renamed), renamed))
        }
        ConflictMode::Overwrite => {
            // File-only overwrite. A directory target (or a kind mismatch) is
            // refused — directory overwrite/merge is out of scope (ADR-0047 D10).
            if initial.is_dir() || src.is_dir {
                Err(CopyError::NameConflict)
            } else {
                Ok((initial, src.name.clone()))
            }
        }
    }
}

/// Recursively copy `src` → `dst`. `src` is a guarded, non-symlink path. For a
/// directory, every child is re-guarded and symlinks are refused (fail-closed),
/// so the copy can never escape A or reach the denylist. The `budget` counter
/// caps total entries.
fn copy_tree(
    src: &Path,
    dst: &Path,
    server_workspace: &Path,
    denylist: &[PathBuf],
    budget: &mut usize,
) -> Result<(), CopyError> {
    *budget += 1;
    if *budget > COPY_ENTRY_BUDGET {
        return Err(CopyError::TooLarge);
    }

    // `src` is canonical and was confirmed non-symlink by the caller; inspect
    // its real kind without following anything new.
    let meta = std::fs::symlink_metadata(src).map_err(|_| CopyError::Failed)?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        // Defensive — should not happen (caller guarantees), fail closed.
        return Err(CopyError::InvalidRequest);
    }

    if ft.is_dir() {
        std::fs::create_dir(dst).map_err(|_| CopyError::Failed)?;
        let read = std::fs::read_dir(src).map_err(|_| CopyError::Failed)?;
        for entry in read {
            let entry = entry.map_err(|_| CopyError::Failed)?;
            let child = entry.path();
            let child_ft = entry.file_type().map_err(|_| CopyError::Failed)?;
            // Refuse any symlink in the tree (escape / denylist-bypass vector).
            if child_ft.is_symlink() {
                return Err(CopyError::InvalidRequest);
            }
            // Re-guard each descendant. `child` = canonical parent + a real
            // (non-symlink) name, so it is itself canonical — a lexical
            // membership check is sound here.
            if !fs_guard::is_path_allowed(&child, server_workspace, denylist) {
                return Err(CopyError::Denied);
            }
            let child_dst = dst.join(entry.file_name());
            copy_tree(&child, &child_dst, server_workspace, denylist, budget)?;
        }
        Ok(())
    } else if ft.is_file() {
        std::fs::copy(src, dst).map(|_| ()).map_err(|_| CopyError::Failed)
    } else {
        // Sockets / fifos / devices — not regular content; fail closed.
        Err(CopyError::InvalidRequest)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn copy_tree_copies_file_and_recursive_directory() {
        let a = TempDir::new().unwrap();
        let a_root = a.path().canonicalize().unwrap();
        let denylist: Vec<PathBuf> = vec![];

        // File copy.
        std::fs::write(a_root.join("f.txt"), b"hi").unwrap();
        let mut budget = 0;
        copy_tree(
            &a_root.join("f.txt"),
            &a_root.join("f-copy.txt"),
            &a_root,
            &denylist,
            &mut budget,
        )
        .unwrap();
        assert_eq!(std::fs::read(a_root.join("f-copy.txt")).unwrap(), b"hi");

        // Recursive directory copy.
        let d = a_root.join("d");
        std::fs::create_dir(&d).unwrap();
        std::fs::write(d.join("a.txt"), b"a").unwrap();
        std::fs::create_dir(d.join("sub")).unwrap();
        std::fs::write(d.join("sub").join("b.txt"), b"b").unwrap();
        let mut budget = 0;
        copy_tree(&d, &a_root.join("d2"), &a_root, &denylist, &mut budget).unwrap();
        assert_eq!(std::fs::read(a_root.join("d2").join("a.txt")).unwrap(), b"a");
        assert_eq!(
            std::fs::read(a_root.join("d2").join("sub").join("b.txt")).unwrap(),
            b"b"
        );
    }

    #[cfg(unix)]
    #[test]
    fn copy_tree_refuses_symlink_in_tree() {
        use std::os::unix::fs::symlink;
        let a = TempDir::new().unwrap();
        let a_root = a.path().canonicalize().unwrap();
        let outside = TempDir::new().unwrap();
        std::fs::write(outside.path().join("secret"), b"s").unwrap();

        let d = a_root.join("d");
        std::fs::create_dir(&d).unwrap();
        std::fs::write(d.join("ok.txt"), b"ok").unwrap();
        symlink(outside.path().join("secret"), d.join("link")).unwrap();

        let mut budget = 0;
        let res = copy_tree(&d, &a_root.join("d2"), &a_root, &[], &mut budget);
        assert!(matches!(res, Err(CopyError::InvalidRequest)));
    }

    #[test]
    fn copy_tree_budget_trips_too_large() {
        let a = TempDir::new().unwrap();
        let a_root = a.path().canonicalize().unwrap();
        std::fs::write(a_root.join("f.txt"), b"x").unwrap();
        // Budget already at the cap → the single entry trips it.
        let mut budget = COPY_ENTRY_BUDGET;
        let res = copy_tree(
            &a_root.join("f.txt"),
            &a_root.join("f2.txt"),
            &a_root,
            &[],
            &mut budget,
        );
        assert!(matches!(res, Err(CopyError::TooLarge)));
    }
}
