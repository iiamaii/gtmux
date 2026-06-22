//! Slice D-5 — `POST /api/shutdown` (BE-9 Tier 3, ADR-0014 D12).
//!
//! Triggers a graceful server exit from the browser. Returns 202
//! immediately so the FE can flip its banner before the process dies;
//! the actual teardown runs on a detached tokio task that:
//!   1. waits ~50 ms for the 202 response to flush
//!   2. publishes a `0x89 SERVER_SHUTDOWN` notify on the hub
//!   3. waits ~200 ms for WS handlers to emit + close (1000 normal)
//!   4. releases every session lock currently held by this server
//!   5. calls `std::process::exit(EXIT_GRACEFUL)`
//!
//! Child-process SIGHUP happens naturally on process exit (the
//! `PtyBackend` `Drop` chain sends SIGTERM/SIGHUP per ADR-0014 D5).
//! Session record flush is a no-op invariant — `PUT /api/layout` is
//! always atomic so the disk is authoritative at every instant.

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use std::time::Duration;

use crate::auth::{apply_security_headers, parse_step_up_body, verify_step_up};
use crate::AppState;

/// Exit code for a graceful shutdown (mirrors ADR-0014 D7's exit-code
/// regimen — `exit 6 = graceful`).
const EXIT_GRACEFUL: i32 = 6;

/// Delay between 202 response and the WS broadcast. Picked to be long
/// enough for axum to flush the response on a localhost socket but
/// short enough that the user perceives the shutdown as immediate.
const PRE_BROADCAST_DELAY: Duration = Duration::from_millis(50);

/// Delay between WS broadcast and process exit. Each WS handler needs
/// time to (a) drain the channel, (b) encode + send the `0x89` frame,
/// (c) send the close frame. 200 ms is comfortable on localhost.
const PRE_EXIT_DELAY: Duration = Duration::from_millis(200);

/// `POST /api/shutdown` — ADR-0014 D12. Schedules a graceful exit.
///
/// Outcomes:
/// - 202 + `{ "shutdown": "scheduled", "expected_exit_code": 6 }`
///   on success — the actual exit lands a few hundred ms later via a
///   detached background task. The auth middleware (`/api/*` bearer or
///   cookie) gates this — same trust level as `gtmux teardown`.
/// - 503 `hub_not_configured` when the hub is missing (unit-test
///   AppState without `with_hub_*`). In production the hub is always
///   present; this branch documents the precondition without panicking.
///
/// Step-up re-auth (ADR-0020 D16): the body carries
/// `{ "credential": "<password | token>" }`, verified mode-aware *before*
/// any teardown is scheduled. A missing credential → `401 credential_required`;
/// a mismatch → `401 invalid_credential`; password-mode brute-force →
/// `429 + Retry-After`. Credential verification is the *first* precondition;
/// the `hub_not_configured` 503 check runs only after it passes.
pub async fn shutdown_handler(State(state): State<AppState>, req: Request<Body>) -> Response {
    // ADR-0020 D16: verify the step-up credential before scheduling teardown.
    // Read the body manually (empty / absent body → `credential_required`,
    // never a deserialize 400/500).
    let (parts, body) = req.into_parts();
    let peer = crate::auth::peer_from_parts(&parts);
    let headers = parts.headers;
    let body = match parse_step_up_body(body).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    if let Err(rejection) = verify_step_up(&state, &headers, peer, &body).await {
        let mut resp = rejection.into_response();
        apply_security_headers(resp.headers_mut(), &state.config);
        return resp;
    }

    // We never schedule the task without a hub — there'd be no way to
    // notify WS subscribers, and FE would see a bare close (1000)
    // without an intent marker.
    if state.hub.is_none() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "hub_not_configured" })),
        )
            .into_response();
    }

    tokio::spawn(perform_shutdown(state));

    (
        StatusCode::ACCEPTED,
        Json(json!({
            "shutdown": "scheduled",
            "expected_exit_code": EXIT_GRACEFUL,
        })),
    )
        .into_response()
}

async fn perform_shutdown(state: AppState) {
    // 1. Let the 202 reach the FE.
    tokio::time::sleep(PRE_BROADCAST_DELAY).await;

    // 2. Notify every WS subscriber. `publish_server_shutdown` is a
    //    fire-and-forget — if no subscribers exist (no WS connections)
    //    the send is a no-op.
    if let Some(hub) = state.hub.as_ref() {
        hub.publish_server_shutdown("user_initiated", EXIT_GRACEFUL);
    }

    // 3. Give WS handlers room to emit + close.
    tokio::time::sleep(PRE_EXIT_DELAY).await;

    // 4. Release session locks explicitly. `std::process::exit` skips
    //    Rust destructors, so we cannot rely on `LockGuard::drop` —
    //    iterate the holder map and clean up before exit. Per ADR-0014
    //    D7 step 3, `${XDG_STATE_HOME}/.locks/<name>.lock` files get
    //    unlinked here so the next boot doesn't see stale orphans.
    {
        let mut holders = state.session_locks.lock().await;
        let names: Vec<String> = holders.keys().cloned().collect();
        for name in &names {
            if let Some(mut guard) = holders.remove(name) {
                guard.release();
            }
        }
    }

    // 5. Bye.
    tracing::info!(exit_code = EXIT_GRACEFUL, "shutdown: graceful exit");
    std::process::exit(EXIT_GRACEFUL);
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Method, Request as HttpRequest, StatusCode};
    use gtmux_auth::{issue_token, TokenString};
    use gtmux_config::{Config, RuntimeConfig, SecurityConfig, ServerConfig};
    use tower::ServiceExt;

    const TEST_HOST: &str = "127.0.0.1:9001";
    const TEST_ORIGIN: &str = "http://localhost:9001";

    fn bearer(token: &TokenString) -> String {
        format!("Bearer {}", token.0)
    }

    fn token_only_state() -> (AppState, TokenString) {
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
            server_workspace: None,
            default_session_workspace: None,
            auth: gtmux_config::AuthConfig::default(),
            assets: gtmux_config::AssetsConfig::default(),
        };
        let state = AppState::new(cfg, token.clone());
        (state, token)
    }

    #[tokio::test]
    async fn shutdown_without_hub_returns_503() {
        // The bare `AppState::new` has no hub — this exercises the
        // precondition branch without actually scheduling exit. The 503
        // here is also the unit-test contract: production wires
        // `with_hub_and_path` so the handler always reaches the 202
        // branch.
        let (state, token) = token_only_state();
        let app = crate::router_with_state(state);
        // ADR-0020 D16: credential is verified *before* the hub precondition,
        // so supply a valid token-mode credential to reach the 503 branch.
        let cred_body = serde_json::to_vec(&json!({ "credential": token.0 })).unwrap();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/shutdown")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(cred_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "hub_not_configured");
    }

    #[tokio::test]
    async fn shutdown_without_auth_returns_401() {
        let (state, _token) = token_only_state();
        let app = crate::router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/shutdown")
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ─── ADR-0020 D16: step-up re-auth on shutdown ───────────────────────
    //
    // The 202-scheduled branch can't be unit-tested (it would
    // `std::process::exit` and tear the harness down), so a *successful*
    // credential is observed by it reaching the next precondition — the
    // `hub_not_configured` 503 (no hub is wired in unit-test `AppState`). The
    // 503 therefore proves "credential passed; the action would proceed". A
    // *rejected* credential is observed by the 401 short-circuit before the
    // hub check ever runs.

    /// Issue a token-mode session cookie against `state` so a cookie-authed
    /// shutdown request can pass the `/api/*` middleware (the credential gate
    /// is independent of the session-auth gate).
    async fn token_cookie(state: &AppState) -> String {
        state
            .session_table
            .issue(crate::auth::AuthMode::Token)
            .await
            .expect("issue cookie")
    }

    #[tokio::test]
    async fn shutdown_requires_credential() {
        // Authenticated session, but no `credential` in the body → 401
        // `credential_required`, and the server is NOT torn down (we reach the
        // 401 short-circuit, never the hub/teardown path).
        let (state, token) = token_only_state();
        let app = crate::router_with_state(state);
        for body in [Body::empty(), Body::from("{}")] {
            let resp = app
                .clone()
                .oneshot(
                    HttpRequest::builder()
                        .method(Method::POST)
                        .uri("/api/shutdown")
                        .header(header::HOST, TEST_HOST)
                        .header(header::AUTHORIZATION, bearer(&token))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
            let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
            let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(v["error"], "credential_required");
        }
    }

    #[tokio::test]
    async fn shutdown_token_mode_verifies() {
        // password_set == false → credential is the server token.
        let (state, token) = token_only_state();
        let cookie = token_cookie(&state).await;
        let app = crate::router_with_state(state);

        // Wrong token → 401 invalid_credential, no teardown.
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/shutdown")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"credential":"not-the-token"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "invalid_credential");

        // Correct token → credential passes, falls through to the hub
        // precondition (503 in unit-test; 202 in production).
        let cred = serde_json::to_vec(&json!({ "credential": token.0 })).unwrap();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/shutdown")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(cred))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            v["error"], "hub_not_configured",
            "correct token must pass the credential gate"
        );
    }

    #[tokio::test]
    async fn shutdown_password_mode_verifies() {
        // password_set == true → credential is the password.
        let (state, _token) = token_only_state();
        let hash = crate::auth::hash_password("shutdownpw1").expect("hash");
        *state.password_hash.write().await = Some(hash);
        let cookie = token_cookie(&state).await;
        let app = crate::router_with_state(state);

        // Wrong password → 401 invalid_credential.
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/shutdown")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"credential":"wrongpw"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "invalid_credential");

        // Correct password → passes credential gate → hub precondition 503.
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/shutdown")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"credential":"shutdownpw1"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            v["error"], "hub_not_configured",
            "correct password must pass the credential gate"
        );
    }

    // Notes on what is NOT unit-tested here:
    //   * 202 + scheduled exit — this would actually `std::process::exit`
    //     and tear down the cargo-test harness. The 202 path is
    //     exercised by smoke gate 5-12 against a release binary running
    //     in its own process.
    //   * `0x89 SERVER_SHUTDOWN` frame emission — covered by smoke gate
    //     5-12's WS read + envelope parse before the close arrives.
}
