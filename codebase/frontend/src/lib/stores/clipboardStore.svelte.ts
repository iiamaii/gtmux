// ClipboardStore — FE in-memory canvas/text app clipboard (ADR-0030).
//
// 정본:
// - ADR-0030 D1 — FE-only, page lifetime (reload 시 손실)
// - ADR-0030 D4 — paste 좌표 = bounding-box top-left + (24, 24) * pasteCount
// - ADR-0030 D5 — Cut = Copy + Delete, caller 책임 분리
// - ADR-0030 D8 — multi-clipboard = array (다중 선택 한번에 보관)
// - ADR-0030 D12 amend ③ — Group entity + 자손 sub-tree 동시 보관 (2026-05-25)
// - ADR-0030 D13 — plain text clipboard 와 canvas item clipboard 의 union
//
// browser clipboard API 미사용 (D1) 은 canvas item payload 에만 해당한다.
// plain text 는 OS clipboard 와 의미가 맞으므로 D13 에서 text kind 로 통합한다.

import type { CanvasItem } from '$lib/types/canvas';
import type { Group } from '$lib/types/group';

const PASTE_OFFSET = 24;

export interface ClipboardPayload {
  /** Top-level + 자손 items (group 의 자손 panel 포함). */
  items: readonly CanvasItem[];
  /** Top-level + nested groups (D12.1 의 sub-tree). */
  groups: readonly Group[];
}

export type ClipboardKind = 'empty' | 'canvas' | 'text';

class ClipboardStore {
  #kind = $state<ClipboardKind>('empty');

  /** ADR-0030 D8 — 다중 선택 copy 의 array 보관. 매 copy/cut 마다 replace. */
  #entries = $state<CanvasItem[]>([]);

  /** ADR-0030 D12.1 — group sub-tree 의 group entity 들 (자손 group 포함). */
  #groups = $state<Group[]>([]);

  /** ADR-0030 D13 — plain text snapshot. */
  #text = $state('');

  /** 연속 paste 의 누적 offset count. 매 copy/cut 마다 0 으로 reset. */
  #pasteCount = $state(0);

  readonly hasItems = $derived(this.#entries.length > 0 || this.#groups.length > 0);
  readonly hasText = $derived(this.#text.trim().length > 0);
  readonly canPaste = $derived(
    this.#kind === 'text'
      ? this.#text.trim().length > 0
      : this.#kind === 'canvas' && (this.#entries.length > 0 || this.#groups.length > 0),
  );

  get kind(): ClipboardKind {
    return this.#kind;
  }

  get entries(): readonly CanvasItem[] {
    return this.#entries;
  }

  get groups(): readonly Group[] {
    return this.#groups;
  }

  get text(): string {
    return this.#text;
  }

  /** Copy — selection 의 deep-clone snapshot 보관. */
  copy(payload: ClipboardPayload): void {
    if (payload.items.length === 0 && payload.groups.length === 0) return;
    this.#entries = payload.items.map(snapshotItem);
    this.#groups = payload.groups.map(snapshotGroup);
    this.#kind = 'canvas';
    this.#pasteCount = 0;
  }

  /** Cut = Copy + (caller 가 별도 applyDeletion). clipboard 갱신만 책임. */
  cut(payload: ClipboardPayload): void {
    this.copy(payload);
  }

  /** ADR-0030 D13 — plain text copy/cut snapshot. */
  copyText(text: string): boolean {
    if (text.trim().length === 0) return false;
    this.#text = text;
    this.#kind = 'text';
    this.#pasteCount = 0;
    return true;
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

  /**
   * Text paste 는 원본 bbox 가 없으므로 첫 paste 는 anchor 그대로, 이후 paste
   * 부터 Figma-style cascade offset 을 적용한다 (ADR-0030 D13.3).
   */
  consumeTextPasteOffset(): { dx: number; dy: number } {
    const out = {
      dx: PASTE_OFFSET * this.#pasteCount,
      dy: PASTE_OFFSET * this.#pasteCount,
    };
    this.#pasteCount += 1;
    return out;
  }

  /** Test / debug — clipboard 비우기. */
  clear(): void {
    this.#kind = 'empty';
    this.#entries = [];
    this.#groups = [];
    this.#text = '';
    this.#pasteCount = 0;
  }
}

/** Svelte reactive proxy 풀고 structuredClone 으로 deep copy. */
function snapshotItem(item: CanvasItem): CanvasItem {
  return structuredClone($state.snapshot(item)) as CanvasItem;
}

function snapshotGroup(group: Group): Group {
  return structuredClone($state.snapshot(group)) as Group;
}

export const clipboardStore = new ClipboardStore();
