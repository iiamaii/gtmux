import { describe, it, expect } from 'vitest';

import {
  measureAnchorIndex,
  restoreScrollTop,
  keyToArrayIndex,
  type AnchorUnit,
} from './documentAnchor';

// Three stacked units: [0,100) [100,100) [200,50).
const units: AnchorUnit[] = [
  { top: 0, height: 100 },
  { top: 100, height: 100 },
  { top: 200, height: 50 },
];

describe('measureAnchorIndex', () => {
  it('returns null for empty geometry', () => {
    expect(measureAnchorIndex(0, [])).toBeNull();
  });

  it('anchors the top unit at scrollTop 0', () => {
    expect(measureAnchorIndex(0, units)).toEqual({ arrayIndex: 0, frac: 0 });
  });

  it('computes progress within the top-visible unit', () => {
    expect(measureAnchorIndex(50, units)).toEqual({ arrayIndex: 0, frac: 0.5 });
  });

  it('advances to the next unit once the fold crosses it', () => {
    expect(measureAnchorIndex(150, units)).toEqual({ arrayIndex: 1, frac: 0.5 });
  });

  it('applies 0.5px slack at a unit boundary', () => {
    expect(measureAnchorIndex(100, units)).toEqual({ arrayIndex: 1, frac: 0 });
  });

  it('clamps frac to 1 when scrolled past the last unit', () => {
    expect(measureAnchorIndex(9999, units)).toEqual({ arrayIndex: 2, frac: 1 });
  });

  it('treats a zero-height unit as frac 0 (no divide-by-zero)', () => {
    expect(measureAnchorIndex(0, [{ top: 0, height: 0 }])).toEqual({
      arrayIndex: 0,
      frac: 0,
    });
  });

  it('coerces a non-finite scrollTop to 0', () => {
    expect(measureAnchorIndex(Number.NaN, units)).toEqual({ arrayIndex: 0, frac: 0 });
  });
});

describe('restoreScrollTop', () => {
  it('returns 0 for empty geometry', () => {
    expect(restoreScrollTop(0, 0.5, [])).toBe(0);
  });

  it('is the inverse of measure for an in-range anchor', () => {
    expect(restoreScrollTop(1, 0.5, units)).toBe(150);
  });

  it('clamps an out-of-range index to the last unit', () => {
    expect(restoreScrollTop(99, 0, units)).toBe(200);
  });

  it('clamps frac into 0..=1', () => {
    expect(restoreScrollTop(0, 5, units)).toBe(100);
    expect(restoreScrollTop(0, -5, units)).toBe(0);
  });
});

describe('keyToArrayIndex', () => {
  it('returns 0 for empty keys', () => {
    expect(keyToArrayIndex([], 5)).toBe(0);
  });

  it('finds an exact match (block index or data-line)', () => {
    expect(keyToArrayIndex([0, 1, 2], 2)).toBe(2);
    expect(keyToArrayIndex([1, 5, 9], 5)).toBe(1);
  });

  it('falls to the first key >= target when the exact line was edited away', () => {
    // data-line rows 1,4,7 — anchored line 5 is gone → snap to the next row.
    expect(keyToArrayIndex([1, 4, 7], 5)).toBe(2);
  });

  it('clamps to the last unit when the anchor is beyond the content end', () => {
    expect(keyToArrayIndex([0, 1, 2], 99)).toBe(2);
  });
});
