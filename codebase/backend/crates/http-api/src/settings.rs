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

/// Mutable behavior settings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BehaviorSettings {
    /// ADR-0021 G25.1.b: when true, panel close = panel + terminal SIGTERM
    /// with no per-action dialog. Default `false` per `bool::default()`.
    pub auto_kill_terminal_on_panel_close: bool,
    /// ADR-0035 D7: when true, FilePicker shows dot-prefixed entries
    /// (e.g. `.git`, `.env`, `.config`). Default `false` — hidden entries
    /// are skipped (typical UX). Toggleable from Settings UI.
    pub picker_show_hidden: bool,
    /// 0077 follow-up: when true, switching from one active session to a
    /// different session triggers a full `window.location.reload()` after
    /// the new layout has loaded. First attach (`idle → session`) and modal
    /// cancel paths (`cancelAttachConfirm`) are *not* affected. Forcing a
    /// reload re-runs the auth gate + attach + self-heal pipeline, so any
    /// FE-side cache divergence from the BE (e.g. stale `terminalPool`,
    /// stuck WS subscribers) is reset at a well-defined boundary.
    /// Default `true` per the user request.
    #[serde(default = "default_reload_on_session_switch")]
    pub reload_on_session_switch: bool,
    /// ADR-0049: when true, the FE may honor terminal OSC 52 clipboard
    /// *write* sequences (e.g. drag-copy from a mouse-mode TUI like
    /// `claude`). Default `false` — security-defaults §1.6 forbids
    /// auto-enable; the user must explicitly opt in. The BE only stores
    /// and exposes this flag; all clipboard logic, the secure-context
    /// gate, and OSC 52 read-blocking live entirely in the FE.
    #[serde(default)]
    pub osc52_clipboard_write_enabled: bool,
}

const fn default_reload_on_session_switch() -> bool {
    true
}

impl Default for BehaviorSettings {
    fn default() -> Self {
        Self {
            auto_kill_terminal_on_panel_close: false,
            picker_show_hidden: false,
            reload_on_session_switch: default_reload_on_session_switch(),
            // Security default: never auto-enable (ADR-0049 D3-a,
            // security-defaults §1.6). Must stay `false`.
            osc52_clipboard_write_enabled: false,
        }
    }
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
    token_present: bool,
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
            token_present,
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

/// Whether a server token is currently issued (ADR-0020 D18.3 — the token is
/// now a `SharedToken` `RwLock`, so we read-lock to inspect it). Always `true`
/// in practice — the CLI mints one at boot and `POST /auth/rotate` only ever
/// replaces it with another non-empty token.
async fn current_token_present(state: &AppState) -> bool {
    !state.token.read().await.0.is_empty()
}

/// `GET /api/settings` — returns the boot-immutable info + the current
/// behavior snapshot.
pub(crate) async fn get_handler(State(state): State<AppState>) -> Response {
    let behavior = *state.behavior_settings.read().await;
    let password_set = current_password_set(&state).await;
    let token_present = current_token_present(&state).await;
    Json(build_snapshot(&state, behavior, password_set, token_present)).into_response()
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
        let token_present = current_token_present(&state).await;
        return Json(build_snapshot(&state, behavior, password_set, token_present)).into_response();
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
        let token_present = current_token_present(&state).await;
        return Json(build_snapshot(&state, behavior, password_set, token_present)).into_response();
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
            "picker_show_hidden" => {
                let Some(b) = value.as_bool() else {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": "type_mismatch",
                            "field": "behavior.picker_show_hidden",
                            "expected": "bool",
                        })),
                    )
                        .into_response();
                };
                next.picker_show_hidden = b;
            }
            "osc52_clipboard_write_enabled" => {
                let Some(b) = value.as_bool() else {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": "type_mismatch",
                            "field": "behavior.osc52_clipboard_write_enabled",
                            "expected": "bool",
                        })),
                    )
                        .into_response();
                };
                next.osc52_clipboard_write_enabled = b;
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
    let token_present = current_token_present(&state).await;
    Json(build_snapshot(&state, next, password_set, token_present)).into_response()
}

// ─── Slice D-3: Auth Stage 7 (POST /api/settings/password) ────────────

/// Minimum length per ADR-0020 D5. zxcvbn is P2+.
const MIN_PASSWORD_LENGTH: usize = 8;

/// Body for `POST /api/settings/password`. `current_password` is required
/// for a *change* (ADR-0020 D12 self-step-up) but absent for an *initial set*
/// (ADR-0020 D17.1 — there is no existing password to verify). The
/// discriminator is the live `state.password_hash`, not the body shape, so we
/// accept `current_password` as optional and enforce its presence only in the
/// change branch.
#[derive(Debug, serde::Deserialize)]
struct PasswordChangeRequest {
    #[serde(default)]
    current_password: Option<String>,
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

/// `POST /api/settings/password` — ADR-0020 D12 (change) + D17 (initial set).
///
/// Body:
///   * change (password already set): `{ current_password, new_password }`.
///   * initial set (no password yet): `{ new_password }` — `current_password`
///     omitted / ignored (ADR-0020 D17.1). The cookie session is sufficient
///     authority; there is no existing password to step up against (D17.2).
///
/// The branch discriminator is the *live* `state.password_hash`, not the body.
///
/// Outcomes:
/// - 200 + `Set-Cookie: gtmux_auth=<new>` — validated, hashed, persisted,
///   caller re-issued, **every other session revoked** (revoked_count
///   echoed). After an initial set `password_set` flips to true.
/// - 400 `weak_password` (min_length echoed) — new password failed validation.
/// - 401 `current_password_mismatch` — change branch only: `current_password`
///   absent or its Argon2 verify failed.
/// - 500 `save_failed` / `hash_failed` / `issue_failed` — disk / KDF / mint
///   failure.
///
/// Side-effect ordering (atomic w.r.t. callers via the
/// `AppState.password_hash` `RwLock`):
/// 1. If a password is already set, verify `current_password`; otherwise
///    (initial set) skip verification.
/// 2. Validate new password.
/// 3. Compute new Argon2id hash.
/// 4. Atomically persist to disk (`save_password_hash` mode 0600).
/// 5. Swap in-memory hash (`password_set` becomes true on initial set).
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

    // 1. Branch on whether a password is already set (ADR-0020 D17.1).
    //
    //   * Set → *change*: verify `current_password` (D12 self-step-up).
    //   * Not set → *initial set*: no current password to verify; the
    //     authenticated cookie session is sufficient authority (D17.2). The
    //     `503 password_not_set` branch no longer fires here — it's been
    //     superseded by the initial-set path.
    //
    // Clone the stored hash out and drop the read lock *before* the Argon2
    // verify, which runs on the blocking pool (the KDF is memory-hard —
    // never hold the lock across it nor stall a tokio worker).
    let existing_hash = {
        let guard = state.password_hash.read().await;
        guard.as_deref().map(|h| h.to_string())
    };

    if let Some(current_hash) = existing_hash {
        // Change branch — `current_password` is mandatory and must verify.
        let Some(current) = parsed.current_password.as_deref() else {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "current_password_mismatch" })),
            )
                .into_response();
        };
        let current_ok =
            crate::auth::verify_password_async(current.to_string(), current_hash).await;
        if !current_ok {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "current_password_mismatch" })),
            )
                .into_response();
        }
    }
    // else: initial set — `current_password` is ignored entirely.

    // 2. Validate new.
    if !password_validates(&parsed.new_password) {
        return weak_password_response();
    }

    // 3. Hash new.
    let new_hash = match crate::auth::hash_password_async(parsed.new_password.clone()).await {
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
    let secure = state.config.tls_required();
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
        resp.headers_mut()
            .insert(axum::http::header::SET_COOKIE, hv);
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

/// `DELETE /api/settings/password` — **password removal / reset** (ADR-0020
/// D19). Disables password login and reverts the server to token-only.
///
/// Authorization is a **union step-up** (D19.2): the `{ credential }` body is
/// accepted if it verifies as *either* the server token *or* the current
/// password (via [`crate::auth::verify_union_credential`], the D18.1 login
/// union applied to a step-up body). This is what makes the endpoint a
/// recovery path — a user who *lost* the password can still authorize with the
/// token, while one who *remembers* it can use the password. This differs from
/// the mode-aware `verify_step_up` (D16) on purpose.
///
/// On success:
///   1. Unlink the hash file at `password_hash_path` (if set and present;
///      a not-found file is ignored — the in-memory clear is the source of
///      truth and the disk delete is best-effort cleanup).
///   2. `state.password_hash = None` → `password_set` flips to false, and
///      `GET /auth/methods`'s `password` reads false immediately (D19.3).
///   3. Return `200` + the full settings snapshot (mirrors GET/PATCH so the FE
///      refreshes its Auth section without a second round-trip).
///
/// Cookies / sessions are **not** revoked (D19.3): removing a credential does
/// not invalidate the caller's existing authenticated session — the token is
/// still valid.
///
/// Idempotent (D19.1): if no password is currently set this is a 200 no-op —
/// but the credential is still verified *first*, so an unauthenticated or
/// invalid caller gets `401` regardless of whether a password exists (in the
/// no-password state only the token axis can match, which is correct).
///
/// Errors (identical wire shape to the step-up endpoints):
///   * `401 { "error": "credential_required" }` — empty / missing credential.
///   * `401 { "error": "invalid_credential" }` — neither axis verified.
///   * `429` + `Retry-After` — password failures over the per-IP rate limit.
pub(crate) async fn reset_password_handler(
    State(state): State<AppState>,
    req: Request,
) -> Response {
    let (parts, body) = req.into_parts();
    let peer = crate::auth::peer_from_parts(&parts);
    let headers = parts.headers;

    // Parse `{ credential }` tolerantly — an empty/absent body becomes
    // `credential: None`, which the union verify maps to `credential_required`
    // (only genuinely malformed JSON yields a 400 `invalid_json`).
    let parsed = match crate::auth::parse_step_up_body(body).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    // Union step-up authorization is the *first* precondition (D19.2) — runs
    // even when no password is set, so an invalid caller always gets 401.
    if let Err(rejection) =
        crate::auth::verify_union_credential(&state, &headers, peer, &parsed).await
    {
        return rejection.into_response();
    }

    // Authorized. Best-effort disk unlink first, then clear in-memory. A
    // missing file is fine (idempotent no-op when already token-only); any
    // other unlink error is logged but does not fail the request — the
    // in-memory clear below is the authoritative state transition.
    if let Some(path) = state.password_hash_path.as_ref() {
        match std::fs::remove_file(path.as_ref()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(error = %e, "password reset: hash file unlink failed (clearing in-memory anyway)");
            }
        }
    }

    // Clear the in-memory hash → `password_set` is now false (D19.3). Idempotent
    // when it was already None.
    *state.password_hash.write().await = None;

    // 200 + fresh snapshot (cookies untouched — D19.3, no Set-Cookie).
    let behavior = *state.behavior_settings.read().await;
    let token_present = current_token_present(&state).await;
    let password_set = current_password_set(&state).await; // now false
    Json(build_snapshot(&state, behavior, password_set, token_present)).into_response()
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
            server_workspace: None,
            default_session_workspace: None,
            auth: gtmux_config::AuthConfig::default(),
            assets: gtmux_config::AssetsConfig::default(),
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
    async fn get_default_osc52_clipboard_write_is_false() {
        // ADR-0049 D3-a / security-defaults §1.6: the OSC 52 clipboard
        // write consent flag MUST default to `false` and be present in
        // the `behavior` section of the GET snapshot.
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
        assert_eq!(
            body["behavior"]["osc52_clipboard_write_enabled"],
            serde_json::Value::Bool(false),
            "OSC 52 clipboard write must default to false (ADR-0049 D3-a)"
        );
    }

    #[tokio::test]
    async fn patch_osc52_clipboard_write_toggle_roundtrips() {
        // PATCH the flag to true via the existing behavior partial-merge
        // path, then confirm both the PATCH response and a follow-up GET
        // reflect the new value (ADR-0049 D3-a acceptance criteria).
        let (state, token) = test_state();
        let app = router(state.clone());
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PATCH)
                    .uri("/api/settings")
                    .header(header::HOST, TEST_HOST)
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"behavior":{"osc52_clipboard_write_enabled":true}}"#,
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
            body["behavior"]["osc52_clipboard_write_enabled"],
            serde_json::Value::Bool(true)
        );
        // Runtime state reflects the change.
        let live = *state.behavior_settings.read().await;
        assert!(live.osc52_clipboard_write_enabled);

        // A follow-up GET surfaces the persisted-in-memory value.
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
        assert_eq!(
            body["behavior"]["osc52_clipboard_write_enabled"],
            serde_json::Value::Bool(true)
        );
    }

    #[tokio::test]
    async fn patch_osc52_clipboard_write_type_mismatch_rejects_400() {
        // A non-bool value for the flag must be rejected like the other
        // behavior toggles.
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
                        r#"{"behavior":{"osc52_clipboard_write_enabled":"yes"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "type_mismatch");
        assert_eq!(v["field"], "behavior.osc52_clipboard_write_enabled");
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
        let disk_hash =
            std::fs::read_to_string(state.password_hash_path.as_ref().unwrap().as_ref()).unwrap();
        assert!(crate::auth::verify_password("newpw456", disk_hash.trim()));
        assert!(!crate::auth::verify_password("oldpw123", disk_hash.trim()));
        // The in-memory hash matches, so a follow-up login flow would verify.
        let mem = state.password_hash.read().await.clone();
        assert!(crate::auth::verify_password(
            "newpw456",
            mem.as_deref().unwrap()
        ));
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

    /// Test state with **no** password set but a per-test on-disk path so an
    /// *initial set* (ADR-0020 D17) can persist. Returns
    /// `(state, tempdir, cookie)` — the cookie is a token-mode session
    /// (matching the pre-set `password_set == false` world).
    async fn unset_password_test_state() -> (AppState, tempfile::TempDir, String) {
        let (state, _token) = test_state();
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("password.argon2");
        let state = state.with_password_hash_path(path);
        // No password hash set → `password_set == false`.
        let cookie = state
            .session_table
            .issue(crate::auth::AuthMode::Token)
            .await
            .expect("issue cookie");
        (state, tmp, cookie)
    }

    #[tokio::test]
    async fn password_initial_set_without_current() {
        // ADR-0020 D17.1: password not set → `{ new_password }` alone is
        // accepted (no `current_password`), and afterwards `password_set`
        // flips to true.
        let (state, _tmp, cookie) = unset_password_test_state().await;
        assert!(
            state.password_hash.read().await.is_none(),
            "precondition: no password set"
        );
        let app = router(state.clone());
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/settings/password")
                    .header(header::HOST, TEST_HOST)
                    .header(header::COOKIE, format!("gtmux_auth={cookie}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"new_password":"initpw123"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // Success shape mirrors the change path: `{ ok, revoked_count }` +
        // fresh Set-Cookie.
        let set_cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .expect("initial set re-issues the caller cookie")
            .to_str()
            .unwrap()
            .to_string();
        assert!(set_cookie.starts_with("gtmux_auth="));
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["revoked_count"].is_number());
        // `password_set` is now true (in-memory + on disk).
        let mem = state.password_hash.read().await.clone();
        assert!(mem.is_some(), "password must now be set");
        assert!(crate::auth::verify_password(
            "initpw123",
            mem.as_deref().unwrap()
        ));
        let disk_hash =
            std::fs::read_to_string(state.password_hash_path.as_ref().unwrap().as_ref()).unwrap();
        assert!(crate::auth::verify_password("initpw123", disk_hash.trim()));
    }

    #[tokio::test]
    async fn password_initial_set_weak_rejected() {
        // ADR-0020 D17.1: the initial-set path enforces the same D5 policy —
        // a weak `new_password` returns the existing `400 weak_password`.
        let (state, _tmp, cookie) = unset_password_test_state().await;
        let app = router(state.clone());
        for body in [
            r#"{"new_password":"short1"}"#,   // too short
            r#"{"new_password":"nodigits"}"#, // long enough, no digit
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
        // A weak initial set must NOT have persisted anything.
        assert!(
            state.password_hash.read().await.is_none(),
            "weak initial set must leave password unset"
        );
    }

    #[tokio::test]
    async fn password_change_still_requires_current() {
        // ADR-0020 D17 regression guard: once a password IS set, the change
        // path still demands `current_password` (D12). Both an absent and a
        // wrong `current_password` → 401 `current_password_mismatch`.
        let (state, _token, _tmp, cookie) = password_test_state("oldpw123").await;
        let app = router(state);
        for body in [
            r#"{"new_password":"newpw456"}"#, // current omitted
            r#"{"current_password":"wrong","new_password":"newpw456"}"#, // wrong current
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
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "body={body}");
            let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(v["error"], "current_password_mismatch");
        }
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

    // ── ADR-0020 D19 — `DELETE /api/settings/password` union-step-up reset ──

    /// Like [`password_test_state`] but also *persists* the hash to disk so a
    /// reset's file-unlink path is exercised for real (the base helper only
    /// sets the in-memory hash). Returns `(state, token, tempdir, cookie)`.
    async fn reset_password_test_state(
        initial_password: &str,
    ) -> (AppState, TokenString, tempfile::TempDir, String) {
        let (state, token, tmp, cookie) = password_test_state(initial_password).await;
        let hash = state.password_hash.read().await.clone().unwrap();
        crate::auth::save_password_hash(state.password_hash_path.as_ref().unwrap().as_ref(), &hash)
            .expect("persist initial hash to disk");
        assert!(
            state.password_hash_path.as_ref().unwrap().as_ref().exists(),
            "precondition: hash file on disk"
        );
        (state, token, tmp, cookie)
    }

    fn reset_request(cookie: &str, credential: Option<&str>) -> HttpRequest<Body> {
        let body = match credential {
            Some(c) => Body::from(serde_json::to_vec(&json!({ "credential": c })).unwrap()),
            None => Body::empty(),
        };
        HttpRequest::builder()
            .method(Method::DELETE)
            .uri("/api/settings/password")
            .header(header::HOST, TEST_HOST)
            .header(header::COOKIE, format!("gtmux_auth={cookie}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .unwrap()
    }

    async fn login_status(app: &axum::Router, body: Value) -> StatusCode {
        app.clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/auth/login")
                    .header(header::HOST, TEST_HOST)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    async fn auth_methods_password(app: &axum::Router) -> bool {
        let resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/auth/methods")
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        v["password"].as_bool().unwrap()
    }

    #[tokio::test]
    async fn reset_disables_password_login() {
        // D19 §검증: after reset → password login 401, token login 200,
        // `/auth/methods` password=false, hash file gone.
        let (state, token, _tmp, cookie) = reset_password_test_state("secretpw1").await;
        let path = state.password_hash_path.as_ref().unwrap().clone();
        let app = router(state);

        // Precondition: password login currently works.
        assert_eq!(
            login_status(&app, json!({ "password": "secretpw1" })).await,
            StatusCode::OK
        );
        assert!(auth_methods_password(&app).await, "precondition: password set");

        // Reset using the password as the credential.
        let resp = app.clone().oneshot(reset_request(&cookie, Some("secretpw1"))).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let snap: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            snap["auth"]["password_set"],
            serde_json::Value::Bool(false),
            "snapshot must report password_set=false after reset"
        );

        // Post-reset: password login is gone, token login still works.
        assert_eq!(
            login_status(&app, json!({ "password": "secretpw1" })).await,
            StatusCode::UNAUTHORIZED,
            "password login must fail after reset"
        );
        assert_eq!(
            login_status(&app, json!({ "token": token.0 })).await,
            StatusCode::OK,
            "token login must still work after reset"
        );
        assert!(
            !auth_methods_password(&app).await,
            "/auth/methods password must be false after reset"
        );
        assert!(!path.as_ref().exists(), "hash file must be unlinked");
    }

    #[tokio::test]
    async fn reset_accepts_token_credential() {
        // The lost-password recovery path: token credential authorizes removal
        // even while a password is still set (union, not mode-aware).
        let (state, token, _tmp, cookie) = reset_password_test_state("secretpw1").await;
        let app = router(state.clone());
        let resp = app
            .oneshot(reset_request(&cookie, Some(&token.0)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            state.password_hash.read().await.is_none(),
            "password must be removed when authorized by token"
        );
    }

    #[tokio::test]
    async fn reset_accepts_password_credential() {
        let (state, _token, _tmp, cookie) = reset_password_test_state("secretpw1").await;
        let app = router(state.clone());
        let resp = app
            .oneshot(reset_request(&cookie, Some("secretpw1")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            state.password_hash.read().await.is_none(),
            "password must be removed when authorized by the password itself"
        );
    }

    #[tokio::test]
    async fn reset_rejects_wrong_credential() {
        // Neither token nor password → 401, password still set (no removal).
        let (state, _token, _tmp, cookie) = reset_password_test_state("secretpw1").await;
        let path = state.password_hash_path.as_ref().unwrap().clone();
        let app = router(state.clone());
        let resp = app
            .oneshot(reset_request(&cookie, Some("totally-wrong")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "invalid_credential");
        assert!(
            state.password_hash.read().await.is_some(),
            "password must remain set after a rejected reset"
        );
        assert!(path.as_ref().exists(), "hash file must remain after a rejected reset");
    }

    #[tokio::test]
    async fn reset_missing_credential_returns_401() {
        // Empty body → credential_required (the credential check is the first
        // precondition, even though removal would be a no-op once authorized).
        let (state, _token, _tmp, cookie) = reset_password_test_state("secretpw1").await;
        let app = router(state.clone());
        let resp = app.oneshot(reset_request(&cookie, None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "credential_required");
        assert!(
            state.password_hash.read().await.is_some(),
            "password must remain set when no credential is presented"
        );
    }

    #[tokio::test]
    async fn reset_idempotent_when_no_password() {
        // No password set → 200 no-op, but only with a valid credential. In the
        // token-only state the token is the only credential that can match, so
        // the union check still gates an unauthorized caller (covered by the
        // 401 below) while a valid token yields a clean idempotent 200.
        let (state, _tmp, cookie) = unset_password_test_state().await;
        let token = state.token.read().await.clone();
        let app = router(state.clone());

        // Unauthorized caller still rejected even with no password to remove.
        let bad = app
            .clone()
            .oneshot(reset_request(&cookie, Some("nope")))
            .await
            .unwrap();
        assert_eq!(
            bad.status(),
            StatusCode::UNAUTHORIZED,
            "invalid credential must 401 even when there is no password"
        );

        // Valid token → 200 no-op.
        let resp = app
            .oneshot(reset_request(&cookie, Some(&token.0)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "no-op reset must be 200");
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let snap: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            snap["auth"]["password_set"],
            serde_json::Value::Bool(false)
        );
        assert!(
            state.password_hash.read().await.is_none(),
            "still no password after a no-op reset"
        );
    }
}
