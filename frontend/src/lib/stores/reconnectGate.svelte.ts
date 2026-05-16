// reconnectGate — page entry blocking 상태 머신 (ADR-0019 D5.4, plan-0008 §4.4).
//
// 책임:
// - `sessionStorage` 의 hint 가 있을 때 AppPage onMount 가 `start(name)` 호출.
// - 본 화면 (Canvas / Toolbar / ...) 은 `canMountApp` 이 true 일 때만 mount.
//   = state === 'success' (정상 진입) OR state === 'idle' (hint 없음 / cancel 후).
// - 최초 page load 는 'booting' 으로 시작한다. auth gate / session hint 확인 전
//   Canvas 를 먼저 mount 하면 빈 workspace → reconnect → hydrated workspace 로
//   불필요한 mount churn 이 생기므로, bootstrap 이 결론을 내릴 때까지 차단한다.
// - `state === 'loading' | 'in_use' | 'not_found' | 'unreachable'` 동안에는
//   ReconnectModal 만 보이고 본 화면은 mount 차단.
//
// AbortController:
// - start/retry 진입 시 새 controller 생성. cancel 호출 시 abort.
// - `attemptReattach` 의 fetch 가 AbortError throw → result 는 'unreachable'
//   로 normalize 되지만 `signal.aborted` 면 state 갱신 안 함 (cancel 이 이미 'idle').
//
// hint clear:
// - cancel() 안에서 sessionStorageHint.clear() 호출 — 다음 reload 도 dialog 흐름.
// - attemptReattach 의 404 분기는 자체 clear 처리 → reconnectGate 중복 clear 무해.

import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { sessionStorageHint } from '$lib/stores/sessionStorageHint';

export type ReconnectState =
  | 'booting'      // auth gate / session hint 검사 중 — 본 화면 mount 금지
  | 'idle'         // hint 없음 / cancel 후 — workspaceSwitcher 가 attach 결정
  | 'attaching'    // POST /attach 진행 중 (loading)
  | 'hydrating'    // 200 응답 후 GET /layout + loadLayout 진행 중
  | 'in_use'       // 409 — 다른 webpage 가 보유
  | 'not_found'    // 404 — session 사라짐
  | 'unreachable'  // 5xx / network
  | 'ready';       // hydrate 완료 — 본 화면 mount 허용

/** ReconnectModal 이 다루는 4 mode — `attaching`/`hydrating` 는 'loading' 로 normalize. */
export type ReconnectModalState = 'loading' | 'in_use' | 'not_found' | 'unreachable';

class ReconnectGate {
  state = $state<ReconnectState>('booting');
  attemptName = $state<string | null>(null);
  /** unreachable state 의 last error message. */
  error = $state<string | null>(null);
  /** unreachable state 의 attempt counter. start = 1, 매 retry += 1. */
  attempt = $state<number>(0);

  #controller: AbortController | null = null;

  /**
   * 본 화면 mount 게이트. ADR-0019 D5.4 + plan-0008 §4.4 + 0045 P0.
   *
   * - 'booting' / 'attaching' / 'hydrating' = bootstrap/attach 진행 중 — 빈/
   *   partial Canvas mount 금지. boot screen 또는 ReconnectModal 'loading' 노출.
   * - 'idle' = hint 없거나 사용자 cancel 후 — workspaceSwitcher 가 mount 결정.
   *   (workspaceSwitcher modal 이 canvas 를 cover 하므로 빈 canvas flicker 허용.)
   * - 'ready' = 정상 reattach + hydrate 완료 후 본 화면 mount 허용.
   * - 그 외 (failed) = ReconnectModal 만 mount, 본 화면 차단.
   */
  canMountApp = $derived(this.state === 'ready' || this.state === 'idle');

  /** ReconnectModal 이 실제로 다룰 수 있는 user-actionable 상태. */
  modalState = $derived.by((): ReconnectModalState | null => {
    switch (this.state) {
      case 'attaching':
      case 'hydrating':
        return 'loading';
      case 'in_use':
      case 'not_found':
      case 'unreachable':
        return this.state;
      case 'booting':
      case 'idle':
      case 'ready':
        return null;
    }
  });

  /** Auth gate 통과 + session hint 없음. WorkspaceSwitcher 흐름으로 진입 가능. */
  markIdle(): void {
    this.state = 'idle';
    this.attemptName = null;
    this.error = null;
    this.attempt = 0;
  }

  async start(name: string): Promise<void> {
    this.attemptName = name;
    this.attempt = 1;
    this.state = 'attaching';
    this.error = null;
    await this.#run(name);
  }

  async retry(): Promise<void> {
    if (this.attemptName === null) return;
    this.attempt += 1;
    this.state = 'attaching';
    this.error = null;
    await this.#run(this.attemptName);
  }

  /**
   * 사용자 명시 cancel ([Switch session…] 클릭).
   *
   * - 진행 중 fetch 가 있으면 abort.
   * - sessionStorage hint clear — 다음 reload 도 dialog 흐름.
   * - state = 'idle' 로 reset — 본 화면 mount 게이트는 통과하지만, AppPage
   *   에서 그 직후 `workspaceSwitcher.open()` 을 호출하므로 사용자가 본
   *   화면 빈 상태를 흘끗 보더라도 즉시 modal 이 덮음.
   */
  cancel(): void {
    this.#controller?.abort();
    this.#controller = null;
    this.markIdle();
    sessionStorageHint.clear();
  }

  /** attemptReattach 200 + loadLayout 완료 후 호출 — sessionStore active/layout set. */
  markReady(): void {
    this.state = 'ready';
  }

  /** @deprecated 0045 P0 — markSuccess → markReady rename. 호환 alias. */
  markSuccess(): void {
    this.markReady();
  }

  async #run(name: string): Promise<void> {
    this.#controller?.abort();
    this.#controller = new AbortController();
    const signal = this.#controller.signal;
    // attemptReattach 내부 흐름: POST /attach → (200 시) GET /layout → loadLayout.
    // attaching → hydrating 전이는 attemptReattach 의 attach 200 응답 직후이지만
    // 본 wrapper 가 그 boundary 를 볼 수 없으므로 attaching 단일 phase 로 시작 후,
    // success 시 markReady 로 직접 진입. (5-state 의 hydrating 은 attemptReattach
    // 가 분해되어 호출자가 attach + loadLayout 을 따로 호출하는 미래 refactor 의
    // hook — 본 P0 fix 에선 modalState='loading' 으로 normalize 되어 사용자 perception
    // 차이 없음.)
    const result = await sessionStore.attemptReattach(name, signal);
    if (signal.aborted) return; // cancel 됨 — state 변경 안 함
    switch (result.kind) {
      case 'success':
        this.markReady();
        return;
      case 'in_use':
        this.state = 'in_use';
        return;
      case 'not_found':
        this.state = 'not_found';
        return;
      case 'unauthorized':
        window.location.href = '/auth';
        return;
      case 'unreachable':
        this.state = 'unreachable';
        this.error = result.message;
        return;
    }
  }
}

export const reconnectGate = new ReconnectGate();
