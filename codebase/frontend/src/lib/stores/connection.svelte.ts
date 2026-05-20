// ConnectionStore — WS 상태 + Reconnect state machine (D21 c2/c3, R8 §F6).
//
// 책임:
// - WS lifecycle 상태(`state`, `attempt`)를 단일 진실로 보관 (MT-3 D13).
// - 끊김 시점(`disconnectedAt`)을 기록해 FE-3 ReconnectBanner가
//   D21 c2의 1s grace를 derive할 수 있게 한다.
// - close code/reason을 보관해 banner가 1008/1011/4001 등 분기 메시지를 표시.
// - zombie pane 집합(D21 c3 = c4 mirror)을 보관 — FE-1이 `pane-died`
//   NOTIFY_MIRROR(0x07)를 받아 setter를 호출하면 banner가 badge로 표시.
// - slow pane 집합(SSoT §2.3 `slow-pane` kind, ADR-0001 D10) — UI panel header
//   "느림" 배지의 1차 진실. dispatcher가 `slow-pane` NOTIFY_MIRROR(0x07) 수신
//   시 markSlow / clearSlow 호출.
//
// 시간 단위:
// - `disconnectedAt`은 `performance.now()` 기준 monotonic ms.
//   Date.now()는 wall-clock jump(NTP/manual)에 약해 1s grace 측정에
//   부적합 — monotonic이 필수.

import { SvelteSet } from 'svelte/reactivity';

export type WsState = 'connecting' | 'open' | 'closing' | 'closed' | 'reconnecting';

class ConnectionStore {
  state = $state<WsState>('connecting');
  attempt = $state<number>(0);
  // performance.now() 기준 끊김 시점 (monotonic). null = 끊김 없음/정상 연결.
  disconnectedAt = $state<number | null>(null);
  // WS close frame 정보 (wire-protocol.md §3: 1000/1008/1011/4001 등).
  closeCode = $state<number | null>(null);
  closeReason = $state<string | null>(null);
  // zombie pane (`pane_dead = 1`) tmux pane id 정수 집합. FE-1과 협업.
  zombiePaneIds = $state<number[]>([]);
  // slow pane (`%pause` mirror) tmux pane id 정수 집합. SvelteSet 으로 entry-level
  // reactivity — panel header badge 가 단일 pane 의 set/clear 만 dirty marking.
  slowPaneIds = $state(new SvelteSet<number>());

  setState(s: WsState): void {
    // grace timer 시작/리셋: 끊김 계열 진입 시 최초 1회만 timestamp 기록.
    if (s === 'reconnecting' || s === 'closed' || s === 'closing') {
      if (this.disconnectedAt === null) {
        this.disconnectedAt = performance.now();
      }
    } else if (s === 'open') {
      // 재연결 성공 시 timer 해제 → banner derived가 즉시 사라짐.
      this.disconnectedAt = null;
      this.closeCode = null;
      this.closeReason = null;
      this.attempt = 0;
    }
    // 'connecting'(최초 부팅)는 disconnectedAt 변화 없음.
    this.state = s;
  }

  setCloseInfo(code: number, reason: string): void {
    this.closeCode = code;
    this.closeReason = reason;
  }

  incrementAttempt(): void {
    this.attempt += 1;
  }

  markZombie(paneIds: number[]): void {
    this.zombiePaneIds = paneIds;
  }

  clearZombie(): void {
    this.zombiePaneIds = [];
  }

  /** SSoT §2.3 `slow-pane` 미러 — append-only set 에 paneId 추가 (idempotent). */
  markSlow(paneId: number): void {
    this.slowPaneIds.add(paneId);
  }

  /** Pane 이 정상 throughput 으로 복귀했음을 시그널 — 현재 발행 경로 없음 (P1+). */
  clearSlow(paneId: number): void {
    this.slowPaneIds.delete(paneId);
  }
}

export const connectionStore = new ConnectionStore();
