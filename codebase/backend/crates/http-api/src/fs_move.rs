//! ADR-0047 D11 — Server Workspace(A)-scoped file/directory **move** (Files-tab
//! tree drag-move):
//!   POST /api/fs/move { sources, dest_dir, on_conflict }
//!
//! Moves one or more workspace entries into a destination directory via
//! `std::fs::rename` (NOT copy+remove — a cross-device `EXDEV` is reported as
//! `500 move_failed` rather than silently falling back, ADR-0047 D11). Both the
//! sources and the destination are clamped to the [`crate::fs_guard`] sandbox
//! (inside the Server Workspace(A) AND outside the Store/config/state denylist),
//! the same single guard as the other `fs/*` mutations.
//!
//! Safety (D11):
//!   * symlink sources and any symlink *inside* a moved directory tree are
//!     refused (fail-closed); every directory descendant is re-guarded during a
//!     read-only preflight walk before the move runs.
//!   * `dest_dir == source` or a descendant of a source is a cycle.
//!   * a `sources[]` set that contains both an ancestor and its descendant is
//!     rejected (defense-in-depth; the FE also dedupes).
//!   * everything (guard / conflict / cycle / tree) is preflighted **before**
//!     any rename; a runtime failure mid-batch triggers a reverse rollback so a
//!     partial move is avoided.
//!
//! The BE never rewrites the canvas layout. Its sole obligation is to return a
//! stable canonical `source` → `path` (target) mapping so the FE can rebind
//! `image`/`document`/`file_path` path references (ADR-0047 D11).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::fs_guard;
use crate::AppState;

/// Read-only preflight walk cap (files + dirs) per directory source — a
/// fail-closed backstop against a pathological tree. Over the cap → 400
/// (the request is too large to validate safely; `move` has no 413 in its
/// contract, ADR-0047 D11).
const MOVE_WALK_BUDGET: usize = 200_000;

#[derive(Debug, Deserialize)]
pub struct FsMoveBody {
    pub sources: Vec<String>,
    pub dest_dir: String,
    /// `reject` (default / FE default) | `rename`. `overwrite` is refused.
    #[serde(default)]
    pub on_conflict: Option<String>,
}

#[derive(Debug, Serialize)]
struct MovedEntry {
    /// Canonical absolute source path (request order preserved).
    source: String,
    /// Canonical absolute target path after the move.
    path: String,
    name: String,
    kind: &'static str,
}

#[derive(Debug)]
enum MoveError {
    InvalidRequest,
    Denied,
    NotFound,
    NameConflict,
    Cycle,
    Failed,
}

impl MoveError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::InvalidRequest => (StatusCode::BAD_REQUEST, "invalid_request"),
            Self::Denied => (StatusCode::FORBIDDEN, "dir_not_allowed"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::NameConflict => (StatusCode::CONFLICT, "name_conflict"),
            Self::Cycle => (StatusCode::CONFLICT, "move_cycle"),
            Self::Failed => (StatusCode::INTERNAL_SERVER_ERROR, "move_failed"),
        };
        (status, Json(json!({ "error": code }))).into_response()
    }
}

/// A source resolved during preflight: canonical path, final target, name, kind.
struct PlannedMove {
    source: PathBuf,
    target: PathBuf,
    name: String,
    kind: &'static str,
}

/// `POST /api/fs/move` — ADR-0047 D11. See module docs for the safety model.
/// 200 `{ entries: [{ source, path, name, kind }] }` (request order) /
/// 400 invalid_request / 403 dir_not_allowed / 404 not_found /
/// 409 name_conflict|move_cycle / 500 move_failed.
pub async fn fs_move_handler(
    State(state): State<AppState>,
    Json(body): Json<FsMoveBody>,
) -> Response {
    match move_request(&state, body) {
        Ok(entries) => (StatusCode::OK, Json(json!({ "entries": entries }))).into_response(),
        Err(e) => e.into_response(),
    }
}

fn move_request(state: &AppState, body: FsMoveBody) -> Result<Vec<MovedEntry>, MoveError> {
    let a = state.server_workspace.as_path();
    let denylist = state.fs_denylist.as_ref();

    // Conflict mode — only reject / rename; overwrite is explicitly unsupported.
    let mode = match body.on_conflict.as_deref() {
        None | Some("reject") => ConflictMode::Reject,
        Some("rename") => ConflictMode::Rename,
        _ => return Err(MoveError::InvalidRequest),
    };
    if body.sources.is_empty() {
        return Err(MoveError::InvalidRequest);
    }

    // dest_dir — absolute, canonical, guarded, existing directory.
    let dest_dir = guarded_canonical(&body.dest_dir, a, denylist)?;
    if !dest_dir.is_dir() {
        return Err(MoveError::Denied);
    }

    // ── Preflight every source (no fs mutation yet) ──
    let mut canonical_sources: Vec<(PathBuf, &'static str)> = Vec::with_capacity(body.sources.len());
    for raw in &body.sources {
        // Refuse a symlink source (its real target may sit elsewhere).
        if is_symlink(raw) {
            return Err(MoveError::InvalidRequest);
        }
        let src = guarded_canonical(raw, a, denylist)?;
        if src == a {
            return Err(MoveError::Denied); // cannot move the A root itself
        }
        // Cycle: moving X into X or a descendant of X.
        if dest_dir == src || dest_dir.starts_with(&src) {
            return Err(MoveError::Cycle);
        }
        let kind = if src.is_dir() {
            // A directory source: read-only walk to fail-closed on any symlink
            // descendant or a descendant that escapes the guard.
            let mut budget = 0usize;
            assert_tree_safe(&src, a, denylist, &mut budget)?;
            "directory"
        } else {
            "file"
        };
        canonical_sources.push((src, kind));
    }

    // Defense-in-depth: reject a set containing both an ancestor and its
    // descendant (e.g. `["proj", "proj/sub"]`) — moving the ancestor first
    // would invalidate the descendant's path. The FE also dedupes.
    for (i, (a_path, _)) in canonical_sources.iter().enumerate() {
        for (j, (b_path, _)) in canonical_sources.iter().enumerate() {
            if i != j && b_path.starts_with(a_path) {
                return Err(MoveError::InvalidRequest);
            }
        }
    }

    // Resolve a free target for each source, tracking in-batch claims so two
    // sources can't resolve to the same destination name.
    let mut claimed: HashSet<PathBuf> = HashSet::new();
    let mut planned: Vec<PlannedMove> = Vec::with_capacity(canonical_sources.len());
    for (src, kind) in canonical_sources {
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(MoveError::InvalidRequest)?
            .to_string();
        let (target, final_name) = resolve_target(&dest_dir, &name, mode, &claimed)?;
        // A no-op self-move (target resolves back to the source) is refused.
        if target == src {
            return Err(MoveError::NameConflict);
        }
        claimed.insert(target.clone());
        planned.push(PlannedMove {
            source: src,
            target,
            name: final_name,
            kind,
        });
    }

    // ── Execute. On a runtime failure, reverse-rollback the moves already done. ──
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::with_capacity(planned.len()); // (target, source)
    for p in &planned {
        if std::fs::rename(&p.source, &p.target).is_err() {
            for (target, source) in done.iter().rev() {
                let _ = std::fs::rename(target, source);
            }
            return Err(MoveError::Failed);
        }
        done.push((p.target.clone(), p.source.clone()));
    }

    Ok(planned
        .into_iter()
        .map(|p| MovedEntry {
            source: p.source.to_string_lossy().into_owned(),
            path: p.target.to_string_lossy().into_owned(),
            name: p.name,
            kind: p.kind,
        })
        .collect())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConflictMode {
    Reject,
    Rename,
}

/// Canonicalize `raw` (absolute, no NUL) and assert it clears the A-scope +
/// denylist guard.
fn guarded_canonical(raw: &str, a: &Path, denylist: &[PathBuf]) -> Result<PathBuf, MoveError> {
    let candidate = PathBuf::from(raw);
    if !candidate.is_absolute() || raw.contains('\0') {
        return Err(MoveError::InvalidRequest);
    }
    let canonical = match candidate.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(MoveError::NotFound),
        Err(_) => return Err(MoveError::InvalidRequest),
    };
    if !fs_guard::is_path_allowed(&canonical, a, denylist) {
        return Err(MoveError::Denied);
    }
    Ok(canonical)
}

/// Whether the final component of `raw` is a symlink (lexical lstat).
fn is_symlink(raw: &str) -> bool {
    std::fs::symlink_metadata(raw)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// Resolve a free target (and final name) under `dest_dir` for `name` per the
/// conflict mode, treating both on-disk existence and in-batch `claimed` as
/// collisions.
fn resolve_target(
    dest_dir: &Path,
    name: &str,
    mode: ConflictMode,
    claimed: &HashSet<PathBuf>,
) -> Result<(PathBuf, String), MoveError> {
    let initial = dest_dir.join(name);
    let taken = |p: &Path| p.exists() || claimed.contains(p);
    if !taken(&initial) {
        return Ok((initial, name.to_string()));
    }
    match mode {
        ConflictMode::Reject => Err(MoveError::NameConflict),
        ConflictMode::Rename => {
            let renamed = first_free_suffixed(dest_dir, name, taken);
            Ok((dest_dir.join(&renamed), renamed))
        }
    }
}

/// First `name (N).ext` (N from 2) under `dir` for which `taken` is false.
/// Mirrors `fs_file::next_free_name`'s `stem (N).ext` split (a leading-dot file
/// like `.env` keeps its whole name as the stem; a directory `docs` → `docs (2)`),
/// but takes a `taken` predicate so on-disk existence *and* in-batch claims are
/// both skipped.
fn first_free_suffixed(dir: &Path, name: &str, taken: impl Fn(&Path) -> bool) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut n = 2u32;
    loop {
        let candidate = format!("{stem} ({n}){ext}");
        if !taken(&dir.join(&candidate)) {
            return candidate;
        }
        n = n.saturating_add(1);
        if n == u32::MAX {
            return candidate;
        }
    }
}

/// Read-only walk of a directory source: fail-closed on any symlink descendant
/// or a descendant that leaves the guard. `budget` caps total entries visited.
fn assert_tree_safe(
    dir: &Path,
    a: &Path,
    denylist: &[PathBuf],
    budget: &mut usize,
) -> Result<(), MoveError> {
    let read = std::fs::read_dir(dir).map_err(|_| MoveError::Failed)?;
    for entry in read {
        *budget += 1;
        if *budget > MOVE_WALK_BUDGET {
            return Err(MoveError::InvalidRequest);
        }
        let entry = entry.map_err(|_| MoveError::Failed)?;
        let ft = entry.file_type().map_err(|_| MoveError::Failed)?;
        if ft.is_symlink() {
            return Err(MoveError::InvalidRequest);
        }
        let child = entry.path();
        // `child` = canonical parent + a real (non-symlink) name → canonical;
        // a lexical guard check is sound.
        if !fs_guard::is_path_allowed(&child, a, denylist) {
            return Err(MoveError::Denied);
        }
        if ft.is_dir() {
            assert_tree_safe(&child, a, denylist, budget)?;
        } else if !ft.is_file() {
            // Sockets / fifos / devices — fail closed.
            return Err(MoveError::InvalidRequest);
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn assert_tree_safe_accepts_plain_tree_rejects_symlink() {
        let a = TempDir::new().unwrap();
        let a_root = a.path().canonicalize().unwrap();
        let d = a_root.join("d");
        std::fs::create_dir(&d).unwrap();
        std::fs::write(d.join("a.txt"), b"a").unwrap();
        std::fs::create_dir(d.join("sub")).unwrap();
        std::fs::write(d.join("sub").join("b.txt"), b"b").unwrap();

        let mut budget = 0;
        assert!(assert_tree_safe(&d, &a_root, &[], &mut budget).is_ok());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = TempDir::new().unwrap();
            symlink(outside.path(), d.join("link")).unwrap();
            let mut budget = 0;
            assert!(matches!(
                assert_tree_safe(&d, &a_root, &[], &mut budget),
                Err(MoveError::InvalidRequest)
            ));
        }
    }

    #[test]
    fn resolve_target_reject_and_rename_with_claims() {
        let dir = TempDir::new().unwrap();
        let d = dir.path();
        std::fs::write(d.join("a.md"), b"x").unwrap();
        let mut claimed: HashSet<PathBuf> = HashSet::new();

        // reject → conflict on existing.
        assert!(matches!(
            resolve_target(d, "a.md", ConflictMode::Reject, &claimed),
            Err(MoveError::NameConflict)
        ));
        // rename → "a (2).md".
        let (t, n) = resolve_target(d, "a.md", ConflictMode::Rename, &claimed).unwrap();
        assert_eq!(n, "a (2).md");
        claimed.insert(t);
        // second rename of the same name must skip the claim → "a (3).md".
        let (_t2, n2) = resolve_target(d, "a.md", ConflictMode::Rename, &claimed).unwrap();
        assert_eq!(n2, "a (3).md");
        // a fresh name lands as-is.
        let (_t3, n3) = resolve_target(d, "fresh.md", ConflictMode::Reject, &claimed).unwrap();
        assert_eq!(n3, "fresh.md");
    }
}
