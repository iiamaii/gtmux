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

export interface BehaviorSettings {
  /** ADR-0021 G25.1.b — panel close 시 modal 우회 + terminal SIGTERM. */
  auto_kill_terminal_on_panel_close: boolean;
  /** ADR-0035 D7 — FilePicker 의 dot-prefixed 항목 노출 여부. */
  picker_show_hidden: boolean;
  /** 0077 follow-up — session switch (직전 active 가 *다른* session) 완료
   *  시 `window.location.reload()`. 첫 attach / modal cancel path 는 제외.
   *  Default `true` (BE 기본). state 정합의 *강제 reset* 의도. */
  reload_on_session_switch: boolean;
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
