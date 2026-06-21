// Auth API request / response types — aligned with BE actual contract.
//
// 정본:
// - ADR-0020 D18 (통합 인증 모델 — token ∪ password, live token rotation)
// - ADR-0020 D2 (Cookie HttpOnly Secure SameSite=Strict, 7d rolling)
// - ADR-0020 D5 (Argon2id 64MiB / iter 3 / parallel 4)
// - ADR-0020 D8 (Rate limit 5 attempts / 5 min per IP — sliding window)
// - codebase/backend/crates/http-api/src/auth.rs LoginBody + handler
//
// ⚠️ D18: 유효 로그인 자격증명 = `{ token(항상) } ∪ { password(설정 시) }`.
//    token 은 항상 활성, password 는 hash 존재 시 추가 활성 (동시 운용).
//    배타 `config.auth.mode` 는 폐기. `POST /auth/login` body 는 제시한
//    credential (`token` 또는 `password`) 만 채우고, BE 가 union 으로 검증한다.

/* ────────────────────────────────────────────────────────────────────────── */
/* POST /auth/login                                                            */
/* ────────────────────────────────────────────────────────────────────────── */

/**
 * BE shape: `{ token?, password?, redirect? }`. 활성 모드의 credential 만
 * 채움 — 다른 field 는 미지정 또는 빈 string.
 */
export interface LoginRequest {
  /** Token 모드 credential. Token 모드일 때만 채운다. */
  token?: string;
  /** Password 모드 credential. Password 모드일 때만 채운다. */
  password?: string;
  /** 로그인 성공 후 redirect target — BE 가 sanitize 후 응답 body 에 echo. */
  redirect?: string;
}

export interface LoginOk {
  kind: 'ok';
  /** BE 가 sanitize 한 redirect path (예: `"/"`). */
  redirect: string;
}

export interface LoginInvalid {
  kind: 'invalid';
  /** BE 의 `{ error: "auth_failed", message: "..." }` 의 message. */
  message?: string;
}

export interface LoginRateLimited {
  kind: 'rate_limited';
  /** BE field name (`retry_after_secs`). 다음 시도까지 남은 초. */
  retry_after_secs: number;
}

/** Password 모드 설정되었으나 hash file 없음 — 503. */
export interface LoginUnavailable {
  kind: 'unavailable';
  message?: string;
}

/** BAD_REQUEST — 모드 mismatch (활성 모드의 credential 누락 등). */
export interface LoginBadRequest {
  kind: 'bad_request';
  message?: string;
}

export type LoginResponse =
  | LoginOk
  | LoginInvalid
  | LoginRateLimited
  | LoginUnavailable
  | LoginBadRequest;

/* ────────────────────────────────────────────────────────────────────────── */
/* POST /auth/logout                                                          */
/* ────────────────────────────────────────────────────────────────────────── */

export interface LogoutResponse {
  kind: 'ok';
}

/* ────────────────────────────────────────────────────────────────────────── */
/* GET /auth/methods — public (unauthenticated) login-method discovery (D18.6) */
/* ────────────────────────────────────────────────────────────────────────── */

/**
 * BE shape (ADR-0020 D18.6): `{ token: true, password: <bool> }`. The auth page
 * is rendered *before* authentication, so it cannot read `GET /api/settings`
 * (authed). This public endpoint exposes only whether a password is set —
 * `token` is always `true` (always a valid login credential). Used to decide
 * which auth fields the login form offers.
 */
export interface AuthMethods {
  /** Always `true` — token is always a valid login credential (D18.1). */
  token: boolean;
  /** `true` once a password hash exists (config/CLI or UI). */
  password: boolean;
}

/* ────────────────────────────────────────────────────────────────────────── */
/* POST /auth/rotate — server-token reissue + step-up re-auth (ADR-0020 D18.3) */
/* ────────────────────────────────────────────────────────────────────────── */

/**
 * BE shape (ADR-0020 D18.3): `{ ok: true, new_token, url? }`. Rotation now
 * *reissues the SERVER token* (not just the caller's cookie): the BE mints a
 * fresh server token, swaps it live, revokes *all* sessions (`revoke_all`), and
 * closes active WebSockets with close 4001. The caller's own cookie is cleared
 * too — i.e. rotation signs everyone out, including you. The response carries
 * the `new_token` and an `url` (open link) so the user can re-login with the
 * fresh credential. `revoked_count` (the D14 cookie-rotation shape) is removed.
 */
export interface RotateTokenResponse {
  ok: boolean;
  /** The freshly minted server token (the old token URL/bookmark is now dead). */
  new_token: string;
  /** Ready-to-open login URL embedding the new token, when the BE provides it. */
  url?: string;
}
