// HTTP client — Auth login/logout/rotate. Aligned with BE Stage 2 actual contract.
//
// BE 정본: `codebase/backend/crates/http-api/src/auth.rs`
//   - GET /auth — server-rendered HTML (FE bundle 무관). FE 가 호출하지 않음.
//   - POST /auth/login — body { token? | password?, redirect? }
//       200 + Set-Cookie + { ok: true, redirect }
//       400 { error, message } — mode credential 누락
//       401 { error, message } — invalid
//       429 + Retry-After + { error, retry_after_secs }
//       503 { error, message } — password 모드 hash 미설정
//   - POST /auth/logout — idempotent 200 { ok: true } + Set-Cookie(Max-Age=0)

import { UnauthorizedError } from './sessions';
import type {
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
/* POST /auth/rotate — BE Stage 2 에서 skip (다음 stage).                     */
/* ────────────────────────────────────────────────────────────────────────── */

export async function rotateToken(): Promise<RotateTokenResponse> {
  const res = await fetch('/auth/rotate', {
    method: 'POST',
    credentials: 'include',
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 404) {
    throw new Error('rotate-token endpoint not implemented (BE next stage)');
  }
  if (!res.ok) throw new Error(`POST /auth/rotate returned ${res.status}`);
  return json<RotateTokenResponse>(res);
}
