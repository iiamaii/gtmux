// HTTP client — Auth methods/login/logout/rotate. Aligned with BE actual contract.
//
// BE 정본: `codebase/backend/crates/http-api/src/auth.rs` (ADR-0020 D18)
//   - GET /auth/methods — public (unauth) { token: true, password: <bool> }
//   - POST /auth/login — body { token? | password?, redirect? } (union, D18.1)
//       200 + Set-Cookie + { ok: true, redirect }
//       400 { error, message } — credential 누락
//       401 { error, message } — invalid
//       429 + Retry-After + { error, retry_after_secs }
//   - POST /auth/logout — idempotent 200 { ok: true } + Set-Cookie(Max-Age=0)
//   - POST /auth/rotate — server-token reissue + revoke_all (D18.3)

import { UnauthorizedError } from './sessions';
import { stepUpErrorFor } from './stepup';
import type {
  AuthMethods,
  LoginRequest,
  LoginResponse,
  LogoutResponse,
  RotateTokenResponse,
} from '$lib/types/auth';

const JSON_HEADERS: Record<string, string> = {
  Accept: 'application/json',
  'Content-Type': 'application/json',
};

async function json<T>(res: Response): Promise<T> {
  try {
    return (await res.json()) as T;
  } catch (e) {
    throw new Error(`response JSON parse failed: ${String(e)}`);
  }
}

/* ────────────────────────────────────────────────────────────────────────── */
/* GET /auth/methods — public login-method discovery (ADR-0020 D18.6).         */
/* ────────────────────────────────────────────────────────────────────────── */

/**
 * Discover which credentials the login form should offer (ADR-0020 D18.6).
 * Unauthenticated and public — the auth page runs before any cookie exists.
 *
 * On any failure (network error, unexpected status, malformed body) we fall
 * back to `{ token: true, password: false }` so the form still shows at least
 * the always-valid token field rather than locking the user out.
 */
export async function authMethods(): Promise<AuthMethods> {
  const fallback: AuthMethods = { token: true, password: false };
  try {
    const res = await fetch('/auth/methods', {
      method: 'GET',
      headers: { Accept: 'application/json' },
      credentials: 'include',
    });
    if (!res.ok) return fallback;
    const body = await json<{ token?: unknown; password?: unknown }>(res);
    return {
      token: body.token !== false, // default true unless BE explicitly says false
      password: body.password === true,
    };
  } catch {
    return fallback;
  }
}

/* ────────────────────────────────────────────────────────────────────────── */
/* POST /auth/login                                                            */
/* ────────────────────────────────────────────────────────────────────────── */

export async function login(req: LoginRequest): Promise<LoginResponse> {
  const res = await fetch('/auth/login', {
    method: 'POST',
    headers: JSON_HEADERS,
    credentials: 'include',
    body: JSON.stringify(req),
  });

  if (res.status === 200) {
    const body = await json<{ ok?: boolean; redirect?: string }>(res).catch(
      () => ({}) as { ok?: boolean; redirect?: string },
    );
    return { kind: 'ok', redirect: body.redirect ?? '/' };
  }

  if (res.status === 429) {
    const body = await json<{ retry_after_secs?: number }>(res).catch(
      () => ({}) as { retry_after_secs?: number },
    );
    return {
      kind: 'rate_limited',
      retry_after_secs: body.retry_after_secs ?? 300,
    };
  }

  if (res.status === 401) {
    const body = await json<{ message?: string }>(res).catch(
      () => ({}) as { message?: string },
    );
    return { kind: 'invalid', message: body.message };
  }

  if (res.status === 400) {
    const body = await json<{ message?: string }>(res).catch(
      () => ({}) as { message?: string },
    );
    return { kind: 'bad_request', message: body.message };
  }

  if (res.status === 503) {
    const body = await json<{ message?: string }>(res).catch(
      () => ({}) as { message?: string },
    );
    return { kind: 'unavailable', message: body.message };
  }

  throw new Error(`POST /auth/login returned ${res.status}`);
}

/* ────────────────────────────────────────────────────────────────────────── */
/* POST /auth/logout                                                          */
/* ────────────────────────────────────────────────────────────────────────── */

export async function logout(): Promise<LogoutResponse> {
  const res = await fetch('/auth/logout', {
    method: 'POST',
    credentials: 'include',
  });
  if (res.status === 401 || res.ok) return { kind: 'ok' };
  throw new Error(`POST /auth/logout returned ${res.status}`);
}

/* ────────────────────────────────────────────────────────────────────────── */
/* POST /auth/rotate — server-token reissue + step-up re-auth (D18.3 + D16).   */
/* ────────────────────────────────────────────────────────────────────────── */

/**
 * `POST /auth/rotate` with a step-up credential (ADR-0020 D18.3 + D16).
 *
 * Reissues the SERVER token: the BE mints a fresh token, swaps it live, revokes
 * *all* sessions (`revoke_all`), and closes active WebSockets with close 4001.
 * This signs everyone out — *including the caller* (its cookie is cleared too).
 * The body carries a mode-aware `credential` (password when `auth.password_set`,
 * else token) re-verified inline by the BE before rotating. Response is
 * `{ ok, new_token, url? }` — the fresh credential the user must re-login with.
 *
 * @throws {InvalidCredentialError} wrong credential — retry in the modal.
 * @throws {CredentialRequiredError} empty / missing credential.
 * @throws {RateLimitedError} 429 (password mode rate limit).
 * @throws {UnauthorizedError} genuine session expiry — redirect to /auth.
 */
export async function rotateToken(
  credential: string,
): Promise<RotateTokenResponse> {
  const res = await fetch('/auth/rotate', {
    method: 'POST',
    headers: JSON_HEADERS,
    credentials: 'include',
    body: JSON.stringify({ credential }),
  });
  const stepUp = await stepUpErrorFor(res);
  if (stepUp !== null) throw stepUp;
  if (res.status === 401) throw new UnauthorizedError();
  if (!res.ok) throw new Error(`POST /auth/rotate returned ${res.status}`);
  return json<RotateTokenResponse>(res);
}
