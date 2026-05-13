// WebSocket client — single connection + auto-reconnect + envelope I/O.
//
// 정본:
// - `docs/adr/0002-transport-websocket.md` D1 (single endpoint), D5 (subprotocol auth),
//   D8 (browser-only reconnect, full re-sync)
// - `docs/adr/0003-security-defaults.md` D5 (Sec-WebSocket-Protocol 토큰)
// - `docs/reports/0010-grill-amendments.md` D21 c2 (1s grace) + c3 (exp backoff
//   0.5→1→2→4→8→16→30s cap, indefinite retry)
// - `docs/reports/0008-frontend-stack.md` §F6 (state machine + banner copy)
//
// 본 모듈은 *transport-only*. envelope 의 의미적 처리 (store fan-out) 는 dispatcher.

import { decodeEnvelope, encodeEnvelope, type Envelope, type FrameTypeCode } from './decode';

// ── 공개 타입 ──────────────────────────────────────────────────────────────

/** 4-상태 단순 머신 — connection.svelte.ts 의 `WsState` 와 정합. */
export type ConnectionState = 'connecting' | 'open' | 'closing' | 'closed' | 'reconnecting';

export interface WsClientOptions {
  /** WS URL — 일반적으로 `computeWsUrl()` 산출. */
  readonly url: string;
  /** base64url 토큰 (XDG_STATE_HOME 의 `.token` 파일). */
  readonly token: string;
  /** 디코드 성공한 envelope 단위 콜백. dispatcher 가 store fan-out 수행. */
  readonly onMessage: (frame: Envelope) => void;
  /** 상태 전이 콜백. ConnectionStore 와 ReconnectBanner 의 single source. */
  readonly onStateChange: (state: ConnectionState, attempt: number) => void;
  /** 선택적 에러 콜백 — WebSocket `onerror` 의 raw event 를 그대로 전달. */
  readonly onError?: (e: unknown) => void;
}

// ── 상수 ────────────────────────────────────────────────────────────────────

/**
 * Reconnect backoff schedule (D21 c3). MVP 는 D21 c2 의 "grace 1s" 를 *첫 시도의 지연*
 * 로 해석하지 않고, *banner 표출 grace* 로 해석 — 즉 첫 시도는 즉시(이 상수의 0번째 항목)
 * 발생하되 banner 는 별도 1s 타이머가 만료된 뒤에 노출. (R8 F6 state machine.)
 *
 * 0.5s, 1s, 2s, 4s, 8s, 16s, 30s cap.
 */
const BACKOFF_MS = [500, 1000, 2000, 4000, 8000, 16000] as const;
const BACKOFF_CAP_MS = 30_000;

/** D21 c2 — 끊김 후 banner 표출까지 grace. */
const BANNER_GRACE_MS = 1000;

/** RFC 6455 normal-close code. */
const CLOSE_NORMAL = 1000;

/** 사용자 호출 stop() 시 close reason — debugging only. */
const CLOSE_REASON_STOP = 'client-stop';

// ── WsClient ───────────────────────────────────────────────────────────────

/**
 * Lifecycle:
 *
 *   created → start() → connecting → open
 *                          │           │
 *                          ▼           ▼
 *                       closed ←── (ws.onclose)
 *                          │
 *                          ▼ (if not stop())
 *                      reconnecting → connecting → …
 *
 * stop() 은 어느 상태에서든 즉시 closed 로 가고 reconnect timer 를 cancel.
 */
export class WsClient {
  readonly #opts: WsClientOptions;
  #ws: WebSocket | null = null;
  #state: ConnectionState = 'closed';
  #attempt = 0;
  #reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  #bannerTimer: ReturnType<typeof setTimeout> | null = null;
  /** stop() 후엔 어떤 close 도 reconnect 를 트리거하지 않는다. */
  #stopped = false;

  constructor(opts: WsClientOptions) {
    this.#opts = opts;
  }

  /** 현재 connection state — UI 가 직접 읽기 위함. */
  get state(): ConnectionState {
    return this.#state;
  }

  /** 누적 reconnect attempt — banner 의 "(attempt N)" 표기용. */
  get attempt(): number {
    return this.#attempt;
  }

  /** Connect 또는 재접속 루프 시작. 이미 살아 있으면 no-op. */
  start(): void {
    this.#stopped = false;
    if (this.#ws !== null) return;
    this.#open();
  }

  /**
   * Envelope 송신. `WebSocket.readyState === OPEN` 일 때만 보낸다 — 그 외엔
   * debug log + drop. MT-3 단일 진실은 서버이므로 클라이언트 측 큐잉은 의미 없음
   * (재연결 시 서버가 `0x81–0x84` 로 현재 상태를 push 한다, ADR-0002 D8).
   */
  send(frame: Envelope): void {
    const ws = this.#ws;
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      console.debug('[ws] send dropped (state=%s)', this.#state);
      return;
    }
    const buf = encodeEnvelope(frame.kind, frame.payload);
    ws.send(buf);
  }

  /** Convenience: build envelope + send in one call. */
  sendFrame(kind: FrameTypeCode, payload: Uint8Array): void {
    this.send({ kind, payload });
  }

  /**
   * Graceful close. close code 1000 (normal) + reconnect timer cancel. 이후
   * 어떤 onclose 도 reconnect 를 트리거하지 않는다.
   */
  stop(): void {
    this.#stopped = true;
    this.#cancelReconnect();
    this.#cancelBanner();
    const ws = this.#ws;
    if (ws !== null) {
      this.#transition('closing');
      try {
        ws.close(CLOSE_NORMAL, CLOSE_REASON_STOP);
      } catch (e) {
        // close() 가 매우 드물게 InvalidAccessError 를 던질 수 있음 (이미 closing/closed).
        console.debug('[ws] stop close error', e);
      }
    } else {
      this.#transition('closed');
    }
  }

  // ── 내부 ────────────────────────────────────────────────────────────────

  #open(): void {
    this.#cancelReconnect();
    this.#transition('connecting');

    let ws: WebSocket;
    try {
      // RFC 6455: subprotocol 은 comma-separated value 가 아니라 *배열* 로
      // 전달해야 한다 (브라우저 WS API). 백엔드 lib.rs 의 `parse_subprotocol` 은
      // comma-separated 를 받아 두 sub-token 을 추출한다.
      ws = new WebSocket(this.#opts.url, ['gtmux.v1', `bearer.${this.#opts.token}`]);
    } catch (e) {
      this.#opts.onError?.(e);
      this.#scheduleReconnect();
      return;
    }
    ws.binaryType = 'arraybuffer';
    this.#ws = ws;

    ws.onopen = () => {
      // 성공적 open — attempt 카운터 0 으로 리셋. banner 도 닫힘.
      this.#attempt = 0;
      this.#cancelBanner();
      this.#transition('open');
    };

    ws.onmessage = (ev: MessageEvent) => {
      const data = ev.data;
      if (!(data instanceof ArrayBuffer)) {
        // SSoT §1.1: text frame 은 protocol 위반. 브라우저 WS API 는 close
        // code 를 client 측에서 보낼 수 없으므로 그냥 무시 + warn.
        console.warn('[ws] non-binary message dropped');
        return;
      }
      const env = decodeEnvelope(data);
      if (env === null) {
        console.warn('[ws] envelope decode failed (%dB)', data.byteLength);
        return;
      }
      try {
        this.#opts.onMessage(env);
      } catch (e) {
        // 핸들러 예외가 connection 을 끊지 않도록 격리.
        console.error('[ws] onMessage handler threw', e);
        this.#opts.onError?.(e);
      }
    };

    ws.onerror = (ev: Event) => {
      this.#opts.onError?.(ev);
    };

    ws.onclose = (ev: CloseEvent) => {
      this.#ws = null;
      // 정상 종료 + stop() 발생: 재연결 없음.
      if (this.#stopped) {
        this.#transition('closed');
        return;
      }
      // server 측 정책 위반 등으로 1000 이 아닌 코드로 끊긴 경우도 *재연결*.
      // 인증 실패(1008 / 4001 token rotated) 도 일단 재시도 — 사용자가 새 URL 을
      // 받기 전까지 backoff 가 더 늘어날 뿐. 인증 실패 영구화는 ConnectionStore
      // 의 attempt 카운터를 보고 UI 가 사용자에게 안내.
      console.debug('[ws] closed code=%d reason=%s', ev.code, ev.reason);
      this.#scheduleReconnect();
    };
  }

  /**
   * D21 c2/c3 의 grace + backoff 통합 구현.
   *
   * - **첫 끊김 발생**: 즉시 attempt=1 로 connect 시도 (50ms 정도의 "0번째" 지연
   *   없이 바로 ws.connect). 동시에 BANNER_GRACE_MS=1s 타이머를 시작해 그 안에
   *   open 으로 복귀하면 banner 노출 없음 — `connecting` 상태로만 유지.
   *   1s 안에 복귀 못 하면 reconnecting 상태로 전이 (banner 노출).
   *
   * - **이어지는 실패**: BACKOFF_MS 에서 (attempt-1) 인덱스 지연 후 재시도.
   *   배열 길이 초과는 BACKOFF_CAP_MS (30s) cap.
   */
  #scheduleReconnect(): void {
    if (this.#stopped) {
      this.#transition('closed');
      return;
    }
    this.#attempt += 1;
    const delay = this.#attempt === 1
      ? 0
      : (BACKOFF_MS[this.#attempt - 2] ?? BACKOFF_CAP_MS);

    // banner grace timer — connecting 1s 내 복귀 못 하면 reconnecting 으로.
    this.#armBannerTimer();

    this.#transition('connecting');
    this.#reconnectTimer = setTimeout(() => {
      this.#reconnectTimer = null;
      this.#open();
    }, delay);
  }

  #armBannerTimer(): void {
    this.#cancelBanner();
    this.#bannerTimer = setTimeout(() => {
      this.#bannerTimer = null;
      // open 으로 이미 돌아갔으면 state 가 'open' 이므로 transition 조건이 막힘.
      if (this.#state === 'connecting') {
        this.#transition('reconnecting');
      }
    }, BANNER_GRACE_MS);
  }

  #cancelReconnect(): void {
    if (this.#reconnectTimer !== null) {
      clearTimeout(this.#reconnectTimer);
      this.#reconnectTimer = null;
    }
  }

  #cancelBanner(): void {
    if (this.#bannerTimer !== null) {
      clearTimeout(this.#bannerTimer);
      this.#bannerTimer = null;
    }
  }

  #transition(next: ConnectionState): void {
    if (this.#state === next) return;
    this.#state = next;
    this.#opts.onStateChange(next, this.#attempt);
  }
}

// ── URL 계산 ────────────────────────────────────────────────────────────────

/**
 * Same-origin WS URL 산출. `window.location` 의 scheme + host 를 그대로 사용해
 * Origin/Host allowlist (ADR-0002 D6) 와 정합한다.
 *
 * - `http:` → `ws:`, `https:` → `wss:`.
 * - path 는 `/ws` 고정 (ADR-0002 D1).
 * - SSR 환경에서는 호출 측이 별도 URL 을 옵션으로 넘겨야 한다 — 본 helper 는
 *   브라우저 전용 (`window` 부재 시 throw).
 */
export function computeWsUrl(): string {
  if (typeof window === 'undefined' || typeof window.location === 'undefined') {
    throw new Error('computeWsUrl: window.location unavailable (SSR context)');
  }
  const loc = window.location;
  const scheme = loc.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${scheme}//${loc.host}/ws`;
}
