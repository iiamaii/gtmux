// HTTP client - server shutdown.
//
// `POST /api/shutdown` is gated by the backend `/api/*` auth middleware.
// Cookie auth is the normal browser path, and the legacy bootstrap token is
// included as a Bearer header when it is still present in sessionStorage.

import { UnauthorizedError } from './sessions';

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

export async function shutdownServer(): Promise<ShutdownResponse> {
  const res = await fetch('/api/shutdown', {
    method: 'POST',
    headers: shutdownHeaders(),
    credentials: 'include',
  });

  if (res.status === 401) throw new UnauthorizedError();
  if (!res.ok) throw new Error(`POST /api/shutdown returned ${res.status}`);
  return json<ShutdownResponse>(res);
}
