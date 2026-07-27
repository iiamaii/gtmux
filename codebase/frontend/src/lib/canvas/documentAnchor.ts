// documentAnchor — pure content-anchor math for document scroll persistence.
//
// 정본: ADR-0056 D2 (content anchor, 픽셀 아님) + D6 (pure helper + vitest).
//
// The DOM glue (querying `.dmv-content` children / `.cv-line[data-line]` rows,
// reading offsetTop/offsetHeight) stays thin in the components; this module
// only does arithmetic + clamps so the block/line anchor math is unit-testable
// in isolation.

import type { DocumentViewAnchor } from '$lib/types/canvas';

/** Re-export under the store-local alias used across the scroll surfaces. */
export type DocViewAnchor = DocumentViewAnchor;

/** One measured scrollable unit: its top offset within the scroll content and
 *  its rendered height (px), both relative to the scroll container's content
 *  origin (scrollTop 0). Units are ordered ascending by `top`. */
export interface AnchorUnit {
  top: number;
  height: number;
}

function clamp01(n: number): number {
  if (!Number.isFinite(n)) return 0;
  if (n < 0) return 0;
  if (n > 1) return 1;
  return n;
}

function clampArrayIndex(i: number, len: number): number {
  if (len <= 0) return 0;
  if (i < 0) return 0;
  if (i > len - 1) return len - 1;
  return i;
}

/**
 * ADR-0056 D2 — given a `scrollTop` and the ordered geometry of a container's
 * units (rendered-markdown blocks or source line rows), find the top *visible*
 * unit (the last unit whose top is at/above the fold) and the progress within
 * it. Returns the array index into `units`; the glue maps that to a 0-based
 * block index or a 1-based `data-line` number. `null` when there are no units.
 */
export function measureAnchorIndex(
  scrollTop: number,
  units: readonly AnchorUnit[],
): { arrayIndex: number; frac: number } | null {
  if (units.length === 0) return null;
  const top = Number.isFinite(scrollTop) ? scrollTop : 0;
  let idx = 0;
  for (let i = 0; i < units.length; i++) {
    const unit = units[i];
    if (unit === undefined) break;
    // 0.5px slack — a unit sitting exactly at the fold still counts top-visible.
    if (unit.top <= top + 0.5) idx = i;
    else break;
  }
  const u = units[idx];
  if (u === undefined) return null;
  const frac = u.height > 0 ? clamp01((top - u.top) / u.height) : 0;
  return { arrayIndex: idx, frac };
}

/**
 * ADR-0056 D2 — inverse of measure: given a target array index + frac and the
 * measured units, compute the `scrollTop` that lands the unit's top at the
 * fold. `arrayIndex` beyond the content clamps to the last unit — documents may
 * have been edited so the anchor over-shoots (accepted approximate restore).
 */
export function restoreScrollTop(
  arrayIndex: number,
  frac: number,
  units: readonly AnchorUnit[],
): number {
  if (units.length === 0) return 0;
  const i = clampArrayIndex(arrayIndex, units.length);
  const u = units[i];
  if (u === undefined) return 0;
  return u.top + clamp01(frac) * u.height;
}

/**
 * Map an anchor `index` (a block index or a `data-line` number, i.e. the
 * `keys[]` entry stored at measure time) back to the array position in the
 * current geometry. Exact match wins; otherwise the first key `>=` target
 * (content shifted up by an edit), else the last unit (anchor beyond the new
 * content end — clamp, ADR-0056 D2).
 */
export function keyToArrayIndex(keys: readonly number[], targetKey: number): number {
  if (keys.length === 0) return 0;
  const exact = keys.indexOf(targetKey);
  if (exact !== -1) return exact;
  for (let i = 0; i < keys.length; i++) {
    const k = keys[i];
    if (k !== undefined && k >= targetKey) return i;
  }
  return keys.length - 1;
}
