import { describe, expect, it } from 'vitest';
import { ancestorIndices, bottomPushOffset } from './stickyAncestors';

const rows = (depths: number[]): { depth: number }[] => depths.map((depth) => ({ depth }));

describe('ancestorIndices', () => {
  it('returns [] for a flat list (all depth 0)', () => {
    const r = rows([0, 0, 0, 0]);
    expect(ancestorIndices(r, 2, 6)).toEqual([]);
  });

  it('returns [] when the target row is itself a root (depth 0)', () => {
    const r = rows([0, 1, 2, 0, 1]);
    expect(ancestorIndices(r, 3, 6)).toEqual([]);
  });

  it('returns [] when topIndex is out of range', () => {
    const r = rows([0, 1, 2]);
    expect(ancestorIndices(r, -1, 6)).toEqual([]);
    expect(ancestorIndices(r, 3, 6)).toEqual([]);
    expect(ancestorIndices(r, 99, 6)).toEqual([]);
  });

  it('returns [] when maxSticky <= 0', () => {
    const r = rows([0, 1, 2]);
    expect(ancestorIndices(r, 2, 0)).toEqual([]);
    expect(ancestorIndices(r, 2, -3)).toEqual([]);
  });

  it('reconstructs a nested chain in top-down order', () => {
    // 0:root  1:dir(d1)  2:sub(d2)  3:file(d3)
    const r = rows([0, 1, 2, 3]);
    // ancestors of row 3 = rows 0,1,2 outermost-first.
    expect(ancestorIndices(r, 3, 6)).toEqual([0, 1, 2]);
  });

  it('skips sibling / deeper-subtree rows when walking back', () => {
    // depths: [0, 1, 2, 2, 1, 0, 1]
    //   idx:    0  1  2  3  4  5  6
    // ancestors of idx 3 (depth 2): nearest depth<2 backward is idx 1 (depth 1),
    // then idx 0 (depth 0). idx 2 (depth 2, sibling) is skipped.
    const r = rows([0, 1, 2, 2, 1, 0, 1]);
    expect(ancestorIndices(r, 3, 6)).toEqual([0, 1]);
  });

  it('handles a realistic DFS sequence at several indices', () => {
    // depths: [0, 1, 2, 2, 1, 0, 1]
    const r = rows([0, 1, 2, 2, 1, 0, 1]);
    expect(ancestorIndices(r, 2, 6)).toEqual([0, 1]); // under idx1 → idx0
    expect(ancestorIndices(r, 4, 6)).toEqual([0]); // depth1 sibling of idx1, parent idx0
    expect(ancestorIndices(r, 5, 6)).toEqual([]); // depth 0 → no ancestors
    expect(ancestorIndices(r, 6, 6)).toEqual([5]); // depth1 under new root idx5
  });

  it('caps a deep chain to the innermost maxSticky ancestors', () => {
    // depths 0..6 strictly increasing; row 6 has 6 ancestors (idx 0..5).
    const r = rows([0, 1, 2, 3, 4, 5, 6]);
    expect(ancestorIndices(r, 6, 6)).toEqual([0, 1, 2, 3, 4, 5]);
    // maxSticky 3 → keep innermost three (idx 3,4,5), still top-down.
    expect(ancestorIndices(r, 6, 3)).toEqual([3, 4, 5]);
    // maxSticky 1 → only the nearest parent.
    expect(ancestorIndices(r, 6, 1)).toEqual([5]);
  });

  it('handles non-contiguous depth jumps in the chain', () => {
    // depths: [0, 2, 4, 5]  (depth can jump; only "strictly smaller" matters)
    //   idx:    0  1  2  3
    // ancestors of idx 3 (depth 5): idx2(4) < 5, idx1(2) < 4, idx0(0) < 2.
    const r = rows([0, 2, 4, 5]);
    expect(ancestorIndices(r, 3, 6)).toEqual([0, 1, 2]);
  });
});

describe('bottomPushOffset', () => {
  // A B c1 c2 c3 D E  — B (depth1) is an expanded child of A; D (depth1, idx5)
  // is B's next sibling = the row that ends B's subtree and pushes [A,B] up.
  const tree = rows([0, 1, 2, 2, 2, 1, 0]);
  const AB = [0, 1]; // sticky = ancestors of any c-row = [A, B]

  it('returns 0 with no sticky stack', () => {
    expect(bottomPushOffset(tree, [], 2, 40, 20, 20)).toBe(0);
  });

  it('returns 0 while the pushing row is still below the band', () => {
    // topIndex 2 (c1 at top), scrollTop 40 → D at viewport-y 100-40=60, band=40 → no overlap.
    expect(bottomPushOffset(tree, AB, 2, 40, 20, 20)).toBe(0);
  });

  it('ramps from 0 to one sticky-row as the pushing row enters the band', () => {
    // topIndex 3, band = 2*20 = 40, D content-top = 100.
    expect(bottomPushOffset(tree, AB, 3, 60, 20, 20)).toBe(0); // D at 40 — just touching
    expect(bottomPushOffset(tree, AB, 3, 70, 20, 20)).toBe(10); // overlap 10
    expect(bottomPushOffset(tree, AB, 3, 80, 20, 20)).toBe(20); // full row
  });

  it('clamps the offset to one sticky-row height', () => {
    // topIndex 4 (c3, B's last child), D nearly at the top of the band.
    expect(bottomPushOffset(tree, AB, 4, 95, 20, 20)).toBe(20);
  });

  it('returns 0 when the bottom subtree is the last in the tree (no pushing row)', () => {
    const t = rows([0, 1, 2]); // A B c — nothing after c at depth ≤ 1
    expect(bottomPushOffset(t, [0, 1], 2, 40, 20, 20)).toBe(0);
  });

  it('works when row pitch differs from sticky-row height', () => {
    // A B c1 c2 D — rowHeight 26, stickyHeight 24. D (idx4) content-top = 104.
    const t = rows([0, 1, 2, 2, 1]);
    // topIndex 3, scrollTop 78 → nbTop 104-78=26, overlap 48-26=22.
    expect(bottomPushOffset(t, [0, 1], 3, 78, 26, 24)).toBe(22);
    // scrollTop 90 → nbTop 14, overlap 34 → clamp 24.
    expect(bottomPushOffset(t, [0, 1], 3, 90, 26, 24)).toBe(24);
  });

  it('guards non-positive heights', () => {
    expect(bottomPushOffset(tree, AB, 3, 70, 0, 20)).toBe(0);
    expect(bottomPushOffset(tree, AB, 3, 70, 20, 0)).toBe(0);
  });
});
