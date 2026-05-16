// reconnectGate — page entry blocking 상태 머신 (ADR-0019 D5.4, plan-0008 §4.4).
//
// 책임:
// - `sessionStorage` 의 hint 가 있을 때 AppPage onMount 가 `start(name)` 호출.
// - 본 화면 (Canvas / Toolbar / ...) 은 `canMountApp` 이 true 일 때만 mount.
//   = state === 'success' (정상 진입) OR state === 'idle' (hint 없음 / cancel 후).
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
  | 'idle'         // 아직 start 안 됨 / cancel 후
  | 'loading'
  | 'in_use'
  | 'not_found'
  | 'unreachable'
  | 'success';     // attemptReattach 200 — 본 화면 mount 가능

class ReconnectGate {
  state = $state<ReconnectState>('idle');
  attemptName = $state<string | null>(null);
  /** unreachable state 의 last error message. */
  error = $state<string | null>(null);
  /** unreachable state 의 attempt counter. start = 1, 매 retry += 1. */
  attempt = $state<number>(0);

  #controller: AbortController | null = null;

  /**
   * 본 화면 mount 게이트. ADR-0019 D5.4 + plan-0008 §4.4.
   *
   * - 'idle' = hint 없거나 사용자 cancel 후 — workspaceSwitcher 가 mount 결정.
   * - 'success' = 정상 reattach 후 본 화면 mount 허용.
   * - 그 외 = ReconnectModal 만 mount, 본 화면 차단.
   */
  canMountApp = $derived(this.state === 'success' || this.state === 'idle');

  async start(name: string): Promise<void> {
    this.attemptName = name;
    this.attempt = 1;
    this.state = 'loading';
    this.error = null;
    await this.#run(name);
  }

  async retry(): Promise<void> {
    if (this.attemptName === null) return;
    this.attempt += 1;
    this.state = 'loading';
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
    this.state = 'idle';
    this.attemptName = null;
    this.error = null;
    this.attempt = 0;
    sessionStorageHint.clear();
  }

  /** attemptReattach 200 분기에서 호출 — sessionStore 가 이미 active/layout set. */
  markSuccess(): void {
    this.state = 'success';
  }

  async #run(name: string): Promise<void> {
    this.#controller?.abort();
    this.#controller = new AbortController();
    const signal = this.#controller.signal;
    const result = await sessionStore.attemptReattach(name, signal);
    if (signal.aborted) return; // cancel 됨 — state 변경 안 함
    switch (result.kind) {
      case 'success':
        this.markSuccess();
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
