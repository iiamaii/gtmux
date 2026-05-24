// HistoryStore — undo / redo stack for canvas layout mutation (ADR-0028).
//
// 정본:
// - ADR-0028 D1 scope: layout mutation only
// - ADR-0028 D2 client memory only (reload 시 손실)
// - ADR-0028 D4 per-session history
// - ADR-0028 D5 capacity 50, FIFO
// - ADR-0028 D7 full CanvasLayout snapshot
// - ADR-0028 D9 etag mismatch / unmatched terminal → reset
// - ADR-0028 D10 new mutation drops redo stack
// - ADR-0028 D12 sessionStore.applyMutation 가 capture 의 단일 entry
//
// Reactivity:
// - canUndo / canRedo 가 $derived 라 toolbar UI 등에서 자동 disable.
// - 각 session 의 stack length 가 active session 의 그것일 때만 derived 갱신
//   되도록 #undoLen / #redoLen 를 $state 로 mirror.

import type { CanvasLayout } from '$lib/types/canvas';

const STACK_CAPACITY = 50;

interface SessionHistory {
  /** PRE-mutation snapshots. push 마다 capacity 초과 시 oldest evict (FIFO). */
  undoStack: CanvasLayout[];
  /** Undo 된 state — 새 mutation 시 drop (D10). */
  redoStack: CanvasLayout[];
}

class HistoryStore {
  /** Per-session stack. session 전환 시 stack 은 reset, 다른 session 의 stack 은 보존 안 함 (D4). */
  #histories = new Map<string, SessionHistory>();

  /** 현재 active session name — sessionStore.setActiveSession / clear 가 set. */
  #activeName: string | null = $state(null);

  /** Reactive length mirror — derived 가 의존. */
  #undoLen = $state(0);
  #redoLen = $state(0);

  /** Active session 의 undo 가능 여부. toolbar button disable 등에 사용. */
  readonly canUndo = $derived(this.#activeName !== null && this.#undoLen > 0);
  readonly canRedo = $derived(this.#activeName !== null && this.#redoLen > 0);

  /**
   * Active session 변경. setActiveSession / clear / switchSession 시 호출.
   *
   * D4 정합 — 다른 session 의 stack 은 *보존 안 함*. 호출 시점에 이전 session
   * 의 stack 도 drop (메모리 절약). 같은 session 재진입 시 빈 stack 으로 재시작.
   */
  setActive(name: string | null): void {
    // D4 — 이전 session 의 stack drop (per-session, 보존 안 함).
    if (this.#activeName !== null && this.#activeName !== name) {
      this.#histories.delete(this.#activeName);
    }
    this.#activeName = name;
    if (name === null) {
      this.#undoLen = 0;
      this.#redoLen = 0;
      return;
    }
    if (!this.#histories.has(name)) {
      this.#histories.set(name, { undoStack: [], redoStack: [] });
    }
    this.#syncLen();
  }

  /**
   * Mutation 직전의 PRE-state snapshot push. new mutation → redo stack drop (D10).
   *
   * 호출 시점은 sessionStore.applyMutation 의 PRE-mutation 단계. 직접 호출은
   * 권장하지 않음 — entry point 통일.
   */
  capture(sessionName: string, snapshot: CanvasLayout): void {
    const h = this.#ensure(sessionName);
    h.undoStack.push(snapshot);
    if (h.undoStack.length > STACK_CAPACITY) h.undoStack.shift();
    h.redoStack.length = 0;
    if (sessionName === this.#activeName) this.#syncLen();
  }

  /**
   * Undo 1 step — PRE-state 를 반환, 동시에 caller 가 보낸 current snapshot 을
   * redo 에 push. caller 책임으로 PRE-state 를 PUT.
   *
   * 반환 null 이면 stack 비어 있음 (no-op).
   */
  popUndo(sessionName: string, currentSnapshot: CanvasLayout): CanvasLayout | null {
    const h = this.#histories.get(sessionName);
    if (h === undefined || h.undoStack.length === 0) return null;
    const pre = h.undoStack.pop()!;
    h.redoStack.push(currentSnapshot);
    if (h.redoStack.length > STACK_CAPACITY) h.redoStack.shift();
    if (sessionName === this.#activeName) this.#syncLen();
    return pre;
  }

  /**
   * PRE-state 만 보고 stack 은 변경하지 않음. caller 가 *비동기 확인 dialog*
   * 등으로 user input 대기 후 popUndo 를 결정해야 할 때 사용 (ADR-0030 D12.8
   * amend ④ — paste 의 undo 시 terminal kill confirm).
   *
   * 반환 null 이면 stack 비어 있음.
   */
  peekUndo(sessionName: string): CanvasLayout | null {
    const h = this.#histories.get(sessionName);
    if (h === undefined || h.undoStack.length === 0) return null;
    return h.undoStack[h.undoStack.length - 1] ?? null;
  }

  /**
   * Redo 1 step — caller 가 보낸 current snapshot 을 undo 에 push (redo drop X).
   *
   * 반환 null 이면 redo stack 비어 있음.
   */
  popRedo(sessionName: string, currentSnapshot: CanvasLayout): CanvasLayout | null {
    const h = this.#histories.get(sessionName);
    if (h === undefined || h.redoStack.length === 0) return null;
    const next = h.redoStack.pop()!;
    h.undoStack.push(currentSnapshot);
    if (h.undoStack.length > STACK_CAPACITY) h.undoStack.shift();
    if (sessionName === this.#activeName) this.#syncLen();
    return next;
  }

  /**
   * 양 stack reset — D9 etag mismatch / D1.2 terminal unmatched 시 호출.
   * Toast 는 caller 책임.
   */
  reset(sessionName: string): void {
    const h = this.#histories.get(sessionName);
    if (h !== undefined) {
      h.undoStack.length = 0;
      h.redoStack.length = 0;
      if (sessionName === this.#activeName) this.#syncLen();
    }
  }

  #ensure(sessionName: string): SessionHistory {
    let h = this.#histories.get(sessionName);
    if (h === undefined) {
      h = { undoStack: [], redoStack: [] };
      this.#histories.set(sessionName, h);
    }
    return h;
  }

  #syncLen(): void {
    if (this.#activeName === null) {
      this.#undoLen = 0;
      this.#redoLen = 0;
      return;
    }
    const h = this.#histories.get(this.#activeName);
    this.#undoLen = h?.undoStack.length ?? 0;
    this.#redoLen = h?.redoStack.length ?? 0;
  }
}

export const historyStore = new HistoryStore();
