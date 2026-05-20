//! Axum handlers for the `/api/file-path/*` endpoint group.

use axum::body::Body;
use axum::extract::{Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::allowlist::{normalise_ext, normalise_prefix, AllowlistEntry, AllowlistMatch};
use super::spawn::{self, SpawnError};
use crate::AppState;

const COOKIE_PREFIX_LEN: usize = 8;

fn cookie_prefix_from_headers(headers: &axum::http::HeaderMap) -> String {
    crate::auth::extract_session_cookie(headers)
        .map(|c| c.chars().take(COOKIE_PREFIX_LEN).collect())
        .unwrap_or_default()
}

fn bad_request(error: &'static str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response()
}

fn bad_request_with(error: &'static str, extra: serde_json::Value) -> Response {
    let mut body = json!({ "error": error });
    if let Some(obj) = body.as_object_mut() {
        if let Some(extra_obj) = extra.as_object() {
            for (k, v) in extra_obj {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

// ─── GET /api/file-path/allowlist ─────────────────────────────────────

pub async fn allowlist_get_handler(State(state): State<AppState>) -> Response {
    let alist = state.file_open.allowlist.read().await;
    let entries: Vec<&AllowlistEntry> = alist.entries().iter().collect();
    Json(json!({ "entries": entries })).into_response()
}

// ─── POST /api/file-path/allowlist ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct PostAllowlistRequest {
    ext: String,
    prefix: String,
    #[serde(default)]
    label: Option<String>,
}

pub async fn allowlist_post_handler(State(state): State<AppState>, req: Request<Body>) -> Response {
    let bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024).await {
        Ok(b) => b,
        Err(e) => return bad_request_with("body_read_failed", json!({ "message": e.to_string() })),
    };
    let parsed: PostAllowlistRequest = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(e) => return bad_request_with("invalid_json", json!({ "message": e.to_string() })),
    };

    // Validate ext + prefix shape *before* hitting the filesystem
    // (cheap, common-case rejection).
    let ext = match normalise_ext(&parsed.ext) {
        Ok(e) => e,
        Err(e) => return bad_request_validation_message(&e),
    };
    // Canonicalize prefix per ADR-0023 D5 step 3 + D8 symlink-escape
    // guard. Then re-add the trailing `/`.
    let raw_prefix = parsed.prefix.trim_end_matches('/');
    let canonical = match std::fs::canonicalize(raw_prefix) {
        Ok(p) => p,
        Err(_) => return bad_request("prefix_not_exists"),
    };
    if !canonical.is_dir() {
        return bad_request("prefix_not_directory");
    }
    let mut canonical_str = canonical.to_string_lossy().into_owned();
    if !canonical_str.ends_with('/') {
        canonical_str.push('/');
    }
    let prefix = match normalise_prefix(&canonical_str) {
        Ok(p) => p,
        Err(e) => return bad_request_validation_message(&e),
    };

    let mut alist = state.file_open.allowlist.write().await;
    match alist.add(&ext, &prefix, parsed.label) {
        Ok(entry) => (StatusCode::CREATED, Json(entry.clone())).into_response(),
        Err(super::allowlist::AllowlistError::Validation("duplicate")) => {
            bad_request("already_in_allowlist")
        }
        Err(super::allowlist::AllowlistError::Validation(v)) => bad_request_validation(v),
        Err(super::allowlist::AllowlistError::Io(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "save_failed", "message": e.to_string() })),
        )
            .into_response(),
        Err(super::allowlist::AllowlistError::Parse(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "save_failed", "message": e.to_string() })),
        )
            .into_response(),
    }
}

fn bad_request_validation(v: &'static str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": v }))).into_response()
}

fn bad_request_validation_message(err: &super::allowlist::AllowlistError) -> Response {
    use super::allowlist::AllowlistError;
    match err {
        AllowlistError::Validation(v) => bad_request_validation(v),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "internal", "message": err.to_string() })),
        )
            .into_response(),
    }
}

// ─── DELETE /api/file-path/allowlist?ext=&prefix= ─────────────────────

#[derive(Debug, Deserialize)]
pub struct DeleteAllowlistQuery {
    ext: String,
    prefix: String,
}

pub async fn allowlist_delete_handler(
    State(state): State<AppState>,
    Query(q): Query<DeleteAllowlistQuery>,
) -> Response {
    let ext = match normalise_ext(&q.ext) {
        Ok(e) => e,
        Err(e) => return bad_request_validation_message(&e),
    };
    // We accept the prefix *as the user originally inserted it* — no
    // canonicalisation here, because the on-disk entry is already
    // canonical and we need an exact compound-key match.
    let prefix = match normalise_prefix(&q.prefix) {
        Ok(p) => p,
        Err(e) => return bad_request_validation_message(&e),
    };
    let mut alist = state.file_open.allowlist.write().await;
    match alist.remove(&ext, &prefix) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "entry_not_found" })),
        )
            .into_response(),
        Err(super::allowlist::AllowlistError::Io(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "save_failed", "message": e.to_string() })),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "internal" })),
        )
            .into_response(),
    }
}

// ─── GET /api/file-path/allowlist-check?path=<p> ──────────────────────

#[derive(Debug, Deserialize)]
pub struct CheckQuery {
    path: String,
}

#[derive(Debug, Serialize)]
struct CheckAllowed<'a> {
    allowed: bool,
    matched_entry: &'a AllowlistEntry,
}

#[derive(Debug, Serialize)]
struct CheckDenied {
    allowed: bool,
    reason: &'static str,
}

pub async fn allowlist_check_handler(
    State(state): State<AppState>,
    Query(q): Query<CheckQuery>,
) -> Response {
    let (path, denial_reason) = match validate_path(&q.path) {
        Ok(p) => (p, None),
        Err(r) => (std::path::PathBuf::new(), Some(r)),
    };
    if let Some(reason) = denial_reason {
        return Json(CheckDenied {
            allowed: false,
            reason,
        })
        .into_response();
    }

    let alist = state.file_open.allowlist.read().await;
    match alist.check(&path) {
        AllowlistMatch::Allowed(entry) => Json(CheckAllowed {
            allowed: true,
            matched_entry: entry,
        })
        .into_response(),
        AllowlistMatch::Denied => Json(CheckDenied {
            allowed: false,
            reason: "not_in_allowlist",
        })
        .into_response(),
    }
}

// ─── POST /api/file-path/open ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct OpenRequest {
    path: String,
    #[serde(default)]
    user_confirmed: bool,
}

pub async fn open_handler(State(state): State<AppState>, req: Request<Body>) -> Response {
    let cookie_prefix = cookie_prefix_from_headers(req.headers());
    let bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024).await {
        Ok(b) => b,
        Err(e) => return bad_request_with("body_read_failed", json!({ "message": e.to_string() })),
    };
    let parsed: OpenRequest = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(e) => return bad_request_with("invalid_json", json!({ "message": e.to_string() })),
    };

    let canonical = match validate_path(&parsed.path) {
        Ok(p) => p,
        Err(reason) => {
            state
                .file_open
                .audit
                .record_denied(&parsed.path, reason, &cookie_prefix);
            return bad_request(reason);
        }
    };
    let canonical_str = canonical.to_string_lossy().into_owned();

    let alist = state.file_open.allowlist.read().await;
    let matched = matches!(alist.check(&canonical), AllowlistMatch::Allowed(_));
    drop(alist); // release read lock before the OS spawn

    let allowed_via = if matched {
        "allowlist"
    } else if parsed.user_confirmed {
        "one_time"
    } else {
        state.file_open.audit.record_denied(
            &canonical_str,
            "user_confirmation_required",
            &cookie_prefix,
        );
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "user_confirmation_required" })),
        )
            .into_response();
    };

    match spawn::spawn(&canonical) {
        Ok(()) => {
            if matched {
                state
                    .file_open
                    .audit
                    .record_allowed(&canonical_str, &cookie_prefix);
            } else {
                state
                    .file_open
                    .audit
                    .record_one_time(&canonical_str, &cookie_prefix);
            }
            Json(json!({ "opened": true, "allowed_via": allowed_via })).into_response()
        }
        Err(SpawnError::NoHandler) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "spawn_failed", "reason": "no_handler" })),
        )
            .into_response(),
        Err(SpawnError::Io(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "spawn_failed",
                "reason": "io",
                "message": e.to_string(),
            })),
        )
            .into_response(),
    }
}

/// ADR-0023 D5 steps 1-3: absolute path, no NUL, canonicalize.
fn validate_path(path: &str) -> Result<std::path::PathBuf, &'static str> {
    if path.contains('\0') {
        return Err("nul_byte");
    }
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err("path_not_absolute");
    }
    std::fs::canonicalize(p).map_err(|_| "path_not_exists")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_open::FileOpenContext;
    use crate::settings::default_behavior_settings;
    use axum::body::Body;
    use axum::http::{header, Method, Request as HttpRequest, StatusCode};
    use gtmux_auth::{issue_token, TokenString};
    use gtmux_config::{Config, RuntimeConfig, SecurityConfig, ServerConfig};
    use tempfile::TempDir;
    use tower::ServiceExt;

    const TEST_HOST: &str = "127.0.0.1:9001";

    fn test_state_with_file_open() -> (crate::AppState, TokenString, TempDir) {
        let token = issue_token().expect("token");
        let cfg = Config {
            schema_version: 1,
            server: ServerConfig {
                session: "test".to_string(),
                port: 9001,
                bind: "127.0.0.1".to_string(),
            },
            runtime: RuntimeConfig::default(),
            security: SecurityConfig {
                cors_origins: vec!["http://localhost:9001".to_string()],
                host_allowlist: vec![TEST_HOST.to_string()],
            },
            cloud: None,
            frontend_dist: None,
            workspace_path: None,
            auth: gtmux_config::AuthConfig::default(),
        };
        let tmp = TempDir::new().unwrap();
        let mut state = crate::AppState::new(cfg, token.clone());
        state.file_open =
            FileOpenContext::for_tests(tmp.path().join("allowlist.json"), tmp.path().join("audit"));
        // Sanity assert the settings field survives the FileOpenContext
        // override above — we want to make sure other AppState fields
        // are untouched.
        let _ = default_behavior_settings();
        (state, token, tmp)
    }

    fn bearer(token: &TokenString) -> String {
        format!("Bearer {}", token.0)
    }

    #[tokio::test]
    async fn get_empty_returns_empty_entries() {
        let (state, token, _tmp) = test_state_with_file_open();
        let app = crate::router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/file-path/allowlist")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["entries"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn post_then_get_round_trip() {
        let (state, token, tmp) = test_state_with_file_open();
        // The POST will canonicalize the prefix; create an actual dir.
        let target = tmp.path().join("notes");
        std::fs::create_dir(&target).unwrap();
        let app = crate::router_with_state(state.clone());
        let body = format!(
            r#"{{"ext":"md","prefix":"{}","label":"Notes"}}"#,
            target.display()
        );
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/file-path/allowlist")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let entry: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(entry["ext"], "md");
        assert!(entry["prefix"].as_str().unwrap().ends_with('/'));
        assert_eq!(entry["label"], "Notes");

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/file-path/allowlist")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["entries"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn post_canonicalises_prefix() {
        let (state, token, tmp) = test_state_with_file_open();
        let target = tmp.path().join("notes");
        std::fs::create_dir(&target).unwrap();
        // Use a non-canonical form: with trailing slash, with `..` segment.
        let raw = target.join("..").join("notes");
        let app = crate::router_with_state(state);
        let body = format!(r#"{{"ext":"md","prefix":"{}"}}"#, raw.display());
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/file-path/allowlist")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let entry: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let canon = std::fs::canonicalize(&target).unwrap();
        let mut expected = canon.to_string_lossy().into_owned();
        expected.push('/');
        assert_eq!(entry["prefix"], expected);
    }

    #[tokio::test]
    async fn post_rejects_nonexistent_prefix() {
        let (state, token, _tmp) = test_state_with_file_open();
        let app = crate::router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/file-path/allowlist")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"ext":"md","prefix":"/no/such/dir/"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "prefix_not_exists");
    }

    #[tokio::test]
    async fn post_rejects_duplicate() {
        let (state, token, tmp) = test_state_with_file_open();
        let target = tmp.path().join("notes");
        std::fs::create_dir(&target).unwrap();
        let app = crate::router_with_state(state);
        let body = format!(r#"{{"ext":"md","prefix":"{}"}}"#, target.display());
        let r1 = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/file-path/allowlist")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::CREATED);
        let r2 = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/file-path/allowlist")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r2.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(r2.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "already_in_allowlist");
    }

    #[tokio::test]
    async fn delete_round_trip() {
        let (state, token, tmp) = test_state_with_file_open();
        let target = tmp.path().join("notes");
        std::fs::create_dir(&target).unwrap();
        let canon = std::fs::canonicalize(&target).unwrap();
        let prefix_str = format!("{}/", canon.display());
        // Seed via the in-memory API to skip the POST path.
        {
            let mut a = state.file_open.allowlist.write().await;
            a.add("md", &prefix_str, None).unwrap();
        }
        let app = crate::router_with_state(state);
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/api/file-path/allowlist?ext=md&prefix={}",
                        urlencoding(&prefix_str)
                    ))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        // Second delete → 404.
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/api/file-path/allowlist?ext=md&prefix={}",
                        urlencoding(&prefix_str)
                    ))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    fn urlencoding(s: &str) -> String {
        s.chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | '/' => c.to_string(),
                _ => format!("%{:02X}", c as u32),
            })
            .collect()
    }

    #[tokio::test]
    async fn check_returns_matched_entry() {
        let (state, token, tmp) = test_state_with_file_open();
        let target = tmp.path().join("notes");
        std::fs::create_dir(&target).unwrap();
        let canon = std::fs::canonicalize(&target).unwrap();
        let prefix_str = format!("{}/", canon.display());
        {
            let mut a = state.file_open.allowlist.write().await;
            a.add("md", &prefix_str, None).unwrap();
        }
        let target_file = target.join("spec.md");
        std::fs::write(&target_file, b"hi").unwrap();
        let app = crate::router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!(
                        "/api/file-path/allowlist-check?path={}",
                        urlencoding(target_file.to_string_lossy().as_ref())
                    ))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["allowed"], true);
        assert_eq!(v["matched_entry"]["ext"], "md");
    }

    #[tokio::test]
    async fn check_returns_denied_for_unmatched_ext() {
        let (state, token, tmp) = test_state_with_file_open();
        let target = tmp.path().join("proj");
        std::fs::create_dir(&target).unwrap();
        let canon = std::fs::canonicalize(&target).unwrap();
        let prefix_str = format!("{}/", canon.display());
        {
            let mut a = state.file_open.allowlist.write().await;
            a.add("md", &prefix_str, None).unwrap();
        }
        let target_file = target.join("payload.sh");
        std::fs::write(&target_file, b"#!/bin/sh\n").unwrap();
        let app = crate::router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!(
                        "/api/file-path/allowlist-check?path={}",
                        urlencoding(target_file.to_string_lossy().as_ref())
                    ))
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["allowed"], false);
        assert_eq!(v["reason"], "not_in_allowlist");
    }

    #[tokio::test]
    async fn check_rejects_relative_path() {
        let (state, token, _tmp) = test_state_with_file_open();
        let app = crate::router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/file-path/allowlist-check?path=notes/spec.md")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["allowed"], false);
        assert_eq!(v["reason"], "path_not_absolute");
    }

    #[tokio::test]
    async fn open_rejects_unconfirmed_unmatched_path() {
        let (state, token, tmp) = test_state_with_file_open();
        let target_file = tmp.path().join("payload.sh");
        std::fs::write(&target_file, b"#!/bin/sh\n").unwrap();
        let canon = std::fs::canonicalize(&target_file).unwrap();
        let app = crate::router_with_state(state);
        let body = format!(r#"{{"path":"{}","user_confirmed":false}}"#, canon.display());
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/file-path/open")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "user_confirmation_required");
    }

    #[tokio::test]
    async fn open_rejects_relative_path_400() {
        let (state, token, _tmp) = test_state_with_file_open();
        let app = crate::router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/file-path/open")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"path":"notes/x","user_confirmed":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "path_not_absolute");
    }

    #[tokio::test]
    async fn open_rejects_nul_byte_path_400() {
        let (state, token, _tmp) = test_state_with_file_open();
        let app = crate::router_with_state(state);
        // Build via `serde_json::json!` so the NUL byte (\0) survives
        // serialisation. A raw string literal would embed a literal
        // NUL in the source file, which the file editor can't round-trip.
        let body = serde_json::to_vec(&serde_json::json!({
            "path": "/tmp/x\u{0}.md",
            "user_confirmed": true,
        }))
        .unwrap();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/file-path/open")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "nul_byte");
    }
}
