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
//! SameSite=Strict; Max-Age=<seconds>; [Secure]`. `Secure` is attached when
//! the effective config requires TLS; explicit non-TLS cloud mode omits it
//! because browsers drop Secure cookies on `http://`.

#![allow(missing_docs)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use argon2::password_hash::SaltString;
use argon2::{Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version};
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use gtmux_auth::{verify_token, TokenString};
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
    /// Parse from `config.auth.mode`.
    ///
    /// **Deprecated for login (ADR-0020 D18.1).** Login is no longer
    /// mode-exclusive — the token is always valid and the password is
    /// additionally valid whenever a hash is loaded (see
    /// [`auth_login_handler`]). This parser is retained only as a back-compat
    /// reader of the now-ignored `config.auth.mode` field; it does **not**
    /// gate any auth path. `AuthMode` itself still tags `SessionTable` entries
    /// with the axis that minted them (diagnostics only).
    ///
    /// Unknown values fall back to `Token` with a warning.
    pub fn from_config_str(s: &str) -> Self {
        match s {
            "password" => Self::Password,
            "token" => Self::Token,
            other => {
                warn!(
                    value = other,
                    "auth: unknown config.auth.mode; defaulting to token"
                );
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
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| AuthError::Argon2(e.to_string()))?;
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

/// Async wrapper for [`verify_password`] — runs the memory-hard Argon2id
/// verify (m=64 MiB, t=3, p=4) on the blocking thread pool so it never stalls
/// a tokio worker (ADR-0020 perf). Returns `false` if the blocking task
/// fails to join (treated as a non-match, fail-closed).
pub async fn verify_password_async(plaintext: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || verify_password(&plaintext, &hash))
        .await
        .unwrap_or(false)
}

/// Async wrapper for [`hash_password`] — offloads the Argon2id KDF to the
/// blocking pool (see [`verify_password_async`]).
pub async fn hash_password_async(plaintext: String) -> Result<String, AuthError> {
    match tokio::task::spawn_blocking(move || hash_password(&plaintext)).await {
        Ok(result) => result,
        Err(join_err) => Err(AuthError::Argon2(format!(
            "hash task join error: {join_err}"
        ))),
    }
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
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(AuthError::Io)?;
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

/// `POST /auth/login` — unified `{ token } ∪ { password }` login (ADR-0020
/// D18.1). The exclusive `config.auth.mode` branch is **gone**: the token is
/// *always* a valid credential, and the password is *additionally* valid
/// whenever a hash has been loaded (`password_hash.is_some()` — boot path or
/// the runtime `POST /api/settings/password`, D18.2). A presented credential
/// is accepted if it verifies on *either* axis.
///
/// Returns:
///   * 200 + Set-Cookie + `{ redirect }` — `(token present && valid)
///     || (password present && set && valid)`.
///   * 401 — a credential was presented but neither axis verified (+ 429 once
///     the per-IP rate limit trips).
///   * 400 — neither `token` nor `password` present (missing credential).
pub async fn auth_login_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> Response {
    let limiter = state.rate_limiter.clone();
    let limit = state.config.auth.rate_limit_per_5min;
    // Rate-limit key: prefer X-Forwarded-For (when behind a trusted proxy in
    // Cloud mode), fall back to a global "_local" bucket in Local mode where
    // ConnectInfo isn't plumbed through `into_make_service()`.
    let key = rate_limit_key(&headers);

    let presented_token = body.token.as_deref().filter(|t| !t.is_empty());
    let presented_password = body.password.as_deref().filter(|p| !p.is_empty());

    // D18.1: missing *both* credentials is a 400 — there is nothing to verify.
    if presented_token.is_none() && presented_password.is_none() {
        return login_error(
            "missing credential: provide `token` or `password`",
            StatusCode::BAD_REQUEST,
        );
    }

    // Token axis — always active (ADR-0020 D18.1: token always valid). Clone
    // the current server token out under the read lock before the
    // constant-time compare (D18.3 — never hold the lock across `verify_token`).
    let token_ok = if let Some(token) = presented_token {
        let current = state.token.read().await.clone();
        verify_token(token, &current)
    } else {
        false
    };

    // Password axis — active only when a hash is loaded (D18.1/D18.2). Clone
    // the stored hash out (releasing the lock) before the memory-hard Argon2
    // verify so a tokio worker never stalls on the KDF (ADR-0020 perf).
    let password_ok = if let Some(password) = presented_password {
        let stored_hash = {
            let guard = state.password_hash.read().await;
            guard.as_deref().map(|h| h.to_string())
        };
        match stored_hash {
            Some(hash) => verify_password_async(password.to_string(), hash).await,
            // Password presented but none is set → this axis simply doesn't
            // verify (token-only server). No 503 here — D18 drops the
            // password-mode-but-unset error in favour of a plain 401/400.
            None => false,
        }
    } else {
        false
    };

    if token_ok || password_ok {
        limiter.reset(&key).await;
        // The cookie does not distinguish *how* the user logged in (D18.4 —
        // it's a single session); carry the matched axis only as the
        // SessionTable `mode` tag for diagnostics.
        let mode = if token_ok {
            AuthMode::Token
        } else {
            AuthMode::Password
        };
        return issue_cookie_response(&state, mode, body.redirect.as_deref()).await;
    }

    // A credential was presented but neither axis verified.
    state
        .auth_failure_counter
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if limiter.note_failure_and_check(&key, limit).await {
        return rate_limited_response();
    }
    login_error("invalid credentials", StatusCode::UNAUTHORIZED)
}

/// `GET /auth/methods` — **unauthenticated** public probe (ADR-0020 D18.6).
///
/// The FE auth page renders *before* any cookie exists, so it cannot call the
/// authed `GET /api/settings` to learn whether a password is set. This
/// endpoint exposes the minimal shape it needs to decide whether to enable the
/// password field:
///
/// ```json
/// { "token": true, "password": <password_hash.is_some()> }
/// ```
///
/// `token` is always `true` (the server always has a token). `password`
/// reflects only the *presence* of a password hash — never its value or
/// format. No rate limit (single-user local; cloud sits behind TLS — the
/// information surface is a single boolean).
pub async fn auth_methods_handler(State(state): State<AppState>) -> Response {
    let password = state.password_hash.read().await.is_some();
    let mut resp = Json(json!({ "token": true, "password": password })).into_response();
    apply_security_headers(resp.headers_mut(), &state.config);
    resp
}

// ─────────────────────────────────────────────────────────────────────────────
//  Step-up re-authentication (ADR-0020 D16) — mode-aware credential verify
// ─────────────────────────────────────────────────────────────────────────────

/// Body shape for a step-up-gated action (`POST /api/shutdown`,
/// `POST /auth/rotate`). A single `credential` field holds either the
/// password (password mode) or the server token (token mode) — the active
/// mode is decided server-side by whether `state.password_hash` holds a
/// hash (ADR-0020 D16.2). The FE picks which to send by reading
/// `GET /api/settings`'s `auth.password_set`.
#[derive(Debug, Deserialize)]
pub(crate) struct StepUpBody {
    #[serde(default)]
    pub credential: Option<String>,
}

/// Typed outcome of [`verify_step_up`] failure. Each gated handler maps this
/// to the wire responses fixed by ADR-0020 D16.4 — identical for shutdown
/// and rotate.
#[derive(Debug)]
pub(crate) enum StepUpRejection {
    /// No `credential` in the body (absent, empty, or empty/missing body).
    /// → `401 { "error": "credential_required" }`.
    CredentialRequired,
    /// `credential` present but didn't match (wrong password / token).
    /// → `401 { "error": "invalid_credential" }`.
    InvalidCredential,
    /// Password-mode failures exceeded the per-IP rate limit (ADR-0020 D5).
    /// → `429` + `Retry-After`.
    RateLimited,
}

impl StepUpRejection {
    /// Render this rejection as its fixed HTTP response. Centralised so
    /// shutdown + rotate emit byte-identical errors (the FE consumes one
    /// contract).
    pub(crate) fn into_response(self) -> Response {
        match self {
            StepUpRejection::CredentialRequired => (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "credential_required" })),
            )
                .into_response(),
            StepUpRejection::InvalidCredential => (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "invalid_credential" })),
            )
                .into_response(),
            StepUpRejection::RateLimited => rate_limited_response(),
        }
    }
}

/// Derive the per-IP rate-limit key from request headers exactly as
/// [`auth_login_handler`] does (ADR-0020 D5): prefer the first
/// `X-Forwarded-For` hop (trusted proxy in Cloud mode), else a single
/// `_local` bucket (Local mode where `ConnectInfo` isn't plumbed). Shared so
/// step-up failures land in the *same* bucket as login failures.
pub(crate) fn rate_limit_key(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "_local".to_string())
}

/// Read a step-up action body into [`StepUpBody`], tolerating an empty or
/// absent body. ADR-0020 D16.4 fixes a missing credential as
/// `401 credential_required`, so this never surfaces a deserialize-level
/// 400/500: an empty body becomes `StepUpBody { credential: None }` (which
/// [`verify_step_up`] then maps to `credential_required`), and only genuinely
/// malformed JSON (non-empty, non-object) returns a `400 invalid_json`.
pub(crate) async fn parse_step_up_body(body: Body) -> Result<StepUpBody, Response> {
    let bytes = match axum::body::to_bytes(body, 64 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "body_read_failed", "message": e.to_string() })),
            )
                .into_response());
        }
    };
    if bytes.is_empty() {
        return Ok(StepUpBody { credential: None });
    }
    match serde_json::from_slice::<StepUpBody>(&bytes) {
        Ok(parsed) => Ok(parsed),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_json", "message": e.to_string() })),
        )
            .into_response()),
    }
}

/// Mode-aware step-up credential verification shared by the shutdown and
/// rotate handlers (ADR-0020 D16). Single source of truth so the two action
/// endpoints can never drift in their re-auth semantics.
///
/// Decision tree (D16.2):
///   * `state.password_hash` is `Some` (password mode) → verify `credential`
///     as the password via Argon2id (`verify_password_async`, blocking pool).
///     Repeated failures run through the per-IP [`RateLimiter`] (D5) keyed by
///     [`rate_limit_key`]; over the limit → [`StepUpRejection::RateLimited`].
///     A successful verify resets that bucket.
///   * `state.password_hash` is `None` (token mode) → constant-time compare
///     `credential` against the server token via [`verify_token`] (decodes +
///     `ring::constant_time`, the same comparator login uses). Token failures
///     are also funnelled through the limiter for uniformity (D16.4) but the
///     constant-time path makes brute-force pointless.
///
/// Returns `Ok(())` on success; the caller then proceeds with the action.
pub(crate) async fn verify_step_up(
    state: &AppState,
    headers: &HeaderMap,
    body: &StepUpBody,
) -> Result<(), StepUpRejection> {
    // Missing / empty credential is *not* a deserialize error — D16.4 fixes
    // it as 401 credential_required.
    let Some(credential) = body.credential.as_deref().filter(|c| !c.is_empty()) else {
        return Err(StepUpRejection::CredentialRequired);
    };

    let limiter = state.rate_limiter.clone();
    let limit = state.config.auth.rate_limit_per_5min;
    let key = rate_limit_key(headers);

    // Clone the stored hash out (and release the read lock) *before* the
    // memory-hard Argon2 verify — never hold the lock across the KDF, never
    // stall a tokio worker (ADR-0020 perf, mirrors password_handler).
    let stored_hash = {
        let guard = state.password_hash.read().await;
        guard.as_deref().map(|h| h.to_string())
    };

    let ok = match stored_hash {
        // Password mode: Argon2id verify on the blocking pool.
        Some(hash) => verify_password_async(credential.to_string(), hash).await,
        // Token mode: constant-time compare against the server token. Clone
        // the current token out under the read lock (ADR-0020 D18.3) — never
        // hold the lock across the compare.
        None => {
            let current = state.token.read().await.clone();
            verify_token(credential, &current)
        }
    };

    if ok {
        // Clear any accumulated failures so a previously-rate-limited
        // operator isn't penalised after a correct credential.
        limiter.reset(&key).await;
        return Ok(());
    }

    state
        .auth_failure_counter
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Run failures through the same limiter login uses (D16.4). Token-mode
    // need not rate-limit but passing through is harmless and uniform.
    if limiter.note_failure_and_check(&key, limit).await {
        return Err(StepUpRejection::RateLimited);
    }
    Err(StepUpRejection::InvalidCredential)
}

/// **Union** step-up credential verification (ADR-0020 D19.2) — accepts the
/// presented `credential` if it verifies as **either** the server token *or*
/// the current password (when one is set). This is the D18.1 *login union*
/// semantics applied to a step-up `{ credential }` body, and differs from
/// [`verify_step_up`] (D16.2), which is **mode-aware** and picks a *single*
/// axis based on `password_set`.
///
/// The union is load-bearing for password *removal*: the whole point of
/// `DELETE /api/settings/password` is to recover when the password is *lost*,
/// so a still-valid token must be accepted even while a password is set —
/// `verify_step_up` would reject the token in that state (it would only try
/// the password axis). Conversely a user who *remembers* the password can
/// also use it. Both axes are tried; either match passes.
///
/// Error mapping mirrors [`verify_step_up`] exactly (one FE contract):
///   * empty / missing credential → [`StepUpRejection::CredentialRequired`].
///   * neither axis verifies → [`StepUpRejection::InvalidCredential`], after
///     funnelling the failure through the per-IP [`RateLimiter`] (D5/D19.2 —
///     password failures are rate-limited; the token axis is constant-time).
///   * over the rate limit → [`StepUpRejection::RateLimited`].
pub(crate) async fn verify_union_credential(
    state: &AppState,
    headers: &HeaderMap,
    body: &StepUpBody,
) -> Result<(), StepUpRejection> {
    let Some(credential) = body.credential.as_deref().filter(|c| !c.is_empty()) else {
        return Err(StepUpRejection::CredentialRequired);
    };

    let limiter = state.rate_limiter.clone();
    let limit = state.config.auth.rate_limit_per_5min;
    let key = rate_limit_key(headers);

    // Token axis — always active (D18.1: token is always a valid credential).
    // Clone the current server token out under the read lock before the
    // constant-time compare (D18.3 — never hold the lock across `verify_token`).
    let token_ok = {
        let current = state.token.read().await.clone();
        verify_token(credential, &current)
    };

    // Password axis — active only when a hash is loaded. Clone the stored hash
    // out (releasing the lock) before the memory-hard Argon2 verify so a tokio
    // worker never stalls on the KDF. Short-circuit: skip the KDF entirely if
    // the token already matched.
    let ok = if token_ok {
        true
    } else {
        let stored_hash = {
            let guard = state.password_hash.read().await;
            guard.as_deref().map(|h| h.to_string())
        };
        match stored_hash {
            Some(hash) => verify_password_async(credential.to_string(), hash).await,
            None => false,
        }
    };

    if ok {
        limiter.reset(&key).await;
        return Ok(());
    }

    state
        .auth_failure_counter
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if limiter.note_failure_and_check(&key, limit).await {
        return Err(StepUpRejection::RateLimited);
    }
    Err(StepUpRejection::InvalidCredential)
}

/// `POST /auth/rotate` — **server token reissue** (ADR-0020 D18.3, supersedes
/// D14's cookie-only rotation). Surfaced by the FE SettingsOverlay's
/// [Rotate token] button. Authenticated by the caller's `gtmux_auth` cookie
/// *and* a step-up credential (D16 gate is retained):
///
///   * 200 + `{ ok: true, new_token, url }` + caller cookie cleared
///     (`Max-Age=0`, **no** new cookie) — the server token is re-minted,
///     persisted, every cookie session is revoked, and every live WS is
///     closed 4001. The new token URL is returned for the operator to copy.
///   * 401 — `session_cookie_required` / `invalid_session` (cookie gate) or
///     `credential_required` / `invalid_credential` (step-up gate).
///   * 429 — step-up password failures over the per-IP rate limit.
///
/// Effect (D18.3): old token URLs, bookmarks, bearer tokens and *all* cookies
/// are immediately invalid → everyone re-authenticates. The password (if set)
/// and its hash are untouched. The offline CLI `gtmux rotate-token` remains
/// the equivalent path for a stopped server; this is its *live* sibling.
///
/// Sits outside `/api/*` so the SPA can fire it during the auth surface
/// transition, but every request still passes the Host + Origin checks so the
/// CSRF surface is bounded.
pub async fn auth_rotate_handler(State(state): State<AppState>, req: Request<Body>) -> Response {
    // ADR-0020 D16/D18 check order: (1) cookie extract+validate, (2) step-up
    // credential verify, (3) reissue. Headers are read before the body is
    // consumed.
    let (parts, body) = req.into_parts();
    let headers = parts.headers;

    let Some(old_cookie) = extract_session_cookie(&headers) else {
        let mut resp = (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "session_cookie_required" })),
        )
            .into_response();
        apply_security_headers(resp.headers_mut(), &state.config);
        return resp;
    };
    // Validate the caller's cookie against the session table. We don't need
    // the returned mode (the cookie is about to be revoked along with all
    // others), only that it is currently a live session.
    if state.session_table.validate(&old_cookie).await.is_none() {
        let mut resp = (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_session" })),
        )
            .into_response();
        apply_security_headers(resp.headers_mut(), &state.config);
        return resp;
    }

    // ADR-0020 D16/D18.5: step-up re-auth (retained). Empty/absent body maps
    // to `credential_required` (401) rather than a deserialize error.
    let body = match parse_step_up_body(body).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    if let Err(rejection) = verify_step_up(&state, &headers, &body).await {
        let mut resp = rejection.into_response();
        apply_security_headers(resp.headers_mut(), &state.config);
        return resp;
    }

    // ── D18.3 reissue sequence ──────────────────────────────────────────
    // 1. Mint a fresh server token (256-bit CSPRNG, ADR-0003 D4).
    let new = match gtmux_auth::issue_token() {
        Ok(t) => t,
        Err(e) => {
            warn!(error = %e, "auth_rotate: failed to mint server token");
            let mut resp = (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "issue_failed" })),
            )
                .into_response();
            apply_security_headers(resp.headers_mut(), &state.config);
            return resp;
        }
    };

    // 2. Persist to the token file (0600) *before* swapping the in-memory
    //    cell — if the disk write fails we abort with the old token still
    //    authoritative (no half-rotated state where the running server
    //    accepts a token the file doesn't hold).
    if let Err(e) = gtmux_auth::save_token(&state.config.server.session, &new) {
        warn!(error = %e, "auth_rotate: failed to persist new server token");
        let mut resp = (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "persist_failed" })),
        )
            .into_response();
        apply_security_headers(resp.headers_mut(), &state.config);
        return resp;
    }

    // 3. Swap the shared in-memory token cell. ws-server holds the *same*
    //    `Arc<RwLock<…>>` (boot wiring, T1) so its handshake path sees the
    //    new token the instant this write lock drops.
    *state.token.write().await = new.clone();

    // 4. Revoke every cookie session (D18.4 — token rotate = 전원 무효화).
    //    The caller's own cookie is included; it is also explicitly cleared
    //    in the response below.
    state.session_table.revoke_all().await;

    // 5. Close every live WS connection with code 4001 (token revoked,
    //    ADR-0003 D12 reused via the hub's token-revoked broadcast). Silent
    //    when no hub is attached (unit-test AppState) or no WS clients exist.
    if let Some(hub) = state.hub.as_ref() {
        hub.publish_token_revoked();
    }

    // 6. Build the open URL the FE displays/copies. `?t=<token>` is the FE
    //    AuthPage magic-link contract; scheme follows TLS, unspecified binds
    //    map to a loopback host the operator can actually reach.
    let url = build_open_url(&state.config, &new);

    // 7. Respond — clear the caller's cookie (Max-Age=0, no replacement) so
    //    the FE drops straight to /auth and re-authenticates with `new`.
    let secure = state.config.tls_required();
    let clear = clear_session_cookie(secure);
    let mut resp = (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "new_token": new.0,
            "url": url,
        })),
    )
        .into_response();
    if let Ok(hv) = axum::http::HeaderValue::from_str(&clear) {
        resp.headers_mut().insert(header::SET_COOKIE, hv);
    }
    apply_security_headers(resp.headers_mut(), &state.config);
    resp
}

/// Assemble the operator-facing sign-in URL returned by `POST /auth/rotate`
/// (ADR-0020 D18.3). `https` when TLS is required, else `http`. An
/// unspecified bind (`0.0.0.0` / `::`) is rendered as `127.0.0.1` so the URL
/// is reachable from the same host. The token rides the FE AuthPage
/// magic-link query (`?t=`).
fn build_open_url(config: &gtmux_config::Config, token: &TokenString) -> String {
    let scheme = if config.tls_required() {
        "https"
    } else {
        "http"
    };
    let bind = config.server.bind.as_str();
    let host = if bind.is_empty()
        || bind == "0.0.0.0"
        || bind == "::"
        || bind == "[::]"
        || bind.starts_with("unix:")
    {
        "127.0.0.1"
    } else {
        bind
    };
    format!("{scheme}://{host}:{}/auth?t={}", config.server.port, token.0)
}

/// `POST /auth/logout` — revoke the cookie in the session table and emit a
/// Max-Age=0 Set-Cookie to clear the browser jar. Idempotent — calling
/// without a cookie is a 200 (no-op).
pub async fn auth_logout_handler(State(state): State<AppState>, req: Request<Body>) -> Response {
    if let Some(value) = extract_cookie_value(req.headers(), COOKIE_NAME) {
        state.session_table.revoke(&value).await;
    }
    let secure = state.config.tls_required();
    let clear = clear_session_cookie(secure);
    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::SET_COOKIE, clear)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "ok": true }).to_string()))
        .expect("static headers");
    apply_security_headers(resp.headers_mut(), &state.config);
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
    let secure = state.config.tls_required();
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
    apply_security_headers(resp.headers_mut(), &state.config);
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
pub(crate) fn apply_security_headers(headers: &mut HeaderMap, config: &gtmux_config::Config) {
    crate::apply_security_headers_for_config(headers, config);
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
        // Clone the current server token out under the read lock (ADR-0020
        // D18.3) before the constant-time compare.
        let current = state.token.read().await.clone();
        if verify_token(&bearer, &current) {
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
        assert_eq!(normalise_redirect_target(Some("/x\r\nSet-Cookie: ev")), "/");
    }
}
