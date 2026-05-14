//! gtmux-http-api — axum HTTP router (P0-HTTP-1 + P0-HTTP-2).
//!
//! Routes:
//!   GET  /healthz           — liveness probe, no auth gate
//!   GET  /auth/bootstrap    — one-shot token→cookie exchange + 302 /
//!   GET  /api/layout        — current snapshot + ETag (304 on If-None-Match)
//!   PUT  /api/layout        — atomic swap, If-Match required, 412 on stale
//!
//! Middleware chain (in order; outermost first):
//!   1. tower_http::trace::TraceLayer        — request span (query-string redacted)
//!   2. OriginCheck                          — cors_origins allowlist (ADR-0003 D3)
//!   3. HostCheck                            — effective_host_allowlist (ADR-0003 D2)
//!   4. BearerAuth                           — only on `/api/*` (ADR-0003 D6, R(rej)2)
//!   5. tower_http::cors::CorsLayer          — preflight + dynamic origin echo
//!
//! Contract references:
//!   * `docs/adr/0003-security-defaults.md`        — D2/D4/D6/D13 + R(rej)2 exception
//!   * `docs/ssot/security-defaults.md`            — §1 headers, §4 cookie attrs
//!   * `docs/ssot/canvas-layout-schema.md`         — §2 ETag normalisation, §3 PUT rules
//!   * `docs/reports/0010-grill-amendments.md` D12 — Canvas layout = HTTP PUT/ETag
//!   * `docs/reports/0012-bootstrap-smoke.md` §3   — P0-HTTP-1, P0-HTTP-2 contracts
//!
//! Security notes:
//!   * SHA256-128 is used for the layout ETag (the first 16 bytes of a SHA-256
//!     digest of the canonical-form JSON payload). MD5 is explicitly avoided
//!     for hygiene — even though ETags are not collision-sensitive in HTTP
//!     semantics, a colliding payload would still confuse If-Match flows.
//!   * The `?redirect=` parameter on `/auth/bootstrap` is normalised to a
//!     host-relative path; any value that does not begin with a single `/`
//!     followed by a path char is replaced with `/`. This blocks the Open
//!     Redirect class (`?redirect=https://evil.example`, `?redirect=//evil`).
//!   * Cookies use `Secure` only in Cloud mode. Local mode is plain HTTP so
//!     `Secure` would cause the browser to silently drop the cookie.
//!   * Authentication failures increment `state.auth_failure_counter` — this
//!     gives downstream throttle middleware a hookable signal without yet
//!     enforcing the limit (P1 work, per ADR-0003 D12 cloud-only).
//!
//! The crate is intentionally `forbid(unsafe_code)` and never `unwrap`s on
//! user input. Schema validation is delegated to `serde_json::Value` for now
//! and a hook (`SchemaValidator`) is exposed for `gtmux-canvas-layout`
//! (Sprint 3+) to slot in without changing the router shape.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use gtmux_auth::{verify_token, TokenString};
use gtmux_config::{Config, Mode};
use ring::digest::{digest, SHA256};
use serde::Deserialize;
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::RwLock;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::warn;

// ─────────────────────────────────────────────────────────────────────────────
//  Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Server-side canvas layout snapshot. The 16-byte raw ETag is the canonical
/// form; the hex string is a rendering helper kept in sync with the bytes.
#[derive(Debug, Clone)]
pub struct LayoutSnapshot {
    /// 16-byte raw ETag (SHA-256-128 of the canonical body bytes).
    pub etag: [u8; 16],
    /// 32-character lowercase hex of `etag` — for `ETag` header rendering.
    pub etag_hex: String,
    /// The current layout body as an opaque JSON value. A future
    /// `gtmux-canvas-layout` crate will replace this with a strongly-typed
    /// struct; the API contract holds because canonical JSON serialisation is
    /// stable for both shapes.
    pub body: Value,
}

impl LayoutSnapshot {
    /// Build the initial empty snapshot per `canvas-layout-schema.md` §4.1.
    pub fn empty() -> Self {
        let body = json!({
            "schema_version": 1,
            "groups": [],
            "panels": [],
        });
        Self::from_body(body)
    }

    fn from_body(body: Value) -> Self {
        let bytes = canonical_serialize(&body);
        let (etag, etag_hex) = compute_etag(&bytes);
        Self {
            etag,
            etag_hex,
            body,
        }
    }
}

/// Shared application state wired into the router. Cloning is cheap (Arc).
#[derive(Clone)]
pub struct AppState {
    /// Loaded gtmux config — used for mode, host/origin allowlists, port.
    pub config: Arc<Config>,
    /// The session token for this Server run.
    pub token: Arc<TokenString>,
    /// Layout snapshot guarded by an `RwLock` for atomic GET / PUT swap.
    pub layout: Arc<RwLock<LayoutSnapshot>>,
    /// Auth-failure counter exposed for downstream rate-limit middleware
    /// (P1 enforcement; ADR-0003 D12 cloud-only). The counter is monotonic.
    pub auth_failure_counter: Arc<AtomicU64>,
    /// Optional WS broadcast hub. When set, `layout_put_handler` publishes
    /// the new ETag so live WS subscribers re-hydrate via the dispatcher's
    /// `LAYOUT_CHANGED` path. `None` in unit-tests that exercise the HTTP
    /// surface in isolation.
    pub hub: Option<gtmux_ws_server::Hub>,
}

impl AppState {
    /// Assemble shared state with a fresh empty layout snapshot.
    /// `hub` is `None`; production callers must use [`AppState::with_hub`].
    pub fn new(config: Config, token: TokenString) -> Self {
        Self {
            config: Arc::new(config),
            token: Arc::new(token),
            layout: Arc::new(RwLock::new(LayoutSnapshot::empty())),
            auth_failure_counter: Arc::new(AtomicU64::new(0)),
            hub: None,
        }
    }

    /// Assemble shared state and attach a WS broadcast hub so PUT-driven
    /// layout commits fan out to live subscribers.
    pub fn with_hub(config: Config, token: TokenString, hub: gtmux_ws_server::Hub) -> Self {
        let mut me = Self::new(config, token);
        me.hub = Some(hub);
        me
    }
}

/// Errors produced by the HTTP API surface. Each variant maps to a stable
/// machine-readable `error` code returned in the JSON body and a HTTP status.
#[derive(Debug, Error)]
pub enum HttpApiError {
    /// Origin header missing or not in allowlist.
    #[error("origin_forbidden")]
    OriginForbidden,
    /// Host header missing or not in allowlist.
    #[error("host_forbidden")]
    HostForbidden,
    /// Authorization missing / malformed / wrong token.
    #[error("unauthorized")]
    Unauthorized,
    /// PUT without `If-Match`.
    #[error("precondition_required")]
    PreconditionRequired,
    /// PUT with stale `If-Match`.
    #[error("precondition_failed")]
    PreconditionFailed,
    /// Body did not satisfy the canvas-layout schema.
    #[error("bad_request: {0}")]
    BadRequest(String),
    /// Payload exceeded the 256 KB cap.
    #[error("payload_too_large")]
    PayloadTooLarge,
    /// Bootstrap query string did not include `token=`.
    #[error("missing_token")]
    MissingToken,
}

impl HttpApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::OriginForbidden | Self::HostForbidden => StatusCode::FORBIDDEN,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::PreconditionRequired => StatusCode::PRECONDITION_REQUIRED,
            Self::PreconditionFailed => StatusCode::PRECONDITION_FAILED,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::MissingToken => StatusCode::BAD_REQUEST,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::OriginForbidden => "origin_forbidden",
            Self::HostForbidden => "host_forbidden",
            Self::Unauthorized => "unauthorized",
            Self::PreconditionRequired => "precondition_required",
            Self::PreconditionFailed => "precondition_failed",
            Self::BadRequest(_) => "bad_request",
            Self::PayloadTooLarge => "payload_too_large",
            Self::MissingToken => "missing_token",
        }
    }
}

impl IntoResponse for HttpApiError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": self.code(),
            "message": self.to_string(),
        });
        (self.status(), Json(body)).into_response()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Router factory
// ─────────────────────────────────────────────────────────────────────────────

/// Build the full HTTP router with the documented middleware chain.
///
/// Returns an owned `axum::Router` ready to be merged with the WebSocket
/// router and handed to `axum::serve`. The config and token are cloned into
/// the `AppState` — callers may continue to hold their own references. No SPA
/// static fallback is wired; unknown paths return the structured 404.
pub fn router(config: &Config, token: &TokenString) -> Router {
    router_with_static(config, token, None)
}

/// Like [`router`] but mounts the built SPA at `frontend_dist` as the catch-all
/// fallback. Unknown paths first try the directory, then fall back to
/// `index.html` so client-side routing works. Used by `gtmux start` to serve
/// the bundled UI from a single port; tests typically pass `None`.
pub fn router_with_static(
    config: &Config,
    token: &TokenString,
    frontend_dist: Option<&Path>,
) -> Router {
    let state = AppState::new(config.clone(), token.clone());
    router_with_state_and_spa(state, frontend_dist)
}

/// Production variant: takes a fully-wired [`AppState`] (typically built
/// via [`AppState::with_hub`]) and an optional bundled SPA directory.
pub fn router_with_app_state(state: AppState, frontend_dist: Option<&Path>) -> Router {
    router_with_state_and_spa(state, frontend_dist)
}

/// Variant of [`router`] that lets callers (and tests) supply a pre-built
/// `AppState` — used to seed a non-empty layout or share counters across
/// multiple router instances. Production callers should prefer [`router`].
pub fn router_with_state(state: AppState) -> Router {
    router_with_state_and_spa(state, None)
}

/// Internal builder shared by every public router constructor. The optional
/// `frontend_dist` swaps the catch-all 404 for a `ServeDir` + `ServeFile`
/// chain so a single port serves both the API and the bundled SPA.
pub fn router_with_state_and_spa(state: AppState, frontend_dist: Option<&Path>) -> Router {
    // Authenticated subtree — `/api/*` routes. Bearer middleware is applied
    // here (not on the outer router) so `/healthz` and `/auth/bootstrap`
    // bypass it. Origin/Host checks still run on every request via the outer
    // chain.
    let api = Router::new()
        .route(
            "/api/layout",
            get(layout_get_handler).put(layout_put_handler),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            bearer_auth_middleware,
        ));

    let mut router = Router::new()
        .merge(api)
        .route("/auth/bootstrap", get(bootstrap_handler))
        .route("/healthz", get(healthz_handler));

    router = match frontend_dist {
        Some(dist) => {
            // SPA fallback: serve from `dist`, deferring unmatched paths to
            // `index.html` so client-side routing works. The Origin/Host
            // middleware still gates these requests; top-level navigations
            // omit Origin and so are passed through (see middleware below).
            let index = dist.join("index.html");
            let serve = ServeDir::new(dist).not_found_service(ServeFile::new(index));
            router.fallback_service(serve)
        }
        None => router.fallback(not_found_handler),
    };

    router
        .layer(middleware::from_fn_with_state(
            state.clone(),
            host_check_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            origin_check_middleware,
        ))
        .layer(TraceLayer::new_for_http().make_span_with(make_redacted_span))
        .with_state(state)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Middleware
// ─────────────────────────────────────────────────────────────────────────────

/// Origin check (ADR-0003 D3 / SSoT §1.2). Skipped for `/healthz` and the
/// bootstrap exchange — both are *entry points* where the browser may not
/// send an `Origin` header (top-level navigation). Cross-origin fetches into
/// `/api/*` would always send `Origin` per the Fetch spec, so the check fires
/// where it matters.
async fn origin_check_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    if path == "/healthz" || path == "/auth/bootstrap" {
        return next.run(req).await;
    }
    if let Some(origin) = req.headers().get(header::ORIGIN) {
        // Reject Origin: null and any wildcard (R(rej)3). Exact match only.
        let origin_str = origin.to_str().unwrap_or("");
        if origin_str.is_empty() || origin_str == "null" {
            return HttpApiError::OriginForbidden.into_response();
        }
        // `effective_cors_origins` falls back to `http://<bind>:<port>` when
        // the user left the list empty (G1 same-origin default). Cloud
        // deployments with TLS terminate at a reverse proxy and must set
        // the list explicitly (no `wss://` synthesis here).
        let allowed = state.config.effective_cors_origins();
        if !allowed.iter().any(|a| a == origin_str) {
            return HttpApiError::OriginForbidden.into_response();
        }
    }
    // Missing Origin on /api/* is permissible — same-origin GET/PUT from the
    // SPA does not include it for non-CORS requests. Bearer auth still gates.
    next.run(req).await
}

/// Host header check (ADR-0003 D2 / SSoT §1.2 — DNS-rebinding defence). Runs
/// on every route including `/healthz` per the spec.
async fn host_check_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let host = match req.headers().get(header::HOST) {
        Some(h) => h.to_str().unwrap_or("").to_string(),
        None => return HttpApiError::HostForbidden.into_response(),
    };
    if host.is_empty() {
        return HttpApiError::HostForbidden.into_response();
    }
    let allowlist = state.config.effective_host_allowlist();
    if !allowlist.iter().any(|h| h == &host) {
        return HttpApiError::HostForbidden.into_response();
    }
    next.run(req).await
}

/// Bearer / cookie authentication (ADR-0003 D6).
///
/// Accepts either:
///   * `Authorization: Bearer <token>` header, or
///   * `Cookie: gtmux_auth=<token>` (set by the bootstrap exchange).
///
/// Failure increments `state.auth_failure_counter` so a future rate-limit
/// middleware can throttle without coupling. The cleartext token is *never*
/// logged — only the failure count and the route path.
async fn bearer_auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let presented = extract_presented_token(req.headers());
    let Some(presented) = presented else {
        state.auth_failure_counter.fetch_add(1, Ordering::Relaxed);
        return HttpApiError::Unauthorized.into_response();
    };
    if !verify_token(&presented, &state.token) {
        state.auth_failure_counter.fetch_add(1, Ordering::Relaxed);
        return HttpApiError::Unauthorized.into_response();
    }
    next.run(req).await
}

fn extract_presented_token(headers: &HeaderMap) -> Option<String> {
    // 1. Authorization: Bearer <token>
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        if let Ok(v) = auth.to_str() {
            if let Some(rest) = v.strip_prefix("Bearer ") {
                let trimmed = rest.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    // 2. Cookie: gtmux_auth=<token>
    if let Some(cookie_header) = headers.get(header::COOKIE) {
        if let Ok(v) = cookie_header.to_str() {
            for pair in v.split(';') {
                let pair = pair.trim();
                if let Some(value) = pair.strip_prefix(concat!(COOKIE_NAME!(), "=")) {
                    if !value.is_empty() {
                        return Some(value.to_string());
                    }
                }
            }
        }
    }
    None
}

// `concat!` cannot consume a `const &str`, so the cookie name is exposed via
// a macro that expands to a literal. Keeps the name single-sourced.
macro_rules! COOKIE_NAME {
    () => {
        "gtmux_auth"
    };
}
pub(crate) use COOKIE_NAME;

/// Cookie name issued by the bootstrap exchange (ADR-0003 D6 / SSoT §1.3).
///
/// Exposed for tests and external smoke scripts. Note that the bootstrap
/// exchange uses `gtmux_auth` rather than the SSoT default `gtmux_session`
/// because the bootstrap exchange has different lifetime semantics from the
/// future cloud-mode `gtmux_session` cookie — and the contract document for
/// this task names `gtmux_auth` explicitly.
pub const COOKIE_NAME_STR: &str = COOKIE_NAME!();

// ─────────────────────────────────────────────────────────────────────────────
//  Handlers
// ─────────────────────────────────────────────────────────────────────────────

async fn healthz_handler() -> Response {
    let mut resp = Json(json!({ "ok": true })).into_response();
    apply_security_headers(resp.headers_mut(), Mode::Local /* harmless */);
    resp
}

async fn not_found_handler() -> Response {
    let body = json!({ "error": "not_found" });
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

#[derive(Debug, Deserialize)]
struct BootstrapQuery {
    token: Option<String>,
    redirect: Option<String>,
}

async fn bootstrap_handler(
    State(state): State<AppState>,
    Query(q): Query<BootstrapQuery>,
) -> Response {
    let Some(presented) = q.token.filter(|t| !t.is_empty()) else {
        return HttpApiError::MissingToken.into_response();
    };
    if !verify_token(&presented, &state.token) {
        state.auth_failure_counter.fetch_add(1, Ordering::Relaxed);
        return HttpApiError::Unauthorized.into_response();
    }

    // Normalise redirect target — only host-relative paths are honoured.
    let target = normalise_redirect_target(q.redirect.as_deref());

    // Build cookie. `Secure` only in Cloud mode (Local is plain HTTP — a
    // Secure cookie there would be silently dropped by the browser).
    let secure_flag = matches!(state.config.mode(), Mode::Cloud);
    let cookie = build_session_cookie(&presented, secure_flag);

    // The cookie is HttpOnly (XSS hardening) so JavaScript cannot read the
    // token. The SPA needs the token for the WS Sec-WebSocket-Protocol
    // header, however, which JS *does* compose — so we ship a one-shot HTML
    // hop that mirrors the token into sessionStorage and then replaces the
    // URL with the redirect target. The cookie still rides along for
    // tower-http ServeDir requests that some user flows depend on.
    let body = render_bootstrap_landing(&presented, &target);

    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::SET_COOKIE, cookie)
        // Tell intermediaries (and the browser) never to cache the landing
        // page — the inline token must not be replayed from disk on the
        // next demo session.
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .unwrap_or_else(|_| {
            // Builder errors only when header values contain CRLF — our inputs
            // are all server-generated, so this is unreachable. We still avoid
            // panicking by returning a generic 500 if it ever does.
            (StatusCode::INTERNAL_SERVER_ERROR, "").into_response()
        });
    apply_security_headers(resp.headers_mut(), state.config.mode());
    resp
}

/// Render the minimal landing page that copies the token into
/// sessionStorage and then redirects to `target`. Token + target are
/// JSON-encoded so any character set (including future formats) ends up
/// in a safe JavaScript string literal; `</` is rewritten as `<\/` so a
/// pathological target cannot terminate the inline `<script>` early.
fn render_bootstrap_landing(token: &str, target: &str) -> String {
    fn js_literal(value: &str) -> String {
        let json = serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string());
        json.replace("</", "<\\/")
    }
    let token_js = js_literal(token);
    let target_js = js_literal(target);
    format!(
        r#"<!doctype html>
<meta charset="utf-8">
<title>gtmux — authenticating</title>
<script>
(function () {{
  try {{ sessionStorage.setItem('gtmux_token', {token_js}); }} catch (e) {{}}
  window.location.replace({target_js});
}})();
</script>
<noscript>JavaScript is required to complete gtmux authentication.</noscript>
"#
    )
}

fn build_session_cookie(token_value: &str, secure: bool) -> String {
    // We intentionally do not set Max-Age in this MVP — the session cookie
    // lives until the browser closes. ADR-0003 D13.1 (local on-start rotation)
    // means a new server boot invalidates anything older anyway.
    let mut parts = Vec::with_capacity(5);
    parts.push(format!("{}={}", COOKIE_NAME_STR, token_value));
    parts.push("Path=/".to_string());
    parts.push("HttpOnly".to_string());
    parts.push("SameSite=Strict".to_string());
    if secure {
        parts.push("Secure".to_string());
    }
    parts.join("; ")
}

fn normalise_redirect_target(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return "/".to_string();
    };
    // Block schemes (`http://...`), protocol-relative (`//...`), and
    // backslash variants (`/\evil.example`). Only `/<path-char>` survives.
    let bytes = raw.as_bytes();
    if bytes.first() != Some(&b'/') {
        return "/".to_string();
    }
    if bytes.get(1) == Some(&b'/') || bytes.get(1) == Some(&b'\\') {
        return "/".to_string();
    }
    // Reject CR/LF (response-splitting belt-and-braces; axum would also catch
    // this when building the header value, but we'd rather fall back cleanly
    // than 500).
    if raw.contains('\r') || raw.contains('\n') {
        return "/".to_string();
    }
    raw.to_string()
}

async fn layout_get_handler(State(state): State<AppState>, req: Request) -> Response {
    let snap = state.layout.read().await;
    let etag_quoted = format!("\"{}\"", snap.etag_hex);

    // RFC 7232: If-None-Match → 304 when current ETag matches.
    if let Some(if_none_match) = req.headers().get(header::IF_NONE_MATCH) {
        if let Ok(v) = if_none_match.to_str() {
            if parse_etag_header(v).is_some_and(|h| h == snap.etag_hex) {
                let mut resp = Response::builder()
                    .status(StatusCode::NOT_MODIFIED)
                    .header(header::ETAG, &etag_quoted)
                    .body(Body::empty())
                    .expect("static headers");
                apply_security_headers(resp.headers_mut(), state.config.mode());
                return resp;
            }
        }
    }

    let body = serde_json::to_vec(&snap.body).expect("snapshot is always valid JSON");
    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ETAG, &etag_quoted)
        .body(Body::from(body))
        .expect("static headers");
    apply_security_headers(resp.headers_mut(), state.config.mode());
    resp
}

/// Soft cap from `canvas-layout-schema.md` §3 R9 (256 KB).
const PUT_MAX_BYTES: usize = 256 * 1024;

async fn layout_put_handler(State(state): State<AppState>, req: Request) -> Response {
    // 1. If-Match — required.
    let if_match = match req.headers().get(header::IF_MATCH) {
        Some(v) => match v.to_str() {
            Ok(s) => match parse_etag_header(s) {
                Some(parsed) => parsed,
                None => return HttpApiError::PreconditionRequired.into_response(),
            },
            Err(_) => return HttpApiError::PreconditionRequired.into_response(),
        },
        None => return HttpApiError::PreconditionRequired.into_response(),
    };

    // 2. Read the body up to the 256 KB cap. axum 0.8 returns `Body`, which
    //    we drain via `http-body-util` for a portable size-bounded read.
    let body_bytes = match read_bounded_body(req, PUT_MAX_BYTES).await {
        Ok(b) => b,
        Err(BodyReadError::TooLarge) => return HttpApiError::PayloadTooLarge.into_response(),
        Err(BodyReadError::Io(msg)) => return HttpApiError::BadRequest(msg).into_response(),
    };

    // 3. Parse JSON.
    let body: Value = match serde_json::from_slice(&body_bytes) {
        Ok(v) => v,
        Err(e) => return HttpApiError::BadRequest(format!("json: {e}")).into_response(),
    };

    // 4. Minimal schema check — full validation is gated behind the
    //    `gtmux-canvas-layout` crate (Sprint 3+).
    if let Err(msg) = minimal_layout_check(&body) {
        return HttpApiError::BadRequest(msg).into_response();
    }

    // 5. Atomic compare-and-swap on the ETag. The whole transition runs under
    //    the write lock so two concurrent PUTs cannot observe the same ETag.
    let mut snap = state.layout.write().await;
    if if_match != snap.etag_hex {
        let current_etag_quoted = format!("\"{}\"", snap.etag_hex);
        let mut resp = HttpApiError::PreconditionFailed.into_response();
        // Hand back the current ETag so the client can refetch and retry.
        resp.headers_mut().insert(
            header::ETAG,
            HeaderValue::from_str(&current_etag_quoted).expect("hex is always valid header value"),
        );
        return resp;
    }

    let new_snap = LayoutSnapshot::from_body(body);
    let new_etag = new_snap.etag;
    let new_etag_quoted = format!("\"{}\"", new_snap.etag_hex);
    *snap = new_snap;
    drop(snap);

    // Fan out LAYOUT_CHANGED to every live WS subscriber so the SPA
    // dispatcher revalidates via `If-None-Match` and rehydrates panels.
    match &state.hub {
        Some(hub) => {
            tracing::debug!(
                etag = %new_etag_quoted,
                "layout_put_handler: publishing LAYOUT_CHANGED to WS subscribers"
            );
            hub.publish_layout_changed(new_etag);
        }
        None => {
            tracing::debug!("layout_put_handler: no hub attached, skipping broadcast");
        }
    }

    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ETAG, &new_etag_quoted)
        .body(Body::from(b"{}".to_vec()))
        .expect("static headers");
    apply_security_headers(resp.headers_mut(), state.config.mode());
    resp
}

// ─────────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Apply the SSoT §1.5 security headers to every response. STS is only
/// emitted in Cloud mode (SSoT line for STS).
fn apply_security_headers(headers: &mut HeaderMap, mode: Mode) {
    static NOSNIFF: HeaderValue = HeaderValue::from_static("nosniff");
    static REFERRER: HeaderValue = HeaderValue::from_static("no-referrer");
    static COOP: HeaderValue = HeaderValue::from_static("same-origin");
    static CORP: HeaderValue = HeaderValue::from_static("same-origin");
    static PERMS: HeaderValue =
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), interest-cohort=()");
    static HSTS: HeaderValue = HeaderValue::from_static("max-age=31536000; includeSubDomains");

    headers.insert(header::X_CONTENT_TYPE_OPTIONS, NOSNIFF.clone());
    headers.insert(header::REFERRER_POLICY, REFERRER.clone());
    headers.insert("cross-origin-opener-policy", COOP.clone());
    headers.insert("cross-origin-resource-policy", CORP.clone());
    headers.insert("permissions-policy", PERMS.clone());
    if matches!(mode, Mode::Cloud) {
        headers.insert(header::STRICT_TRANSPORT_SECURITY, HSTS.clone());
    }
}

/// Canonical-form serialisation for ETag stability. We use `serde_json`'s
/// default (compact, no trailing whitespace, deterministic struct ordering)
/// — sufficient for the MVP. Future tightening (sorted keys, normalised
/// numbers) lives in the `gtmux-canvas-layout` crate so behaviour can change
/// without affecting the HTTP surface.
fn canonical_serialize(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).expect("Value is always serialisable")
}

/// Compute SHA256-128 → (raw bytes, lowercase hex). 32 chars.
fn compute_etag(bytes: &[u8]) -> ([u8; 16], String) {
    let d = digest(&SHA256, bytes);
    let full = d.as_ref();
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&full[..16]);
    let mut hex = String::with_capacity(32);
    for b in raw.iter() {
        hex.push_str(&format!("{:02x}", b));
    }
    (raw, hex)
}

/// Parse the value of an `ETag` / `If-Match` / `If-None-Match` header into
/// its 32-character lowercase-hex inner content. Returns `None` if the
/// header is not a single, strong, 32-hex ETag.
fn parse_etag_header(v: &str) -> Option<String> {
    let trimmed = v.trim();
    // RFC 7232: weak ETags begin with `W/`. We accept neither weak nor
    // wildcard (`*`) — PUT must use the strong opaque-tag form.
    if trimmed.starts_with("W/") || trimmed == "*" {
        return None;
    }
    let inner = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))?;
    if inner.len() != 32 {
        return None;
    }
    if !inner
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return None;
    }
    Some(inner.to_string())
}

/// Minimal schema check pending the full `gtmux-canvas-layout` crate. We
/// reject anything that obviously cannot satisfy the schema so the hook into
/// the future validator is a drop-in.
fn minimal_layout_check(body: &Value) -> Result<(), String> {
    let obj = body.as_object().ok_or("body must be a JSON object")?;
    if let Some(g) = obj.get("groups") {
        if !g.is_array() {
            return Err("groups must be an array".to_string());
        }
    } else {
        return Err("missing required field: groups".to_string());
    }
    if let Some(p) = obj.get("panels") {
        if !p.is_array() {
            return Err("panels must be an array".to_string());
        }
    } else {
        return Err("missing required field: panels".to_string());
    }
    if let Some(sv) = obj.get("schema_version") {
        if sv.as_u64() != Some(1) {
            return Err("schema_version must be 1".to_string());
        }
    }
    Ok(())
}

#[derive(Debug)]
enum BodyReadError {
    TooLarge,
    Io(String),
}

/// Drain the request body, refusing payloads larger than `cap`. We do not
/// trust `Content-Length` alone — a malicious client could lie — so the
/// stream is read incrementally and short-circuited at the cap.
async fn read_bounded_body(req: Request, cap: usize) -> Result<Vec<u8>, BodyReadError> {
    use http_body_util::BodyExt;

    let body = req.into_body();
    let collected = body
        .collect()
        .await
        .map_err(|e| BodyReadError::Io(format!("body read: {e}")))?;
    let bytes = collected.to_bytes();
    if bytes.len() > cap {
        return Err(BodyReadError::TooLarge);
    }
    Ok(bytes.to_vec())
}

/// `MakeSpan` impl that records only the path — not the query string. This
/// keeps the bootstrap token out of trace exports (the URL otherwise lands
/// in spans, jaeger payloads, journald, etc.). ADR-0003 §C R(rej)2 redaction.
fn make_redacted_span(req: &Request) -> tracing::Span {
    let path = req.uri().path();
    let method = req.method().as_str();
    tracing::info_span!(
        "http_request",
        method = %method,
        path = %path,
        // query is *intentionally* omitted — never log the raw URI.
    )
}

// Methods/Uri unused-import shake: keep linter quiet without changing
// behaviour. (Some axum builds re-export these via prelude; explicit imports
// document intent.)
const _: fn(&Method, &Uri) = |_, _| {};

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request as HttpRequest;
    use gtmux_auth::issue_token;
    use gtmux_config::{Config, RuntimeConfig, SecurityConfig, ServerConfig};
    use tower::ServiceExt; // for `oneshot`

    const TEST_HOST: &str = "127.0.0.1:9001";
    const TEST_ORIGIN: &str = "http://localhost:9001";

    fn test_config() -> Config {
        Config {
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
        }
    }

    fn make_app() -> (Router, TokenString) {
        let token = issue_token().expect("token");
        let cfg = test_config();
        let app = router(&cfg, &token);
        (app, token)
    }

    fn bearer(token: &TokenString) -> String {
        format!("Bearer {}", token.0)
    }

    #[tokio::test]
    async fn healthz_no_auth() {
        let (app, _token) = make_app();
        let req = HttpRequest::builder()
            .uri("/healthz")
            .header(header::HOST, TEST_HOST)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body, json!({"ok": true}));
    }

    #[tokio::test]
    async fn layout_get_initial() {
        let (app, token) = make_app();
        let req = HttpRequest::builder()
            .uri("/api/layout")
            .header(header::HOST, TEST_HOST)
            .header(header::AUTHORIZATION, bearer(&token))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let etag = resp.headers().get(header::ETAG).cloned();
        assert!(etag.is_some(), "ETag header must be present");
        let etag_str = etag.unwrap();
        let inner = parse_etag_header(etag_str.to_str().unwrap());
        assert!(inner.is_some(), "ETag must be quoted 32-hex");
        assert_eq!(inner.unwrap().len(), 32);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["groups"].as_array().unwrap().len(), 0);
        assert_eq!(body["panels"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn layout_get_304_on_if_none_match() {
        let (app, token) = make_app();
        let first = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let etag = first
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let second = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::IF_NONE_MATCH, &etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn layout_put_requires_if_match() {
        let (app, token) = make_app();
        let body = serde_json::to_vec(&json!({
            "schema_version": 1,
            "groups": [],
            "panels": [],
        }))
        .unwrap();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PRECONDITION_REQUIRED);
    }

    #[tokio::test]
    async fn layout_put_etag_mismatch() {
        let (app, token) = make_app();
        let body = serde_json::to_vec(&json!({
            "schema_version": 1,
            "groups": [],
            "panels": [],
        }))
        .unwrap();
        let stale = "\"00000000000000000000000000000000\"";
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::IF_MATCH, stale)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PRECONDITION_FAILED);
        // The current ETag must be attached so clients can rebase.
        assert!(resp.headers().get(header::ETAG).is_some());
    }

    #[tokio::test]
    async fn layout_put_success_updates_etag() {
        let (app, token) = make_app();
        // 1. Fetch current ETag.
        let get1 = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let etag1 = get1
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        // 2. PUT a new body.
        let new_body = serde_json::to_vec(&json!({
            "schema_version": 1,
            "groups": [{
                "id": "ga1",
                "parent_id": null,
                "label": "main",
                "color": "#abcdef",
                "visibility": true,
                "locked": false,
                "order": 0,
            }],
            "panels": [],
        }))
        .unwrap();
        let put = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::IF_MATCH, &etag1)
                    .body(Body::from(new_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put.status(), StatusCode::OK);
        let etag2 = put
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_ne!(etag1, etag2);

        // 3. GET reflects the new content.
        let get2 = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(get2.into_body(), 64 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["groups"].as_array().unwrap().len(), 1);
        assert_eq!(body["groups"][0]["id"], "ga1");
    }

    #[tokio::test]
    async fn layout_put_schema_violation() {
        let (app, token) = make_app();
        // Get the current ETag first so If-Match matches.
        let get1 = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let etag = get1
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let bogus_body = serde_json::to_vec(&json!({
            "groups": "not an array",
        }))
        .unwrap();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::IF_MATCH, &etag)
                    .body(Body::from(bogus_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn auth_required_for_layout() {
        let (app, _token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn origin_check_blocks_disallowed() {
        let (app, token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::ORIGIN, "http://evil.example")
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn host_check_blocks_disallowed() {
        let (app, token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, "evil.example")
                    .header(header::AUTHORIZATION, bearer(&token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn bootstrap_success_sets_cookie_and_returns_landing_html() {
        let (app, token) = make_app();
        let uri = format!("/auth/bootstrap?token={}", token.0);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri(&uri)
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("content-type set")
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            content_type.starts_with("text/html"),
            "expected text/html landing, got {content_type}"
        );
        let cache_control = resp
            .headers()
            .get(header::CACHE_CONTROL)
            .expect("cache-control set")
            .to_str()
            .unwrap();
        assert_eq!(cache_control, "no-store");
        let set_cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .expect("Set-Cookie present")
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            set_cookie.starts_with(&format!("{}=", COOKIE_NAME_STR)),
            "cookie must start with {}=, got: {set_cookie}",
            COOKIE_NAME_STR,
        );
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Strict"));
        assert!(set_cookie.contains("Path=/"));
        // Local mode → no Secure flag (TLS-less HTTP would drop it).
        assert!(!set_cookie.contains("Secure"));

        let body_bytes = axum::body::to_bytes(resp.into_body(), 8 * 1024)
            .await
            .unwrap();
        let body = std::str::from_utf8(&body_bytes).unwrap().to_string();
        // Token is mirrored into sessionStorage so the SPA can read it for
        // the WS Sec-WebSocket-Protocol path.
        assert!(
            body.contains(&format!(
                "sessionStorage.setItem('gtmux_token', \"{}\"",
                token.0
            )),
            "landing must inject sessionStorage token, got: {body}"
        );
        // And the user lands on `/` (or any normalised target).
        assert!(
            body.contains("window.location.replace(\"/\")"),
            "landing must replace location to '/', got: {body}"
        );
    }

    #[test]
    fn bootstrap_landing_escapes_inline_script_terminator() {
        // Direct unit-level pin on `render_bootstrap_landing` — exercising
        // the renderer with values that *would* close `<script>` early if
        // we ever stopped rewriting `</` to `<\/` inside JS string literals.
        let body = render_bootstrap_landing("abc-token", "/path</script><b>oops");
        assert!(
            !body.contains("</script><b>oops"),
            "raw </script> leaked into HTML: {body}"
        );
        assert!(
            body.contains("<\\/script>"),
            "expected escaped <\\/script>, got: {body}"
        );
        // And the token still lands intact in the JS string literal.
        assert!(
            body.contains("sessionStorage.setItem('gtmux_token', \"abc-token\")"),
            "token must be present in landing body: {body}"
        );
    }

    #[tokio::test]
    async fn bootstrap_wrong_token() {
        let (app, _token) = make_app();
        // 43-char base64url-no-pad shape but wrong content.
        let wrong = "A".repeat(43);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/auth/bootstrap?token={wrong}"))
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(resp.headers().get(header::SET_COOKIE).is_none());
    }

    #[tokio::test]
    async fn bootstrap_missing_token() {
        let (app, _token) = make_app();
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/auth/bootstrap")
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn bootstrap_redirect_target_rejects_external() {
        // The redirect lives inside the inline landing JS now (no Location
        // header). External / protocol-relative inputs must still be
        // normalised down to "/" before they reach the JS string literal.
        for evil in [
            "https://evil.example/x",
            "//evil.example",
            "/\\evil.example",
        ] {
            let (app, token) = make_app();
            let uri = format!("/auth/bootstrap?token={}&redirect={evil}", token.0);
            let resp = app
                .oneshot(
                    HttpRequest::builder()
                        .uri(&uri)
                        .header(header::HOST, TEST_HOST)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 8 * 1024)
                .await
                .unwrap();
            let body = std::str::from_utf8(&body_bytes).unwrap().to_string();
            assert!(
                body.contains("window.location.replace(\"/\")"),
                "evil input {evil} must normalise to '/', got: {body}"
            );
            assert!(
                !body.contains("evil.example"),
                "evil host leaked into landing for input {evil}: {body}"
            );
        }
    }

    #[tokio::test]
    async fn cookie_auth_works_after_bootstrap() {
        let (app, token) = make_app();
        // 1. Bootstrap to obtain the cookie.
        let bootstrap_resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/auth/bootstrap?token={}", token.0))
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let set_cookie = bootstrap_resp
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        // Extract the `name=value` part for the next request.
        let name_value = set_cookie.split(';').next().unwrap().trim().to_string();

        // 2. GET /api/layout with the cookie — no Authorization header.
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/layout")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, &name_value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "cookie auth must satisfy the bearer middleware",
        );
    }

    // ── pure-function unit tests for the helpers ──

    #[test]
    fn etag_helper_is_deterministic() {
        let snap1 = LayoutSnapshot::empty();
        let snap2 = LayoutSnapshot::empty();
        assert_eq!(snap1.etag, snap2.etag);
        assert_eq!(snap1.etag_hex.len(), 32);
        assert!(snap1.etag_hex.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn parse_etag_header_rejects_weak_and_uppercase() {
        assert!(parse_etag_header("W/\"deadbeefdeadbeefdeadbeefdeadbeef\"").is_none());
        assert!(parse_etag_header("\"DEADBEEFDEADBEEFDEADBEEFDEADBEEF\"").is_none());
        assert!(parse_etag_header("*").is_none());
        assert!(parse_etag_header("\"deadbeefdeadbeefdeadbeefdeadbeef\"").is_some());
    }

    #[test]
    fn normalise_redirect_blocks_open_redirect() {
        assert_eq!(normalise_redirect_target(None), "/");
        assert_eq!(normalise_redirect_target(Some("//evil")), "/");
        assert_eq!(normalise_redirect_target(Some("/\\evil")), "/");
        assert_eq!(normalise_redirect_target(Some("https://evil")), "/");
        assert_eq!(normalise_redirect_target(Some("evil")), "/");
        // Legitimate host-relative paths survive.
        assert_eq!(normalise_redirect_target(Some("/canvas")), "/canvas");
        // CR/LF rejected to prevent header-splitting.
        assert_eq!(normalise_redirect_target(Some("/x\r\nSet-Cookie: ev")), "/");
    }
}
