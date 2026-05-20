// WS heartbeat watchdog — ADR-0021 D6 정합.
//
// 정본:
// - ADR-0021 D6 (server-driven RFC 6455 PING 15s / 30s timeout)
// - plan-0008 §6 Phase 2 (Case II silent reattach — idle detection 입력)
//
// 책임:
// - WS frame 수신 timestamp 추적 (`lastFrameAt`) — server liveness.
// - User activity timestamp 추적 (`lastActivityAt`) — Phase 2 idle reactivate
//   trigger 의 기준.
// - 1s 틱으로 derived 상태 갱신 (`isStale`, `isIdle`).
//
// 사용:
//   import { heartbeatStore } from '$lib/ws/heartbeat.svelte';
//   heartbeatStore.start();           // page mount
//   heartbeatStore.markFrame();       // dispatcher 가 매 frame 수신 시 호출
//   heartbeatStore.markActivity();    // window keydown / mousedown 시
//   if (heartbeatStore.isIdle) { ... } // Phase 2 silent reattach 조건
//   heartbeatStore.stop();            // page unmount
//
// 노트:
// - RFC 6455 PING/PONG 자체는 browser 의 WebSocket 구현이 자동 처리하므로 본
//   store 는 application-level frame 만 본다. server 가 PING 외 frame 을
//   30s 동안 보내지 않으면 `isStale=true` — 정상이지만 idle (no PANE_OUT/
//   NOTIFY) 상태. server-side close (정말 끊김) 는 `ws/client.ts` 의 close
//   handler 가 connectionStore 의 state 로 즉시 노출하므로 본 store 와 별도.

/** Stale 임계 — 마지막 server frame 후 30s 이상이면 `isStale=true`. */
const STALE_THRESHOLD_MS = 30_000;

/** Idle 임계 — 마지막 user 입력 후 15s 이상이면 `isIdle=true` (Phase 2). */
const IDLE_THRESHOLD_MS = 15_000;

/** Tick 주기 — derived 갱신 빈도. 1s 면 사용자 perception 직선상 충분. */
const TICK_INTERVAL_MS = 1_000;

class HeartbeatStore {
  /** 마지막 server WS frame 수신 시각 (Date.now() ms). 0 = mount 직후 아직 수신 안 함. */
  lastFrameAt = $state<number>(0);

  /** 마지막 user 입력 시각 (Date.now() ms). 0 = mount 직후 아직 입력 안 함. */
  lastActivityAt = $state<number>(0);

  /** 현재 tick 의 now — derived 의 reactive 기반. */
  #now = $state<number>(Date.now());

  /** Tick interval id — start/stop 정합. */
  #tickId: ReturnType<typeof setInterval> | null = null;

  /** Activity listener bound 여부 — start 중복 호출 방지. */
  #activityBound = false;

  /**
   * 30s 무수신 stale — server 가 살아있어도 application-level activity 0.
   * Phase 2 의 silent reattach trigger 의 보조 신호 (idle 와 별도 또는 결합).
   * `lastFrameAt === 0` (아직 1 frame 도 수신 안 함) 일 때는 false — bootstrap
   * race 보호.
   */
  isStale = $derived(
    this.lastFrameAt > 0 && this.#now - this.lastFrameAt > STALE_THRESHOLD_MS,
  );

  /**
   * 15s+ 사용자 idle — Phase 2 의 Case II silent reattach trigger.
   * `lastActivityAt === 0` (아직 입력 없음) 일 때는 false — page 진입 직후
   * idle 로 즉시 reattach 시도하는 noise 방지.
   */
  isIdle = $derived(
    this.lastActivityAt > 0 &&
      this.#now - this.lastActivityAt > IDLE_THRESHOLD_MS,
  );

  /** 마지막 frame 후 경과 ms — debug surface. */
  msSinceLastFrame = $derived(
    this.lastFrameAt === 0 ? null : this.#now - this.lastFrameAt,
  );

  /** 마지막 activity 후 경과 ms — debug surface. */
  msSinceLastActivity = $derived(
    this.lastActivityAt === 0 ? null : this.#now - this.lastActivityAt,
  );

  /**
   * Page mount 시 호출 — tick interval + activity listener 활성.
   * 중복 호출 idempotent.
   */
  start(): void {
    if (typeof window === 'undefined') return;
    if (this.#tickId !== null) return;
    this.#tickId = setInterval(() => {
      this.#now = Date.now();
    }, TICK_INTERVAL_MS);
    if (!this.#activityBound) {
      window.addEventListener('keydown', this.#onActivity, { passive: true });
      window.addEventListener('mousedown', this.#onActivity, { passive: true });
      window.addEventListener('touchstart', this.#onActivity, { passive: true });
      this.#activityBound = true;
    }
  }

  /** Page unmount 시 호출 — listener / interval 해제. */
  stop(): void {
    if (this.#tickId !== null) {
      clearInterval(this.#tickId);
      this.#tickId = null;
    }
    if (typeof window !== 'undefined' && this.#activityBound) {
      window.removeEventListener('keydown', this.#onActivity);
      window.removeEventListener('mousedown', this.#onActivity);
      window.removeEventListener('touchstart', this.#onActivity);
      this.#activityBound = false;
    }
  }

  /** Dispatcher 가 매 WS frame 수신 시 호출 — last server activity timestamp 갱신. */
  markFrame(): void {
    this.lastFrameAt = Date.now();
  }

  /** Mutation guard / Phase 2 reattach 성공 후 reset — fresh baseline. */
  reset(): void {
    const now = Date.now();
    this.lastFrameAt = now;
    this.lastActivityAt = now;
  }

  #onActivity = (): void => {
    this.lastActivityAt = Date.now();
  };
}

export const heartbeatStore = new HeartbeatStore();
