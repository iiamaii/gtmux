import { describe, expect, it } from 'vitest';
import { ancestorIndices } from './stickyAncestors';

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
