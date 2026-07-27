// findHighlight — CSS Custom Highlight adapter + DOM Range builders for
// in-document find (ADR-0058 D3).
//
// Highlighting uses `CSS.highlights` + `::highlight()` so the sanitized
// markdown DOM and Shiki token DOM are NEVER mutated (escape-on-render /
// no-innerHTML invariant, CLAUDE.md §4 / ADR-0037 D7.3). On browsers without
// the API the highlight is simply skipped — match counting, navigation and
// scrolling keep working (ADR-0058 D3 degrade).

import type { LineMatch } from './textFind';

/** Registry name for every match (styled in `src/styles/global.css`). */
export const FIND_HIGHLIGHT_ALL = 'gtmux-find';
/** Registry name for the current match (stronger accent wash). */
export const FIND_HIGHLIGHT_CURRENT = 'gtmux-find-current';

/** Feature-detect the CSS Custom Highlight API (ADR-0058 D3 degrade gate). */
export function supportsCustomHighlight(): boolean {
  return (
    typeof CSS !== 'undefined' &&
    'highlights' in CSS &&
    typeof Highlight !== 'undefined'
  );
}

/** Replace both find registries. No-op when the API is unsupported. */
export function setFindHighlights(all: readonly Range[], current: Range | null): void {
  if (!supportsCustomHighlight()) return;
  CSS.highlights.set(FIND_HIGHLIGHT_ALL, new Highlight(...all));
  if (current !== null) {
    CSS.highlights.set(FIND_HIGHLIGHT_CURRENT, new Highlight(current));
  } else {
    CSS.highlights.delete(FIND_HIGHLIGHT_CURRENT);
  }
}

/** Drop both find registries (close/unmount cleanup). */
export function clearFindHighlights(): void {
  if (!supportsCustomHighlight()) return;
  CSS.highlights.delete(FIND_HIGHLIGHT_ALL);
  CSS.highlights.delete(FIND_HIGHLIGHT_CURRENT);
}

/** All text nodes under `root` in document order (TreeWalker). */
export function collectTextNodes(root: Node): Text[] {
  const doc = root.ownerDocument;
  if (doc === null) return [];
  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  let node = walker.nextNode();
  while (node !== null) {
    nodes.push(node as Text);
    node = walker.nextNode();
  }
  return nodes;
}

/**
 * Concatenated text content of `nodes` — the haystack for the markdown
 * surface. Offsets into this string map 1:1 onto the node list, which is what
 * `rangeOverTextNodes` consumes (ADR-0058 D3: TreeWalker → offset → Range).
 */
export function concatTextNodes(nodes: readonly Text[]): string {
  let out = '';
  for (const node of nodes) out += node.data;
  return out;
}

/**
 * Build a DOM Range over `[start, end)` offsets of the concatenated text of
 * `nodes`. Returns null when the offsets fall outside the available text
 * (stale match against a re-rendered DOM).
 */
export function rangeOverTextNodes(nodes: readonly Text[], start: number, end: number): Range | null {
  if (end <= start) return null;
  const doc = nodes[0]?.ownerDocument ?? null;
  if (doc === null) return null;

  let cursor = 0;
  let startNode: Text | null = null;
  let startOffset = 0;
  let endNode: Text | null = null;
  let endOffset = 0;

  for (const node of nodes) {
    const next = cursor + node.data.length;
    if (startNode === null && start < next) {
      startNode = node;
      startOffset = start - cursor;
    }
    // End boundary is exclusive, so `end <= next` keeps it inside this node.
    if (startNode !== null && end <= next) {
      endNode = node;
      endOffset = end - cursor;
      break;
    }
    cursor = next;
  }

  if (startNode === null || endNode === null) return null;
  const range = doc.createRange();
  range.setStart(startNode, startOffset);
  range.setEnd(endNode, endOffset);
  return range;
}

/** Batch variant of `rangeOverTextNodes` — drops unmappable offsets. */
export function rangesForOffsets(
  nodes: readonly Text[],
  offsets: ReadonlyArray<readonly [number, number]>,
): Range[] {
  const ranges: Range[] = [];
  for (const [start, end] of offsets) {
    const range = rangeOverTextNodes(nodes, start, end);
    if (range !== null) ranges.push(range);
  }
  return ranges;
}

/**
 * Build a Range for one CodeViewer line match. Targets only the `[data-code]`
 * text (the gutter's line number is excluded). The token spans produced by
 * Shiki concatenate to the raw line text, so `(col, len)` maps directly onto
 * the line's text nodes.
 */
export function rangeForLineMatch(codeViewerEl: HTMLElement, match: LineMatch): Range | null {
  const codeEl = codeViewerEl.querySelector(`.cv-line[data-line="${match.line}"] [data-code]`);
  if (codeEl === null) return null;
  return rangeOverTextNodes(collectTextNodes(codeEl), match.col, match.col + match.len);
}

/**
 * Scroll `container` so `range` sits (roughly) centered. Rect deltas are in
 * screen pixels while scrollTop/Left are in content pixels, so divide by the
 * container's effective scale — the canvas zoom transform would otherwise
 * over-scroll (DocumentNode surfaces live inside a scaled SvelteFlow node).
 */
export function scrollRangeIntoCenter(container: HTMLElement, range: Range): void {
  const rect = range.getBoundingClientRect();
  if (rect.width === 0 && rect.height === 0) return;
  const containerRect = container.getBoundingClientRect();
  const scale = container.offsetHeight > 0 ? containerRect.height / container.offsetHeight : 1;
  if (!Number.isFinite(scale) || scale <= 0) return;

  const top = (rect.top - containerRect.top) / scale;
  container.scrollTop += top - container.clientHeight / 2 + rect.height / scale / 2;

  const left = (rect.left - containerRect.left) / scale;
  const width = rect.width / scale;
  if (left < 0 || left + width > container.clientWidth) {
    container.scrollLeft += left - container.clientWidth / 2;
  }
}
