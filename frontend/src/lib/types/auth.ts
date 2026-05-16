// Auth API request / response types — aligned with BE Stage 2 actual contract.
//
// 정본:
// - ADR-0020 D1 (token + password — 서버 설정으로 모드 결정)
// - ADR-0020 D2 (Cookie HttpOnly Secure SameSite=Strict, 7d rolling)
// - ADR-0020 D5 (Argon2id 64MiB / iter 3 / parallel 4)
// - ADR-0020 D8 (Rate limit 5 attempts / 5 min per IP — sliding window)
// - codebase/backend/crates/http-api/src/auth.rs:436 LoginBody + 458 handler
//
// ⚠️ 모드는 *서버 config.auth.mode* 가 결정 — 클라이언트가 선택하지 않는다.
//    같은 `POST /auth/login` 으로 token 모드 / password 모드 둘 다 받지만,
//    body 에 *해당 모드의 credential* 만 채워야 한다. 다른 field 가 채워지면
//    BE 는 400 BAD_REQUEST.

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
/* POST /auth/rotate (token only) — BE Stage 2 에서 still ⏳ (skip noted).    */
/* ────────────────────────────────────────────────────────────────────────── */

export interface RotateTokenResponse {
  kind: 'ok';
  /** 새 token 값 (UI 표시용). cookie 는 server 가 자동 rotate. */
  new_token: string;
}
