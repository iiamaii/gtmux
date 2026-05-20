// ClipboardStore — FE in-memory canvas item clipboard (ADR-0030).
//
// 정본:
// - ADR-0030 D1 — FE-only, page lifetime (reload 시 손실)
// - ADR-0030 D4 — paste 좌표 = bounding-box top-left + (24, 24) * pasteCount
// - ADR-0030 D5 — Cut = Copy + Delete, caller 책임 분리
// - ADR-0030 D8 — multi-clipboard = array (다중 선택 한번에 보관)
//
// browser clipboard API 미사용 (D1) — canvas item schema 는 cross-app 의미 없음 +
// terminal 의 server-side reference 는 browser clipboard 로 전달 불가.

import type { CanvasItem } from '$lib/types/canvas';

const PASTE_OFFSET = 24;

class ClipboardStore {
  /** ADR-0030 D8 — 다중 선택 copy 의 array 보관. 매 copy/cut 마다 replace. */
  #entries = $state<CanvasItem[]>([]);

  /** 연속 paste 의 누적 offset count. 매 copy/cut 마다 0 으로 reset. */
  #pasteCount = $state(0);

  readonly hasItems = $derived(this.#entries.length > 0);

  get entries(): readonly CanvasItem[] {
    return this.#entries;
  }

  /** Copy — selection 의 deep-clone snapshot 보관. */
  copy(items: readonly CanvasItem[]): void {
    if (items.length === 0) return;
    this.#entries = items.map(snapshotItem);
    this.#pasteCount = 0;
  }

  /** Cut = Copy + (caller 가 별도 applyDeletion). clipboard 갱신만 책임. */
  cut(items: readonly CanvasItem[]): void {
    this.copy(items);
  }

  /**
   * Paste 호출 시 누적 offset 반환. D4 — 연속 paste 마다 (24, 24) 누적.
   * 호출자가 새 좌표 계산에 사용.
   */
  consumePasteOffset(): { dx: number; dy: number } {
    this.#pasteCount += 1;
    return {
      dx: PASTE_OFFSET * this.#pasteCount,
      dy: PASTE_OFFSET * this.#pasteCount,
    };
  }

  /** Test / debug — clipboard 비우기. */
  clear(): void {
    this.#entries = [];
    this.#pasteCount = 0;
  }
}

/** Svelte reactive proxy 풀고 structuredClone 으로 deep copy. */
function snapshotItem(item: CanvasItem): CanvasItem {
  return structuredClone($state.snapshot(item)) as CanvasItem;
}

export const clipboardStore = new ClipboardStore();
