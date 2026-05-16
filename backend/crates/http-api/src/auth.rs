//! Cookie-based auth lifecycle — token + password modes (ADR-0020).
//!
//! Source-of-truth: `docs/adr/0020-auth-lifecycle.md` D1–D10.
//!
//! Responsibilities of this module:
//!   * Issue / validate / revoke opaque session cookies (`gtmux_auth`)
//!     against an in-memory [`SessionTable`].
//!   * Hash + verify passwords with Argon2id (D5 parameters: m=64 MiB,
//!     iter=3, parallelism=4).
//!   * Per-IP sliding-window rate limit on failed POST `/auth/login`.
//!   * Serve `GET /auth`, `POST /auth/login`, `POST /auth/logout`.
//!
//! What this module deliberately does NOT do:
//!   * Cookie persistence across server restarts — D2: in-memory map only
//!     (P1+ for sqlite). Server reboot = all users re-authenticate, which
//!     is *safer* by default.
//!   * WS handshake auth — that lives in `ws-server` and reads the same
//!     `Cookie: gtmux_auth=<id>` header. This module exports
//!     [`SessionTable::validate`] for it to call.
//!   * Settings UI / change-password endpoint — Stage 7 (BE-9).
//!
//! Cookie shape: `gtmux_auth=<base64url-32-bytes>; Path=/; HttpOnly;
//! SameSite=Strict; Max-Age=<seconds>; [Secure]`. `Secure` only in Cloud
//! mode (Local is plain HTTP — browser drops Secure cookies on `http://`).

#![allow(missing_docs)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use argon2::password_hash::SaltString;
use argon2::{Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version};
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use async_trait::async_trait;
use gtmux_auth::{verify_token, TokenString};
use gtmux_config::Mode;
use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::AppState;

// ─────────────────────────────────────────────────────────────────────────────
//  Constants & errors
// ─────────────────────────────────────────────────────────────────────────────

/// Cookie name — single source of truth, must match the value in `lib.rs`.
pub(crate) const COOKIE_NAME: &str = "gtmux_auth";

/// Bytes of random material in the cookie value. base64url-encoded → 43 chars.
const COOKIE_TOKEN_BYTES: usize = 32;

/// Argon2id parameters per ADR-0020 D5: 64 MiB memory, 3 iters, 4 lanes.
const ARGON_MEMORY_KIB: u32 = 64 * 1024;
const ARGON_ITERATIONS: u32 = 3;
const ARGON_PARALLELISM: u32 = 4;

/// Sliding-window length for password-login rate limiting (ADR-0020 D5).
const RATE_WINDOW: Duration = Duration::from_secs(300);

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("argon2: {0}")]
    Argon2(String),
    #[error("password hash io: {0}")]
    Io(#[from] std::io::Error),
    #[error("password mode requires a hash file at {0}")]
    HashFileMissing(PathBuf),
    #[error("rand: {0}")]
    Rand(String),
}

// ─────────────────────────────────────────────────────────────────────────────
//  AuthMode — wraps `config.auth.mode` so handlers don't string-match
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Token,
    Password,
}

impl AuthMode {
    /// Parse from `config.auth.mode`. Unknown values fall back to `Token`
    /// with a warning — fail-open here would silently disable the password
    /// gate, so we *fail-closed* by defaulting to the safer token path.
    pub fn from_config_str(s: &str) -> Self {
        match s {
            "password" => Self::Password,
            "token" => Self::Token,
            other => {
                warn!(value = other, "auth: unknown config.auth.mode; defaulting to token");
                Self::Token
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  SessionTable — in-memory `cookie_token → AuthSession` map
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AuthSession {
    /// Monotonic `Instant` at which this session must be considered expired.
    /// We use `Instant` rather than `SystemTime` so clock skew on the host
    /// cannot revive a long-expired session — only `validate` advances it,
    /// and only forward by the configured `max_age`.
    expires_at: Instant,
    /// Which side of D1 minted this cookie. Carried so a future Settings UI
    /// can render "logged in as token" vs "logged in as password" without a
    /// second round-trip.
    mode: AuthMode,
}

/// In-memory map of live cookies (ADR-0020 D2 §Server-side session table).
///
/// All access goes through the inner `tokio::sync::Mutex` — the surface is
/// tiny (issue/validate/revoke) and writes always touch a single key, so a
/// `Mutex` is the right choice over a `DashMap` (no read-side contention to
/// exploit). The `Arc` wrapper lets `AppState` clone cheaply.
#[derive(Debug)]
pub struct SessionTable {
    inner: Mutex<HashMap<String, AuthSession>>,
    max_age: Duration,
    rng: SystemRandom,
}

impl SessionTable {
    pub fn new(max_age: Duration) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            max_age,
            rng: SystemRandom::new(),
        }
    }

    pub fn max_age(&self) -> Duration {
        self.max_age
    }

    /// Issue a fresh session cookie and return its opaque token.
    pub async fn issue(&self, mode: AuthMode) -> Result<String, AuthError> {
        let cookie_token = generate_cookie_token(&self.rng)?;
        let mut map = self.inner.lock().await;
        map.insert(
            cookie_token.clone(),
            AuthSession {
                expires_at: Instant::now() + self.max_age,
                mode,
            },
        );
        Ok(cookie_token)
    }

    /// Validate a cookie value. On success returns the auth mode and bumps
    /// `expires_at` forward by `max_age` (rolling renewal, D3). Expired
    /// entries are removed.
    pub async fn validate(&self, cookie_token: &str) -> Option<AuthMode> {
        let mut map = self.inner.lock().await;
        let entry = map.get_mut(cookie_token)?;
        let now = Instant::now();
        if entry.expires_at <= now {
            map.remove(cookie_token);
            return None;
        }
        entry.expires_at = now + self.max_age;
        Some(entry.mode)
    }

    /// Drop a specific session — used by `POST /auth/logout`.
    pub async fn revoke(&self, cookie_token: &str) {
        let mut map = self.inner.lock().await;
        map.remove(cookie_token);
    }

    /// Drop every active session — used by `gtmux rotate-token` and the
    /// future "logout all" Settings action.
    pub async fn revoke_all(&self) {
        let mut map = self.inner.lock().await;
        map.clear();
    }

    /// Drop every active session except the caller's cookie. Returns the
    /// number of sessions revoked. Slice D-3 (`POST /api/settings/password`,
    /// `POST /api/settings/logout-all`) per ADR-0020 D4 / D5: the caller
    /// stays logged in while every other tab / device is logged out.
    pub async fn revoke_others(&self, except: &str) -> usize {
        let mut map = self.inner.lock().await;
        let before = map.len();
        map.retain(|cookie, _| cookie == except);
        before.saturating_sub(map.len())
    }

    /// Test/debug helper — number of live entries.
    #[doc(hidden)]
    pub async fn len(&self) -> usize {
        self.inner.lock().await.len()
    }
}

/// Stage 5 D10 α (ADR-0020 D10 additive): the WS handshake accepts a
/// cookie as an alternative to the subprotocol bearer. `validate` here
/// delegates to [`SessionTable::validate`] which already implements the
/// expiry + rolling-renewal semantics — we only flatten the
/// `Option<AuthMode>` return into the boolean the WS handler needs.
///
/// Side effect: a positive validation **does** bump the session's
/// `expires_at` (rolling renewal — see D3). That's the intended behaviour
/// for an active WS upgrade — the cookie's session counts as alive.
#[async_trait]
impl gtmux_ws_server::CookieValidator for SessionTable {
    async fn validate(&self, cookie_value: &str) -> bool {
        SessionTable::validate(self, cookie_value).await.is_some()
    }
}

fn generate_cookie_token(rng: &SystemRandom) -> Result<String, AuthError> {
    let mut buf = [0u8; COOKIE_TOKEN_BYTES];
    rng.fill(&mut buf)
        .map_err(|e| AuthError::Rand(e.to_string()))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf))
}

// ─────────────────────────────────────────────────────────────────────────────
//  Rate limiter — per-IP sliding window over RATE_WINDOW
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct RateLimiter {
    inner: Mutex<HashMap<String, Vec<Instant>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one failed attempt for `key` and return `true` iff the
    /// caller is now over the configured limit. The window is rolling —
    /// entries older than [`RATE_WINDOW`] are pruned on every call so the
    /// map cannot grow unboundedly across days of probing.
    pub async fn note_failure_and_check(&self, key: &str, limit: u32) -> bool {
        let now = Instant::now();
        let cutoff = now - RATE_WINDOW;
        let mut map = self.inner.lock().await;
        let bucket = map.entry(key.to_string()).or_default();
        bucket.retain(|&t| t >= cutoff);
        bucket.push(now);
        let too_many = bucket.len() as u32 > limit;
        if too_many {
            debug!(
                key,
                attempts = bucket.len(),
                limit,
                "auth: rate-limited login attempts"
            );
        }
        too_many
    }

    /// Wipe the bucket for `key` (called after a *successful* login so a
    /// previously-rate-limited operator isn't penalised after fixing their
    /// password).
    pub async fn reset(&self, key: &str) {
        let mut map = self.inner.lock().await;
        map.remove(key);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Argon2id password hash/verify
// ─────────────────────────────────────────────────────────────────────────────

fn argon2_instance() -> Argon2<'static> {
    let params = Params::new(ARGON_MEMORY_KIB, ARGON_ITERATIONS, ARGON_PARALLELISM, None)
        .expect("static Argon2 params are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Hash `plaintext` and return the PHC-formatted string ready for disk.
/// Salt is 16 bytes from `ring::SystemRandom` (already a workspace dep) so
/// we don't pull in `rand_core` just for the salt.
pub fn hash_password(plaintext: &str) -> Result<String, AuthError> {
    let rng = SystemRandom::new();
    let mut salt_bytes = [0u8; 16];
    rng.fill(&mut salt_bytes)
        .map_err(|e| AuthError::Rand(e.to_string()))?;
    let salt =
        SaltString::encode_b64(&salt_bytes).map_err(|e| AuthError::Argon2(e.to_string()))?;
    let argon = argon2_instance();
    let hash = argon
        .hash_password(plaintext.as_bytes(), &salt)
        .map_err(|e| AuthError::Argon2(e.to_string()))?
        .to_string();
    Ok(hash)
}

/// Constant-time verify of `plaintext` against a stored PHC `hash`.
pub fn verify_password(plaintext: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "auth: stored password hash is malformed");
            return false;
        }
    };
    argon2_instance()
        .verify_password(plaintext.as_bytes(), &parsed)
        .is_ok()
}

/// Persist `hash` to `path` with mode 0600 (ADR-0020 D5). The parent dir is
/// created with mode 0700 if missing — mirrors the layout-store pattern.
pub fn save_password_hash(path: &Path, hash: &str) -> Result<(), AuthError> {
    use atomic_write_file::unix::OpenOptionsExt as AwfOpenOptionsExt;
    use atomic_write_file::OpenOptions as AwfOpenOptions;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt as StdOpenOptionsExt;
    use std::os::unix::fs::PermissionsExt;

    let dir = path.parent().ok_or_else(|| {
        AuthError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "password hash path has no parent",
        ))
    })?;
    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(AuthError::Io)?;
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .map_err(AuthError::Io)?;
    let mut f = AwfOpenOptions::new()
        .mode(0o600)
        .preserve_mode(false)
        .open(path)
        .map_err(|e| AuthError::Io(e.into()))?;
    f.write_all(hash.as_bytes()).map_err(AuthError::Io)?;
    f.commit().map_err(|e| AuthError::Io(e.into()))?;
    Ok(())
}

/// Read the PHC hash from disk. Returns [`AuthError::HashFileMissing`] when
/// the file is absent so callers can distinguish a missing setup from a
/// genuine IO failure.
pub fn load_password_hash(path: &Path) -> Result<String, AuthError> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s.trim_end_matches('\n').to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(AuthError::HashFileMissing(path.to_path_buf()))
        }
        Err(e) => Err(AuthError::Io(e)),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Cookie helpers
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) fn build_session_cookie(value: &str, max_age: Duration, secure: bool) -> String {
    let mut parts = Vec::with_capacity(6);
    parts.push(format!("{COOKIE_NAME}={value}"));
    parts.push("Path=/".into());
    parts.push("HttpOnly".into());
    parts.push("SameSite=Strict".into());
    parts.push(format!("Max-Age={}", max_age.as_secs()));
    if secure {
        parts.push("Secure".into());
    }
    parts.join("; ")
}

pub(crate) fn clear_session_cookie(secure: bool) -> String {
    let mut parts = Vec::with_capacity(6);
    parts.push(format!("{COOKIE_NAME}="));
    parts.push("Path=/".into());
    parts.push("HttpOnly".into());
    parts.push("SameSite=Strict".into());
    parts.push("Max-Age=0".into());
    if secure {
        parts.push("Secure".into());
    }
    parts.join("; ")
}

pub(crate) fn extract_cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in raw.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix(&format!("{name}=")) {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
//  Handlers
// ─────────────────────────────────────────────────────────────────────────────

// `/auth` GET is owned by the FE bundle (ADR-0020 D13) — no server-side
// handler. The legacy `auth_page_handler` (server-rendered token-mode entry
// + landing HTML) was removed when the SPA took over the sign-in surface;
// `/auth/login` is now the single token/password exchange path.

#[derive(Debug, Deserialize)]
pub struct LoginBody {
    /// Token-mode login — equivalent to `GET /auth?token=X` but for
    /// programmatic clients. Optional in password mode.
    #[serde(default)]
    pub token: Option<String>,
    /// Password-mode login. Optional in token mode.
    #[serde(default)]
    pub password: Option<String>,
    /// Optional redirect path returned in the response body so the FE can
    /// follow it after a successful login. The cookie is set regardless.
    #[serde(default)]
    pub redirect: Option<String>,
}

/// `POST /auth/login` — accepts `{ token | password }` per the active
/// `config.auth.mode`. Returns:
///   * 200 + Set-Cookie + `{ redirect }` on success
///   * 401 on bad credentials
///   * 429 + `Retry-After` when the per-IP rate limit is exceeded
///   * 400 when the body doesn't carry the credential the active mode
///     expects (e.g. password mode but `{ token: "x" }`)
pub async fn auth_login_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> Response {
    let mode = AuthMode::from_config_str(&state.config.auth.mode);
    let limiter = state.rate_limiter.clone();
    let limit = state.config.auth.rate_limit_per_5min;
    // Rate-limit key: prefer X-Forwarded-For (when behind a trusted proxy in
    // Cloud mode), fall back to a global "_local" bucket in Local mode where
    // ConnectInfo isn't plumbed through `into_make_service()`.
    let key = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "_local".to_string());

    match mode {
        AuthMode::Token => {
            let Some(token) = body.token.as_deref().filter(|t| !t.is_empty()) else {
                return login_error("token mode: missing `token` in request body", StatusCode::BAD_REQUEST);
            };
            if !verify_token(token, &state.token) {
                state
                    .auth_failure_counter
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if limiter.note_failure_and_check(&key, limit).await {
                    return rate_limited_response();
                }
                return login_error("invalid token", StatusCode::UNAUTHORIZED);
            }
            limiter.reset(&key).await;
            issue_cookie_response(&state, AuthMode::Token, body.redirect.as_deref()).await
        }
        AuthMode::Password => {
            let Some(password) = body.password.as_deref().filter(|p| !p.is_empty()) else {
                return login_error(
                    "password mode: missing `password` in request body",
                    StatusCode::BAD_REQUEST,
                );
            };
            let hash = {
                let guard = state.password_hash.read().await;
                match guard.as_deref() {
                    Some(h) => h.to_string(),
                    None => {
                        return login_error(
                            "password mode is configured but no password is set; run `gtmux set-password`",
                            StatusCode::SERVICE_UNAVAILABLE,
                        );
                    }
                }
            };
            if !verify_password(password, &hash) {
                state
                    .auth_failure_counter
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if limiter.note_failure_and_check(&key, limit).await {
                    return rate_limited_response();
                }
                return login_error("invalid password", StatusCode::UNAUTHORIZED);
            }
            limiter.reset(&key).await;
            issue_cookie_response(&state, AuthMode::Password, body.redirect.as_deref()).await
        }
    }
}

/// `POST /auth/rotate` — ADR-0020 D14. Token-rotation action surfaced by the
/// FE SettingsOverlay's [Rotate token] button. Authenticated by the caller's
/// `gtmux_auth` cookie:
///   * 200 + new Set-Cookie + `{ ok: true, revoked_count }` — caller's old
///     cookie and every *other* session are dropped; the caller gets a fresh
///     opaque session-id (same mode as before).
///   * 401 — no cookie or invalid/expired cookie. No state change.
///
/// Notes:
///   * The *server* token (`state.token`, used by `?t=` / bearer paths)
///     is **not** rotated here — that's a CLI-side operation (`gtmux
///     rotate-token`). This endpoint rotates the in-memory cookie session
///     only. ADR-0020 D14 §"거절된 대안" R1.
///   * Sits outside `/api/*` so the SPA can fire it during the auth surface
///     transition (e.g. immediately after sign-in), but every request still
///     passes through the Host + Origin checks so CSRF surface is bounded.
pub async fn auth_rotate_handler(State(state): State<AppState>, req: Request<Body>) -> Response {
    let Some(old_cookie) = extract_session_cookie(req.headers()) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "session_cookie_required" })),
        )
            .into_response();
    };
    // Constant-time-equivalent validate against the session table — also
    // returns the active `AuthMode` so the rotated cookie inherits it.
    let Some(mode) = state.session_table.validate(&old_cookie).await else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_session" })),
        )
            .into_response();
    };

    // 1. Revoke every *other* session first, capture the count so the FE
    //    can surface "logged out of N other devices".
    let revoked_others = state.session_table.revoke_others(&old_cookie).await;
    // 2. Revoke the caller's previous cookie so even an attacker who
    //    captured it before rotation loses access.
    state.session_table.revoke(&old_cookie).await;
    // 3. Mint a fresh opaque cookie token (same mode).
    let new_token = match state.session_table.issue(mode).await {
        Ok(t) => t,
        Err(e) => {
            warn!(error = %e, "auth_rotate: failed to mint replacement cookie");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "issue_failed" })),
            )
                .into_response();
        }
    };
    let secure = matches!(state.config.mode(), Mode::Cloud);
    let cookie_header =
        build_session_cookie(&new_token, state.session_table.max_age(), secure);

    let revoked_count = revoked_others + 1;
    let mut resp = (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "revoked_count": revoked_count,
        })),
    )
        .into_response();
    if let Ok(hv) = axum::http::HeaderValue::from_str(&cookie_header) {
        resp.headers_mut().insert(header::SET_COOKIE, hv);
    }
    apply_security_headers(resp.headers_mut(), state.config.mode());
    resp
}

/// `POST /auth/logout` — revoke the cookie in the session table and emit a
/// Max-Age=0 Set-Cookie to clear the browser jar. Idempotent — calling
/// without a cookie is a 200 (no-op).
pub async fn auth_logout_handler(State(state): State<AppState>, req: Request<Body>) -> Response {
    if let Some(value) = extract_cookie_value(req.headers(), COOKIE_NAME) {
        state.session_table.revoke(&value).await;
    }
    let secure = matches!(state.config.mode(), Mode::Cloud);
    let clear = clear_session_cookie(secure);
    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::SET_COOKIE, clear)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "ok": true }).to_string()))
        .expect("static headers");
    apply_security_headers(resp.headers_mut(), state.config.mode());
    resp
}

// ─────────────────────────────────────────────────────────────────────────────
//  Handler helpers
// ─────────────────────────────────────────────────────────────────────────────

async fn issue_cookie_response(
    state: &AppState,
    mode: AuthMode,
    redirect_raw: Option<&str>,
) -> Response {
    let cookie_token = match state.session_table.issue(mode).await {
        Ok(t) => t,
        Err(e) => {
            warn!(error = %e, "auth: failed to mint session cookie");
            return (StatusCode::INTERNAL_SERVER_ERROR, "auth init failed").into_response();
        }
    };
    let target = normalise_redirect_target(redirect_raw);
    let secure = matches!(state.config.mode(), Mode::Cloud);
    let max_age = Duration::from_secs(
        u64::from(state.config.auth.cookie_max_age_days).saturating_mul(24 * 3600),
    );
    let cookie = build_session_cookie(&cookie_token, max_age, secure);
    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::SET_COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "ok": true, "redirect": target }).to_string(),
        ))
        .expect("static headers");
    apply_security_headers(resp.headers_mut(), state.config.mode());
    resp
}

fn rate_limited_response() -> Response {
    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header(header::RETRY_AFTER, HeaderValue::from_static("300"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "error": "rate_limited", "retry_after_secs": 300 }).to_string(),
        ))
        .expect("static headers")
}

fn login_error(msg: &str, status: StatusCode) -> Response {
    (
        status,
        Json(json!({ "error": "auth_failed", "message": msg })),
    )
        .into_response()
}

/// Same hardening as the legacy bootstrap path — only `/<path>` survives.
pub(crate) fn normalise_redirect_target(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return "/".to_string();
    };
    let bytes = raw.as_bytes();
    if bytes.first() != Some(&b'/') {
        return "/".to_string();
    }
    if bytes.get(1) == Some(&b'/') || bytes.get(1) == Some(&b'\\') {
        return "/".to_string();
    }
    if raw.contains('\r') || raw.contains('\n') {
        return "/".to_string();
    }
    raw.to_string()
}

// `lib.rs` owns the real `apply_security_headers`; we re-export through a
// crate-private wrapper so handlers in this module can call it without
// recursive imports. The wrapper is `pub(crate)` to keep the surface tight.
pub(crate) fn apply_security_headers(headers: &mut HeaderMap, mode: Mode) {
    crate::apply_security_headers(headers, mode);
}

// ─────────────────────────────────────────────────────────────────────────────
//  Re-exports for `lib.rs` middleware integration
// ─────────────────────────────────────────────────────────────────────────────

/// Token presented in `Authorization: Bearer <X>` or `Cookie: gtmux_auth=<X>`.
/// `Bearer` is always the stable server token (constant-time compared in
/// the caller). `Cookie` is an opaque session-id that must be looked up via
/// [`SessionTable::validate`]. Centralised here so `lib.rs` can stay focused
/// on the router shape.
pub(crate) fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let v = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let rest = v.strip_prefix("Bearer ")?.trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

pub(crate) fn extract_session_cookie(headers: &HeaderMap) -> Option<String> {
    extract_cookie_value(headers, COOKIE_NAME)
}

/// Convenience: per-AppState cookie-or-bearer check used by the
/// `/api/*` middleware. Returns `Ok(())` on success.
pub(crate) async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<(), ()> {
    if let Some(bearer) = extract_bearer(headers) {
        if verify_token(&bearer, &state.token) {
            return Ok(());
        }
        return Err(());
    }
    if let Some(cookie) = extract_session_cookie(headers) {
        if state.session_table.validate(&cookie).await.is_some() {
            return Ok(());
        }
    }
    Err(())
}

/// Wrapper used by `AppState::new` so the constructor doesn't have to know
/// about ADR-0020 default values.
pub fn default_session_table(cookie_max_age_days: u32) -> Arc<SessionTable> {
    let max_age = Duration::from_secs(u64::from(cookie_max_age_days).saturating_mul(24 * 3600));
    Arc::new(SessionTable::new(max_age))
}

pub fn default_rate_limiter() -> Arc<RateLimiter> {
    Arc::new(RateLimiter::new())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Token-only helper kept for the legacy bootstrap path
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve a default password-hash path under XDG_STATE_HOME. Used by both
/// `gtmux set-password` (write) and the server boot (read).
pub fn default_password_hash_path() -> Result<PathBuf, AuthError> {
    let base = if let Some(s) = std::env::var_os("XDG_STATE_HOME") {
        let p = PathBuf::from(s);
        if p.as_os_str().is_empty() {
            return Err(AuthError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "XDG_STATE_HOME is set but empty",
            )));
        }
        p
    } else {
        let home = std::env::var_os("HOME").ok_or_else(|| {
            AuthError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "$HOME not set; cannot resolve XDG_STATE_HOME default",
            ))
        })?;
        PathBuf::from(home).join(".local").join("state")
    };
    Ok(base.join("gtmux").join("password.argon2"))
}

// Methods/types used internally enough that we want a stable accessor for
// tests in `lib.rs`.
#[doc(hidden)]
pub fn _internal_cookie_name() -> &'static str {
    COOKIE_NAME
}

// Silence unused-import linter when the connect-info extractor isn't reached
// in a given build profile.
const _: fn(TokenString) = |_| {};

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn session_table_issue_then_validate_then_revoke() {
        let table = SessionTable::new(Duration::from_secs(60));
        let cookie = table.issue(AuthMode::Token).await.unwrap();
        assert!(!cookie.is_empty());
        assert_eq!(table.len().await, 1);
        // Each token is 32 bytes → base64url-no-pad is ceil(32 * 4/3) = 43 chars.
        assert_eq!(cookie.len(), 43);
        assert_eq!(table.validate(&cookie).await, Some(AuthMode::Token));
        table.revoke(&cookie).await;
        assert_eq!(table.validate(&cookie).await, None);
        assert_eq!(table.len().await, 0);
    }

    #[tokio::test]
    async fn session_table_expired_entries_are_evicted_on_validate() {
        let table = SessionTable::new(Duration::from_millis(20));
        let cookie = table.issue(AuthMode::Password).await.unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(table.validate(&cookie).await, None);
        assert_eq!(table.len().await, 0, "expired entry must be evicted");
    }

    #[tokio::test]
    async fn session_table_rolling_renewal_extends_lifetime() {
        let table = SessionTable::new(Duration::from_millis(80));
        let cookie = table.issue(AuthMode::Token).await.unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        // Mid-window validate must succeed and bump the expiry…
        assert_eq!(table.validate(&cookie).await, Some(AuthMode::Token));
        tokio::time::sleep(Duration::from_millis(60)).await;
        // …so a second check 60 ms later (originally past the 80 ms window
        // from issuance) is still valid because the validate bumped it.
        assert_eq!(table.validate(&cookie).await, Some(AuthMode::Token));
    }

    #[tokio::test]
    async fn rate_limiter_thresholds_at_limit() {
        let limiter = RateLimiter::new();
        for i in 0..5 {
            assert!(
                !limiter.note_failure_and_check("ip", 5).await,
                "attempt {i} must not yet trip the limit"
            );
        }
        assert!(
            limiter.note_failure_and_check("ip", 5).await,
            "6th attempt must trip the limit"
        );
    }

    #[tokio::test]
    async fn rate_limiter_reset_clears_history() {
        let limiter = RateLimiter::new();
        for _ in 0..6 {
            let _ = limiter.note_failure_and_check("ip", 5).await;
        }
        limiter.reset("ip").await;
        assert!(!limiter.note_failure_and_check("ip", 5).await);
    }

    #[test]
    fn argon2_round_trip() {
        let h = hash_password("hunter2-correct-horse").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("hunter2-correct-horse", &h));
        assert!(!verify_password("wrong", &h));
    }

    #[test]
    fn argon2_load_save_round_trip() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("password.argon2");
        let h = hash_password("a-good-password").unwrap();
        save_password_hash(&path, &h).unwrap();
        // 0600 enforced on disk.
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let loaded = load_password_hash(&path).unwrap();
        assert_eq!(loaded, h);
        assert!(verify_password("a-good-password", &loaded));
    }

    #[test]
    fn load_password_hash_missing_returns_typed_error() {
        let err = load_password_hash(Path::new("/nonexistent/path/x")).unwrap_err();
        assert!(matches!(err, AuthError::HashFileMissing(_)));
    }

    #[test]
    fn cookie_build_local_no_secure() {
        let c = build_session_cookie("abc", Duration::from_secs(60), false);
        assert!(c.contains("gtmux_auth=abc"));
        assert!(c.contains("HttpOnly"));
        assert!(c.contains("SameSite=Strict"));
        assert!(c.contains("Max-Age=60"));
        assert!(!c.contains("Secure"));
    }

    #[test]
    fn cookie_build_cloud_has_secure() {
        let c = build_session_cookie("abc", Duration::from_secs(60), true);
        assert!(c.contains("Secure"));
    }

    #[test]
    fn cookie_clear_has_max_age_zero() {
        let c = clear_session_cookie(false);
        assert!(c.contains("Max-Age=0"));
        assert!(c.starts_with("gtmux_auth="));
    }

    #[test]
    fn redirect_normalisation_blocks_open_redirect() {
        assert_eq!(normalise_redirect_target(None), "/");
        assert_eq!(normalise_redirect_target(Some("//evil")), "/");
        assert_eq!(normalise_redirect_target(Some("/\\evil")), "/");
        assert_eq!(normalise_redirect_target(Some("https://evil")), "/");
        assert_eq!(normalise_redirect_target(Some("/canvas")), "/canvas");
        assert_eq!(
            normalise_redirect_target(Some("/x\r\nSet-Cookie: ev")),
            "/"
        );
    }
}
