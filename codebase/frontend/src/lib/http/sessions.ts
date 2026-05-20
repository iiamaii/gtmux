// HTTP client — Session CRUD + attach + detach.
//
// 정본:
// - ADR-0019 (Workspace/Session model)
// - ADR-0018 D6 (match-or-spawn)
// - plan-0007 §13 BE-NEW-2 (Session CRUD endpoints)
// - plan-0007 §14.20 (UX: 1s polling for SessionListModal)
//
// 인증: Cookie 기반 (ADR-0020 D2). `credentials: 'include'` 로 cross-fetch 시
// cookie 동봉 — 동 origin 이라 'same-origin' 도 동등하나 일관성 위해 'include'.
//
// 에러 매핑:
// - 401 Unauthorized → 호출 측이 `/auth` 리디렉트 (cookie 만료/없음)
// - 409 Conflict → AttachConflict (single-attach 충돌, ADR-0019 D3)
// - 429 Too Many Requests → rate limit
// - 4xx 기타 → Error throw with status code
// - 5xx → Error throw

import type { CanvasLayout } from '$lib/types/canvas';
import type {
  AttachConfirmResponse,
  AttachRequest,
  AttachResponse,
  CreateSessionRequest,
  CreateSessionResponse,
  DetachResponse,
  SessionInfo,
  SessionListResponse,
} from '$lib/types/sessions';
import { getWebpageId, webpageHeaders } from '$lib/session/webpageId';
import { observeServerId } from '$lib/session/serverId';

const JSON_HEADERS: Record<string, string> = {
  Accept: 'application/json',
  'Content-Type': 'application/json',
};

const JSON_WEBPAGE_HEADERS = (): Record<string, string> => ({
  ...JSON_HEADERS,
  ...webpageHeaders(),
});

/** Thrown when the server responds with an HTTP status that the caller is
 *  expected to redirect on (currently 401). */
export class UnauthorizedError extends Error {
  constructor(message = 'Unauthorized — session cookie missing or expired') {
    super(message);
    this.name = 'UnauthorizedError';
  }
}

async function json<T>(res: Response): Promise<T> {
  try {
    return (await res.json()) as T;
  } catch (e) {
    throw new Error(`response JSON parse failed: ${String(e)}`);
  }
}

/* ────────────────────────────────────────────────────────────────────────── */
/* GET /api/sessions                                                          */
/* ────────────────────────────────────────────────────────────────────────── */

export async function listSessions(): Promise<SessionListResponse> {
  const res = await fetch('/api/sessions', {
    method: 'GET',
    headers: { Accept: 'application/json', ...webpageHeaders() },
    credentials: 'include',
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (!res.ok) throw new Error(`GET /api/sessions returned ${res.status}`);
  // 0074 Phase 1: BE emits `X-Gtmux-Server-Id` (boot uuid). Compare with
  // the tab-scoped observed value — mismatch means the Server restarted
  // while this tab held local state, and the registered handler nukes
  // store + reconnect hint so the user falls back to session selection.
  observeServerId(res.headers.get('x-gtmux-server-id'));
  // BE shape (sessions.rs:296): bare array `[{ name, active }, ...]`.
  // Normalise to `{ sessions: SessionInfo[] }` for stable FE consumption.
  const arr = await json<Array<{ name: string; active?: boolean }>>(res);
  const sessions: SessionInfo[] = arr.map((s) => ({
    name: s.name,
    active: s.active ?? false,
    // BE 가 last_used_at / item_count 미노출 — placeholder. SessionInfo 의
    // 해당 필드는 optional 이라 빈 값 허용.
    last_used_at: '',
  }));
  return { sessions };
}

/* ────────────────────────────────────────────────────────────────────────── */
/* POST /api/sessions                                                         */
/* ────────────────────────────────────────────────────────────────────────── */

/** ADR-0019 의 name validation 정규식 (FE-side preflight). */
export const SESSION_NAME_REGEX = /^[A-Za-z0-9_-]{1,64}$/;

export async function createSession(req: CreateSessionRequest): Promise<CreateSessionResponse> {
  if (!SESSION_NAME_REGEX.test(req.name)) {
    throw new Error(
      `session name "${req.name}" must match ${SESSION_NAME_REGEX.source}`,
    );
  }
  const res = await fetch('/api/sessions', {
    method: 'POST',
    headers: JSON_HEADERS,
    credentials: 'include',
    body: JSON.stringify(req),
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 409) throw new Error(`session "${req.name}" already exists`);
  if (!res.ok) throw new Error(`POST /api/sessions returned ${res.status}`);
  // BE shape (sessions.rs:510-513): flat `{ name }` (201 CREATED).
  const body = await json<{ name: string }>(res);
  return {
    session: {
      name: body.name,
      active: false,
      last_used_at: '',
    },
  };
}

/* ────────────────────────────────────────────────────────────────────────── */
/* DELETE /api/sessions/<name>                                                */
/* ────────────────────────────────────────────────────────────────────────── */

export async function deleteSession(name: string): Promise<void> {
  const res = await fetch(`/api/sessions/${encodeURIComponent(name)}`, {
    method: 'DELETE',
    credentials: 'include',
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 404) return; // idempotent
  if (!res.ok) throw new Error(`DELETE /api/sessions/${name} returned ${res.status}`);
}

/* ────────────────────────────────────────────────────────────────────────── */
/* POST /api/sessions/<name>/attach                                           */
/* ────────────────────────────────────────────────────────────────────────── */

/**
 * Attach to a session.
 *
 * BE 실제 응답 (sessions.rs:330+):
 *   - 200 `{ name, attached, server_id }` — lock 획득 성공. layout 미동봉 →
 *     본 함수가 `GET /api/sessions/<name>/layout` 으로 별도 fetch.
 *   - 409 `{ error, message, holder?: { pid, server_id, lease_until_unix } }`
 *     — 다른 webpage 가 attach 중. SessionListModal 의 해당 row disabled.
 *   - 404 — session 미존재.
 *
 * `confirm_required` (match-or-spawn) 분기는 BE 미구현 — Stage 3+ 후속.
 */
export async function attachSession(
  name: string,
  req: AttachRequest,
): Promise<AttachResponse> {
  const res = await fetch(`/api/sessions/${encodeURIComponent(name)}/attach`, {
    method: 'POST',
    headers: JSON_WEBPAGE_HEADERS(),
    credentials: 'include',
    body: JSON.stringify({ ...req, ws_conn_id: req.ws_conn_id || getWebpageId() }),
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 409) {
    const body = await json<{
      holder?: { pid?: number; server_id?: string };
    }>(res).catch(
      () => ({}) as { holder?: { pid?: number; server_id?: string } },
    );
    return { kind: 'conflict', active_server_pid: body.holder?.pid };
  }
  if (res.status === 404) {
    throw new Error(`session "${name}" not found`);
  }
  if (!res.ok) throw new Error(`POST attach returned ${res.status}`);
  // 200 — BE Stage 4-C: { name, attached, server_id, matched, unmatched }.
  // unmatched.length > 0 → 사용자 confirm 필요 (AttachConfirmModal 진입).
  // 그 외 → layout fetch + ok.
  const body = await json<{
    matched?: string[];
    unmatched?: string[];
    server_id?: string;
  }>(res).catch(
    () => ({}) as { matched?: string[]; unmatched?: string[]; server_id?: string },
  );
  // 0074 Phase 1 — body 의 server_id 도 detection 채널 (list 호출 전에 attach
  // 가 먼저 일어나는 reconnect 흐름 대비). list 의 response header 와 같은
  // observer 를 공유 — 결과적으로 매 boot 의 첫 attach/list 가 mismatch
  // handler 를 한 번 발화.
  observeServerId(body.server_id ?? null);
  const matched = body.matched ?? [];
  const unmatched = body.unmatched ?? [];
  if (unmatched.length > 0) {
    return {
      kind: 'confirm_required',
      summary: {
        spawn_count: unmatched.length,
        unmatched_item_ids: unmatched,
        matched_item_ids: matched,
      },
    };
  }
  const { layout, etag } = await getLayout(name);
  return { kind: 'ok', layout, etag, matched };
}

/**
 * `POST /api/sessions/<name>/attach/confirm` — BE Stage 4-C.
 *
 * 호출 조건: 이미 동일 cookie 로 attach 가 완료된 상태에서 unmatched 의
 * 모든 UUID 에 대해 BE 가 `spawn_terminal_with_uuid` 발급. 결과:
 *   - 200 `{ name, spawned, already_present, failed }`
 *   - 403 — cookie 가 그 session 의 lock 보유자가 아님 (UnauthorizedError 와
 *     다른 의미 — `not_attached` flow 로 toast 만)
 *   - 503 — workspace / hub 미설정
 */
export async function attachConfirm(
  name: string,
): Promise<AttachConfirmResponse> {
  const res = await fetch(
    `/api/sessions/${encodeURIComponent(name)}/attach/confirm`,
    {
      method: 'POST',
      headers: JSON_WEBPAGE_HEADERS(),
      credentials: 'include',
      body: JSON.stringify({}),
    },
  );
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 403) {
    throw new Error(
      `not the lock holder of "${name}" — call attach first`,
    );
  }
  if (res.status === 503) throw new Error('workspace or hub not configured');
  if (!res.ok) throw new Error(`POST attach/confirm returned ${res.status}`);
  return json<AttachConfirmResponse>(res);
}

/**
 * `DELETE /api/sessions/<name>/items/<id>?kill_terminal=bool` — BE Stage 4-D.
 *
 * Panel/Terminal close 분리. `kill_terminal=true` 면 terminal 도 종료
 * (다른 session 의 mirror 도 영향 — caller 의 confirm 책임). 응답 204 +
 * 새 ETag.
 */
export async function deleteItem(
  sessionName: string,
  itemId: string,
  killTerminal = false,
): Promise<void> {
  const qs = killTerminal ? '?kill_terminal=true' : '';
  const res = await fetch(
    `/api/sessions/${encodeURIComponent(sessionName)}/items/${encodeURIComponent(itemId)}${qs}`,
    {
      method: 'DELETE',
      headers: webpageHeaders(),
      credentials: 'include',
    },
  );
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 404) throw new Error(`item "${itemId}" not found`);
  if (!res.ok) throw new Error(`DELETE item returned ${res.status}`);
}

/**
 * `GET /api/sessions/<name>/layout` — canonical Canvas Layout (schema v2)
 * + ETag. ADR-0006 / sessions.rs:544.
 */
export async function getLayout(
  name: string,
): Promise<{ layout: CanvasLayout; etag: string }> {
  const res = await fetch(
    `/api/sessions/${encodeURIComponent(name)}/layout`,
    {
      method: 'GET',
      headers: { Accept: 'application/json' },
      credentials: 'include',
    },
  );
  if (res.status === 401) throw new UnauthorizedError();
  if (!res.ok) throw new Error(`GET layout returned ${res.status}`);
  const layout = await json<CanvasLayout>(res);
  const etag = (res.headers.get('ETag') ?? '').replace(/^"|"$/g, '');
  return { layout, etag };
}

/**
 * `PUT /api/sessions/<name>/layout` — atomic CAS via `If-Match` ETag.
 *
 * BE: `sessions.rs:579 layout_put_handler` — requires `If-Match` (412
 * PreconditionRequired if missing), validates schema, atomic rename write.
 *
 * Returns the new ETag on success. 412 (PreconditionFailed) → caller should
 * GET + retry — 본 helper 는 *1회 자동 rebase* 까지만 (caller closure 가
 * mutator 를 제공).
 */
export async function putLayout(
  name: string,
  layout: CanvasLayout,
  etag: string,
): Promise<{ etag: string }> {
  const res = await fetch(
    `/api/sessions/${encodeURIComponent(name)}/layout`,
    {
      method: 'PUT',
      headers: {
        ...JSON_HEADERS,
        ...webpageHeaders(),
        'If-Match': `"${etag}"`,
      },
      credentials: 'include',
      body: JSON.stringify(layout),
    },
  );
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 412) throw new EtagMismatchError();
  if (res.status === 428) throw new Error('precondition required (If-Match missing)');
  if (!res.ok) throw new Error(`PUT layout returned ${res.status}`);
  const newEtag = (res.headers.get('ETag') ?? '').replace(/^"|"$/g, '');
  return { etag: newEtag };
}

/** Thrown by `putLayout` on 412 — caller should GET + retry. */
export class EtagMismatchError extends Error {
  constructor(message = 'ETag mismatch — layout changed since last fetch') {
    super(message);
    this.name = 'EtagMismatchError';
  }
}

/**
 * Mutate-and-PUT helper — GET layout, apply `mutate`, PUT with `If-Match`.
 * On 412, refetch + retry once. Throws on second 412.
 *
 * Returns the final layout + etag (stored in sessionStore.loadLayout by caller).
 */
export async function mutateLayout(
  name: string,
  mutate: (layout: CanvasLayout) => CanvasLayout,
): Promise<{ layout: CanvasLayout; etag: string }> {
  const attempt = async (): Promise<{ layout: CanvasLayout; etag: string }> => {
    const { layout, etag } = await getLayout(name);
    const next = mutate(layout);
    const { etag: newEtag } = await putLayout(name, next, etag);
    return { layout: next, etag: newEtag };
  };
  try {
    return await attempt();
  } catch (err) {
    if (err instanceof EtagMismatchError) {
      // rebase once
      return await attempt();
    }
    throw err;
  }
}

/* ────────────────────────────────────────────────────────────────────────── */
/* Import / Export — ADR-0029                                                 */
/* ────────────────────────────────────────────────────────────────────────── */

/**
 * ADR-0029 D2 — `gtmux Session Export Envelope v1`. BE 가 export 시 반환,
 * FE 가 import 시 parse / validate.
 */
export interface SessionExportEnvelope {
  kind: 'gtmux.session.export';
  export_version: 1;
  exported_at: string;
  session_name: string;
  layout: CanvasLayout;
  metadata?: { app?: string; app_version?: string | null };
}

export class EnvelopeParseError extends Error {
  constructor(public readonly reason: string) {
    super(`Invalid session export file — ${reason}`);
  }
}

/** Type-guard 형 파싱 — JSON.parse 결과를 검증. 실패 시 EnvelopeParseError. */
export function parseEnvelope(raw: unknown): SessionExportEnvelope {
  if (typeof raw !== 'object' || raw === null) {
    throw new EnvelopeParseError('not an object');
  }
  const r = raw as Record<string, unknown>;
  if (r.kind !== 'gtmux.session.export') {
    throw new EnvelopeParseError('missing or wrong `kind` (expected "gtmux.session.export")');
  }
  if (r.export_version !== 1) {
    throw new EnvelopeParseError(`unsupported export_version (${String(r.export_version)})`);
  }
  if (typeof r.session_name !== 'string' || r.session_name.length === 0) {
    throw new EnvelopeParseError('missing `session_name`');
  }
  if (typeof r.layout !== 'object' || r.layout === null) {
    throw new EnvelopeParseError('missing `layout`');
  }
  const l = r.layout as Record<string, unknown>;
  if (l.schema_version !== 2) {
    throw new EnvelopeParseError(`unsupported layout.schema_version (${String(l.schema_version)})`);
  }
  if (!Array.isArray(l.items) || !Array.isArray(l.groups)) {
    throw new EnvelopeParseError('layout.items/groups must be arrays');
  }
  if (typeof l.viewport !== 'object' || l.viewport === null) {
    throw new EnvelopeParseError('layout.viewport missing');
  }
  return raw as SessionExportEnvelope;
}

export interface ImportSessionResponse {
  name: string;
  created_at: number;
}

export class ImportNameConflictError extends Error {
  public override readonly name: string;
  constructor(name: string) {
    super(`Session "${name}" already exists`);
    this.name = name;
  }
}

export class ImportSchemaError extends Error {
  public readonly field: string;
  public readonly details: string;
  constructor(field: string, details: string) {
    super(`Schema invalid (${field}): ${details}`);
    this.field = field;
    this.details = details;
  }
}

/** `POST /api/sessions/import { name, layout }` — ADR-0029 D3. */
export async function importSession(
  name: string,
  layout: CanvasLayout,
): Promise<ImportSessionResponse> {
  const res = await fetch('/api/sessions/import', {
    method: 'POST',
    credentials: 'include',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, layout }),
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 409) throw new ImportNameConflictError(name);
  if (res.status === 400) {
    const body = (await res.json().catch(() => ({}))) as {
      field?: string;
      details?: string;
    };
    throw new ImportSchemaError(body.field ?? 'unknown', body.details ?? '');
  }
  if (!res.ok) throw new Error(`POST import returned ${res.status}`);
  return json<ImportSessionResponse>(res);
}

/**
 * `GET /api/sessions/{name}/export` — ADR-0029 D4 / BE work package 0052.
 *
 * BE ship 전: 503 또는 404 — caller 가 toast 로 안내.
 * BE ship 후: envelope 본문 + Content-Disposition filename.
 */
export interface ExportSessionResult {
  envelope: SessionExportEnvelope;
  filename: string;
  blob: Blob;
}

export async function exportSession(name: string): Promise<ExportSessionResult> {
  const res = await fetch(`/api/sessions/${encodeURIComponent(name)}/export`, {
    method: 'GET',
    credentials: 'include',
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 404) throw new Error('Session not found');
  if (!res.ok) throw new Error(`GET export returned ${res.status}`);
  const text = await res.text();
  const blob = new Blob([text], { type: 'application/json' });
  const envelope = parseEnvelope(JSON.parse(text));
  // Content-Disposition 의 filename — fallback 은 session_name.
  const disposition = res.headers.get('content-disposition') ?? '';
  const match = disposition.match(/filename="?([^";]+)"?/i);
  const filename = match?.[1] ?? `${name}.gtmux-session.json`;
  return { envelope, filename, blob };
}

/* ────────────────────────────────────────────────────────────────────────── */
/* DELETE /api/sessions/<name>/attach                                         */
/* ────────────────────────────────────────────────────────────────────────── */

export async function detachSession(name: string): Promise<DetachResponse> {
  const res = await fetch(`/api/sessions/${encodeURIComponent(name)}/attach`, {
    method: 'DELETE',
    headers: webpageHeaders(),
    credentials: 'include',
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 404) return { kind: 'not_attached' };
  if (!res.ok) throw new Error(`DELETE attach returned ${res.status}`);
  return { kind: 'ok' };
}
