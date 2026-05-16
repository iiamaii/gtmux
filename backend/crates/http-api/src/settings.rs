//! Slice D-1 — `GET /api/settings` + `PATCH /api/settings` (BE-9).
//!
//! Spec source: `docs/reports/0042-be-slice-d-work-package.md` §3.1/§3.2.
//! Wires the FE Settings overlay's Debug + Behavior sections.
//!
//! Sections:
//!   * `build`    — boot-immutable build metadata (git sha, crate version,
//!                  rust toolchain). Sourced from compile-time env vars.
//!   * `server`   — boot-immutable runtime info (pid, bind, port). Sourced
//!                  from `AppState.config` + the running process.
//!   * `behavior` — **mutable** behavior toggles. Currently just
//!                  `auto_kill_terminal_on_panel_close` (ADR-0021 G25.1.b).
//!   * `auth`     — boot-immutable auth info (token present, password set,
//!                  Argon2id parameters per ADR-0020 D5).
//!
//! `PATCH /api/settings` accepts a JSON body whose top-level keys must be
//! a subset of `{ "behavior" }`. Any other top-level key returns 400
//! `boot_immutable` (the FE consumed `0042` §3.2's error contract — keep
//! the wire stable). Unknown nested keys under `behavior` return 400
//! `unknown_field` so an FE-side typo surfaces immediately instead of
//! being silently ignored.
//!
//! Persistence is **in-memory only** for the Stage 7 minimal slice — the
//! `behavior` toggle survives WS reconnects and HTTP retries inside one
//! server boot, but resets on restart. Disk persistence is a follow-up
//! item; the wire contract here does not depend on it.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::AppState;

/// Mutable behavior settings. The only toggle for Stage 7 minimal is
/// ADR-0021 G25.1.b's `auto_kill_terminal_on_panel_close` — when `true`,
/// the FE skips the per-panel close dialog and SIGTERMs the matching
/// terminal directly. Default = `false` (safer — confirm dialog default).
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BehaviorSettings {
    /// ADR-0021 G25.1.b: when true, panel close = panel + terminal SIGTERM
    /// with no per-action dialog. Default `false` per `bool::default()`.
    pub auto_kill_terminal_on_panel_close: bool,
}

/// Compile-time build metadata. Sourced from `Cargo.toml` + optional
/// `GTMUX_BUILD_SHA` env var (set by a future build.rs / CI step).
#[derive(Debug, Serialize)]
struct BuildInfo {
    /// Short git sha at build time. `"unknown"` when no build.rs / CI has
    /// populated `GTMUX_BUILD_SHA` — this is the cold dev-machine case.
    sha: &'static str,
    /// `gtmux-http-api` crate version (= workspace version).
    version: &'static str,
    /// Rust toolchain channel/version at build time. Pulled from
    /// `rust-toolchain.toml` indirectly via env at compile time when
    /// `GTMUX_BUILD_RUST_VERSION` is set, otherwise `"unknown"`.
    rust: &'static str,
}

impl BuildInfo {
    fn current() -> Self {
        Self {
            sha: option_env!("GTMUX_BUILD_SHA").unwrap_or("unknown"),
            version: env!("CARGO_PKG_VERSION"),
            rust: option_env!("GTMUX_BUILD_RUST_VERSION").unwrap_or("unknown"),
        }
    }
}

/// Runtime server info. Boot-immutable — pid + bound socket cannot change
/// once the process is up.
#[derive(Debug, Serialize)]
struct ServerInfo {
    pid: u32,
    bind: String,
    port: u16,
    /// gtmux currently logs to stderr only (no log file plumbing yet).
    /// Surfaced as `null` so the FE can render "stderr" or hide the row.
    log_path: Option<String>,
}

/// Argon2id parameters (ADR-0020 D5: m=64 MiB, t=3, p=4). Surfaced as
/// an object so the FE can render all three; the `0042` §3.1 example
/// suggestion of a single `argon2_cost` integer is widened to the full
/// shape — the FE consumer only reads what it needs.
#[derive(Debug, Serialize)]
struct ArgonParams {
    m_cost_kib: u32,
    t_cost: u32,
    p_cost: u32,
}

#[derive(Debug, Serialize)]
struct AuthInfo {
    /// Whether a bearer token is currently issued for this boot. Always
    /// `true` in practice — the CLI mints one at start.
    token_present: bool,
    /// Whether a password hash has been loaded from disk (i.e. the
    /// password-mode login path is wired). `false` in token-only mode.
    password_set: bool,
    argon2: ArgonParams,
}

/// Full snapshot returned by `GET /api/settings`.
#[derive(Debug, Serialize)]
struct SettingsSnapshot {
    build: BuildInfo,
    server: ServerInfo,
    behavior: BehaviorSettings,
    auth: AuthInfo,
}

fn build_snapshot(state: &AppState, behavior: BehaviorSettings) -> SettingsSnapshot {
    SettingsSnapshot {
        build: BuildInfo::current(),
        server: ServerInfo {
            pid: std::process::id(),
            bind: state.config.server.bind.clone(),
            port: state.config.server.port,
            log_path: None,
        },
        behavior,
        auth: AuthInfo {
            token_present: !state.token.0.is_empty(),
            password_set: state.password_hash.is_some(),
            argon2: ArgonParams {
                // Mirror the constants in `auth.rs`. Re-exposing them
                // here as literals keeps the response stable even if the
                // private constants are tuned — the wire shape stays
                // unchanged.
                m_cost_kib: 64 * 1024,
                t_cost: 3,
                p_cost: 4,
            },
        },
    }
}

/// `GET /api/settings` — returns the boot-immutable info + the current
/// behavior snapshot.
pub(crate) async fn get_handler(State(state): State<AppState>) -> Response {
    let behavior = *state.behavior_settings.read().await;
    Json(build_snapshot(&state, behavior)).into_response()
}

/// `PATCH /api/settings` — accepts a partial JSON body. Only `behavior`
/// is mutable; any other top-level key returns 400 `boot_immutable`.
/// Unknown nested keys under `behavior` return 400 `unknown_field`.
///
/// On success the response is the full updated `SettingsSnapshot` so
/// the FE doesn't need a follow-up GET.
pub(crate) async fn patch_handler(State(state): State<AppState>, req: Request<Body>) -> Response {
    let body_bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "body_read_failed", "message": e.to_string() })),
            )
                .into_response()
        }
    };

    // Empty body = no-op; return current snapshot. Matches a curl
    // `-X PATCH` with no `-d` flag, which is a reasonable "just fetch"
    // probe pattern.
    if body_bytes.is_empty() {
        let behavior = *state.behavior_settings.read().await;
        return Json(build_snapshot(&state, behavior)).into_response();
    }

    let parsed: Value = match serde_json::from_slice(&body_bytes) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_json", "message": e.to_string() })),
            )
                .into_response()
        }
    };

    let Some(obj) = parsed.as_object() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "body_not_object" })),
        )
            .into_response();
    };

    // Reject boot-immutable sections explicitly so a FE typo of
    // `{"build": {...}}` (instead of `{"behavior": {...}}`) gets a
    // descriptive error instead of being silently dropped.
    for immutable in ["build", "server", "auth"] {
        if obj.contains_key(immutable) {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "boot_immutable",
                    "field": immutable,
                })),
            )
                .into_response();
        }
    }

    // Reject any unknown top-level key (anything other than `behavior`).
    for key in obj.keys() {
        if key != "behavior" {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "unknown_field",
                    "field": key,
                })),
            )
                .into_response();
        }
    }

    // From here we know the body is `{ "behavior": {...} }`. Deserialise
    // the inner object with `deny_unknown_fields` so an FE typo of
    // `{"behaviour": {...}}` (UK spelling, easy mistake) or
    // `{"behavior": {"auto_kill": true}}` (truncated key) is surfaced.
    let Some(behavior_value) = obj.get("behavior") else {
        // `obj` has no immutable sections and no non-`behavior` keys.
        // If `behavior` itself is missing the body is just `{}` — treat
        // as a no-op and return the current snapshot.
        let behavior = *state.behavior_settings.read().await;
        return Json(build_snapshot(&state, behavior)).into_response();
    };

    // Start from the *current* behavior so partial updates merge
    // semantically (FE can send `{"behavior":{"auto_kill_terminal_on_panel_close":true}}`
    // without echoing every other field — when more fields land).
    let mut next = *state.behavior_settings.read().await;
    let Some(behavior_obj) = behavior_value.as_object() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "behavior_not_object" })),
        )
            .into_response();
    };

    for (key, value) in behavior_obj {
        match key.as_str() {
            "auto_kill_terminal_on_panel_close" => {
                let Some(b) = value.as_bool() else {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": "type_mismatch",
                            "field": "behavior.auto_kill_terminal_on_panel_close",
                            "expected": "bool",
                        })),
                    )
                        .into_response();
                };
                next.auto_kill_terminal_on_panel_close = b;
            }
            other => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "unknown_field",
                        "field": format!("behavior.{other}"),
                    })),
                )
                    .into_response()
            }
        }
    }

    // Commit + reflect in the response. Two `await` boundaries (read
    // above, write here) are fine — same task, no contention possible.
    {
        let mut w = state.behavior_settings.write().await;
        *w = next;
    }
    Json(build_snapshot(&state, next)).into_response()
}

/// Convenience constructor for the `behavior_settings` field on
/// [`AppState`]. Wrapped here so callers don't have to know the exact
/// `Arc<RwLock<...>>` shape.
pub fn default_behavior_settings() -> Arc<tokio::sync::RwLock<BehaviorSettings>> {
    Arc::new(tokio::sync::RwLock::new(BehaviorSettings::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{header, Method, Request as HttpRequest, StatusCode},
    };
    use gtmux_auth::{issue_token, TokenString};
    use gtmux_config::{Config, RuntimeConfig, SecurityConfig, ServerConfig};
    use tower::ServiceExt;

    const TEST_HOST: &str = "127.0.0.1:9001";
    const TEST_ORIGIN: &str = "http://localhost:9001";

    fn bearer(token: &TokenString) -> String {
        format!("Bearer {}", token.0)
    }

    /// Minimal `AppState` for the settings handler tests. The handler
    /// only reads `behavior_settings`, `config.server.bind/port`,
    /// `token`, and `password_hash` — no workspace / hub plumbing needed.
    fn test_state() -> (AppState, TokenString) {
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
                cors_origins: vec![TEST_ORIGIN.to_string()],
                host_allowlist: vec![TEST_HOST.to_string()],
            },
            cloud: None,
            frontend_dist: None,
            workspace_path: None,
            auth: gtmux_config::AuthConfig::default(),
        };
        let state = AppState::new(cfg, token.clone());
        (state, token)
    }

    fn router(state: AppState) -> axum::Router {
        crate::router_with_state(state)
    }

    #[tokio::test]
    async fn get_returns_full_snapshot_shape() {
        let (state, token) = test_state();
        let app = router(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/settings")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        for section in ["build", "server", "behavior", "auth"] {
            assert!(body.get(section).is_some(), "missing section {section}");
            assert!(body[section].is_object(), "{section} must be object");
        }
        assert_eq!(
            body["behavior"]["auto_kill_terminal_on_panel_close"],
            serde_json::Value::Bool(false),
            "default must be false (ADR-0021 G25.1.b safer default)"
        );
        assert_eq!(body["build"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(body["server"]["pid"], std::process::id());
        assert!(body["auth"]["argon2"].is_object());
        assert_eq!(body["auth"]["argon2"]["t_cost"], 3);
    }

    #[tokio::test]
    async fn get_without_auth_returns_401() {
        let (state, _token) = test_state();
        let app = router(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/settings")
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn patch_behavior_toggle_succeeds() {
        let (state, token) = test_state();
        let app = router(state.clone());
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PATCH)
                    .uri("/api/settings")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"behavior":{"auto_kill_terminal_on_panel_close":true}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body["behavior"]["auto_kill_terminal_on_panel_close"],
            serde_json::Value::Bool(true)
        );
        // The runtime state must reflect the change so the next GET (or
        // an internal consumer like ADR-0021 G25.1.b's close handler) sees
        // it without a re-PATCH.
        let live = *state.behavior_settings.read().await;
        assert!(live.auto_kill_terminal_on_panel_close);
    }

    #[tokio::test]
    async fn patch_boot_immutable_section_rejects_400() {
        let (state, token) = test_state();
        let app = router(state);
        for (section, body) in [
            ("build", r#"{"build":{"sha":"abcd"}}"#),
            ("server", r#"{"server":{"port":9999}}"#),
            ("auth", r#"{"auth":{"password_set":false}}"#),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    HttpRequest::builder()
                        .method(Method::PATCH)
                        .uri("/api/settings")
                        .header(header::HOST, TEST_HOST)
                        .header(header::AUTHORIZATION, bearer(&token))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::BAD_REQUEST,
                "PATCH of immutable section {section} must 400"
            );
            let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(v["error"], "boot_immutable");
            assert_eq!(v["field"], section);
        }
    }

    #[tokio::test]
    async fn patch_unknown_top_level_field_rejects_400() {
        let (state, token) = test_state();
        let app = router(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PATCH)
                    .uri("/api/settings")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"theme":{"mode":"dark"}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "unknown_field");
        assert_eq!(v["field"], "theme");
    }

    #[tokio::test]
    async fn patch_unknown_nested_field_rejects_400() {
        let (state, token) = test_state();
        let app = router(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PATCH)
                    .uri("/api/settings")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"behavior":{"auto_kill":true}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "unknown_field");
        assert_eq!(v["field"], "behavior.auto_kill");
    }

    #[tokio::test]
    async fn patch_type_mismatch_rejects_400() {
        let (state, token) = test_state();
        let app = router(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PATCH)
                    .uri("/api/settings")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"behavior":{"auto_kill_terminal_on_panel_close":"yes"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "type_mismatch");
        assert_eq!(v["field"], "behavior.auto_kill_terminal_on_panel_close");
    }

    #[tokio::test]
    async fn patch_empty_body_returns_current_snapshot() {
        let (state, token) = test_state();
        let app = router(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PATCH)
                    .uri("/api/settings")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            v["behavior"]["auto_kill_terminal_on_panel_close"],
            serde_json::Value::Bool(false)
        );
    }
}
