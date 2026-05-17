// Envelope dispatcher — R8 §F4 메인 스레드 단일 dispatcher.
//
// 책임:
// - WsClient 가 디코드한 envelope 을 frame type 별로 fan-out:
//     * 0x02 PANE_OUT  → registered per-pane handler (xterm.write)
//     * 0x07 NOTIFY_MIRROR → connection / panel state hints (e.g. pane-died zombie)
//     * 0x80 LAYOUT_CHANGED → multi-session 에서는 mutateLayout 의 응답이 진실
//       (legacy v1 refetch 는 0044 dual-source 제거 시 폐기). 본 핸들러는 외부
//       hook 만 호출하고 자체 store mutation 은 안 함.
//     * 0x81 M_CHANGED       → sessionStore.M (active session 매칭 시)
//     * 0x82 I_CHANGED       → sessionStore.I (active session 매칭 시)
//     * 0x83 VIEWPORT_CHANGED → sessionStore.viewport (active session 매칭 시)
//     * 0x84 FOCUS_MODE_CHANGED → sessionStore.focusMode (active session 매칭 시)
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

import { connectionStore } from '$lib/stores/connection.svelte';
import { danglingTerminals } from '$lib/stores/danglingTerminals.svelte';
import { heartbeatStore } from '$lib/ws/heartbeat.svelte';
import { muxStore } from '$lib/stores/mux.svelte';
import { reconnectGate } from '$lib/stores/reconnectGate.svelte';
import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { terminalPool } from '$lib/stores/terminalPool.svelte';
import type { CanvasItem, TerminalItem } from '$lib/types/canvas';
import {
  FRAME_TYPE,
  decodeCtrl,
  decodeFocusMode,
  decodeIChanged,
  decodeLayoutChanged,
  decodeMChanged,
  decodeMountCascade,
  decodeNotifyMirror,
  decodePaneOut,
  decodeTerminalDied,
  decodeTerminalListUpdate,
  decodeTerminalSpawned,
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
//
// **Multi-subscriber (ADR-0021 D1 — mirror)**: 같은 UUID terminal 이 multiple panel
// 에 mount 될 수 있음 (mirror). 각 XtermHost 가 본 paneId 에 자신의 handler 를 등록 —
// dispatcher 가 fan-out 으로 모두에게 같은 bytes 를 흘려보낸다. 등록 / 해제는 *handler
// identity* 기반 (Set membership).

type PaneOutHandler = (buf: Uint8Array, cb: () => void) => void;

const paneOutHandlers = new Map<string, Set<PaneOutHandler>>();

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

/**
 * Hot-path debug log gate. Vite 가 production build 시 `import.meta.env.DEV`
 * 를 `false` literal 로 inline → `if (false) console.debug(...)` 가 dead-code
 * elimination 되어 PANE_OUT 마다의 console call 비용 0. Dev 에서는 그대로
 * 동작.
 */
const DEBUG_PANE_OUT = import.meta.env.DEV;

/** Per-pane late buffer + running total (length recompute 비용 제거). */
type LateBufferEntry = { chunks: Uint8Array[]; total: number };
const paneOutLateBuffers = new Map<string, LateBufferEntry>();

function appendLateBuffer(paneKey: string, bytes: Uint8Array): void {
  let entry = paneOutLateBuffers.get(paneKey);
  if (!entry) {
    entry = { chunks: [], total: 0 };
    paneOutLateBuffers.set(paneKey, entry);
  }
  // Drop oldest until the new chunk fits. (FIFO — keep the *most recent*
  // bytes, which are usually the most relevant for visual catch-up.)
  // running `total` 로 O(k) 처리 — 직전 구현의 reduce-in-while 가 O(k²) 였음.
  while (entry.chunks.length > 0 && entry.total + bytes.length > PANE_LATE_BUFFER_CAP) {
    const dropped = entry.chunks.shift();
    if (dropped !== undefined) entry.total -= dropped.length;
  }
  entry.chunks.push(bytes);
  entry.total += bytes.length;
}

export function registerPaneOut(paneId: string, handler: PaneOutHandler): void {
  let set = paneOutHandlers.get(paneId);
  if (!set) {
    set = new Set();
    paneOutHandlers.set(paneId, set);
  }
  const wasEmpty = set.size === 0;
  set.add(handler);
  // Only the FIRST subscriber on a pane drains the late buffer — subsequent
  // mirror subscribers join the live stream from here on (no historical replay;
  // that's ADR-0021 D6 catch-up territory, out of scope for the dispatcher).
  if (wasEmpty) {
    const entry = paneOutLateBuffers.get(paneId);
    if (entry && entry.chunks.length > 0) {
      if (DEBUG_PANE_OUT) {
        console.debug('[ws] registerPaneOut pane=%s flushing %d buffered chunk(s)',
          paneId, entry.chunks.length);
      }
      for (const bytes of entry.chunks) handler(bytes, noop);
      paneOutLateBuffers.delete(paneId);
    } else if (DEBUG_PANE_OUT) {
      console.debug('[ws] registerPaneOut pane=%s (no buffered bytes)', paneId);
    }
  } else if (DEBUG_PANE_OUT) {
    console.debug('[ws] registerPaneOut pane=%s subscriber=%d (mirror)', paneId, set.size);
  }
}

export function unregisterPaneOut(paneId: string, handler: PaneOutHandler): void {
  const set = paneOutHandlers.get(paneId);
  if (!set) return;
  set.delete(handler);
  if (set.size === 0) paneOutHandlers.delete(paneId);
}

// ── Dispatcher factory ─────────────────────────────────────────────────────

export interface DispatcherOptions {
  /**
   * base64url Bearer token. `null` 인 경우 cookie-only handshake (D10 α). WS
   * subprotocol 은 `gtmux.v1` 만 송신 — BE 의 cookie_validator 가 cookie 로 upgrade
   * 인증.
   */
  readonly token: string | null;
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
  // Heartbeat: server liveness watchdog (ADR-0021 D6). 매 frame 의 수신
  // timestamp 갱신 — Phase 2 의 stale detection 입력.
  heartbeatStore.markFrame();
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
    case FRAME_TYPE.TERMINAL_DIED:
      return handleTerminalDied(env.payload);
    case FRAME_TYPE.MOUNT_CASCADE:
      return handleMountCascade(env.payload);
    case FRAME_TYPE.TERMINAL_LIST_UPDATE:
      return handleTerminalListUpdate(env.payload);
    case FRAME_TYPE.TERMINAL_SPAWNED:
      return handleTerminalSpawned(env.payload);
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
  const handlers = paneOutHandlers.get(key);
  if (!handlers || handlers.size === 0) {
    if (DEBUG_PANE_OUT) {
      console.debug('[ws] PANE_OUT pane=%s len=%d → late-buffer (no subscribers)',
        key, decoded.bytes.length);
    }
    appendLateBuffer(key, decoded.bytes);
    return;
  }
  // Fan out to every subscriber (ADR-0021 D1 mirror). Each xterm gets the same
  // bytes — they'll all converge on the same screen state (modulo their own
  // local cursor / scrollback). Snapshot the set so a subscriber unregistering
  // mid-iteration (e.g. via term.write triggering an unmount) doesn't skip
  // siblings.
  if (DEBUG_PANE_OUT) {
    console.debug('[ws] PANE_OUT pane=%s len=%d → %d subscriber(s)',
      key, decoded.bytes.length, handlers.size);
  }
  for (const handler of [...handlers]) {
    handler(decoded.bytes, noop);
  }
}

function handleNotifyMirror(payload: Uint8Array): void {
  const decoded = decodeNotifyMirror(payload);
  if (!decoded) {
    console.warn('[ws] 0x07 NOTIFY_MIRROR decode failed');
    return;
  }
  const kind = typeof decoded.body['kind'] === 'string' ? (decoded.body['kind'] as string) : '';
  // Stage-B NOTIFY_MIRROR kinds (ADR-0013 D10 / 0026 §2.5). All
  // tmux-era kinds (window-add / window-renamed / window-close /
  // session-changed / layout-change / pane-mode-changed /
  // subscription-changed / slow-pane) are permanently retired —
  // the backend never emits them.
  switch (kind) {
    case 'pane-spawned': {
      // Idempotent — addPane is also called from PANE_OUT first-sight
      // for the catch-up race. Carry the optional `request_id` to
      // ctrl-registry if it correlates with a pending request: a
      // missing request_id is the (more common) broadcast case where
      // the NOTIFY simply tells every WS subscriber a new pane exists.
      muxStore.addPane(decoded.paneId);
      // Multi-session path: pane-spawned is informational. Canvas mount
      // is owned by `0x86 MOUNT_CASCADE` / `0x88 TERMINAL_SPAWNED`.
      return;
    }
    case 'pane-died':
      // D21 c4 — panel header 의 zombie badge 와 직결.
      muxStore.killPane(decoded.paneId);
      addZombie(decoded.paneId);
      return;
    case 'layout-changed':
      // The 0x80 LAYOUT_CHANGED envelope is the primary signal —
      // the NOTIFY_MIRROR carries no additional data. Debug only.
      console.debug('[ws] NOTIFY layout-changed (0x80 will follow)');
      return;
    case 'server-ready':
      // Reserved for boot-complete UX surfacing. No-op for now.
      console.debug('[ws] NOTIFY server-ready');
      return;
    default:
      // Forward-compat — silently ignore future kinds.
      return;
  }
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
  // Multi-session: mutateLayout() 호출의 응답이 진실 — 본 broadcast 는 같은
  // session 의 *다른 webpage* 가 변경했을 때 의미가 있는데 single-attach lock
  // 으로 그 경로가 닫혀 있어 현재로서는 informational. Phase 2 (cross-tab
  // multi-attach) 가 land 하면 `/api/sessions/<name>/layout` refetch 추가.
  console.debug('[ws] 0x80 LAYOUT_CHANGED received (multi-session no-op)');
}

/**
 * Stage 5-C 사전-scaffold (0034 §4 deferred).
 *
 * 5-C 가 ship 되면 0x81/0x82/0x83/0x84 frame body 에 *optional* `session_id` 가
 * 포함된다 (0034 §8.2 option (a) — top-level field 권장). BE 가 cookie ↔
 * session_id mapping (5-A) 으로 *해당 session 의 webpage 에만* fan-out 하므로
 * 정상 흐름에서는 FE 측 추가 필터 불필요. 다만:
 *   - BE 가 mid-attach race 로 잘못된 connection 에 보낼 가능성
 *   - FE 의 sessionStore.active 가 BE 보다 먼저 변경됐을 race
 * 위 두 경우 본 helper 가 drop 정책을 통일한다.
 *
 * 사용 시점: 5-C 가 BE 에 land 하면 각 handler 의 decoder 가 `sessionId` 를 추가
 * 반환하도록 amend → handler 시작부에서 `if (!isFrameForActiveSession(decoded.sessionId)) return;`
 * 한 줄 추가. wire 의 *frame-level shape* (binary varint vs JSON envelope) 는
 * BE 가 확정 후 결정 — 현재 dispatcher 는 binary varint 형식 그대로 처리.
 */
export function isFrameForActiveSession(frameSessionId: string | null | undefined): boolean {
  if (frameSessionId === null || frameSessionId === undefined) return true;
  const active = sessionStore.active;
  if (active === null) return false;
  return active.name === frameSessionId;
}

function handleMChanged(payload: Uint8Array): void {
  const decoded = decodeMChanged(payload);
  if (!decoded) {
    console.warn('[ws] 0x81 M_CHANGED decode failed');
    return;
  }
  // Active session 이 없으면 drop — pre-attach race 안전 가드.
  if (sessionStore.active === null) return;
  sessionStore.setM(decoded.panelIds.map(String));
}

function handleIChanged(payload: Uint8Array): void {
  const decoded = decodeIChanged(payload);
  if (!decoded) {
    console.warn('[ws] 0x82 I_CHANGED decode failed');
    return;
  }
  if (sessionStore.active === null) return;
  sessionStore.setI(decoded.paneId === null ? null : String(decoded.paneId));
}

function handleViewportChanged(payload: Uint8Array): void {
  const decoded = decodeViewport(payload);
  if (!decoded) {
    console.warn('[ws] 0x83 VIEWPORT_CHANGED decode failed');
    return;
  }
  if (sessionStore.active === null) return;
  // 직접 set — updateViewport 의 debounce PUT 은 외부 변경 (다른 tab 등) 에는
  // 부적합. 본 broadcast 는 BE 가 이미 영속한 상태의 통보.
  sessionStore.viewport = { x: decoded.x, y: decoded.y, zoom: decoded.zoom };
}

function handleTerminalDied(payload: Uint8Array): void {
  const decoded = decodeTerminalDied(payload);
  if (!decoded) {
    console.warn('[ws] 0x85 TERMINAL_DIED decode failed');
    return;
  }
  // server-wide broadcast (BE 0034 §3.5). Mirror panels in every session share the
  // same UUID, so a single mark covers every PanelDanglingOverlay subscribed to it.
  danglingTerminals.mark(decoded.terminalId, decoded.reason);
  // Stale UUID→PaneId binding 폐기 — respawn 후 새 PaneId 가 0x88 로 도착하면
  // 그때 다시 bind. 폐기를 미루면 dangling 상태에서 잠시 옛 PaneId 로 stream
  // subscribe 시도가 일어날 수 있음.
  terminalPool.unbindPaneId(decoded.terminalId);
  // Pool snapshot follows (alive: false). Refresh to drop the latency from the
  // 5-s poll window so TerminalsPanel / PaneInfoPanel show the change too.
  void terminalPool.refresh();
}

/**
 * 0x86 MOUNT_CASCADE — BE Stage 5-D (0034 §8.3).
 *
 * Trigger-session-only frame (BE routes via 5-A session_table). Append a fresh
 * TerminalItem at the server-provided coordinates if not already on canvas.
 * No-op when no active session (race: WS open before attach).
 */
function handleMountCascade(payload: Uint8Array): void {
  const decoded = decodeMountCascade(payload);
  if (!decoded) {
    console.warn('[ws] 0x86 MOUNT_CASCADE decode failed');
    return;
  }
  const active = sessionStore.active;
  if (active === null) {
    console.debug('[ws] MOUNT_CASCADE received without active session — drop');
    return;
  }
  const name = active.name;
  if (sessionStore.items.has(decoded.terminalId)) {
    // Idempotent — BE may broadcast on a retry.
    return;
  }
  // ADR-0028 D1.1 — WS-driven mutation 은 사용자 액션 이 아니므로 history
  // capture 제외. captureHistory:false 로 stack 오염 방지.
  void sessionStore
    .applyMutation(
      (cur) => {
        if (cur.items.some((it: CanvasItem) => it.id === decoded.terminalId)) {
          return cur;
        }
        const maxZ = cur.items.reduce(
          (m: number, it: CanvasItem) => (it.z > m ? it.z : m),
          0,
        );
        const item: TerminalItem = {
          id: decoded.terminalId,
          type: 'terminal',
          parent_id: null,
          x: decoded.x,
          y: decoded.y,
          w: decoded.w,
          h: decoded.h,
          z: maxZ + 1,
          visibility: 'visible',
          locked: false,
          minimized: false,
        };
        return { ...cur, items: [...cur.items, item] };
      },
      {
        captureHistory: false,
        failMessage: 'Terminal mount-cascade failed',
      },
    )
    .then((result) => {
      if (result.ok) void terminalPool.refresh();
    });
}

/**
 * 0x87 TERMINAL_LIST_UPDATE — BE Stage 5-D (0034 §8.3).
 *
 * Non-trigger-session frame. `added`/`removed` is a hint delta; `GET /api/terminals`
 * is authoritative. Just refresh the pool.
 */
function handleTerminalListUpdate(payload: Uint8Array): void {
  const decoded = decodeTerminalListUpdate(payload);
  if (!decoded) {
    console.warn('[ws] 0x87 TERMINAL_LIST_UPDATE decode failed');
    return;
  }
  void terminalPool.refresh();
}

/**
 * 0x88 TERMINAL_SPAWNED — BE batch `d00db66` (0039 §1.2 + §2.3).
 *
 * server-wide broadcast. UUID → numeric PaneId binding 갱신 — XtermHost 의
 * terminal 모드가 reactive 하게 PANE_OUT/IN/RESIZE 흐름을 시작한다 (Option C1
 * unblocker).
 */
function handleTerminalSpawned(payload: Uint8Array): void {
  const decoded = decodeTerminalSpawned(payload);
  if (!decoded) {
    console.warn('[ws] 0x88 TERMINAL_SPAWNED decode failed');
    return;
  }
  terminalPool.bindPaneId(decoded.terminalId, decoded.paneId);
  // multi-webpage / multi-panel 의 무한 respawn loop 차단 — 한 webpage 가
  // respawn 트리거 → 0x88 broadcast → 모든 webpage 의 dangling overlay 자동
  // 해제. clear 호출 누락 시 다른 webpage 의 overlay 가 stale 한 채 남아
  // 사용자가 click → 또 spawn → infinite loop 발생.
  danglingTerminals.clear(decoded.terminalId);
}

function handleFocusModeChanged(payload: Uint8Array): void {
  const decoded = decodeFocusMode(payload);
  if (!decoded) {
    console.warn('[ws] 0x84 FOCUS_MODE_CHANGED decode failed');
    return;
  }
  if (sessionStore.active === null) return;
  sessionStore.focusMode = {
    enabled: decoded.enabled,
    targetPanelId: decoded.targetPanelId === null ? null : String(decoded.targetPanelId),
  };
}

// ── ConnectionStore 어댑터 ─────────────────────────────────────────────────

/** 직전 state — Phase 2 의 reconnecting → open 전이 감지용. */
let prevWsState: ConnectionState = 'closed';

function adaptStateChange(state: ConnectionState, attempt: number): void {
  connectionStore.setState(state);
  // setState 가 open 진입 시 attempt 를 0 으로 리셋하므로, 그 이후 라이프사이클에서만
  // attempt 를 따로 반영해야 한다 — open 이 아닌 경우만 직접 set.
  if (state !== 'open') {
    connectionStore.attempt = attempt;
  }
  // Phase 2 (plan-0008 §6) — WS 가 reconnect 후 open 진입 시 silent reattach
  // 시도. server restart 등으로 cookie-side attach binding 이 사라졌을 가능성
  // 대비. canMountApp 가드가 켜진 동안 (idle/success) 만 trigger — Phase 1
  // 의 blocking modal 흐름과 충돌 방지.
  if (prevWsState === 'reconnecting' && state === 'open') {
    const active = sessionStore.active;
    if (active !== null && reconnectGate.canMountApp) {
      void sessionStore
        .silentReattach(active.name)
        .then((result) => {
          if (result.kind !== 'success') {
            console.debug('[gtmux] silent reattach failed', result);
          }
        });
    }
  }
  prevWsState = state;
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
