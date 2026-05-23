// zStore — Z-index mutation 의 4 액션 (ADR-0024 D2 + 2026-05-22 amend D9~D17).
//
// 정본:
// - ADR-0024 신 D3' (group implicit consecutive z-range, atomic block 모델)
// - ADR-0024 D9 (multi-z atomic block batch) + D11 (boundary disabled) + D12 (block swap)
// - plan-0012 §3.1 A.5 (array signature + atomic block + boundary + normalize)
// - ADR-0028 D12 (applyMutation 단일 entry + priorSnapshot rollback)
//
// 동작 (super-block 모델):
//   - bringToFront(ids):  M atomic block 들 → 부모 z-range 의 top 으로 묶음 이동.
//                         M 의 상대 z 순서 보존.
//   - sendToBack(ids):    동일하게 bottom.
//   - bringForward(ids):  super-block 이 다음 non-M sibling block 과 swap.
//   - sendBackward(ids):  이전 non-M sibling block 과 swap.
//
// Same-parent 제약 (plan-0012 §3.1 A.5): ids 의 모든 block 의 parent_id 가 같아야
// 함. 다른 부모 mix 면 본 store 는 noop (boundary check 동일하게 false 반환).
//
// 모든 mutation 후 자손 z 는 *consecutive integer 정합* (ADR-0024 신 D3' invariant).
// 4 method 가 `normalizeLayout` 통해 자동 정합.
//
// Mutate 후 caller 가 별도 PUT 호출 안 함 — 본 store 가 `optimisticMutation` 으로
// 직접 store + BE 동기 + history capture (ADR-0028 D12 단일 entry).

import { sessionStore } from './sessionStore.svelte';
import { applyZOperation, canApplyZOp, type ZOperation } from './zSpace';

class ZStore {
  /** ADR-0024 D9 — M atomic block 들을 부모 z-range 의 top 으로 묶음 이동. */
  bringToFront(ids: readonly string[]): void {
    this.#mutate(ids, 'front', 'Bring to front');
  }

  /** ADR-0024 D9 — bottom 으로 묶음 이동. */
  sendToBack(ids: readonly string[]): void {
    this.#mutate(ids, 'back', 'Send to back');
  }

  /** ADR-0024 D12 — super-block 이 다음 non-M sibling block 과 swap. */
  bringForward(ids: readonly string[]): void {
    this.#mutate(ids, 'forward', 'Bring forward');
  }

  /** ADR-0024 D12 — 이전 non-M sibling block 과 swap. */
  sendBackward(ids: readonly string[]): void {
    this.#mutate(ids, 'backward', 'Send backward');
  }

  /**
   * ADR-0024 D11 — boundary disabled state.
   *
   * 사용 위치: ContextMenu / Inspector 의 entry disabled 결정 (entry 회색 + tooltip
   * — ADR-0010 O5). Keyboard path 는 boundary 도달 시 silent noop.
   *
   * `false` 의 조건:
   * - ids 가 비었거나 모두 layout 에 부재
   * - ids 의 block 들이 *서로 다른 부모* (mixed parent — same-parent 제약)
   * - 부모 안 M 외 sibling 이 없음 (이동할 곳 없음)
   * - 이미 boundary (forward → top, backward → bottom) 도달
   */
  canBringToFront(ids: readonly string[]): boolean {
    return this.#can(ids, 'front');
  }
  canSendToBack(ids: readonly string[]): boolean {
    return this.#can(ids, 'back');
  }
  canBringForward(ids: readonly string[]): boolean {
    return this.#can(ids, 'forward');
  }
  canSendBackward(ids: readonly string[]): boolean {
    return this.#can(ids, 'backward');
  }

  #mutate(ids: readonly string[], op: ZOperation, label: string): void {
    if (ids.length === 0) return;
    const idsCopy = [...ids];
    // optimisticMutation: items.z 즉시 store 반영 + PUT 실패 시 priorSnapshot 으로
    // 자동 rollback. z mutation 은 1-shot commit 이라 optimistic 패턴 자연.
    void sessionStore.optimisticMutation(
      (cur) => {
        const next = applyZOperation(cur, idsCopy, op);
        return next ?? cur;
      },
      {
        abortMessage: 'Z order change aborted — session reconnect failed.',
        failMessage: `${label} failed`,
      },
    );
  }

  #can(ids: readonly string[], op: ZOperation): boolean {
    if (ids.length === 0) return false;
    return canApplyZOp(sessionStore.layoutSnapshot(), ids, op);
  }
}

export const zStore = new ZStore();
