// HTTP client — `GET /api/settings` + `PATCH /api/settings` (BE-9).
//
// 정본:
// - BE: `codebase/backend/crates/http-api/src/settings.rs`
//   * GET → SettingsSnapshot { build, server, behavior, auth }
//   * PATCH → body `{ "behavior": {...} }` → 반환은 동일 snapshot
//
// 본 라운드 wire 범위는 behavior 섹션만 — build/server/auth 는 type
// 만 노출하고 consumer 추가는 후속.

import { UnauthorizedError } from './sessions';
import { stepUpErrorFor } from './stepup';

export interface BehaviorSettings {
  /** ADR-0021 G25.1.b — panel close 시 modal 우회 + terminal SIGTERM. */
  auto_kill_terminal_on_panel_close: boolean;
  /** ADR-0035 D7 — FilePicker 의 dot-prefixed 항목 노출 여부. */
  picker_show_hidden: boolean;
  /** 0077 follow-up — session switch (직전 active 가 *다른* session) 완료
   *  시 `window.location.reload()`. 첫 attach / modal cancel path 는 제외.
   *  Default `true` (BE 기본). state 정합의 *강제 reset* 의도. */
  reload_on_session_switch: boolean;
  /** ADR-0049 D3-(a) — terminal OSC 52 클립보드 write 동의. Default `false`
   *  (security-defaults §1.6). secure context 와 AND 되어야 실제 write 수행.
   *  BE 미배선 시 snapshot 에서 누락될 수 있어 store default 가 false fallback. */
  osc52_clipboard_write_enabled: boolean;
}

export interface BuildInfo {
  sha: string;
  version: string;
  rust: string;
}

export interface ServerInfo {
  pid: number;
  bind: string;
  port: number;
  log_path: string | null;
}

export interface ArgonParams {
  m_cost_kib: number;
  t_cost: number;
  p_cost: number;
}

export interface AuthInfo {
  token_present: boolean;
  password_set: boolean;
  argon2: ArgonParams;
}

export interface SettingsSnapshot {
  build: BuildInfo;
  server: ServerInfo;
  behavior: BehaviorSettings;
  auth: AuthInfo;
}

async function json<T>(res: Response): Promise<T> {
  try {
    return (await res.json()) as T;
  } catch (e) {
    throw new Error(`response JSON parse failed: ${String(e)}`);
  }
}

/** `GET /api/settings` — full snapshot. */
export async function getSettings(): Promise<SettingsSnapshot> {
  const res = await fetch('/api/settings', {
    method: 'GET',
    headers: { Accept: 'application/json' },
    credentials: 'include',
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (!res.ok) throw new Error(`GET /api/settings returned ${res.status}`);
  return json<SettingsSnapshot>(res);
}

/**
 * `PATCH /api/settings` — 부분 갱신. body 의 top-level 은 `{ "behavior": {...} }`
 * 만 허용 (BE-side `deny_unknown_fields`). 응답은 갱신 후 snapshot.
 */
export async function patchBehavior(
  partial: Partial<BehaviorSettings>,
): Promise<SettingsSnapshot> {
  const res = await fetch('/api/settings', {
    method: 'PATCH',
    headers: {
      Accept: 'application/json',
      'Content-Type': 'application/json',
    },
    credentials: 'include',
    body: JSON.stringify({ behavior: partial }),
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (!res.ok) throw new Error(`PATCH /api/settings returned ${res.status}`);
  return json<SettingsSnapshot>(res);
}

/* ────────────────────────────────────────────────────────────────────────── */
/* POST /api/settings/password — password initial-set / change (ADR-0020 D17) */
/* ────────────────────────────────────────────────────────────────────────── */

/** Distinct error codes the password endpoint can return (D5 / D12 / D17). */
export type PasswordErrorCode =
  | 'weak_password' // 400 — new password fails len ≥ 8 + letter + digit.
  | 'current_password_mismatch'; // 401 — wrong current (change path only).

/** Thrown by `setPassword` / `changePassword` on a recognised 400/401. */
export class PasswordError extends Error {
  readonly code: PasswordErrorCode;
  constructor(code: PasswordErrorCode, message?: string) {
    super(message ?? code);
    this.name = 'PasswordError';
    this.code = code;
  }
}

async function postPassword(body: Record<string, string>): Promise<void> {
  const res = await fetch('/api/settings/password', {
    method: 'POST',
    headers: {
      Accept: 'application/json',
      'Content-Type': 'application/json',
    },
    credentials: 'include',
    body: JSON.stringify(body),
  });

  if (res.ok) return;

  if (res.status === 400 || res.status === 401) {
    const parsed = await res
      .json()
      .catch(() => ({}) as { error?: string; message?: string });
    const code = (parsed as { error?: string }).error;
    const message = (parsed as { message?: string }).message;
    if (code === 'weak_password') throw new PasswordError('weak_password', message);
    if (code === 'current_password_mismatch') {
      throw new PasswordError('current_password_mismatch', message);
    }
  }

  if (res.status === 401) throw new UnauthorizedError();
  throw new Error(`POST /api/settings/password returned ${res.status}`);
}

/**
 * Initial password set (ADR-0020 D17.1, `password_set === false`). Body carries
 * only `{ new_password }` — there is no existing password to verify. The cookie
 * session is sufficient authority (D17.2), so no step-up credential is required.
 *
 * @throws {PasswordError} `weak_password` — caller surfaces inline.
 */
export async function setPassword(newPassword: string): Promise<void> {
  await postPassword({ new_password: newPassword });
}

/**
 * Password change (ADR-0020 D12, `password_set === true`). Verifying the current
 * password is the self-step-up, so this path is *not* additionally gated by the
 * ReauthModal (D16.1).
 *
 * @throws {PasswordError} `current_password_mismatch` (wrong current) or
 *   `weak_password` (new fails policy) — caller surfaces inline.
 */
export async function changePassword(
  currentPassword: string,
  newPassword: string,
): Promise<void> {
  await postPassword({
    current_password: currentPassword,
    new_password: newPassword,
  });
}

/* ────────────────────────────────────────────────────────────────────────── */
/* DELETE /api/settings/password — remove password / token-only reset (D19)   */
/* ────────────────────────────────────────────────────────────────────────── */

/**
 * Remove the account password (ADR-0020 D19) → token-only sign-in. Authorised
 * by a **union step-up** (D19.2): `credential` may be EITHER the current
 * password OR the server token — whichever the user has. Lost-password recovery
 * uses the token; a remembered password also works.
 *
 * On success the BE unlinks the hash file + clears `state.password_hash`, so
 * `password_set` (and `GET /auth/methods`) flips false. The cookie/session is
 * unchanged (the token is still valid), so no redirect happens here. The 200
 * snapshot is returned so the caller can refresh the form mode.
 *
 * Reuses the shared shutdown/rotate step-up error mapping (`stepUpErrorFor`):
 * a 401 `invalid_credential` / `credential_required` or 429 surfaces as a
 * step-up error and keeps the ReauthModal open; a 401 *without* a step-up code
 * is a genuine session expiry → `UnauthorizedError` (redirect).
 *
 * @throws {InvalidCredentialError} wrong credential — retry in the modal.
 * @throws {CredentialRequiredError} empty / missing credential.
 * @throws {RateLimitedError} 429 (password mode rate limit).
 * @throws {UnauthorizedError} genuine session expiry — redirect to /auth.
 */
export async function resetPassword(
  credential: string,
): Promise<SettingsSnapshot> {
  const res = await fetch('/api/settings/password', {
    method: 'DELETE',
    headers: {
      Accept: 'application/json',
      'Content-Type': 'application/json',
    },
    credentials: 'include',
    body: JSON.stringify({ credential }),
  });

  const stepUp = await stepUpErrorFor(res);
  if (stepUp !== null) throw stepUp;
  if (res.status === 401) throw new UnauthorizedError();
  if (!res.ok) throw new Error(`DELETE /api/settings/password returned ${res.status}`);
  return json<SettingsSnapshot>(res);
}
