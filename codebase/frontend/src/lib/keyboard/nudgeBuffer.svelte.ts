// NudgeBuffer — Arrow nudge 의 250ms idle debounce (ADR-0017 D6 amend ⑧).
//
// 정본:
// - ADR-0017 D6 amend ⑥ (=⑧ 번 amend) — Nudge history grain = 250ms idle debounce
// - ADR-0028 D11.1 — Optimistic update + priorSnapshot 으로 실패 시 store rollback
//
// 동작:
// 1. 매 keydown 에 `tick(ids, dx, dy)` 호출 — sessionStore.items 의 좌표를 즉시 갱신 (DOM optimistic).
// 2. 250ms idle 시 flush — applyMutation PUT (history capture + priorSnapshot rollback 계약).
// 3. 같은 session 안의 다음 tick 은 같은 batch 로 누적; session 전환 시 cancel.

import type { CanvasLayout, CanvasItem, LineItem } from '$lib/types/canvas';
import { sessionStore } from '$lib/stores/sessionStore.svelte';

const NUDGE_FLUSH_MS = 250;

class NudgeBuffer {
  #timer: ReturnType<typeof setTimeout> | null = null;
  #priorSnapshot: CanvasLayout | null = null;
  #activeIds = new Set<string>();
  #sessionName: string | null = null;

  /** 한 번의 nudge keystroke — selected ids 의 좌표 optimistic 갱신 + flush 예약. */
  tick(selectedIds: readonly string[], dx: number, dy: number): void {
    const active = sessionStore.active;
    if (active === null) return;
    if (selectedIds.length === 0) return;

    if (this.#sessionName !== null && this.#sessionName !== active.name) {
      // session switched 도중 — 이전 session 의 buffer drop (BE 미반영, 회귀 모니터).
      this.cancel();
    }

    if (this.#priorSnapshot === null) {
      this.#priorSnapshot = sessionStore.layoutSnapshot();
      this.#sessionName = active.name;
    }

    for (const id of selectedIds) {
      const it = sessionStore.items.get(id);
      if (it === undefined) continue;
      if (it.locked) continue;
      this.#activeIds.add(id);
      const next = { ...it, x: it.x + dx, y: it.y + dy } as CanvasItem;
      if (next.type === 'line') {
        const lineSrc = it as LineItem;
        const lineOut = next as LineItem;
        lineOut.x2 = lineSrc.x2 + dx;
        lineOut.y2 = lineSrc.y2 + dy;
      }
      sessionStore.items.set(id, next);
    }

    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.flush(), NUDGE_FLUSH_MS);
  }

  /** Buffer drop without flush — session 전환 시 internal 호출. */
  cancel(): void {
    if (this.#timer !== null) {
      clearTimeout(this.#timer);
      this.#timer = null;
    }
    this.#priorSnapshot = null;
    this.#activeIds.clear();
    this.#sessionName = null;
  }

  /** 250ms idle 후 발화. store 의 optimistic 좌표를 BE 에 PUT + history capture. */
  flush(): void {
    if (this.#priorSnapshot === null) return;
    if (this.#sessionName === null) return;

    const prior = this.#priorSnapshot;
    const ids = Array.from(this.#activeIds);
    this.#priorSnapshot = null;
    this.#activeIds.clear();
    this.#sessionName = null;
    this.#timer = null;

    void sessionStore.applyMutation(
      (cur: CanvasLayout) => {
        const updated = cur.items.map((it) => {
          if (!ids.includes(it.id)) return it;
          const fresh = sessionStore.items.get(it.id);
          if (fresh === undefined) return it;
          const next = { ...it, x: fresh.x, y: fresh.y } as CanvasItem;
          if (next.type === 'line') {
            const lineOut = next as LineItem;
            lineOut.x2 = (fresh as LineItem).x2;
            lineOut.y2 = (fresh as LineItem).y2;
          }
          return next;
        });
        return { ...cur, items: updated };
      },
      {
        priorSnapshot: prior,
        failMessage: 'Nudge failed — reverted to previous position.',
      },
    );
  }
}

export const nudgeBuffer = new NudgeBuffer();
