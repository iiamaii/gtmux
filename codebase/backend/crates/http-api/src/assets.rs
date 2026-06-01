//! `/api/assets` — **legacy** content-addressed binary store for image/document
//! items (ADR-0033). **Superseded by ADR-0047 (D7):** image/document items now
//! reference real files in the session Workspace(B) (uploaded via
//! `POST /api/fs/upload`, served by `GET /api/fs/file`). This module is kept
//! only for *read* back-compat of pre-ADR-0047 layouts.
//!
//! Endpoints:
//!   * `POST   /api/assets`             — **deprecated, `410 Gone`** (no new assets).
//!   * `POST   /api/assets/from-path`   — **deprecated, `410 Gone`**.
//!   * `GET    /api/assets/{asset_id}`  — serve raw bytes, immutable cache (legacy read).
//!
//! Storage layout: `<store>/.assets/<sha256_hex>` — phased out (no new writes).
//! `asset_id` is validated against `[a-f0-9]{64}` before any path resolution,
//! so `..`-style traversal is impossible by construction.
//!
//! MIME (serve): magic-byte sniff — png / jpeg / gif / webp / svg+xml /
//! text/plain · application/json · application/pdf, else octet-stream. The
//! same sniff backs `GET /api/fs/file` ([`sniff_any`]) and upload gating
//! ([`sniff_allowed_upload`]).
//!
//! Auth: this module piggybacks on the bearer/cookie middleware in
//! `lib.rs::bearer_auth_middleware` — `/api/*` is gated at the router
//! level, so handlers here can assume the request is authenticated.

use axum::extract::{Path as AxumPath, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use tracing::warn;

use crate::AppState;

/// Multipart framing headroom added on top of configured asset byte limit.
pub(crate) const ASSET_MULTIPART_HEADROOM_BYTES: usize = 1024 * 1024;

/// Stored asset filename = 64 lowercase hex characters of sha256(bytes).
const ASSET_ID_HEX_LEN: usize = 64;

/// `POST /api/assets` — **deprecated (ADR-0047 D7).** The content-hash asset
/// store (ADR-0033) is superseded by workspace-file references: image/document
/// items now live inside the session Workspace(B) and are uploaded via
/// `POST /api/fs/upload`, then referenced by relative `path`. No new assets are
/// created — always `410 Gone`. (`GET /api/assets/{id}` still serves the legacy
/// records that pre-date ADR-0047.)
pub async fn upload_handler(State(_state): State<AppState>) -> Response {
    assets_deprecated_response()
}

/// `410 Gone` body shared by the deprecated asset-upload endpoints (ADR-0047 D7).
fn assets_deprecated_response() -> Response {
    (
        StatusCode::GONE,
        Json(json!({
            "error": "assets_deprecated",
            "message": "the asset store is superseded by workspace file references (ADR-0047); \
                        upload via POST /api/fs/upload and reference the file by path",
        })),
    )
        .into_response()
}

/// `POST /api/assets/from-path` — **deprecated (ADR-0047 D7).** See
/// [`upload_handler`]. Always `410 Gone`.
pub async fn upload_from_path_handler(State(_state): State<AppState>) -> Response {
    assets_deprecated_response()
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
    // Read the (immutable, content-addressed) asset off the blocking pool so a
    // large image read doesn't stall a tokio worker thread.
    let read_path = path.clone();
    let read_result = tokio::task::spawn_blocking(move || std::fs::read(&read_path)).await;
    let bytes = match read_result {
        Ok(Ok(b)) => b,
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "asset_not_found" })),
            )
                .into_response();
        }
        Ok(Err(e)) => {
            warn!(error = %e, path = %path.display(), "assets: read failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "read_failed" })),
            )
                .into_response();
        }
        Err(e) => {
            warn!(error = %e, path = %path.display(), "assets: read task panicked");
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
    let mime = sniff_any(&bytes);

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
            HeaderValue::from_static(SVG_SERVE_CSP),
        );
    }

    builder
        .body(axum::body::Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// ADR-0047 D3 — sniff any allowed image *or* document MIME, falling back to
/// `application/octet-stream`. Used by `GET /api/fs/file` to serve arbitrary
/// workspace files (the source is the user's own project dir, so an unknown
/// type is served opaquely rather than rejected). The magic-byte logic is the
/// same allowlist the asset store used (ADR-0033 D4).
pub(crate) fn sniff_any(bytes: &[u8]) -> &'static str {
    sniff_image(bytes)
        .or_else(|| sniff_document(bytes))
        .unwrap_or("application/octet-stream")
}

/// ADR-0047 D2 — sniff an *uploadable* MIME: `Some` iff the bytes are an
/// allowed image or document type (the ADR-0033 D3 allowlist), else `None`
/// (caller → 415). `POST /api/fs/upload` gates on this so the workspace only
/// ever receives the same content classes the asset store accepted.
pub(crate) fn sniff_allowed_upload(bytes: &[u8]) -> Option<&'static str> {
    sniff_image(bytes).or_else(|| sniff_document(bytes))
}

/// ADR-0033 D6 / ADR-0047 D8 — the CSP that neutralises scripts in a directly
/// navigated SVG (inside `<img>` the browser already sandboxes it, but typing
/// the URL renders it as a top-level document where inline `<script>` /
/// `foreignObject` / event handlers would otherwise execute). Shared by the
/// legacy asset serve and the workspace-file serve (`GET /api/fs/file`).
pub(crate) const SVG_SERVE_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; sandbox";

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

fn is_valid_asset_id(s: &str) -> bool {
    s.len() == ASSET_ID_HEX_LEN && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
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

}
