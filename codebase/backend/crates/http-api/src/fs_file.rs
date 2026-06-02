//! ADR-0047 D2 / D3 — Server Workspace(A)-scoped file *bytes* in and out:
//!   GET  /api/fs/file?path=<abs>   — stream a workspace file (image/document render source)
//!   POST /api/fs/upload            — multipart upload into a workspace directory
//!
//! These complete the workspace-sourced-asset model (ADR-0047, supersedes the
//! `/api/assets` content-hash store): image/document canvas items now hold a
//! workspace(B)-relative `path` instead of an opaque `asset_id`. The FE
//! resolves that relative path → absolute against the session's effective
//! `workspace_root` (delivered in the attach response, F1b) and fetches the
//! bytes from `GET /api/fs/file`.
//!
//! Both endpoints share the single [`crate::fs_guard`] sandbox: a path must
//! `canonicalize` *inside the Server Workspace(A)* **and** outside the M2
//! denylist (Store / gtmux config / gtmux state — ADR-0045 D6). All paths on
//! the wire are **absolute**; the B-relative form lives only on canvas items,
//! and the FE bridges the two (`GET /api/fs/list` likewise returns absolute
//! paths). MIME is magic-byte sniffed (never trusted from the client) by the
//! same allowlist the asset store used (ADR-0033 D3/D4), reused from
//! [`crate::assets`].

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::{Multipart, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::assets::{sniff_allowed_upload, sniff_any, SVG_SERVE_CSP};
use crate::fs_guard;
use crate::AppState;

// ─────────────────────────────────────────────────────────────────────────────
//  GET /api/fs/file — serve workspace file bytes (ADR-0047 D3)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct FsFileQuery {
    /// Absolute path of the file to serve. Must resolve inside A and outside
    /// the denylist.
    pub path: String,
}

/// `GET /api/fs/file?path=<abs>` — ADR-0047 D3. Streams a workspace file's
/// bytes with a magic-byte-sniffed `Content-Type`. The ETag is `"<mtime>-<size>"`
/// (workspace files are mutable, unlike the content-hash asset store, so the
/// cache is revalidation-based — `Cache-Control: no-cache` + `If-None-Match`
/// → 304). Range requests are P2 (not implemented).
///
/// Outcomes: 200 (bytes) / 304 (If-None-Match) / 400 invalid_path /
/// 400 not_a_file / 403 path_not_allowed / 404 file_not_found / 500 read_failed.
pub async fn fs_file_serve_handler(
    State(state): State<AppState>,
    Query(q): Query<FsFileQuery>,
    req: axum::http::Request<Body>,
) -> Response {
    let server_workspace = state.server_workspace.as_path();
    let candidate = PathBuf::from(&q.path);
    if !candidate.is_absolute() || q.path.contains('\0') {
        return error(StatusCode::BAD_REQUEST, "invalid_path");
    }
    let canonical = match candidate.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return error(StatusCode::NOT_FOUND, "file_not_found");
        }
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_path"),
    };
    // Single hard boundary: inside A AND outside the denylist (ADR-0045 D6).
    if !fs_guard::is_path_allowed(&canonical, server_workspace, &state.fs_denylist) {
        return error(StatusCode::FORBIDDEN, "path_not_allowed");
    }

    let meta = match std::fs::metadata(&canonical) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return error(StatusCode::NOT_FOUND, "file_not_found");
        }
        Err(_) => return internal_500("stat_failed"),
    };
    if !meta.is_file() {
        return error(StatusCode::BAD_REQUEST, "not_a_file");
    }

    // ETag = mtime(nanos)-size. Mutable files → revalidate via If-None-Match.
    let mtime = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let etag = format!("\"{mtime}-{}\"", meta.len());
    if let Some(inm) = req.headers().get(header::IF_NONE_MATCH) {
        if inm.to_str().map(|v| v == etag).unwrap_or(false) {
            return Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, &etag)
                .header(header::CACHE_CONTROL, "no-cache")
                .body(Body::empty())
                .expect("static headers");
        }
    }

    // Read off the blocking pool so a large image doesn't stall a worker.
    let read_path = canonical.clone();
    let bytes = match tokio::task::spawn_blocking(move || std::fs::read(&read_path)).await {
        Ok(Ok(b)) => b,
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return error(StatusCode::NOT_FOUND, "file_not_found");
        }
        Ok(Err(_)) | Err(_) => return internal_500("read_failed"),
    };

    let mime = sniff_any(&bytes);
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, bytes.len())
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ETAG, &etag);
    if mime == "image/svg+xml" {
        // Direct navigation to an SVG renders it as a top-level document where
        // inline scripts execute — stamp the same hardening CSP the asset
        // serve uses (inside <img> the browser already sandboxes it).
        builder = builder.header("Content-Security-Policy", SVG_SERVE_CSP);
    }
    builder
        .body(Body::from(bytes))
        .unwrap_or_else(|_| internal_500("response_build_failed"))
}

// ─────────────────────────────────────────────────────────────────────────────
//  POST /api/fs/upload — multipart upload into a workspace dir (ADR-0047 D2)
// ─────────────────────────────────────────────────────────────────────────────

/// ADR-0047 D2 — what to do when an uploaded file's name already exists in the
/// target directory. Wire values `reject` (default) / `rename` / `overwrite`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConflictMode {
    Reject,
    Rename,
    Overwrite,
}

impl ConflictMode {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "reject" => Some(Self::Reject),
            "rename" => Some(Self::Rename),
            "overwrite" => Some(Self::Overwrite),
            _ => None,
        }
    }
}

/// One validated, ready-to-write file from the multipart body.
struct PendingFile {
    name: String,
    bytes: Vec<u8>,
    mime: &'static str,
}

#[derive(Debug, Serialize)]
struct UploadedFile {
    /// Absolute path of the written file (FE relativizes against the session's
    /// effective `workspace_root` to form the canvas item's B-relative path).
    path: String,
    /// Final on-disk filename (may differ from the uploaded name under `rename`).
    name: String,
    mime: &'static str,
    size: u64,
    /// `true` iff the original name collided (renamed under `rename`, replaced
    /// under `overwrite`).
    conflict: bool,
}

/// `POST /api/fs/upload` — ADR-0047 D2. Multipart body:
///   * `dir`         — absolute target directory (inside A + outside denylist).
///   * `on_conflict` — `reject` (default) / `rename` / `overwrite`.
///   * `file` (or `files`) — one or more file parts.
///
/// Outcomes: 201 `{ files: [...] }` / 400 (bad request / empty / bad filename /
/// bad on_conflict) / 403 dir_not_allowed / 409 name_conflict (reject) /
/// 413 payload_too_large / 415 unsupported_media_type / 500.
pub async fn fs_upload_handler(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Response {
    let server_workspace = state.server_workspace.as_path();
    let max_size_bytes = state.config.assets.max_size_bytes;

    let mut dir: Option<String> = None;
    let mut on_conflict: Option<String> = None;
    let mut raw_files: Vec<(Option<String>, Vec<u8>)> = Vec::new();

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                let msg = e.to_string();
                let (status, code) =
                    if e.status() == StatusCode::PAYLOAD_TOO_LARGE || msg.contains("size limit") {
                        (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large")
                    } else {
                        (StatusCode::BAD_REQUEST, "bad_multipart")
                    };
                return error(status, code);
            }
        };
        match field.name() {
            Some("dir") => match field.text().await {
                Ok(t) => dir = Some(t),
                Err(_) => return error(StatusCode::BAD_REQUEST, "bad_multipart"),
            },
            Some("on_conflict") => match field.text().await {
                Ok(t) => on_conflict = Some(t),
                Err(_) => return error(StatusCode::BAD_REQUEST, "bad_multipart"),
            },
            Some("file") | Some("files") => {
                let file_name = field.file_name().map(|s| s.to_string());
                match field.bytes().await {
                    Ok(b) => {
                        if b.len() as u64 > max_size_bytes {
                            return error(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large");
                        }
                        raw_files.push((file_name, b.to_vec()));
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        let (status, code) = if e.status() == StatusCode::PAYLOAD_TOO_LARGE
                            || msg.contains("size limit")
                        {
                            (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large")
                        } else {
                            (StatusCode::BAD_REQUEST, "bad_multipart")
                        };
                        return error(status, code);
                    }
                }
            }
            _ => {
                // Drain unknown fields so the loop advances.
                let _ = field.bytes().await;
            }
        }
    }

    // on_conflict (default reject).
    let mode = match on_conflict.as_deref() {
        None => ConflictMode::Reject,
        Some(s) => match ConflictMode::parse(s) {
            Some(m) => m,
            None => return error(StatusCode::BAD_REQUEST, "bad_on_conflict"),
        },
    };

    // dir — required, absolute, inside A, outside denylist, a directory.
    let Some(dir_raw) = dir else {
        return error(StatusCode::BAD_REQUEST, "missing_dir");
    };
    let dir_path = PathBuf::from(&dir_raw);
    if !dir_path.is_absolute() || dir_raw.contains('\0') {
        return error(StatusCode::BAD_REQUEST, "invalid_dir");
    }
    let dir_canonical = match dir_path.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return error(StatusCode::BAD_REQUEST, "dir_not_found");
        }
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_dir"),
    };
    if !fs_guard::is_path_allowed(&dir_canonical, server_workspace, &state.fs_denylist) {
        return error(StatusCode::FORBIDDEN, "dir_not_allowed");
    }
    if !dir_canonical.is_dir() {
        return error(StatusCode::BAD_REQUEST, "dir_not_a_directory");
    }

    if raw_files.is_empty() {
        return error(StatusCode::BAD_REQUEST, "missing_file");
    }

    // Validate every file (name shape, non-empty, MIME allowlist) before any
    // write so a bad part fails the whole batch atomically (nothing written).
    let mut pending: Vec<PendingFile> = Vec::with_capacity(raw_files.len());
    for (raw_name, bytes) in raw_files {
        if bytes.is_empty() {
            return error(StatusCode::BAD_REQUEST, "empty_file");
        }
        let Some(name) = raw_name.as_deref().and_then(sanitize_filename) else {
            return error(StatusCode::BAD_REQUEST, "bad_filename");
        };
        let Some(mime) = sniff_allowed_upload(&bytes) else {
            return error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type");
        };
        pending.push(PendingFile { name, bytes, mime });
    }

    // Conflict-resolve + write on the blocking pool. `reject` is pre-checked
    // across the whole batch (write nothing on any collision).
    let dir_for_write = dir_canonical.clone();
    let write_result = tokio::task::spawn_blocking(move || write_uploads(&dir_for_write, pending, mode))
        .await;
    match write_result {
        Ok(Ok(files)) => (StatusCode::CREATED, Json(json!({ "files": files }))).into_response(),
        Ok(Err(UploadFailure::Conflict(name))) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "name_conflict", "name": name })),
        )
            .into_response(),
        Ok(Err(UploadFailure::Io(reason))) => internal_500(reason),
        Err(_) => internal_500("upload_task_panicked"),
    }
}

#[derive(Debug)]
enum UploadFailure {
    /// `reject` mode: a target name already exists.
    Conflict(String),
    Io(&'static str),
}

/// Resolve conflicts per [`ConflictMode`] and write each file. In `reject`
/// mode every target is pre-checked first so a collision writes nothing.
fn write_uploads(
    dir: &Path,
    pending: Vec<PendingFile>,
    mode: ConflictMode,
) -> Result<Vec<UploadedFile>, UploadFailure> {
    if mode == ConflictMode::Reject {
        for f in &pending {
            if dir.join(&f.name).exists() {
                return Err(UploadFailure::Conflict(f.name.clone()));
            }
        }
    }
    let mut out = Vec::with_capacity(pending.len());
    for f in pending {
        let (target, final_name, conflict) = match mode {
            ConflictMode::Reject => (dir.join(&f.name), f.name.clone(), false),
            ConflictMode::Overwrite => {
                let collided = dir.join(&f.name).exists();
                (dir.join(&f.name), f.name.clone(), collided)
            }
            ConflictMode::Rename => {
                if dir.join(&f.name).exists() {
                    let renamed = next_free_name(dir, &f.name);
                    (dir.join(&renamed), renamed, true)
                } else {
                    (dir.join(&f.name), f.name.clone(), false)
                }
            }
        };
        if let Err(_e) = std::fs::write(&target, &f.bytes) {
            return Err(UploadFailure::Io("write_failed"));
        }
        out.push(UploadedFile {
            path: target.to_string_lossy().into_owned(),
            name: final_name,
            mime: f.mime,
            size: f.bytes.len() as u64,
            conflict,
        });
    }
    Ok(out)
}

/// Produce the first non-colliding `name (N).ext` (N starts at 2) for `name`
/// inside `dir`. Splits on the final extension so `a.tar.gz` → `a.tar (2).gz`
/// (acceptable for MVP; the common case is a single extension). A name with no
/// extension (e.g. a directory `docs`) becomes `docs (2)`. Shared with the
/// `/api/fs/copy` paste flow (ADR-0047 D10).
pub(crate) fn next_free_name(dir: &Path, name: &str) -> String {
    let (stem, ext) = match name.rfind('.') {
        // Keep a leading-dot file (e.g. `.env`) intact — no split.
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut n = 2u32;
    loop {
        let candidate = format!("{stem} ({n}){ext}");
        if !dir.join(&candidate).exists() {
            return candidate;
        }
        n = n.saturating_add(1);
        if n == u32::MAX {
            // Pathological dir — fall back to appending the count anyway.
            return format!("{stem} ({n}){ext}");
        }
    }
}

/// Reduce a client-supplied multipart filename to a safe single path
/// component: strip any directory parts, reject empty / `.` / `..` / NUL.
/// Returns `None` when nothing safe remains.
fn sanitize_filename(raw: &str) -> Option<String> {
    if raw.contains('\0') {
        return None;
    }
    // Take the basename — clients may send "a/b/c.png" or a Windows path.
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw).trim();
    if base.is_empty() || base == "." || base == ".." {
        return None;
    }
    Some(base.to_string())
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn sanitize_filename_strips_dirs_and_rejects_traversal() {
        assert_eq!(sanitize_filename("a.png").as_deref(), Some("a.png"));
        assert_eq!(sanitize_filename("x/y/a.png").as_deref(), Some("a.png"));
        assert_eq!(sanitize_filename("c:\\d\\a.png").as_deref(), Some("a.png"));
        assert_eq!(sanitize_filename(".env").as_deref(), Some(".env"));
        assert_eq!(sanitize_filename(""), None);
        assert_eq!(sanitize_filename(".."), None);
        assert_eq!(sanitize_filename("."), None);
        assert_eq!(sanitize_filename("a\0b"), None);
        // A trailing-slash path reduces to empty basename → rejected.
        assert_eq!(sanitize_filename("a/b/"), None);
    }

    #[test]
    fn next_free_name_increments_suffix() {
        let dir = TempDir::new().unwrap();
        let d = dir.path();
        assert_eq!(next_free_name(d, "a.png"), "a (2).png");
        std::fs::write(d.join("a (2).png"), b"x").unwrap();
        assert_eq!(next_free_name(d, "a.png"), "a (3).png");
        // No-extension name.
        assert_eq!(next_free_name(d, "README"), "README (2)");
        // Leading-dot file keeps its name as the stem.
        assert_eq!(next_free_name(d, ".env"), ".env (2)");
    }

    #[test]
    fn write_uploads_reject_mode_is_atomic_on_conflict() {
        let dir = TempDir::new().unwrap();
        let d = dir.path();
        std::fs::write(d.join("dup.txt"), b"existing").unwrap();
        let pending = vec![
            PendingFile {
                name: "fresh.txt".into(),
                bytes: b"hello".to_vec(),
                mime: "text/plain",
            },
            PendingFile {
                name: "dup.txt".into(),
                bytes: b"new".to_vec(),
                mime: "text/plain",
            },
        ];
        let res = write_uploads(d, pending, ConflictMode::Reject);
        assert!(matches!(res, Err(UploadFailure::Conflict(n)) if n == "dup.txt"));
        // Nothing from the batch was written (fresh.txt must not exist).
        assert!(!d.join("fresh.txt").exists());
        assert_eq!(std::fs::read(d.join("dup.txt")).unwrap(), b"existing");
    }

    #[test]
    fn write_uploads_rename_and_overwrite() {
        let dir = TempDir::new().unwrap();
        let d = dir.path();
        std::fs::write(d.join("a.txt"), b"old").unwrap();

        let renamed = write_uploads(
            d,
            vec![PendingFile {
                name: "a.txt".into(),
                bytes: b"r".to_vec(),
                mime: "text/plain",
            }],
            ConflictMode::Rename,
        )
        .unwrap();
        assert_eq!(renamed[0].name, "a (2).txt");
        assert!(renamed[0].conflict);
        assert_eq!(std::fs::read(d.join("a.txt")).unwrap(), b"old");
        assert_eq!(std::fs::read(d.join("a (2).txt")).unwrap(), b"r");

        let overwritten = write_uploads(
            d,
            vec![PendingFile {
                name: "a.txt".into(),
                bytes: b"new".to_vec(),
                mime: "text/plain",
            }],
            ConflictMode::Overwrite,
        )
        .unwrap();
        assert!(overwritten[0].conflict);
        assert_eq!(std::fs::read(d.join("a.txt")).unwrap(), b"new");
    }
}
