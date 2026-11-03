//! Authenticated lifecycle requests. The embedding host owns process teardown.
use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use crate::auth::{apply_security_headers, parse_step_up_body, verify_step_up};
use crate::AppState;

pub async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let stopping = state.shutdown_signal.as_ref().is_some_and(|s| *s.borrow());
    Json(json!({
        "state": if stopping { "stopping" } else { "running" },
        "instance": state.config.server.session,
        "bind": state.config.server.bind, "port": state.config.server.port,
        "active_terminals": state.hub.as_ref().map(|h| h.backend().pane_count()).unwrap_or(0),
        "attached_sessions": state.session_locks.lock().await.len(),
        "can_shutdown": state.shutdown_signal.as_ref().is_some_and(|s| !s.is_closed()),
        "can_restart": false,
    }))
}

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

    let Some(signal) = state.shutdown_signal.as_ref().filter(|s| !s.is_closed()) else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "shutdown_not_managed"}))).into_response();
    };
    // Idempotent request; the host delays teardown until this response can flush.
    signal.send_replace(true);
    (StatusCode::ACCEPTED, Json(json!({ "shutdown": "scheduled", "expected_exit_code": 6 }))).into_response()
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
            behavior: gtmux_config::BehaviorSettings::default(),
        public_origin: None,
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

    #[tokio::test]
    async fn shutdown_is_host_owned_idempotent_and_observable() {
        let (mut state, token) = token_only_state();
        state.hub = Some(gtmux_ws_server::Hub::new(gtmux_pty_backend::PtyBackend::new()));
        let request = || HttpRequest::builder().method(Method::POST).uri("/api/shutdown")
            .header(header::HOST, TEST_HOST).header(header::AUTHORIZATION, bearer(&token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&json!({"credential":token.0})).unwrap())).unwrap();
        let unmanaged = crate::router_with_state(state.clone()).oneshot(request()).await.unwrap();
        assert_eq!(unmanaged.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(unmanaged.into_body(), 4096).await.unwrap();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["error"], "shutdown_not_managed");
        let (tx, mut rx) = tokio::sync::watch::channel(false);
        let state = state.with_shutdown_signal(tx);
        assert_eq!(status(State(state.clone())).await.0["state"], "running");
        let app = crate::router_with_state(state.clone());
        for _ in 0..2 {
            assert_eq!(app.clone().oneshot(request()).await.unwrap().status(), StatusCode::ACCEPTED);
        }
        rx.changed().await.unwrap();
        assert!(*rx.borrow());
        let snapshot = status(State(state.clone())).await.0;
        assert_eq!(snapshot["state"], "stopping");
        assert_eq!(snapshot["can_restart"], false);
        drop(rx);
        assert_eq!(app.oneshot(request()).await.unwrap().status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn server_status_requires_authentication() {
        let (state, _) = token_only_state();
        let response = crate::router_with_state(state).oneshot(HttpRequest::builder()
            .uri("/api/server/status").header(header::HOST, TEST_HOST).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

}
