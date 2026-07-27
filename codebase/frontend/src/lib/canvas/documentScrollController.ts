// documentScrollController — thin DOM glue binding a document scroll container
// to the shared anchor store + durable saver.
//
// 정본: ADR-0056 D1/D2/D4. Pure anchor math lives in documentAnchor.ts; this
// module only reads DOM geometry (block children / `[data-line]` rows) and
// wires the scroll listener + restore. Components drive it from an `$effect`
// that passes the *current* scroll container for the active view — re-running
// (re-attaching) on view switch / content load.

import {
  measureAnchorIndex,
  restoreScrollTop,
  keyToArrayIndex,
  type AnchorUnit,
  type DocViewAnchor,
} from './documentAnchor';
import { documentScrollStore } from '$lib/stores/documentScroll.svelte';
import {
  buildDocumentViewState,
  scheduleDocumentViewStateSave,
} from '$lib/stores/documentViewStateSaver';

export type ScrollSurfaceKind = 'block' | 'line';

export interface AttachDocumentScrollOptions {
  /** The live scroll container for the active view (overflow:auto element). */
  el: HTMLElement;
  /** `line` = source view (`.cv-line[data-line]`), `block` = rendered markdown. */
  kind: ScrollSurfaceKind;
  itemId: string;
  /** Durable seed from `item.view_state.anchor` (used only when the live store
   *  has no entry for this item). */
  seedAnchor: DocViewAnchor | null;
  /** Live render mode — read at save time so a debounced save carries the
   *  current mode (ADR-0056 D3). */
  getMode: () => 'rendered' | 'source';
}

/**
 * Collect the ordered geometry of the container's anchor units + their keys.
 * - block: direct children of `.dmv-content` (fallback: container children);
 *   key = 0-based array position (ADR-0056 D2 block index).
 * - line: `.cv-line[data-line]` rows; key = 1-based `data-line`.
 */
function collectUnits(
  el: HTMLElement,
  kind: ScrollSurfaceKind,
): { units: AnchorUnit[]; keys: number[] } {
  // Content-space origin: container top in viewport coords minus its scrollTop.
  const base = el.getBoundingClientRect().top - el.scrollTop;
  let rows: Element[];
  if (kind === 'line') {
    rows = Array.from(el.querySelectorAll('.cv-line[data-line]'));
  } else {
    const content = el.querySelector('.dmv-content') ?? el;
    rows = Array.from(content.children);
  }
  const units: AnchorUnit[] = [];
  const keys: number[] = [];
  rows.forEach((row, i) => {
    const r = row.getBoundingClientRect();
    units.push({ top: r.top - base, height: r.height });
    if (kind === 'line') {
      const ln = Number.parseInt((row as HTMLElement).dataset.line ?? '', 10);
      keys.push(Number.isFinite(ln) ? ln : i + 1);
    } else {
      keys.push(i);
    }
  });
  return { units, keys };
}

/**
 * Wire scroll persistence for one container. Returns a cleanup that removes the
 * listener. Restore (store → seedAnchor) runs once on attach behind the
 * restore-in-progress guard so the programmatic scroll doesn't feed back into a
 * save (ADR-0056 D4).
 */
export function attachDocumentScroll(opts: AttachDocumentScrollOptions): () => void {
  const { el, kind, itemId, seedAnchor, getMode } = opts;

  // ADR-0056 D1 seed priority: live store → durable seed (item.view_state).
  const anchor = documentScrollStore.get(itemId) ?? seedAnchor;
  if (anchor !== null && anchor.kind === kind) {
    // Promote the (possibly durable-seeded) anchor into the live store so an
    // immediate flush before the first user scroll — e.g. maximizing right
    // after F5 — persists the restored position instead of wiping it (D4).
    documentScrollStore.set(itemId, anchor);
    documentScrollStore.beginRestore(itemId);
    // rAF — freshly rendered content (markdown {@html} / async-highlighted
    // source rows) needs one frame to lay out before geometry is measurable.
    requestAnimationFrame(() => {
      const { units, keys } = collectUnits(el, kind);
      const arrayIndex = keyToArrayIndex(keys, anchor.index);
      el.scrollTop = restoreScrollTop(arrayIndex, anchor.frac, units);
      // Release the guard a frame later so the restore's own scroll event
      // (dispatched async) is swallowed.
      requestAnimationFrame(() => documentScrollStore.endRestore(itemId));
    });
  }

  const onScroll = (): void => {
    if (documentScrollStore.isRestoring(itemId)) return;
    const { units, keys } = collectUnits(el, kind);
    const m = measureAnchorIndex(el.scrollTop, units);
    if (m === null) return;
    const index = keys[m.arrayIndex] ?? (kind === 'line' ? 1 : 0);
    const next: DocViewAnchor = { kind, index, frac: m.frac };
    documentScrollStore.set(itemId, next);
    scheduleDocumentViewStateSave(itemId, buildDocumentViewState(getMode(), next));
  };
  el.addEventListener('scroll', onScroll, { passive: true });

  return () => {
    el.removeEventListener('scroll', onScroll);
  };
}
