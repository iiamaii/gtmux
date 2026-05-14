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
// layoutStore 의 etag 갱신은 `lib/http/layout::fetchLayoutAndHydrate` 가 단일
// 책임을 진다 — broadcast 도착 시점에 setEtag 하면 후속 GET 의 If-None-Match
// 와 일치해 304 가 떨어지므로, 본 모듈은 etag 를 만지지 않는다.
import { muxStore } from '$lib/stores/mux.svelte';
import {
  FRAME_TYPE,
  decodeCtrl,
  decodeFocusMode,
  decodeIChanged,
  decodeLayoutChanged,
  decodeMChanged,
  decodeNotifyMirror,
  decodePaneOut,
  decodeViewport,
  type Envelope,
} from './decode';
import { resolveCtrl, type CtrlResponse } from './ctrl-registry';
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

/**
 * Buffer of PANE_OUT bytes that arrived BEFORE a handler was registered.
 *
 * The New-Panel flow has a mount-vs-emit race: tmux emits the new pane's
 * first shell prompt (a single PANE_OUT frame) ~immediately after
 * `new-window`, while the SPA only mounts the corresponding XtermHost
 * after `PUT /api/layout` → `LAYOUT_CHANGED` → re-hydrate. Without a
 * buffer, that prompt is dropped and the panel is blank until the user
 * types something.
 *
 * Capped per-pane to avoid memory blow-up if a panel never mounts —
 * 256 KiB matches the backend's PUT body cap roughly and is well over
 * a typical shell-prompt redraw (a few hundred bytes including SGR).
 */
const PANE_LATE_BUFFER_CAP = 256 * 1024;
const paneOutLateBuffers = new Map<string, Uint8Array[]>();

function appendLateBuffer(paneKey: string, bytes: Uint8Array): void {
  const queued = paneOutLateBuffers.get(paneKey) ?? [];
  const totalSoFar = queued.reduce((acc, b) => acc + b.length, 0);
  if (totalSoFar >= PANE_LATE_BUFFER_CAP) {
    // Drop oldest until the new chunk fits. (FIFO — keep the *most recent*
    // bytes, which are usually the most relevant for visual catch-up.)
    while (queued.length > 0 && queued.reduce((a, b) => a + b.length, 0) + bytes.length > PANE_LATE_BUFFER_CAP) {
      queued.shift();
    }
  }
  queued.push(bytes);
  paneOutLateBuffers.set(paneKey, queued);
}

export function registerPaneOut(paneId: string, handler: PaneOutHandler): void {
  paneOutHandlers.set(paneId, handler);
  // Flush any pre-mount bytes we stashed for this pane.
  const queued = paneOutLateBuffers.get(paneId);
  if (queued && queued.length > 0) {
    for (const bytes of queued) handler(bytes, noop);
    paneOutLateBuffers.delete(paneId);
  }
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
      return handleCtrlResponse(env.payload);
    case FRAME_TYPE.PANE_IN:
    case FRAME_TYPE.PANE_RESIZE:
    case FRAME_TYPE.PANE_PAUSE:
    case FRAME_TYPE.PANE_RESUME:
      // 본 4종은 client→server 방향 — 수신 시 loopback / echo 시나리오만 가능.
      console.debug('[ws] client-origin frame echoed back kind=0x%s', env.kind.toString(16));
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
  // First-sight pane discovery — backend 는 별도 `pane-add` event 를 emit 하지 않으므로
  // PANE_OUT 의 paneId 를 mux store 의 진실로 차용. addPane 은 idempotent.
  // 본 보조 매칭이 New Panel UX 흐름의 pane_id 캡처에 쓰인다 (S5-FE-NEW-PANEL 의
  // pending action 우회 경로 — backend success ack 정식 wire 전까지).
  muxStore.addPane(decoded.paneId);
  const key = String(decoded.paneId);
  const handler = paneOutHandlers.get(key);
  if (!handler) {
    // Panel not yet mounted — stash the bytes so the eventual
    // registerPaneOut call can flush them. The New-Panel flow always
    // hits this branch for the very first PANE_OUT of the new pane.
    appendLateBuffer(key, decoded.bytes);
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
      routeWindowAdd(decoded.body);
      return;
    case 'window-renamed':
      routeWindowRenamed(decoded.body);
      return;
    case 'window-close':
      routeWindowClose(decoded.body);
      return;
    case 'session-changed':
      routeSessionChanged(decoded.body);
      return;
    case 'layout-change':
      routeLayoutChange(decoded.body);
      return;
    case 'pane-mode-changed':
      routePaneModeChanged(decoded.paneId, decoded.body);
      return;
    case 'subscription-changed':
      // SSoT §2.3 — `{name, value}` 형식. mux store 에 currently 저장 슬롯 없음
      // (백엔드가 subscription 자체를 관리하므로 미러 필요성 미정 — P1+ 평가).
      console.debug('[gtmux] subscription-changed pane=%d', decoded.paneId);
      return;
    default:
      // 미정의 kind 는 SSoT §2.3 forward-compat 에 따라 조용히 무시.
      return;
  }
}

// ── NOTIFY_MIRROR kind routing helpers ─────────────────────────────────────
//
// 각 helper 는 SSoT §2.3 의 추가 JSON 필드를 읽어 mux store 메서드로 위임한다.
// JSON 필드 누락은 *해당 frame drop* (forward-compat) — 빈 문자열 fallback 도 가능
// 하지만 store 진실을 noise 로 덮어쓰지 않도록 명시적으로 검증.

function routeWindowAdd(body: Readonly<Record<string, unknown>>): void {
  const windowId = pickString(body, 'window_id');
  if (windowId === null) return;
  const name = pickString(body, 'name') ?? '';
  muxStore.addWindow(windowId, name);
}

function routeWindowRenamed(body: Readonly<Record<string, unknown>>): void {
  const windowId = pickString(body, 'window_id');
  if (windowId === null) return;
  const name = pickString(body, 'name') ?? '';
  muxStore.renameWindow(windowId, name);
}

function routeWindowClose(body: Readonly<Record<string, unknown>>): void {
  const windowId = pickString(body, 'window_id');
  if (windowId === null) return;
  muxStore.closeWindow(windowId);
}

function routeSessionChanged(body: Readonly<Record<string, unknown>>): void {
  const sessionId = pickString(body, 'session_id');
  if (sessionId === null) return;
  const name = pickString(body, 'name') ?? '';
  muxStore.setSession(sessionId, name);
}

function routeLayoutChange(body: Readonly<Record<string, unknown>>): void {
  const windowId = pickString(body, 'window_id');
  if (windowId === null) return;
  const layout = pickString(body, 'layout') ?? '';
  muxStore.setLayout(windowId, layout);
}

function routePaneModeChanged(paneId: number, body: Readonly<Record<string, unknown>>): void {
  const mode = pickString(body, 'mode') ?? '';
  muxStore.setPaneMode(paneId, mode);
}

function pickString(body: Readonly<Record<string, unknown>>, key: string): string | null {
  const v = body[key];
  return typeof v === 'string' ? v : null;
}

// ── 0x01 CTRL response 처리 ────────────────────────────────────────────────

function handleCtrlResponse(payload: Uint8Array): void {
  const decoded = decodeCtrl(payload);
  if (!decoded) {
    console.warn('[ws] 0x01 CTRL response decode failed');
    return;
  }
  if (decoded.id === null) {
    // 서버가 id 없는 응답을 보내는 경우는 ERR_BAD_REQUEST 같은 broadcast-style 에러.
    // pending registry 매칭이 불가능 — debug 만 남김.
    console.debug('[ws] CTRL response without id', decoded.body);
    return;
  }
  const ok = decoded.body['ok'] === true;
  const response: CtrlResponse = {
    id: decoded.id,
    ok,
    ...(ok && typeof decoded.body['result'] === 'object' && decoded.body['result'] !== null
      ? { result: decoded.body['result'] as Readonly<Record<string, unknown>> }
      : {}),
    ...(typeof decoded.body['error'] === 'string'
      ? { error: decoded.body['error'] as string }
      : {}),
    ...(typeof decoded.body['code'] === 'string'
      ? { code: decoded.body['code'] as string }
      : {}),
  };
  resolveCtrl(response);
}

function handleLayoutChanged(payload: Uint8Array): void {
  const decoded = decodeLayoutChanged(payload);
  if (!decoded) {
    console.warn('[ws] 0x80 LAYOUT_CHANGED decode failed');
    return;
  }
  // DO NOT setEtag here. If we cache the broadcast etag *before* the
  // re-fetch fires, fetchLayoutAndHydrate sends If-None-Match=<new_etag>
  // and the server responds 304 — leaving panelsStore with the pre-PUT
  // contents and the new panel invisible until a manual refresh. Let
  // fetchLayoutAndHydrate own the etag transition: it calls setEtag on
  // the 200 response after hydratePanels, and leaves the store alone on
  // 304. The broadcast etag bytes are passed through for callers that
  // want to short-circuit conditional fetches (none today).
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

function noop(): void {
  /* PANE_OUT ack — Sprint 4 에서 backpressure watermark 와 연결. */
}
