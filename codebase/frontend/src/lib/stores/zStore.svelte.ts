// zStore — Z-index mutation 의 4 액션 (ADR-0024 D2).
//
// 정본:
// - ADR-0024 D2 (Bring/Send to front/back, Bring/Send forward/backward)
// - ADR-0018 D3 (z field), D7 (신규 item z = max(z) + 1)
// - plan-0007 §14 FE-7 + ContextMenu ARRANGE
//
// 동작:
//   - Bring to front:    z = max(other.z) + 1
//   - Send to back:      z = min(other.z) - 1
//   - Bring forward:     z 가 자신보다 큰 것 중 *가장 작은* 것과 swap
//   - Send backward:     z 가 자신보다 작은 것 중 *가장 큰* 것과 swap
//
// Group 은 z 없음 (ADR-0024 D3) — group sibling 도 flat global z space 에서 비교.
// Tree drag reorder 와는 무관 (organization 만).
//
// Mutate 후 caller 가 mutateLayout PUT 호출 책임. 본 store 는 *순수 계산* 만.

import { sessionStore } from './sessionStore.svelte';
import type { CanvasItem, CanvasLayout } from '$lib/types/canvas';

class ZStore {
  /**
   * Bring to front — z = max(other.z) + 1.
   * 자기 자신만 있는 경우 z 그대로 (이미 단독 최상위).
   */
  bringToFront(id: string): void {
    this.#mutate(id, (item, others) => {
      const maxZ = others.reduce((m, it) => (it.z > m ? it.z : m), Number.NEGATIVE_INFINITY);
      if (!isFinite(maxZ)) return item;
      return { ...item, z: maxZ + 1 };
    });
  }

  /** Send to back — z = min(other.z) - 1. */
  sendToBack(id: string): void {
    this.#mutate(id, (item, others) => {
      const minZ = others.reduce((m, it) => (it.z < m ? it.z : m), Number.POSITIVE_INFINITY);
      if (!isFinite(minZ)) return item;
      return { ...item, z: minZ - 1 };
    });
  }

  /** Bring forward — z 가 자신보다 큰 것 중 *가장 작은* 것과 swap. */
  bringForward(id: string): void {
    this.#swap(id, 'up');
  }

  /** Send backward — z 가 자신보다 작은 것 중 *가장 큰* 것과 swap. */
  sendBackward(id: string): void {
    this.#swap(id, 'down');
  }

  #swap(id: string, dir: 'up' | 'down'): void {
    const item = sessionStore.items.get(id);
    if (item === undefined) return;
    const items = Array.from(sessionStore.items.values());
    const candidates = items.filter((it) => it.id !== id);
    // dir=up: 자신보다 큰 z 중 최소 / dir=down: 자신보다 작은 z 중 최대
    const partner =
      dir === 'up'
        ? candidates
            .filter((it) => it.z > item.z)
            .reduce<CanvasItem | null>(
              (best, it) =>
                best === null || it.z < best.z ? it : best,
              null,
            )
        : candidates
            .filter((it) => it.z < item.z)
            .reduce<CanvasItem | null>(
              (best, it) =>
                best === null || it.z > best.z ? it : best,
              null,
            );
    if (partner === null) return; // 더 이상 swap 대상 없음 (이미 극단)
    const movedSelf = { ...item, z: partner.z };
    const movedPartner = { ...partner, z: item.z };
    this.#applyTwo(movedSelf, movedPartner);
  }

  #mutate(
    id: string,
    fn: (item: CanvasItem, others: CanvasItem[]) => CanvasItem,
  ): void {
    const item = sessionStore.items.get(id);
    if (item === undefined) return;
    const others = Array.from(sessionStore.items.values()).filter(
      (it) => it.id !== id,
    );
    const next = fn(item, others);
    if (next.z === item.z) return; // no-op
    sessionStore.items.set(id, next);
    this.#commit((cur) => ({
      ...cur,
      items: cur.items.map((it) => (it.id === id ? next : it)),
    }));
  }

  #applyTwo(a: CanvasItem, b: CanvasItem): void {
    sessionStore.items.set(a.id, a);
    sessionStore.items.set(b.id, b);
    this.#commit((cur) => ({
      ...cur,
      items: cur.items.map((it) =>
        it.id === a.id ? a : it.id === b.id ? b : it,
      ),
    }));
  }

  #commit(mutator: (cur: CanvasLayout) => CanvasLayout): void {
    void sessionStore.applyMutation(mutator, {
      abortMessage: 'Z order change aborted — session reconnect failed.',
      failMessage: 'Z order change failed',
    });
  }
}

export const zStore = new ZStore();
