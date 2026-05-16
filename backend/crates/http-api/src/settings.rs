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

fn build_snapshot(
    state: &AppState,
    behavior: BehaviorSettings,
    password_set: bool,
) -> SettingsSnapshot {
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
            password_set,
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

async fn current_password_set(state: &AppState) -> bool {
    state.password_hash.read().await.is_some()
}

/// `GET /api/settings` — returns the boot-immutable info + the current
/// behavior snapshot.
pub(crate) async fn get_handler(State(state): State<AppState>) -> Response {
    let behavior = *state.behavior_settings.read().await;
    let password_set = current_password_set(&state).await;
    Json(build_snapshot(&state, behavior, password_set)).into_response()
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
        let password_set = current_password_set(&state).await;
        return Json(build_snapshot(&state, behavior, password_set)).into_response();
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
        let password_set = current_password_set(&state).await;
        return Json(build_snapshot(&state, behavior, password_set)).into_response();
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
    let password_set = current_password_set(&state).await;
    Json(build_snapshot(&state, next, password_set)).into_response()
}

// ─── Slice D-3: Auth Stage 7 (POST /api/settings/password) ────────────

/// Minimum length per ADR-0020 D5. zxcvbn is P2+.
const MIN_PASSWORD_LENGTH: usize = 8;

#[derive(Debug, serde::Deserialize)]
struct PasswordChangeRequest {
    current_password: String,
    new_password: String,
}

fn weak_password_response() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": "weak_password",
            "min_length": MIN_PASSWORD_LENGTH,
        })),
    )
        .into_response()
}

fn password_validates(new_password: &str) -> bool {
    if new_password.len() < MIN_PASSWORD_LENGTH {
        return false;
    }
    // ADR-0020 D5: "8자 + 영문 + 숫자 (zxcvbn 검사 P2+)" — MVP enforces
    // the letter+digit half of the rule. Full zxcvbn lands later.
    let has_letter = new_password.chars().any(|c| c.is_ascii_alphabetic());
    let has_digit = new_password.chars().any(|c| c.is_ascii_digit());
    has_letter && has_digit
}

/// `POST /api/settings/password` — ADR-0020 D4 password rotation.
///
/// Body: `{ current_password, new_password }`.
///
/// Outcomes:
/// - 200 + `Set-Cookie: gtmux_auth=<new>` — verified, rehashed, persisted,
///   caller re-issued, **every other session revoked** (revoked_count
///   echoed). The caller's old cookie is replaced by the new one in
///   the response.
/// - 400 `weak_password` (min_length echoed) — new password failed validation.
/// - 401 `current_password_mismatch` — Argon2 verify of `current_password`
///   failed.
/// - 503 `password_not_set` — server is in token mode or the password
///   hash is missing; caller must set the password via the CLI first.
/// - 500 `save_failed` — disk write failed.
///
/// Side-effect ordering (atomic w.r.t. callers via the
/// `AppState.password_hash` `RwLock`):
/// 1. Read + verify current hash.
/// 2. Validate new password.
/// 3. Compute new Argon2id hash.
/// 4. Atomically persist to disk (`save_password_hash` mode 0600).
/// 5. Swap in-memory hash.
/// 6. Revoke all sessions *except* the caller, then re-issue the
///    caller's cookie under a fresh value so even the old cookie of
///    the caller is dead (any other tab that happened to share it loses
///    auth).
pub(crate) async fn password_handler(
    State(state): State<AppState>,
    req: axum::extract::Request<axum::body::Body>,
) -> Response {
    use crate::auth::extract_session_cookie;

    let caller_cookie = match extract_session_cookie(req.headers()) {
        Some(c) => c,
        None => {
            // `/api/*` middleware should have already gated this — but
            // defend against bearer-only requests reaching us.
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "session_cookie_required" })),
            )
                .into_response();
        }
    };

    let bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "body_read_failed", "message": e.to_string() })),
            )
                .into_response()
        }
    };
    let parsed: PasswordChangeRequest = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_json", "message": e.to_string() })),
            )
                .into_response()
        }
    };

    // 1. Verify current.
    let current_ok = {
        let guard = state.password_hash.read().await;
        match guard.as_deref() {
            Some(h) => crate::auth::verify_password(&parsed.current_password, h),
            None => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "error": "password_not_set",
                        "message": "server is in token mode or password is not yet set; \
                                    run `gtmux set-password` first",
                    })),
                )
                    .into_response();
            }
        }
    };
    if !current_ok {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "current_password_mismatch" })),
        )
            .into_response();
    }

    // 2. Validate new.
    if !password_validates(&parsed.new_password) {
        return weak_password_response();
    }

    // 3. Hash new.
    let new_hash = match crate::auth::hash_password(&parsed.new_password) {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(error = %e, "password rotation: hash failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "hash_failed" })),
            )
                .into_response();
        }
    };

    // 4. Persist.
    if let Some(path) = state.password_hash_path.as_ref() {
        if let Err(e) = crate::auth::save_password_hash(path, &new_hash) {
            tracing::error!(error = %e, "password rotation: save failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "save_failed", "message": e.to_string() })),
            )
                .into_response();
        }
    } else {
        // Production wiring always sets the path; missing it is a
        // boot-misconfiguration. Fail loud rather than silently dropping
        // the new hash.
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "save_failed", "reason": "no_hash_path" })),
        )
            .into_response();
    }

    // 5. In-memory swap.
    *state.password_hash.write().await = Some(new_hash);

    // 6. Revoke others, then re-issue caller cookie so even the old
    //    cookie is dead (defence in depth — another tab sharing this
    //    cookie loses auth too). Per ADR-0020 D4 the caller's *previous*
    //    cookie token is now revoked along with every other one; the
    //    fresh token replaces it via Set-Cookie.
    let revoked = state.session_table.revoke_others(&caller_cookie).await;
    state.session_table.revoke(&caller_cookie).await;

    let new_token = match state
        .session_table
        .issue(crate::auth::AuthMode::Password)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, "password rotation: issue new cookie failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "issue_failed" })),
            )
                .into_response();
        }
    };
    let secure = matches!(state.config.mode(), gtmux_config::Mode::Cloud);
    let cookie_header =
        crate::auth::build_session_cookie(&new_token, state.session_table.max_age(), secure);

    let mut resp = (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "revoked_count": revoked,
        })),
    )
        .into_response();
    if let Ok(hv) = axum::http::HeaderValue::from_str(&cookie_header) {
        resp.headers_mut().insert(axum::http::header::SET_COOKIE, hv);
    }
    resp
}

#[derive(Debug, serde::Serialize)]
struct LogoutAllResponse {
    revoked_count: usize,
}

/// `POST /api/settings/logout-all` — ADR-0020 D4 "logout all".
///
/// Revokes every session *except* the caller's. The caller stays
/// logged in via the same cookie; every other tab / device hits 401
/// on its next request and bounces back to the auth page.
pub(crate) async fn logout_all_handler(
    State(state): State<AppState>,
    req: axum::extract::Request<axum::body::Body>,
) -> Response {
    use crate::auth::extract_session_cookie;
    let Some(caller_cookie) = extract_session_cookie(req.headers()) else {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "session_cookie_required" })),
        )
            .into_response();
    };
    let revoked = state.session_table.revoke_others(&caller_cookie).await;
    Json(LogoutAllResponse {
        revoked_count: revoked,
    })
    .into_response()
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

    /// Test state with a password hash + a per-test on-disk path so
    /// `POST /api/settings/password` can persist a rotation. Returns a
    /// `(state, token, tempdir, cookie_value)` tuple — the cookie is
    /// pre-issued through `SessionTable::issue` so the test can pass
    /// it directly in `Cookie: gtmux_auth=...` without round-tripping
    /// `/auth/login`.
    async fn password_test_state(
        initial_password: &str,
    ) -> (AppState, TokenString, tempfile::TempDir, String) {
        let (state, token) = test_state();
        let initial_hash = crate::auth::hash_password(initial_password).expect("hash");
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("password.argon2");
        let state = state.with_password_hash_path(path);
        *state.password_hash.write().await = Some(initial_hash);
        let cookie = state
            .session_table
            .issue(crate::auth::AuthMode::Password)
            .await
            .expect("issue cookie");
        (state, token, tmp, cookie)
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

    // ─── Slice D-3: POST /api/settings/password ─────────────────────

    #[tokio::test]
    async fn password_change_happy_path_persists_and_reissues_cookie() {
        let (state, _token, _tmp, cookie) = password_test_state("oldpw123").await;
        let app = router(state.clone());
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/settings/password")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"current_password":"oldpw123","new_password":"newpw456"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // Set-Cookie is issued, replacing the caller's cookie.
        let set_cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .expect("Set-Cookie present")
            .to_str()
            .unwrap()
            .to_string();
        assert!(set_cookie.starts_with("gtmux_auth="));
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["revoked_count"].is_number());
        // The disk file now contains a hash that verifies the new password.
        let disk_hash = std::fs::read_to_string(
            state.password_hash_path.as_ref().unwrap().as_ref(),
        )
        .unwrap();
        assert!(crate::auth::verify_password("newpw456", disk_hash.trim()));
        assert!(!crate::auth::verify_password("oldpw123", disk_hash.trim()));
        // The in-memory hash matches, so a follow-up login flow would verify.
        let mem = state.password_hash.read().await.clone();
        assert!(crate::auth::verify_password("newpw456", mem.as_deref().unwrap()));
        // The caller's *old* cookie is dead (we revoked + re-issued).
        assert!(state.session_table.validate(&cookie).await.is_none());
    }

    #[tokio::test]
    async fn password_change_wrong_current_returns_401() {
        let (state, _token, _tmp, cookie) = password_test_state("oldpw123").await;
        let app = router(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/settings/password")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"current_password":"wrong","new_password":"newpw456"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "current_password_mismatch");
    }

    #[tokio::test]
    async fn password_change_weak_new_returns_400() {
        let (state, _token, _tmp, cookie) = password_test_state("oldpw123").await;
        let app = router(state);
        // Both branches of weak validation: too short, and long enough
        // but missing a digit.
        for body in [
            r#"{"current_password":"oldpw123","new_password":"short1"}"#,
            r#"{"current_password":"oldpw123","new_password":"nodigits"}"#,
        ] {
            let resp = app
                .clone()
                .oneshot(
                    HttpRequest::builder()
                        .method(Method::POST)
                        .uri("/api/settings/password")
                        .header(header::HOST, TEST_HOST)
                        .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "body={body}");
            let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(v["error"], "weak_password");
            assert_eq!(v["min_length"], MIN_PASSWORD_LENGTH);
        }
    }

    #[tokio::test]
    async fn password_change_without_password_set_returns_503() {
        // No initial hash + no path → password_not_set.
        let (state, _token) = test_state();
        let cookie = state
            .session_table
            .issue(crate::auth::AuthMode::Token)
            .await
            .unwrap();
        let app = router(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/settings/password")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"current_password":"x","new_password":"newpw456"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "password_not_set");
    }

    #[tokio::test]
    async fn password_change_revokes_others_keeps_caller_pending_reissue() {
        let (state, _token, _tmp, cookie) = password_test_state("oldpw123").await;
        // Pre-seed 3 *other* sessions; each must be revoked.
        for _ in 0..3 {
            state
                .session_table
                .issue(crate::auth::AuthMode::Password)
                .await
                .unwrap();
        }
        assert_eq!(state.session_table.len().await, 4); // 3 + caller
        let app = router(state.clone());
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/settings/password")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"current_password":"oldpw123","new_password":"newpw456"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        // Revoked_count is the 3 other sessions (the caller's cookie is
        // revoked *after* `revoke_others`, so it's not counted in the
        // wire response).
        assert_eq!(v["revoked_count"], 3);
        // Exactly one live session remains — the freshly issued caller
        // cookie from the Set-Cookie header.
        assert_eq!(state.session_table.len().await, 1);
    }

    // ─── Slice D-3: POST /api/settings/logout-all ───────────────────

    #[tokio::test]
    async fn logout_all_revokes_others_keeps_caller() {
        let (state, _token, _tmp, cookie) = password_test_state("oldpw123").await;
        for _ in 0..2 {
            state
                .session_table
                .issue(crate::auth::AuthMode::Password)
                .await
                .unwrap();
        }
        assert_eq!(state.session_table.len().await, 3);
        let app = router(state.clone());
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/settings/logout-all")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["revoked_count"], 2);
        // Caller still validates.
        assert!(state.session_table.validate(&cookie).await.is_some());
        // Other sessions gone.
        assert_eq!(state.session_table.len().await, 1);
    }

    #[tokio::test]
    async fn logout_all_without_cookie_returns_403() {
        // Bearer-only request — should be blocked by the explicit
        // session-cookie check inside the handler.
        let (state, token) = test_state();
        let app = router(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/settings/logout-all")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "session_cookie_required");
    }
}
