// CTRL pending-request registry — 0x01 CTRL 의 request/response 상관.
//
// 정본:
// - `docs/ssot/wire-protocol.md` §2.4 (`{id, ok, result/error, code}` JSON shape)
// - `docs/adr/0001-tmux-integration-control-mode.md` D4 (command-number 매칭)
// - `docs/adr/0008-single-pane-window-and-group.md` (allowlist — CTRL `cmd` 표)
//
// 책임:
// - `sendCtrl(client, cmd, args)` 호출 시 UUID-v4 id 생성, registry 에 resolver 등록,
//   0x01 CTRL envelope 송신.
// - dispatcher 가 CTRL 응답 frame 을 디코드해 `resolveCtrl(id, response)` 호출.
// - 일정 시간 응답 없으면 registry 가 자동 timeout → reject.
//
// **현재 backend 상태 (S5-B 시점)**:
// - 에러 응답은 보낸다 (ws-server::lib.rs encode_ctrl_error). 성공 ack 는 아직
//   안 보냄 (success encoder 미작성). 따라서 ok=true 응답은 backend 정식 wire 후에
//   resolver 에 도달한다. 본 모듈은 *그 시점을 미리 준비*해 둔 인터페이스 골격.
// - 호출 측 (NewPanelButton) 은 본 registry 대신 mux store 의 `addPane` watcher 로
//   현재는 우회 매칭한다 — 본 모듈의 timeout 만료가 fallback 신호 역할.

import type { WsClient } from './client';
import { FRAME_TYPE } from './decode';
import { encodeCtrlRequest } from '$lib/types/envelope';

/** CTRL 응답 JSON 의 정규화된 형태. */
export interface CtrlResponse {
  readonly id: string;
  readonly ok: boolean;
  /** `ok: true` 인 경우의 응답 payload. backend 정의에 따라 임의 shape. */
  readonly result?: Readonly<Record<string, unknown>>;
  /** `ok: false` 인 경우의 에러 메시지. */
  readonly error?: string;
  /** `ok: false` 인 경우의 enum 코드 (예: `ERR_NOT_ALLOWED`). */
  readonly code?: string;
}

interface PendingEntry {
  resolve: (response: CtrlResponse) => void;
  reject: (reason: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

const pending = new Map<string, PendingEntry>();

/** Sprint 5-B 디폴트 timeout — backend 가 success ack 를 보내기 전까지는 timeout 이 정상 동작. */
const DEFAULT_TIMEOUT_MS = 5_000;

export interface SendCtrlOptions {
  /** 응답 대기 timeout (ms). 미지정 = `DEFAULT_TIMEOUT_MS`. */
  readonly timeoutMs?: number;
}

/**
 * 0x01 CTRL request 송신 — UUID-v4 id 생성, registry 에 promise 등록 후
 * envelope 발사. 응답은 dispatcher 가 `resolveCtrl` 로 깨운다.
 *
 * @throws never (네트워크 send 실패는 timeout 으로 표면화).
 */
export function sendCtrl(
  client: WsClient,
  cmd: string,
  args: readonly string[],
  opts: SendCtrlOptions = {},
): { id: string; response: Promise<CtrlResponse> } {
  const id = generateUuidV4();
  const payload = encodeCtrlRequest(id, cmd, args);
  const timeoutMs = opts.timeoutMs ?? DEFAULT_TIMEOUT_MS;

  const response = new Promise<CtrlResponse>((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`CTRL request timed out after ${timeoutMs}ms (cmd=${cmd}, id=${id})`));
    }, timeoutMs);
    pending.set(id, { resolve, reject, timer });
  });

  client.sendFrame(FRAME_TYPE.CTRL, payload);
  return { id, response };
}

/**
 * dispatcher 가 CTRL response envelope 수신 시 호출. id 미일치는 silently drop
 * (다른 클라이언트 인스턴스의 응답이거나 이미 timeout 된 응답일 수 있음).
 */
export function resolveCtrl(response: CtrlResponse): void {
  const entry = pending.get(response.id);
  if (!entry) return;
  pending.delete(response.id);
  clearTimeout(entry.timer);
  entry.resolve(response);
}

/** 테스트 / cleanup 용 — 모든 pending 을 reject 한다. */
export function rejectAllPending(reason: Error): void {
  for (const [id, entry] of pending) {
    clearTimeout(entry.timer);
    entry.reject(reason);
    pending.delete(id);
  }
}

// ── UUID v4 ────────────────────────────────────────────────────────────────

/**
 * SSoT §2.4 의 id 는 UUID v4. 브라우저 native `crypto.randomUUID()` 우선,
 * 부재 시 (구형 브라우저) `crypto.getRandomValues` 폴백.
 */
function generateUuidV4(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  // Fallback: RFC 4122 §4.4 — 16 random bytes, set version / variant bits.
  const bytes = new Uint8Array(16);
  if (typeof crypto !== 'undefined' && typeof crypto.getRandomValues === 'function') {
    crypto.getRandomValues(bytes);
  } else {
    // crypto 자체가 없는 환경 — Node.js test 등. Math.random 폴백 (보안 의미는 없지만 id 충돌 회피만 충족).
    for (let i = 0; i < 16; i += 1) bytes[i] = Math.floor(Math.random() * 256);
  }
  // version 4 (random) + variant 10.
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x40;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  const hex: string[] = [];
  for (let i = 0; i < 16; i += 1) {
    hex.push((bytes[i] ?? 0).toString(16).padStart(2, '0'));
  }
  return `${hex.slice(0, 4).join('')}-${hex.slice(4, 6).join('')}-${hex.slice(6, 8).join('')}-${hex.slice(8, 10).join('')}-${hex.slice(10, 16).join('')}`;
}
