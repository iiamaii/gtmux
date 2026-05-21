// Session API response / request types.
//
// 정본:
// - ADR-0019 D1/D2/D5/D6 (Session record, single-attach, cross-server lock)
// - ADR-0018 D6 (match-or-spawn confirm 응답)
// - plan-0007 §13 의 BE-NEW-2 (Session CRUD API contracts)
// - plan-0007 §14.20 (UX rules — modal hint, polling)
//
// BE 의 ADR-0019 D5 영속 schema 는 그대로 — 본 module 은 *wire format* (JSON
// over HTTP) 의 TS shape. ETag / If-Match 등 HTTP 헤더 정합은 별도 (lib/http).

import type { CanvasLayout } from './canvas';

/* ────────────────────────────────────────────────────────────────────────── */
/* GET /api/sessions — list                                                   */
/* ────────────────────────────────────────────────────────────────────────── */

/**
 * 한 session 의 list-level metadata. 실제 layout 은 attach 후 별 GET.
 * `active = true` 이면 *다른 webpage* 가 attach 중 — 본 webpage 는 attach 불가.
 */
export interface SessionInfo {
  /** 사용자 부여 이름. ADR-0019 정규식 `^[A-Za-z0-9_-]{1,64}$`. */
  name: string;
  /**
   * ISO 8601 timestamp — 마지막 attach 종료 시각. fresh session 은 created_at.
   * 현재 BE list endpoint 는 아직 이 값을 내려주지 않으므로 optional.
   */
  last_used_at?: string;
  /** 다른 webpage 가 현재 attach 상태인가. SessionListModal 의 "in use" 섹션 결정. */
  active: boolean;
  /**
   * `active = true` 인 경우 attach 중인 server pid. local-only display
   * (e.g. `in use by server-pid 12345`). cross-server lock 의 detection.
   */
  active_server_pid?: number;
  /** Item count (panel + non-terminal) 의 hint — list row 의 subtitle. optional. */
  item_count?: number;
}

export interface SessionListResponse {
  sessions: SessionInfo[];
}

/* ────────────────────────────────────────────────────────────────────────── */
/* POST /api/sessions — create                                                */
/* ────────────────────────────────────────────────────────────────────────── */

export interface CreateSessionRequest {
  name: string;
}

export interface CreateSessionResponse {
  session: SessionInfo;
}

/* ────────────────────────────────────────────────────────────────────────── */
/* POST /api/sessions/<name>/attach — match-or-spawn                          */
/* ────────────────────────────────────────────────────────────────────────── */

/**
 * Attach 진행 전 사용자 확인이 필요한 경우의 summary (ADR-0018 D6).
 * BE Stage 4-C 의 `attach` 200 응답이 `{matched, unmatched}` 를 동봉하면
 * FE 가 `unmatched.length > 0` 일 때 본 shape 으로 정규화.
 */
export interface AttachConfirmSummary {
  /** 새로 spawn 될 terminal 수 (= unmatched_item_ids.length). */
  spawn_count: number;
  /** layout 의 terminal item 중 server-pool 매칭 없는 UUID들. */
  unmatched_item_ids: string[];
  /** BE 가 layout 의 *matched* 분류로 분류한 UUID들 (display 용). */
  matched_item_ids: string[];
}

/** 사용자 confirm 필요 — AttachConfirmModal 진입. */
export interface AttachConfirmRequired {
  kind: 'confirm_required';
  summary: AttachConfirmSummary;
}

/** Attach 성공 — layout 전체 페이로드. */
export interface AttachOk {
  kind: 'ok';
  layout: CanvasLayout;
  /** Layout file 의 raw etag (hex) — 후속 PUT 의 If-Match 입력. */
  etag: string;
  /** BE 4-C 의 matched UUID list — 정합 검증 / debug. */
  matched?: string[];
}

/** Server 가 이미 다른 webpage 의 attach 를 가지고 있음 (ADR-0019 D3 single-attach). */
export interface AttachConflict {
  kind: 'conflict';
  /** Optional — 다른 attach 의 server pid (UX hint). */
  active_server_pid?: number;
}

export type AttachResponse = AttachConfirmRequired | AttachOk | AttachConflict;

export interface AttachRequest {
  /** Caller WS connection id — BE 가 session ↔ ws_conn 매핑. */
  ws_conn_id: string;
  /**
   * Deprecated (BE Stage 4-C 후) — BE 는 attach 가 *항상* match-only 로 진행.
   * Confirm 분기는 별 endpoint `POST attach/confirm` 사용. 본 field 유지는
   * 호출자 호환성 위한 placeholder.
   */
  confirmed?: boolean;
}

/** `POST /api/sessions/<name>/attach/confirm` 응답. BE Stage 4-C. */
export interface AttachConfirmResponse {
  name: string;
  /** 성공적으로 spawn 된 UUID들. */
  spawned: string[];
  /** 이미 pool 에 있던 UUID들 (no-op). */
  already_present: string[];
  /** 실패 — per-UUID 에러 메시지. */
  failed: { id: string; error: string }[];
}

/* ────────────────────────────────────────────────────────────────────────── */
/* DELETE /api/sessions/<name>/attach                                         */
/* ────────────────────────────────────────────────────────────────────────── */

export type DetachResponse = { kind: 'ok' } | { kind: 'not_attached' };
