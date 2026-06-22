// ADR-0052 D7 — VSCode-style sticky parent headers (pure, no DOM).
//
// Both the Files tree (FileTreeView) and the Layers tree (LayerTreeView)
// flatten their tree into an array of rows via DFS, where each row carries a
// `depth` (0 = root). Because DFS preserves the property that a row's ancestors
// are exactly the nearest preceding rows of strictly decreasing depth, the full
// ancestor chain can be reconstructed from `depth` alone — no parent links.
//
// Algorithm (`ancestorIndices`):
//   Start with the row at `topIndex` and its depth. Scan BACKWARD from
//   `topIndex - 1`. The first row whose depth is strictly less than the current
//   tracked depth is the nearest enclosing ancestor: record its index and lower
//   the tracked depth to that row's depth. Repeat until depth 0 is reached (a
//   root row has no ancestor) or the scan runs out. Rows whose depth is >= the
//   tracked depth are siblings / deeper subtrees and are skipped.
//
// The chain is collected innermost-first while scanning backward, then reversed
// to top-down order (outermost ancestor first … nearest parent last). If the
// chain is deeper than `maxSticky`, the innermost `maxSticky` ancestors are
// kept (the ones closest to the current row).

/**
 * Reconstruct the ancestor row indices for `rows[topIndex]` from a flattened
 * DFS tree using only `depth`. Returns indices in top-down order, capped to the
 * innermost `maxSticky` ancestors.
 *
 * Guards: returns `[]` when `topIndex` is out of range, `maxSticky <= 0`, or the
 * target row is itself a root (`depth === 0`).
 */
export function ancestorIndices(
  rows: ReadonlyArray<{ depth: number }>,
  topIndex: number,
  maxSticky: number,
): number[] {
  if (maxSticky <= 0) return [];
  if (topIndex < 0 || topIndex >= rows.length) return [];

  const target = rows[topIndex];
  if (target === undefined || target.depth === 0) return [];

  // Walk backward, collecting ancestors innermost-first.
  const innermostFirst: number[] = [];
  let trackedDepth = target.depth;
  for (let i = topIndex - 1; i >= 0 && trackedDepth > 0; i -= 1) {
    const row = rows[i];
    if (row === undefined) continue;
    if (row.depth < trackedDepth) {
      innermostFirst.push(i);
      trackedDepth = row.depth;
    }
  }

  // Cap to the innermost `maxSticky` ancestors, then flip to top-down order.
  const capped = innermostFirst.slice(0, maxSticky);
  capped.reverse();
  return capped;
}

/**
 * VSCode-style "push-out" offset (px) for the BOTTOM sticky header (ADR-0052 D7
 * amend ④, push-out-only). When the next row that ends the deepest sticky
 * ancestor's subtree (the first row after `topIndex` whose depth ≤ that
 * ancestor's depth — its next sibling / uncle) rises into the sticky band, the
 * bottom sticky header is pushed up by their overlap, clamped to one sticky-row
 * height so only the bottom row slides before the set recomputes.
 *
 * Returns 0 when there is no sticky stack, no pushing row exists yet, or the row
 * has not entered the band. Pure arithmetic — no DOM. The caller applies the
 * result as `transform: translateY(-offset)` on the last sticky row only
 * (compositor-only; keeps the overlay base pinned per amend ③).
 *
 * @param rows           flattened DFS rows carrying `depth`
 * @param stickyIndices  current sticky ancestor indices (top-down) of the top row
 * @param topIndex       floor(scrollTop / rowHeight) — the topmost visible row
 * @param scrollTop      live scroll position (px)
 * @param rowHeight      tree row pitch (px)
 * @param stickyHeight   one sticky header row height (px)
 */
export function bottomPushOffset(
  rows: ReadonlyArray<{ depth: number }>,
  stickyIndices: ReadonlyArray<number>,
  topIndex: number,
  scrollTop: number,
  rowHeight: number,
  stickyHeight: number,
): number {
  const k = stickyIndices.length;
  if (k === 0 || rowHeight <= 0 || stickyHeight <= 0) return 0;

  const lastIdx = stickyIndices[k - 1];
  if (lastIdx === undefined) return 0;
  const last = rows[lastIdx];
  if (last === undefined) return 0;
  const deepestDepth = last.depth;

  // First row after the top row that is NOT inside the bottom ancestor's subtree
  // (depth ≤ its depth). That row is what pushes the bottom header up.
  let nb = -1;
  for (let i = Math.max(topIndex, lastIdx) + 1; i < rows.length; i += 1) {
    const r = rows[i];
    if (r !== undefined && r.depth <= deepestDepth) {
      nb = i;
      break;
    }
  }
  if (nb < 0) return 0;

  const nbTop = nb * rowHeight - scrollTop; // viewport-y of the pushing row
  const overlap = k * stickyHeight - nbTop; // > 0 once it enters the band
  if (overlap <= 0) return 0;
  return Math.min(overlap, stickyHeight);
}
