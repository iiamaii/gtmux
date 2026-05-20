// HTTP client — `GET /api/terminals` (BE-NEW-10 / Phase 4-B).
//
// 정본:
// - BE: `codebase/backend/crates/http-api/src/terminals.rs:128` (list_handler)
// - 본 endpoint 는 `bare Json(Vec<TerminalInfo>)` 응답 — FE 측에서
//   `{ terminals }` 로 정규화 (lib/http/sessions.ts 의 listSessions 패턴 정합).

import { UnauthorizedError } from './sessions';
import type { TerminalInfo, TerminalListResponse } from '$lib/types/terminals';

async function json<T>(res: Response): Promise<T> {
  try {
    return (await res.json()) as T;
  } catch (e) {
    throw new Error(`response JSON parse failed: ${String(e)}`);
  }
}

/**
 * Terminal pool list.
 *
 * 응답:
 * - 200 → `[TerminalInfo, ...]` (가능: 빈 배열)
 * - 401 → UnauthorizedError (caller 가 `/auth` redirect)
 * - 503 → workspace 미설정 — `kind:"unavailable"` 반환 (FE 측 graceful)
 */
export async function listTerminals(): Promise<TerminalListResponse> {
  const res = await fetch('/api/terminals', {
    method: 'GET',
    headers: { Accept: 'application/json' },
    credentials: 'include',
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 503) return { terminals: [] };
  if (!res.ok) throw new Error(`GET /api/terminals returned ${res.status}`);
  const arr = await json<TerminalInfo[]>(res);
  return { terminals: arr };
}

/**
 * `POST /api/terminals/<id>/kill` — BE Stage 4-D.
 *
 * SIGTERM 후 map/meta 정리. layout 의 terminal item 은 그대로 (FE 에서
 * 의도적으로 보존 — 사용자가 "dangling" 상태로 보고 panel 자체는 유지).
 * Layout 에서도 제거하려면 `deleteItem(name, id, true)` 사용.
 *
 * 응답: 204 / 404 (pool 에 없음) / 503 (hub 미준비)
 */
export async function killTerminal(id: string): Promise<void> {
  const res = await fetch(
    `/api/terminals/${encodeURIComponent(id)}/kill`,
    {
      method: 'POST',
      credentials: 'include',
    },
  );
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 404) throw new Error(`terminal "${id}" not in pool`);
  if (res.status === 503) throw new Error('hub not configured');
  if (!res.ok) throw new Error(`POST kill returned ${res.status}`);
}

/**
 * `POST /api/terminals/<id>/respawn` — BE Stage 4-D.
 *
 * 기존 (dangling) UUID 로 fresh PaneId spawn. ADR-0021 D10 의 dangling
 * recovery 진입점. Idempotent — 이미 alive 면 그대로 반환.
 *
 * 응답: 200 `{ id }` / 503
 */
export async function respawnTerminal(id: string): Promise<{ id: string }> {
  const res = await fetch(
    `/api/terminals/${encodeURIComponent(id)}/respawn`,
    {
      method: 'POST',
      headers: { Accept: 'application/json' },
      credentials: 'include',
    },
  );
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 503) throw new Error('hub not configured');
  if (!res.ok) throw new Error(`POST respawn returned ${res.status}`);
  return json<{ id: string }>(res);
}

/** `patchTerminalLabel` 의 label 길이 cap. BE `MAX_LABEL_BYTES = 4096` 정합. */
export const TERMINAL_LABEL_MAX_BYTES = 4096;

/**
 * `PATCH /api/terminals/<id>` — BE Stage 4 cleanup C (0034 §1.5).
 *
 * body: `{ label: string }`. 4 KiB cap (BE 의 `MAX_LABEL_BYTES`). 빈 string 허용
 * — BE 가 metadata 의 label field 를 그대로 set.
 *
 * 응답:
 *  - 204 → 성공
 *  - 400 → label 가 cap 초과 또는 형식 위반
 *  - 401 → UnauthorizedError
 *  - 404 → terminal UUID 가 metadata 에 없음 (pool 에서 unregister 됨)
 */
export async function patchTerminalLabel(
  id: string,
  label: string,
): Promise<void> {
  // FE-side cap check — UTF-8 byte length. BE 가 동일 정책으로 4096 byte 거부.
  const bytes = new TextEncoder().encode(label).length;
  if (bytes > TERMINAL_LABEL_MAX_BYTES) {
    throw new Error(
      `label too long: ${bytes} bytes > ${TERMINAL_LABEL_MAX_BYTES}`,
    );
  }
  const res = await fetch(
    `/api/terminals/${encodeURIComponent(id)}`,
    {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ label }),
    },
  );
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 404) throw new Error(`terminal "${id}" not in metadata`);
  if (res.status === 400) throw new Error(`PATCH terminal label rejected (400)`);
  if (!res.ok) throw new Error(`PATCH terminal returned ${res.status}`);
}
