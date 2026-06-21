// HTTP client - server shutdown.
//
// `POST /api/shutdown` is gated by the backend `/api/*` auth middleware.
// Cookie auth is the normal browser path, and the legacy bootstrap token is
// included as a Bearer header when it is still present in sessionStorage.
//
// ADR-0020 D16 — step-up re-auth: shutdown is a sensitive action, so the body
// carries a mode-aware `credential` (password when `auth.password_set`, else
// token) that the backend re-verifies inline *before* scheduling shutdown.
// Failures: 401 `credential_required` / `invalid_credential`, 429 (password
// mode rate limit). These surface as typed step-up errors so the ReauthModal
// can stay open and let the user retry instead of redirecting to /auth.

import { UnauthorizedError } from './sessions';
import { stepUpErrorFor } from './stepup';

const TOKEN_STORAGE_KEY = 'gtmux_token';

export interface ShutdownResponse {
  shutdown: 'scheduled';
  expected_exit_code: number;
}

function readStoredToken(): string | null {
  if (typeof sessionStorage === 'undefined') return null;
  try {
    const token = sessionStorage.getItem(TOKEN_STORAGE_KEY);
    return token && token.length > 0 ? token : null;
  } catch {
    return null;
  }
}

function shutdownHeaders(): Record<string, string> {
  const headers: Record<string, string> = {
    Accept: 'application/json',
    'Content-Type': 'application/json',
  };
  const token = readStoredToken();
  if (token !== null) {
    headers.Authorization = `Bearer ${token}`;
  }
  return headers;
}

async function json<T>(res: Response): Promise<T> {
  try {
    return (await res.json()) as T;
  } catch (e) {
    throw new Error(`response JSON parse failed: ${String(e)}`);
  }
}

/**
 * `POST /api/shutdown` with a step-up credential (ADR-0020 D16).
 *
 * @param credential mode-aware password | token, re-verified inline by the BE.
 * @throws {InvalidCredentialError} wrong credential — retry in the modal.
 * @throws {CredentialRequiredError} empty / missing credential.
 * @throws {RateLimitedError} 429 (password mode rate limit).
 * @throws {UnauthorizedError} genuine session expiry — redirect to /auth.
 */
export async function shutdownServer(
  credential: string,
): Promise<ShutdownResponse> {
  const res = await fetch('/api/shutdown', {
    method: 'POST',
    headers: shutdownHeaders(),
    credentials: 'include',
    body: JSON.stringify({ credential }),
  });

  const stepUp = await stepUpErrorFor(res);
  if (stepUp !== null) throw stepUp;
  if (res.status === 401) throw new UnauthorizedError();
  if (!res.ok) throw new Error(`POST /api/shutdown returned ${res.status}`);
  return json<ShutdownResponse>(res);
}
