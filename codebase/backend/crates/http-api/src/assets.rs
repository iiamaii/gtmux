//! `/api/assets` — content-addressed binary store for image/document items
//! (ADR-0033, 0080 BE handover).
//!
//! Endpoints:
//!   * `POST   /api/assets`             — multipart upload, sha256 dedupe.
//!   * `POST   /api/assets/from-path`   — copy a workspace file into assets.
//!   * `GET    /api/assets/{asset_id}`  — serve raw bytes, immutable cache.
//!
//! Storage layout: `<workspace>/.assets/<sha256_hex>`. The file is named by
//! its sha256 digest so identical bytes uploaded twice hit the same record
//! (idempotent). `asset_id` is validated against `[a-f0-9]{64}` before any
//! path resolution, so `..`-style traversal is impossible by construction.
//!
//! MIME policy (0080 §3 — MVP scope):
//!   * `kind=image`    → png / jpeg / gif / webp / svg+xml
//!   * `kind=document` → text/plain · text/markdown · application/json · application/pdf
//!
//! Detection is magic-byte based — the multipart `Content-Type` field is
//! never trusted on its own. SVG falls back to a lightweight text-shape
//! check (must start with `<?xml` or `<svg`).
//!
//! Size cap: configurable via `Config.assets.max_size_bytes` (default 50 MiB).
//! Enforced by a route-scoped
//! [`axum::extract::DefaultBodyLimit`] *and* a defensive recount inside the
//! handler (axum's limit is best-effort against chunked uploads). Over-cap
//! → 413.
//!
//! For images we also extract `original_w` / `original_h` so the FE can
//! seed the panel with the natural aspect ratio. PNG / JPEG / GIF / WebP
//! parse a few bytes each; SVG declines (parsing the root `width=` /
//! `viewBox=` attribute reliably needs a real XML parser — skipped for MVP).
//!
//! Auth: this module piggybacks on the bearer/cookie middleware in
//! `lib.rs::bearer_auth_middleware` — `/api/*` is gated at the router
//! level, so handlers here can assume the request is authenticated.

use std::path::{Path, PathBuf};

use axum::extract::{Multipart, Path as AxumPath, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use ring::digest::{Context as DigestContext, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::warn;

use crate::{AppState, WorkspaceManager};

/// Multipart framing headroom added on top of configured asset byte limit.
pub(crate) const ASSET_MULTIPART_HEADROOM_BYTES: usize = 1024 * 1024;

/// Stored asset filename = 64 lowercase hex characters of sha256(bytes).
const ASSET_ID_HEX_LEN: usize = 64;

/// Asset upload response — ADR-0033 D5 + 0080 §2.1.
#[derive(Debug, Serialize)]
struct UploadResponse {
    asset_id: String,
    mime: &'static str,
    file_name: String,
    size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    original_w: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    original_h: Option<u32>,
}

/// `POST /api/assets/from-path` request body.
#[derive(Debug, Deserialize)]
pub struct UploadFromPathRequest {
    path: String,
    kind: String,
}

/// `POST /api/assets` — multipart upload of an image or document file.
///
/// Fields:
///   * `file` (required) — the file binary.
///   * `kind` (required) — `"image"` or `"document"`.
///
/// Returns 201 on first write *and* on idempotent re-upload (same bytes
/// → same `asset_id`). The handler does not distinguish between these two
/// cases on the wire — same sha256 always returns the same shape. ADR-0033
/// D8 (deduplication) is the underlying guarantee.
pub async fn upload_handler(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    let Some(wm) = state.workspace.as_ref().cloned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "workspace_not_configured" })),
        )
            .into_response();
    };
    let max_size_bytes = state.config.assets.max_size_bytes;

    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;
    let mut kind: Option<String> = None;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                // axum's MultipartError covers oversize + malformed boundaries
                // both. Surface the size case as 413 explicitly so the FE can
                // tell a hostile boundary apart from "your file is too big".
                let msg = e.to_string();
                let (status, code) =
                    if e.status() == StatusCode::PAYLOAD_TOO_LARGE || msg.contains("size limit") {
                        (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large")
                    } else {
                        (StatusCode::BAD_REQUEST, "bad_multipart")
                    };
                return (status, Json(json!({ "error": code, "message": msg }))).into_response();
            }
        };
        match field.name() {
            Some("file") => {
                if file_bytes.is_some() {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "duplicate_file_field" })),
                    )
                        .into_response();
                }
                file_name = field.file_name().map(|s| s.to_string());
                match field.bytes().await {
                    Ok(b) => {
                        if b.len() as u64 > max_size_bytes {
                            return (
                                StatusCode::PAYLOAD_TOO_LARGE,
                                Json(json!({ "error": "payload_too_large" })),
                            )
                                .into_response();
                        }
                        file_bytes = Some(b.to_vec());
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
                        return (status, Json(json!({ "error": code, "message": msg })))
                            .into_response();
                    }
                }
            }
            Some("kind") => match field.text().await {
                Ok(t) => kind = Some(t),
                Err(_) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "bad_multipart" })),
                    )
                        .into_response();
                }
            },
            _ => {
                // Drain unknown fields so the next_field loop advances.
                let _ = field.bytes().await;
            }
        }
    }

    let Some(bytes) = file_bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing_file" })),
        )
            .into_response();
    };
    if bytes.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "empty_file" })),
        )
            .into_response();
    }
    let kind = match kind.as_deref() {
        Some("image") => AssetKind::Image,
        Some("document") => AssetKind::Document,
        Some(other) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_kind", "kind": other })),
            )
                .into_response();
        }
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "missing_kind" })),
            )
                .into_response();
        }
    };

    store_asset_response(
        &wm,
        bytes,
        file_name.unwrap_or_else(|| "asset".to_string()),
        kind,
    )
}

/// `POST /api/assets/from-path` — copy a workspace file into the asset store.
///
/// This is the server-side counterpart to the existing file-system picker:
/// the picker returns an absolute path inside the configured workspace, and
/// this handler reads that file, validates its MIME against `kind`, then stores
/// it through the same content-addressed path as multipart upload.
pub async fn upload_from_path_handler(
    State(state): State<AppState>,
    Json(req): Json<UploadFromPathRequest>,
) -> Response {
    let Some(wm) = state.workspace.as_ref().cloned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "workspace_not_configured" })),
        )
            .into_response();
    };
    let max_size_bytes = state.config.assets.max_size_bytes;

    let kind = match req.kind.as_str() {
        "image" => AssetKind::Image,
        "document" => AssetKind::Document,
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_kind", "kind": other })),
            )
                .into_response();
        }
    };

    let raw = PathBuf::from(&req.path);
    let canonical = match raw.canonicalize() {
        Ok(path) => path,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "source_not_found" })),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_source_path" })),
            )
                .into_response();
        }
    };

    let workspace_canonical = match wm.path().canonicalize() {
        Ok(path) => path,
        Err(e) => {
            warn!(error = %e, "assets: workspace canonicalize failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "workspace_canonicalize_failed" })),
            )
                .into_response();
        }
    };
    if !canonical.starts_with(&workspace_canonical) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "source_not_allowed" })),
        )
            .into_response();
    }
    if !canonical.is_file() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "source_not_file" })),
        )
            .into_response();
    }

    let meta = match std::fs::metadata(&canonical) {
        Ok(meta) => meta,
        Err(e) => {
            warn!(error = %e, path = %canonical.display(), "assets: stat source failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "source_stat_failed" })),
            )
                .into_response();
        }
    };
    if meta.len() > max_size_bytes {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "error": "payload_too_large" })),
        )
            .into_response();
    }

    let bytes = match std::fs::read(&canonical) {
        Ok(bytes) => bytes,
        Err(e) => {
            warn!(error = %e, path = %canonical.display(), "assets: read source failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "source_read_failed" })),
            )
                .into_response();
        }
    };
    if bytes.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "empty_file" })),
        )
            .into_response();
    }

    let file_name = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("asset")
        .to_string();
    store_asset_response(&wm, bytes, file_name, kind)
}

fn store_asset_response(
    wm: &WorkspaceManager,
    bytes: Vec<u8>,
    file_name: String,
    kind: AssetKind,
) -> Response {
    let mime = match sniff_mime(&bytes, kind) {
        Some(m) => m,
        None => {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(json!({ "error": "unsupported_media_type", "kind": kind.as_str() })),
            )
                .into_response();
        }
    };

    let asset_id = sha256_hex(&bytes);
    let dest = match wm.ensure_assets_dir() {
        Ok(dir) => dir.join(&asset_id),
        Err(e) => {
            warn!(error = %e, "assets: ensure_assets_dir failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "save_failed" })),
            )
                .into_response();
        }
    };

    if let Err(e) = atomic_write_asset(&dest, &bytes) {
        warn!(error = %e, path = %dest.display(), "assets: write failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "save_failed" })),
        )
            .into_response();
    }

    let (original_w, original_h) = if matches!(kind, AssetKind::Image) {
        image_dimensions(mime, &bytes).unwrap_or((None, None))
    } else {
        (None, None)
    };

    let resp = UploadResponse {
        asset_id,
        mime,
        file_name,
        size_bytes: bytes.len() as u64,
        original_w,
        original_h,
    };
    (StatusCode::CREATED, Json(resp)).into_response()
}

/// `GET /api/assets/{asset_id}` — serve raw bytes with the stored MIME.
///
/// `asset_id` is path-validated against the 64-char lowercase-hex shape
/// before any FS access. SVG responses carry a hardened CSP so direct
/// navigation (`<address-bar>/api/assets/<svg>`) cannot execute scripts.
pub async fn serve_handler(
    State(state): State<AppState>,
    AxumPath(asset_id): AxumPath<String>,
) -> Response {
    let Some(wm) = state.workspace.as_ref().cloned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "workspace_not_configured" })),
        )
            .into_response();
    };

    if !is_valid_asset_id(&asset_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_asset_id" })),
        )
            .into_response();
    }

    let path = wm.assets_dir().join(&asset_id);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "asset_not_found" })),
            )
                .into_response();
        }
        Err(e) => {
            warn!(error = %e, path = %path.display(), "assets: read failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "read_failed" })),
            )
                .into_response();
        }
    };

    // Re-sniff at serve time so a tampered-with on-disk file can't masquerade
    // as a different content type. Falls back to application/octet-stream if
    // sniff fails (which shouldn't happen — uploads are sniff-gated).
    let mime = sniff_mime(&bytes, AssetKind::Image)
        .or_else(|| sniff_mime(&bytes, AssetKind::Document))
        .unwrap_or("application/octet-stream");

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, bytes.len())
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .header(header::ETAG, format!("\"{asset_id}\""));

    if mime == "image/svg+xml" {
        // SVG inside <img> is sandboxed by the browser, but direct navigation
        // (typing the URL in the address bar) renders it as a top-level
        // document where scripts CAN execute. Stamp a CSP that neutralises
        // any inline <script>, foreignObject script, or event handler.
        builder = builder.header(
            "Content-Security-Policy",
            HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; sandbox"),
        );
    }

    builder
        .body(axum::body::Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum AssetKind {
    Image,
    Document,
}

impl AssetKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Document => "document",
        }
    }
}

/// Detect the MIME from magic bytes. Returns the canonical static string
/// matching the allowlist for `kind`, or `None` if the content doesn't
/// belong to the declared kind.
fn sniff_mime(bytes: &[u8], kind: AssetKind) -> Option<&'static str> {
    match kind {
        AssetKind::Image => sniff_image(bytes),
        AssetKind::Document => sniff_document(bytes),
    }
}

fn sniff_image(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if b.starts_with(b"\xFF\xD8\xFF") {
        return Some("image/jpeg");
    }
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if looks_like_svg(b) {
        return Some("image/svg+xml");
    }
    None
}

fn sniff_document(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"%PDF-") {
        return Some("application/pdf");
    }
    // JSON: must be valid UTF-8 and parse to a serde_json::Value. The leading
    // BOM is stripped to match common text editors.
    let trimmed = strip_utf8_bom(b);
    if let Ok(s) = std::str::from_utf8(trimmed) {
        let t = s.trim_start();
        if (t.starts_with('{') || t.starts_with('['))
            && serde_json::from_str::<serde_json::Value>(s).is_ok()
        {
            return Some("application/json");
        }
        // Plain text fallback — accept any valid UTF-8 that survives
        // a NUL-byte sanity check. This covers .txt, .md, .log, .csv,
        // and similar source files the user would reasonably drop in.
        if !s.as_bytes().contains(&0) {
            return Some("text/plain");
        }
    }
    None
}

fn looks_like_svg(b: &[u8]) -> bool {
    // Cap the inspection window — a 20 MiB file with a `<svg>` 19 MiB in is
    // still SVG, but we only need the head. SVG ships with `<?xml` or `<svg`
    // near the start, whitespace permitted.
    let head_len = b.len().min(2048);
    let Ok(head) = std::str::from_utf8(strip_utf8_bom(&b[..head_len])) else {
        return false;
    };
    let trimmed = head.trim_start();
    trimmed.starts_with("<?xml") && trimmed.contains("<svg") || trimmed.starts_with("<svg")
}

fn strip_utf8_bom(b: &[u8]) -> &[u8] {
    b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut ctx = DigestContext::new(&SHA256);
    ctx.update(bytes);
    let d = ctx.finish();
    let mut hex = String::with_capacity(ASSET_ID_HEX_LEN);
    for b in d.as_ref() {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

fn is_valid_asset_id(s: &str) -> bool {
    s.len() == ASSET_ID_HEX_LEN && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Best-effort dimension extraction. Returns `(Some(w), Some(h))` for
/// formats we can parse cheaply (PNG / JPEG / GIF / WebP-VP8X) and
/// `(None, None)` for everything else.
fn image_dimensions(mime: &str, b: &[u8]) -> Option<(Option<u32>, Option<u32>)> {
    let dims = match mime {
        "image/png" => png_dimensions(b),
        "image/jpeg" => jpeg_dimensions(b),
        "image/gif" => gif_dimensions(b),
        "image/webp" => webp_dimensions(b),
        _ => None,
    };
    Some((dims.map(|d| d.0), dims.map(|d| d.1)))
}

fn png_dimensions(b: &[u8]) -> Option<(u32, u32)> {
    // 8-byte signature + 4-byte chunk-size + "IHDR" + width(BE u32) + height(BE u32)
    if b.len() < 24 || !b.starts_with(b"\x89PNG\r\n\x1a\n") || &b[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(b[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(b[20..24].try_into().ok()?);
    Some((w, h))
}

fn jpeg_dimensions(b: &[u8]) -> Option<(u32, u32)> {
    // Walk JPEG markers until SOF0..SOF15 (excluding SOF4 / SOF8 / SOF12 which
    // are reserved/non-frame). The Start-Of-Frame block carries height
    // (2 bytes BE) then width (2 bytes BE) at offsets 5–8 from the marker.
    if b.len() < 4 || !b.starts_with(b"\xFF\xD8") {
        return None;
    }
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return None;
        }
        // Skip fill bytes (0xFF 0xFF padding).
        let mut j = i + 1;
        while j < b.len() && b[j] == 0xFF {
            j += 1;
        }
        if j >= b.len() {
            return None;
        }
        let marker = b[j];
        i = j + 1;
        // Standalone markers (no payload) — should not appear before SOF.
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if i + 2 > b.len() {
            return None;
        }
        let seg_len = u16::from_be_bytes([b[i], b[i + 1]]) as usize;
        if seg_len < 2 || i + seg_len > b.len() {
            return None;
        }
        let is_sof = matches!(marker,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF);
        if is_sof {
            if seg_len < 7 {
                return None;
            }
            let h = u16::from_be_bytes([b[i + 3], b[i + 4]]) as u32;
            let w = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
            return Some((w, h));
        }
        i += seg_len;
    }
    None
}

fn gif_dimensions(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 10 || !(b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")) {
        return None;
    }
    let w = u16::from_le_bytes([b[6], b[7]]) as u32;
    let h = u16::from_le_bytes([b[8], b[9]]) as u32;
    Some((w, h))
}

fn webp_dimensions(b: &[u8]) -> Option<(u32, u32)> {
    // RIFF / WebP container — minimum 30 bytes for VP8/VP8L/VP8X header.
    if b.len() < 30 || &b[..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return None;
    }
    match &b[12..16] {
        // Lossy VP8: width @ 26 (14 bits LE), height @ 28 (14 bits LE).
        b"VP8 " => {
            if b.len() < 30 {
                return None;
            }
            let w = u16::from_le_bytes([b[26], b[27]]) as u32 & 0x3FFF;
            let h = u16::from_le_bytes([b[28], b[29]]) as u32 & 0x3FFF;
            Some((w, h))
        }
        // Lossless VP8L: 14-bit width-1, then 14-bit height-1 starting at byte 21.
        b"VP8L" => {
            if b.len() < 25 {
                return None;
            }
            let v = u32::from_le_bytes([b[21], b[22], b[23], b[24]]);
            let w = (v & 0x3FFF) + 1;
            let h = ((v >> 14) & 0x3FFF) + 1;
            Some((w, h))
        }
        // Extended VP8X: 24-bit width-1 @ byte 24, 24-bit height-1 @ byte 27.
        b"VP8X" => {
            if b.len() < 30 {
                return None;
            }
            let w = (b[24] as u32 | ((b[25] as u32) << 8) | ((b[26] as u32) << 16)) + 1;
            let h = (b[27] as u32 | ((b[28] as u32) << 8) | ((b[29] as u32) << 16)) + 1;
            Some((w, h))
        }
        _ => None,
    }
}

/// Atomic write: tempfile + rename. Idempotent — re-uploading the same
/// bytes overwrites the file with identical content. The directory is
/// already guaranteed mode 0700 by [`WorkspaceManager::ensure_assets_dir`].
fn atomic_write_asset(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt as StdOpenOptionsExt;

    use atomic_write_file::unix::OpenOptionsExt as AwfOpenOptionsExt;
    use atomic_write_file::OpenOptions as AwfOpenOptions;

    // Mark the silly-import for non-unix targets — this module is unix-only
    // until ADR-0033 carves out cross-platform support.
    let _: fn(&mut std::fs::OpenOptions, u32) -> &mut std::fs::OpenOptions =
        StdOpenOptionsExt::mode;

    let mut f = AwfOpenOptions::new()
        .mode(0o600)
        .preserve_mode(false)
        .open(dest)?;
    f.write_all(bytes)?;
    f.commit()?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod unit_tests {
    use super::*;

    fn make_png(width: u32, height: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        v.extend_from_slice(&[0, 0, 0, 13]); // IHDR length
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&width.to_be_bytes());
        v.extend_from_slice(&height.to_be_bytes());
        // bit_depth + color_type + compression + filter + interlace
        v.extend_from_slice(&[8, 6, 0, 0, 0]);
        // CRC placeholder — sniff ignores it
        v.extend_from_slice(&[0, 0, 0, 0]);
        v
    }

    fn make_gif(width: u16, height: u16) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"GIF89a");
        v.extend_from_slice(&width.to_le_bytes());
        v.extend_from_slice(&height.to_le_bytes());
        v.push(0); // packed
        v.push(0); // bg color
        v.push(0); // aspect
        v
    }

    fn make_webp_vp8x(width_minus_1: u32, height_minus_1: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&[0u8; 4]); // size, ignored by sniff
        v.extend_from_slice(b"WEBP");
        v.extend_from_slice(b"VP8X");
        v.extend_from_slice(&[10, 0, 0, 0]); // chunk size
        v.extend_from_slice(&[0, 0, 0, 0]); // flags + reserved
        let w = width_minus_1.to_le_bytes();
        v.extend_from_slice(&w[..3]);
        let h = height_minus_1.to_le_bytes();
        v.extend_from_slice(&h[..3]);
        v
    }

    #[test]
    fn sniff_image_png() {
        assert_eq!(sniff_image(&make_png(10, 20)), Some("image/png"));
    }

    #[test]
    fn sniff_image_jpeg() {
        assert_eq!(sniff_image(b"\xFF\xD8\xFF\xE0..."), Some("image/jpeg"));
    }

    #[test]
    fn sniff_image_gif() {
        assert_eq!(sniff_image(&make_gif(8, 9)), Some("image/gif"));
    }

    #[test]
    fn sniff_image_webp() {
        assert_eq!(sniff_image(&make_webp_vp8x(99, 199)), Some("image/webp"));
    }

    #[test]
    fn sniff_image_svg() {
        assert_eq!(
            sniff_image(b"<svg xmlns='http://www.w3.org/2000/svg'><rect/></svg>"),
            Some("image/svg+xml")
        );
        assert_eq!(
            sniff_image(b"<?xml version='1.0'?><svg></svg>"),
            Some("image/svg+xml")
        );
    }

    #[test]
    fn sniff_image_rejects_non_image() {
        assert_eq!(sniff_image(b"<html>"), None);
        assert_eq!(sniff_image(b"%PDF-1.4"), None);
        assert_eq!(sniff_image(b"random binary \x01\x02"), None);
    }

    #[test]
    fn sniff_document_pdf_json_text() {
        assert_eq!(sniff_document(b"%PDF-1.4\n..."), Some("application/pdf"));
        assert_eq!(sniff_document(b"{\"a\":1}"), Some("application/json"));
        assert_eq!(sniff_document(b"# title\n\nbody text"), Some("text/plain"));
    }

    #[test]
    fn sniff_document_rejects_binary() {
        assert_eq!(sniff_document(&make_png(1, 1)), None);
    }

    #[test]
    fn asset_id_validation() {
        let mut ok = String::with_capacity(64);
        for _ in 0..64 {
            ok.push('a');
        }
        assert!(is_valid_asset_id(&ok));
        assert!(!is_valid_asset_id(""));
        assert!(!is_valid_asset_id(&"a".repeat(63)));
        assert!(!is_valid_asset_id(&"a".repeat(65)));
        assert!(!is_valid_asset_id(&"A".repeat(64))); // uppercase reject
        let mut bad = ok.clone();
        bad.replace_range(..1, "/");
        assert!(!is_valid_asset_id(&bad));
        bad.replace_range(..1, ".");
        assert!(!is_valid_asset_id(&bad));
    }

    #[test]
    fn sha256_hex_known_vector() {
        // sha256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn image_dimensions_png_jpeg_gif_webp() {
        assert_eq!(png_dimensions(&make_png(100, 200)), Some((100, 200)));
        assert_eq!(gif_dimensions(&make_gif(8, 9)), Some((8, 9)));
        assert_eq!(webp_dimensions(&make_webp_vp8x(99, 199)), Some((100, 200)));
    }
}
