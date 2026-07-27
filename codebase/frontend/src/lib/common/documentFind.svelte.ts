// DocumentFindController — per-surface in-document find state + match/DOM
// plumbing (ADR-0058 D1/D3). One instance per host component (DocumentNode /
// MaximizedItemModal / FilePreviewView); state is component-local ephemeral —
// never persisted (ADR-0058 D6, R4).
//
// The searchable viewer is discovered from the host's stable wrapper on every
// recompute (`.code-viewer` → line/col matching over the raw text,
// `.document-markdown-view` → TreeWalker text-node matching), so viewMode
// toggles and async Shiki token swaps need no per-host wiring: a
// MutationObserver on the wrapper triggers an index-preserving recompute.

import {
  clearFindHighlights,
  collectTextNodes,
  concatTextNodes,
  rangeForLineMatch,
  rangesForOffsets,
  scrollRangeIntoCenter,
  setFindHighlights,
} from './findHighlight';
import { findLineMatches, findMatches } from './textFind';

export interface DocumentFindHost {
  /** Stable wrapper that contains the (swappable) searchable viewer. */
  getRoot: () => HTMLElement | null;
  /** Raw text backing a CodeViewer surface (line/col matching, ADR-0058 D3). */
  getCodeText: () => string;
}

// Single active find across the app — opening one surface's bar closes any
// other, which also keeps the global CSS.highlights registries single-owner.
let activeController: DocumentFindController | null = null;

// Maximize/restore find handoff (ADR-0058 D1 override 2026-07-23 — find must
// SURVIVE maximize/restore). A document item's find lives in two hosts with
// two controllers (`node:<id>` canvas node ↔ `max:<id>` maximized modal). The
// host being torn down by a maximize/restore transition stashes its live
// query + current match index here; the counterpart host consumes it on mount,
// keyed by itemId so an unrelated maximize never resurrects a stale record.
// Explicit user close (Esc/×) does NOT write a record — it clears the
// controller's `open` first, so the transition sees nothing to hand off
// (explicit close = closed everywhere).
interface FindHandoff {
  itemId: string;
  query: string;
  currentIndex: number;
}
let pendingHandoff: FindHandoff | null = null;

/** Record a find handoff for `itemId` (transition teardown while open). */
export function recordFindHandoff(itemId: string, query: string, currentIndex: number): void {
  pendingHandoff = { itemId, query, currentIndex };
}

/** Consume (and clear) a pending handoff for `itemId`, or null when none. */
export function consumeFindHandoff(itemId: string): FindHandoff | null {
  if (pendingHandoff === null || pendingHandoff.itemId !== itemId) return null;
  const handoff = pendingHandoff;
  pendingHandoff = null;
  return handoff;
}

export class DocumentFindController {
  open = $state(false);
  query = $state('');
  count = $state(0);
  capped = $state(false);
  /** 0-based index of the current match (0 when there are no matches). */
  currentIndex = $state(0);

  #host: DocumentFindHost;
  #ranges: Range[] = [];
  /** Per-match `data-line` anchor for code surfaces; null on markdown. */
  #lineAnchors: Array<number | null> = [];
  #scrollContainer: HTMLElement | null = null;
  // ADR-0057/0058 amend (2026-07-24) — draft find on the CodeEditArea overlay.
  // The overlay (`.cea-overlay`) is transform-translated and non-scrolling; the
  // real scroller is its sibling `<textarea>`, so match navigation must scroll
  // the textarea (line-metric based) while the highlight paints on the overlay.
  #editTextarea: HTMLTextAreaElement | null = null;
  #observer: MutationObserver | null = null;
  #raf: number | null = null;

  constructor(host: DocumentFindHost) {
    this.#host = host;
  }

  /** Open the bar (idempotent) and recompute against the current query. */
  openBar(): void {
    if (activeController !== this) {
      activeController?.close();
      activeController = this;
    }
    if (!this.open) {
      this.open = true;
      this.#startObserver();
    }
    this.#recompute(false);
  }

  /**
   * Re-point the observer at the current root and rebuild ranges without
   * losing the query or the current match position (ADR-0058 D1 override
   * 2026-07-23 — survive maximize/restore). The host swaps the mounted surface
   * underneath an open bar (e.g. preview inline↔maximize): the old observer
   * watched the now-unmounted root and the ranges point at detached DOM. Call
   * after `tick()` once the new variant's surface has mounted. No-op when
   * closed.
   */
  retarget(): void {
    if (!this.open) return;
    this.#stopObserver();
    this.#startObserver();
    // Index-preserving so the user is not yanked back to the first match.
    this.#recompute(true);
  }

  /**
   * Open with a restored query + match index (maximize/restore handoff,
   * ADR-0058 D1 override). Recomputes against the current root, then clamps to
   * the carried index and scrolls to it. Call after `tick()` so the surface is
   * mounted.
   */
  openWith(query: string, index: number): void {
    this.openBar();
    this.setQuery(query);
    if (this.count > 0) {
      this.currentIndex = Math.min(Math.max(index, 0), this.count - 1);
      this.#applyHighlights();
      this.#scrollToCurrent();
    }
  }

  /** Close the bar; the query is kept for reopen, highlights are cleared. */
  close(): void {
    if (activeController === this) activeController = null;
    if (!this.open) return;
    this.open = false;
    this.#stopObserver();
    this.#resetMatches();
    clearFindHighlights();
  }

  /** Unmount cleanup — same as close (registries must not outlive the host). */
  destroy(): void {
    this.close();
  }

  setQuery(query: string): void {
    if (query === this.query) return;
    this.query = query;
    this.#recompute(false);
  }

  navigate(dir: 1 | -1): void {
    if (this.count === 0) return;
    // Wraps end→start / start→end.
    this.currentIndex = (this.currentIndex + dir + this.count) % this.count;
    this.#applyHighlights();
    this.#scrollToCurrent();
  }

  #resetMatches(): void {
    this.#ranges = [];
    this.#lineAnchors = [];
    this.count = 0;
    this.capped = false;
    this.currentIndex = 0;
  }

  /**
   * Recompute matches + DOM ranges + highlights. `preserveIndex` keeps the
   * current position across DOM mutations (e.g. async Shiki token swap) so
   * the user is not yanked back to the first match; query changes reset to
   * the first match and scroll to it.
   */
  #recompute(preserveIndex: boolean): void {
    const root = this.open ? this.#host.getRoot() : null;
    if (root === null || this.query.length === 0) {
      this.#resetMatches();
      clearFindHighlights();
      return;
    }

    const codeEl = root.querySelector<HTMLElement>('.code-viewer');
    // ADR-0057/0058 amend — the edit-mode overlay is a line-addressed code
    // surface too (`.cea-overlay` carries `.cv-line[data-line] [data-code]`),
    // but its scroller is the sibling textarea, not itself.
    const editOverlayEl =
      codeEl === null ? root.querySelector<HTMLElement>('.cea-overlay') : null;
    const lineSurface = codeEl ?? editOverlayEl;
    const mdEl =
      lineSurface === null ? root.querySelector<HTMLElement>('.document-markdown-view') : null;
    const prevIndex = this.currentIndex;
    this.#ranges = [];
    this.#lineAnchors = [];
    this.#editTextarea = null;
    let capped = false;

    if (lineSurface !== null) {
      // CodeViewer / edit overlay — match the raw (or draft) text per line,
      // then map onto the rendered `.cv-line[data-line]` rows (ADR-0058 D3).
      const result = findLineMatches(this.#host.getCodeText().split('\n'), this.query);
      capped = result.capped;
      for (const match of result.matches) {
        const range = rangeForLineMatch(lineSurface, match);
        if (range !== null) {
          this.#ranges.push(range);
          this.#lineAnchors.push(match.line);
        }
      }
      if (editOverlayEl !== null) {
        // Draft surface — scroll the mirroring textarea, not the overlay.
        this.#editTextarea = root.querySelector<HTMLTextAreaElement>('.cea-textarea');
      }
      this.#scrollContainer = lineSurface;
    } else if (mdEl !== null) {
      // Markdown surface — TreeWalker text nodes → concatenated haystack →
      // DOM Ranges (ADR-0058 D3).
      const nodes = collectTextNodes(mdEl);
      const result = findMatches(concatTextNodes(nodes), this.query);
      capped = result.capped;
      this.#ranges = rangesForOffsets(nodes, result.ranges);
      this.#lineAnchors = this.#ranges.map(() => null);
      this.#scrollContainer = mdEl;
    } else {
      // No searchable viewer mounted (e.g. mid view-mode swap).
      this.#resetMatches();
      clearFindHighlights();
      return;
    }

    this.capped = capped;
    this.count = this.#ranges.length;
    this.currentIndex =
      this.count === 0 ? 0 : preserveIndex ? Math.min(prevIndex, this.count - 1) : 0;
    this.#applyHighlights();
    if (!preserveIndex && this.count > 0) this.#scrollToCurrent();
  }

  #applyHighlights(): void {
    // No-op on browsers without CSS.highlights — counter/navigation keep
    // working, only the paint is skipped (ADR-0058 D3 degrade).
    setFindHighlights(this.#ranges, this.#ranges[this.currentIndex] ?? null);
  }

  #scrollToCurrent(): void {
    const container = this.#scrollContainer;
    const range = this.#ranges[this.currentIndex];
    if (container === null || range === undefined) return;
    const line = this.#lineAnchors[this.currentIndex];
    // Edit overlay — scroll the sibling textarea to the match line (the overlay
    // itself is transform-translated + non-scrolling). ADR-0057/0058 amend.
    if (this.#editTextarea !== null && line !== null && line !== undefined) {
      this.#scrollEditToLine(this.#editTextarea, line, range);
      return;
    }
    if (line !== null && line !== undefined) {
      // Code surface — vertical via the data-line anchor (ADR-0037 D7.5 /
      // ADR-0058 D3), then a horizontal nudge for matches past the right edge.
      container
        .querySelector(`.cv-line[data-line="${line}"]`)
        ?.scrollIntoView({ block: 'center' });
      const rect = range.getBoundingClientRect();
      const containerRect = container.getBoundingClientRect();
      const scale =
        container.offsetHeight > 0 ? containerRect.height / container.offsetHeight : 1;
      if (Number.isFinite(scale) && scale > 0) {
        const left = (rect.left - containerRect.left) / scale;
        if (left < 0 || left + rect.width / scale > container.clientWidth) {
          container.scrollLeft += left - container.clientWidth / 2;
        }
      }
      return;
    }
    // Markdown surface — scroll by the Range rect (ADR-0058 D3).
    scrollRangeIntoCenter(container, range);
  }

  /**
   * Scroll a CodeEditArea textarea so its 1-based `line` sits centered, then
   * nudge horizontally so the matched column is visible. Vertical uses the
   * shared line-height metric (textarea + overlay are metric-identical);
   * horizontal reuses the on-screen Range rect (the overlay mirrors the
   * textarea, so their content-x coincides). ADR-0057/0058 amend (2026-07-24).
   */
  #scrollEditToLine(ta: HTMLTextAreaElement, line: number, range: Range): void {
    const cs = getComputedStyle(ta);
    let lineH = parseFloat(cs.lineHeight);
    if (!Number.isFinite(lineH) || lineH <= 0) {
      const fontSize = parseFloat(cs.fontSize);
      lineH = (Number.isFinite(fontSize) ? fontSize : 12) * 1.6;
    }
    const padTop = parseFloat(cs.paddingTop) || 0;
    const targetTop = padTop + (line - 1) * lineH;
    const desired = targetTop - ta.clientHeight / 2 + lineH / 2;
    const maxTop = Math.max(0, ta.scrollHeight - ta.clientHeight);
    ta.scrollTop = Math.max(0, Math.min(desired, maxTop));
    // Setting scrollTop fires the textarea's scroll handler which re-syncs the
    // overlay transform; the Range's horizontal position is unaffected by the
    // vertical scroll, so reading it here is safe.
    const rect = range.getBoundingClientRect();
    const taRect = ta.getBoundingClientRect();
    const left = rect.left - taRect.left;
    if (left < 0 || left + rect.width > ta.clientWidth) {
      ta.scrollLeft += left - ta.clientWidth / 2;
    }
  }

  // The observer watches the stable wrapper so re-renders inside it (view
  // mode swap, async highlight, content edit echo) rebuild the ranges. The
  // Custom Highlight API never mutates the DOM, so re-applying highlights
  // cannot re-trigger the observer.
  #startObserver(): void {
    if (typeof MutationObserver === 'undefined') return;
    const root = this.#host.getRoot();
    if (root === null) return;
    this.#observer = new MutationObserver(() => this.#scheduleRecompute());
    this.#observer.observe(root, { subtree: true, childList: true, characterData: true });
  }

  #stopObserver(): void {
    this.#observer?.disconnect();
    this.#observer = null;
    if (this.#raf !== null) {
      cancelAnimationFrame(this.#raf);
      this.#raf = null;
    }
  }

  #scheduleRecompute(): void {
    if (this.#raf !== null) return;
    this.#raf = requestAnimationFrame(() => {
      this.#raf = null;
      this.#recompute(true);
    });
  }
}
