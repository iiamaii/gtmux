import { describe, expect, it } from 'vitest';
import { matchNamePath, mergeRanges, tokenizeQuery } from './treeMatch';

describe('tokenizeQuery', () => {
  it('splits on whitespace and slashes, drops empties, lowercases', () => {
    expect(tokenizeQuery('  Auth/Card  TSX ')).toEqual(['auth', 'card', 'tsx']);
  });

  it('returns no tokens for an empty / whitespace-only query', () => {
    expect(tokenizeQuery('')).toEqual([]);
    expect(tokenizeQuery('   / / ')).toEqual([]);
  });
});

describe('matchNamePath', () => {
  it('treats an empty query as no-filter (matched, no ranges)', () => {
    const r = matchNamePath('', 'AuthCard.tsx', 'src/auth/AuthCard.tsx');
    expect(r.matched).toBe(true);
    expect(r.ranges).toEqual([]);
  });

  it('matches a name substring case-insensitively with ranges on name', () => {
    const r = matchNamePath('auth', 'AuthCard.tsx', 'src/auth/AuthCard.tsx');
    expect(r.matched).toBe(true);
    // "Auth" at [0,4) in the name.
    expect(r.ranges).toEqual([[0, 4]]);
  });

  it('requires every token to match (token-AND) across name OR path', () => {
    // Both "auth" [0,4) and "card" [4,8) appear in name and are adjacent, so
    // the merge step collapses them into a single [0,8) range.
    const both = matchNamePath('auth card', 'AuthCard.tsx', 'src/auth/AuthCard.tsx');
    expect(both.matched).toBe(true);
    expect(both.ranges).toEqual([[0, 8]]);

    // "auth/card" tokenizes identically (slash split) → same outcome.
    const slash = matchNamePath('auth/card', 'AuthCard.tsx', 'src/auth/AuthCard.tsx');
    expect(slash.matched).toBe(true);
    expect(slash.ranges).toEqual([[0, 8]]);
  });

  it('fails when any single token matches neither name nor path', () => {
    const r = matchNamePath('auth zzz', 'AuthCard.tsx', 'src/auth/AuthCard.tsx');
    expect(r.matched).toBe(false);
    expect(r.ranges).toEqual([]);
  });

  it('matches via path only and contributes no name ranges', () => {
    // "src" is only in the relpath, not in the name.
    const r = matchNamePath('src', 'AuthCard.tsx', 'src/auth/AuthCard.tsx');
    expect(r.matched).toBe(true);
    expect(r.ranges).toEqual([]);
  });

  it('mixes name-token (with ranges) and path-only token (no ranges)', () => {
    // "card" → name range; "auth" matches both, also adds name range.
    const r = matchNamePath('card src', 'AuthCard.tsx', 'src/auth/AuthCard.tsx');
    expect(r.matched).toBe(true);
    // "Card" at [4,8); "src" only in path → no extra name range.
    expect(r.ranges).toEqual([[4, 8]]);
  });

  it('returns no match when nothing matches at all', () => {
    const r = matchNamePath('xyz', 'AuthCard.tsx', 'src/auth/AuthCard.tsx');
    expect(r.matched).toBe(false);
    expect(r.ranges).toEqual([]);
  });

  it('captures multiple occurrences of a token within the name', () => {
    const r = matchNamePath('a', 'aXaXa', 'p/aXaXa');
    expect(r.matched).toBe(true);
    expect(r.ranges).toEqual([
      [0, 1],
      [2, 3],
      [4, 5],
    ]);
  });

  it('merges overlapping ranges from multiple tokens', () => {
    // "abc" → [0,3); "bcd" → [1,4); overlap merges to [0,4).
    const r = matchNamePath('abc bcd', 'abcd', 'p/abcd');
    expect(r.matched).toBe(true);
    expect(r.ranges).toEqual([[0, 4]]);
  });

  it('merges adjacent (touching) ranges into one', () => {
    // "ab" → [0,2); "cd" → [2,4); touching → merged to [0,4).
    const r = matchNamePath('ab cd', 'abcd', 'p/abcd');
    expect(r.matched).toBe(true);
    expect(r.ranges).toEqual([[0, 4]]);
  });
});

describe('mergeRanges', () => {
  it('returns a copy for 0 or 1 ranges', () => {
    expect(mergeRanges([])).toEqual([]);
    expect(mergeRanges([[2, 5]])).toEqual([[2, 5]]);
  });

  it('sorts and merges overlapping / adjacent ranges', () => {
    expect(
      mergeRanges([
        [5, 7],
        [0, 3],
        [2, 4],
        [7, 9],
      ]),
    ).toEqual([
      [0, 4],
      [5, 9],
    ]);
  });

  it('keeps disjoint ranges separate', () => {
    expect(
      mergeRanges([
        [0, 2],
        [4, 6],
      ]),
    ).toEqual([
      [0, 2],
      [4, 6],
    ]);
  });
});
