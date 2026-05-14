// Envelope dispatcher — R8 §F4 메인 스레드 단일 dispatcher.
//
// 책임:
// - WsClient 가 디코드한 envelope 을 frame type 별로 fan-out:
//     * 0x02 PANE_OUT  → registered per-pane handler (xterm.write)
//     * 0x07 NOTIFY_MIRROR → connection / panel state hints (e.g. pane-died zombie)
//     * 0x80 LAYOUT_CHANGED → layoutStore.etag 갱신 → HTTP GET /api/layout (호출 측)
//     * 0x81 M_CHANGED       → ephemeralStore.m
//     * 0x82 I_CHANGED       → ephemeralStore.i
//     * 0x83 VIEWPORT_CHANGED → ephemeralStore.viewport
//     * 0x84 FOCUS_MODE_CHANGED → ephemeralStore.focusMode
//     * 0x01/0x03/0x04/0x05/0x06: client→server 방향이므로 수신은 echo/loopback
//       시나리오에서만 발생 — debug log + drop.
// - WsClient lifecycle 콜백을 connectionStore 로 어댑팅.
// - PANE_OUT handler 의 register/unregister API 유지 (XtermHost.svelte 가 사용).
//
// 정본:
// - `docs/ssot/wire-protocol.md` §2 (32 슬롯)
// - `docs/adr/0002-transport-websocket.md` D3/D4 (web-domain / tmux-domain 분리)
// - `docs/reports/0008-frontend-stack.md` §F4 (dispatcher 골격)
// - `docs/reports/0010-grill-amendments.md` D13 (MT-3 broadcast — 자기 발신도 echo
//   수신), D16 (Streaming State), D21 c4 (zombie badge from `pane-died`)

import { SvelteSet } from 'svelte/reactivity';

import { connectionStore } from '$lib/stores/connection.svelte';
import { ephemeralStore } from '$lib/stores/ephemeral.svelte';
import { layoutStore } from '$lib/stores/layout.svelte';
import {
  FRAME_TYPE,
  decodeFocusMode,
  decodeIChanged,
  decodeLayoutChanged,
  decodeMChanged,
  decodeNotifyMirror,
  decodePaneOut,
  decodeViewport,
  type Envelope,
} from './decode';
import { WsClient, computeWsUrl, type ConnectionState, type WsClientOptions } from './client';

// ── PANE_OUT handler 레지스트리 ────────────────────────────────────────────
//
// XtermHost.svelte 의 $effect 마운트 콜백이 paneId 와 handler 를 등록한다.
// handler 는 `term.write(buf, cb)` 와 호환되는 시그니처: 두 번째 인자는 백프레셔
// watermark 갱신용 ack callback (R8 F4 §"PANE_OUT 처리").
//
// **paneId 형식**: tmux pane id 의 정수 부분만 *문자열로* 저장 (예: `"%37"` → `"37"`).
// 다른 store 들이 string id 를 쓰고 있어 일관성을 맞춤 — wire 의 number 와 변환은
// dispatcher 에서 처리 (`String(number)`).

type PaneOutHandler = (buf: Uint8Array, cb: () => void) => void;

const paneOutHandlers = new Map<string, PaneOutHandler>();

export function registerPaneOut(paneId: string, handler: PaneOutHandler): void {
  paneOutHandlers.set(paneId, handler);
}

export function unregisterPaneOut(paneId: string): void {
  paneOutHandlers.delete(paneId);
}

// ── 외부에서 layoutStore 갱신을 트리거하는 hook ──────────────────────────
//
// `0x80 LAYOUT_CHANGED` 수신 시 dispatcher 는 store 의 etag 만 갱신하고, 실제
// HTTP `GET /api/layout` re-fetch 는 *다른 모듈* (`$lib/http/layout`) 의 책임.
// 그 모듈이 본 hook 을 등록해 fan-out 의 마지막 단계를 처리한다.
//
// 시그니처: `(etag: Uint8Array) => Promise<void> | void`. 인자 etag 는 broadcast
// 페이로드의 raw 16B — handler 가 그 값을 If-Match 로 흘려 412 rebase 를 구현할
// 수 있도록 전달한다 (현재 GET 경로는 응답 ETag 를 권위로 삼아 인자를 사용하지
// 않으나, 시그니처는 미래 안정.)
//
// 미등록 시 (bootstrap 이전): warn + drop.

export type LayoutRefetchHandler = (etag: Uint8Array) => Promise<void> | void;
let layoutRefetchHandler: LayoutRefetchHandler | null = null;

export function setLayoutRefetchHandler(handler: LayoutRefetchHandler | null): void {
  layoutRefetchHandler = handler;
}

// ── Dispatcher factory ─────────────────────────────────────────────────────

export interface DispatcherOptions {
  /** base64url 토큰. */
  readonly token: string;
  /** 기본은 `computeWsUrl()` — 테스트 hook 용도. */
  readonly url?: string;
  /** Optional override for the on-frame handler (테스트 격리용). */
  readonly onMessage?: WsClientOptions['onMessage'];
  /** Optional override for state change (테스트 격리용). */
  readonly onStateChange?: WsClientOptions['onStateChange'];
  /** Optional override for close info (테스트 격리용). */
  readonly onClose?: WsClientOptions['onClose'];
}

/**
 * Create a `WsClient` wired to the store fan-out. Caller is responsible for
 * invoking `.start()` and `.stop()` — typically routes/+page.svelte at mount.
 */
export function createDispatcher(opts: DispatcherOptions): WsClient {
  const url = opts.url ?? computeWsUrl();
  return new WsClient({
    url,
    token: opts.token,
    onMessage: opts.onMessage ?? dispatch,
    onStateChange: opts.onStateChange ?? adaptStateChange,
    onClose: opts.onClose ?? adaptClose,
  });
}

/** Frame fan-out — 단일 메인 스레드 entry. */
export function dispatch(env: Envelope): void {
  switch (env.kind) {
    case FRAME_TYPE.PANE_OUT:
      return handlePaneOut(env.payload);
    case FRAME_TYPE.NOTIFY_MIRROR:
      return handleNotifyMirror(env.payload);
    case FRAME_TYPE.LAYOUT_CHANGED:
      return handleLayoutChanged(env.payload);
    case FRAME_TYPE.M_CHANGED:
      return handleMChanged(env.payload);
    case FRAME_TYPE.I_CHANGED:
      return handleIChanged(env.payload);
    case FRAME_TYPE.VIEWPORT_CHANGED:
      return handleViewportChanged(env.payload);
    case FRAME_TYPE.FOCUS_MODE_CHANGED:
      return handleFocusModeChanged(env.payload);
    case FRAME_TYPE.CTRL:
    case FRAME_TYPE.PANE_IN:
    case FRAME_TYPE.PANE_RESIZE:
    case FRAME_TYPE.PANE_PAUSE:
    case FRAME_TYPE.PANE_RESUME:
      // CTRL response 는 추후 Sprint 4 에서 command-id 매칭으로 별도 라우터를 두지만
      // MVP dispatcher 는 그 frame 들을 silently drop (FE-1 범위 밖).
      console.debug('[ws] tmux-domain frame ignored kind=0x%s', env.kind.toString(16));
      return;
    default: {
      // Exhaustiveness check — FRAME_TYPE 에 새 슬롯이 추가되면 컴파일 에러.
      const _exhaustive: never = env.kind;
      console.debug('[ws] unknown frame', _exhaustive);
    }
  }
}

// ── 개별 frame 처리 ───────────────────────────────────────────────────────

function handlePaneOut(payload: Uint8Array): void {
  const decoded = decodePaneOut(payload);
  if (!decoded) {
    console.warn('[ws] 0x02 PANE_OUT decode failed');
    return;
  }
  const handler = paneOutHandlers.get(String(decoded.paneId));
  if (!handler) {
    // panel 이 아직 마운트되지 않은 시점에 ring buffer replay 가 도착할 수 있음 —
    // MVP 는 drop (ADR-0002 D8 의 replay 는 attach 직후 한꺼번에 도착하므로
    // panel mount 가 그 안에 끝나야 함, R8 F1).
    return;
  }
  handler(decoded.bytes, noop);
}

function handleNotifyMirror(payload: Uint8Array): void {
  const decoded = decodeNotifyMirror(payload);
  if (!decoded) {
    console.warn('[ws] 0x07 NOTIFY_MIRROR decode failed');
    return;
  }
  const kind = typeof decoded.body['kind'] === 'string' ? (decoded.body['kind'] as string) : '';
  // SSoT §2.3 kind 매핑.
  switch (kind) {
    case 'pane-died':
      // D21 c4 — panel header 의 zombie badge 와 직결.
      addZombie(decoded.paneId);
      return;
    case 'slow-pane':
      // ADR-0001 D10 — `%pause` 미러 → panel header "느림" 배지.
      connectionStore.markSlow(decoded.paneId);
      return;
    case 'window-add':
    case 'window-renamed':
    case 'window-close':
    case 'session-changed':
    case 'layout-change':
    case 'subscription-changed':
    case 'pane-mode-changed':
      // FE-4 의 fetchLayout 트리거가 일부 (window-add/close/layout-change) 흡수 —
      // P1+ 에서 mux mirror 모듈로 위임 예정. 현재는 debug log only.
      console.debug('[gtmux] notify kind=%s pane=%d', kind, decoded.paneId);
      return;
    default:
      // 미정의 kind 는 SSoT §2.3 forward-compat 에 따라 조용히 무시.
      return;
  }
}

function handleLayoutChanged(payload: Uint8Array): void {
  const decoded = decodeLayoutChanged(payload);
  if (!decoded) {
    console.warn('[ws] 0x80 LAYOUT_CHANGED decode failed');
    return;
  }
  // ETag hex 직렬화 — store 가 string 으로 보관해 If-Match 헤더에 그대로 쓰기 위함
  // (canvas-layout-schema.md §2 의 ETag 정규화: WS 구간 raw 16B, HTTP 구간 hex).
  layoutStore.setEtag(bytesToHex(decoded.etag));
  // Pull-through-notify: re-fetch 는 `$lib/http/layout` 책임. 미등록이면 MVP scope.
  // handler 가 Promise 를 반환해도 dispatcher 는 await 하지 않는다 — fan-out 은
  // non-blocking, fetch 결과는 store hydrate 로 도달.
  const handler = layoutRefetchHandler;
  if (handler) {
    const result = handler(decoded.etag);
    if (result instanceof Promise) {
      result.catch((e: unknown) => {
        console.warn('[gtmux] layout refetch failed', e);
      });
    }
  }
}

function handleMChanged(payload: Uint8Array): void {
  const decoded = decodeMChanged(payload);
  if (!decoded) {
    console.warn('[ws] 0x81 M_CHANGED decode failed');
    return;
  }
  // EphemeralStore.m 은 `SvelteSet<string>` — paneId 정수를 문자열로 변환.
  ephemeralStore.m = new SvelteSet(decoded.panelIds.map(String));
}

function handleIChanged(payload: Uint8Array): void {
  const decoded = decodeIChanged(payload);
  if (!decoded) {
    console.warn('[ws] 0x82 I_CHANGED decode failed');
    return;
  }
  ephemeralStore.i = decoded.paneId === null ? null : String(decoded.paneId);
}

function handleViewportChanged(payload: Uint8Array): void {
  const decoded = decodeViewport(payload);
  if (!decoded) {
    console.warn('[ws] 0x83 VIEWPORT_CHANGED decode failed');
    return;
  }
  ephemeralStore.viewport = { x: decoded.x, y: decoded.y, zoom: decoded.zoom };
}

function handleFocusModeChanged(payload: Uint8Array): void {
  const decoded = decodeFocusMode(payload);
  if (!decoded) {
    console.warn('[ws] 0x84 FOCUS_MODE_CHANGED decode failed');
    return;
  }
  ephemeralStore.focusMode = {
    enabled: decoded.enabled,
    targetPanelId: decoded.targetPanelId === null ? null : String(decoded.targetPanelId),
  };
}

// ── ConnectionStore 어댑터 ─────────────────────────────────────────────────

function adaptStateChange(state: ConnectionState, attempt: number): void {
  connectionStore.setState(state);
  // setState 가 open 진입 시 attempt 를 0 으로 리셋하므로, 그 이후 라이프사이클에서만
  // attempt 를 따로 반영해야 한다 — open 이 아닌 경우만 직접 set.
  if (state !== 'open') {
    connectionStore.attempt = attempt;
  }
}

function adaptClose(code: number, reason: string): void {
  // FE-5 — banner derived 가 closeCode/closeReason 을 보고 1008/1011/4001 분기를
  // 그린다. 1000 (normal stop) 은 banner 가 자체적으로 noise 로 무시.
  connectionStore.setCloseInfo(code, reason);
}

// ── helpers ────────────────────────────────────────────────────────────────

function addZombie(paneId: number): void {
  const current = connectionStore.zombiePaneIds;
  if (current.includes(paneId)) return;
  connectionStore.markZombie([...current, paneId]);
}

const HEX_CHARS = '0123456789abcdef';

function bytesToHex(bytes: Uint8Array): string {
  let out = '';
  for (let i = 0; i < bytes.length; i += 1) {
    const b = bytes[i] ?? 0;
    out += HEX_CHARS[(b >>> 4) & 0x0f];
    out += HEX_CHARS[b & 0x0f];
  }
  return out;
}

function noop(): void {
  /* PANE_OUT ack — Sprint 4 에서 backpressure watermark 와 연결. */
}
