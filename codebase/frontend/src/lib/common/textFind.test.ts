// textFind — ADR-0058 D2 match semantics: literal substring, case-insensitive,
// non-overlapping, FIND_MAX_MATCHES cap with explicit `capped` flag.

import { describe, expect, it } from 'vitest';

import { FIND_MAX_MATCHES, findLineMatches, findMatches } from './textFind';

describe('findMatches', () => {
  it('returns [start, end) offsets in document order', () => {
    expect(findMatches('abcabc', 'b').ranges).toEqual([
      [1, 2],
      [4, 5],
    ]);
  });

  it('matches at the text boundaries (start / end / whole string)', () => {
    expect(findMatches('needle haystack needle', 'needle').ranges).toEqual([
      [0, 6],
      [16, 22],
    ]);
    expect(findMatches('needle', 'needle').ranges).toEqual([[0, 6]]);
  });

  it('is non-overlapping but counts adjacent hits', () => {
    // Overlap suppressed: "aaa" holds only one "aa".
    expect(findMatches('aaa', 'aa').ranges).toEqual([[0, 2]]);
    // Adjacent hits both count.
    expect(findMatches('aaaa', 'aa').ranges).toEqual([
      [0, 2],
      [2, 4],
    ]);
  });

  it('is case-insensitive both ways', () => {
    expect(findMatches('FooBAR foobar FOOBAR', 'fooBar').ranges).toEqual([
      [0, 6],
      [7, 13],
      [14, 20],
    ]);
    expect(findMatches('straße STRASSE', 'ÄÖÜ').ranges).toEqual([]);
    expect(findMatches('Äöü', 'äöü').ranges).toEqual([[0, 3]]);
  });

  it('matches Korean text with correct UTF-16 offsets', () => {
    const text = '한글 검색 테스트 — 검색어';
    expect(findMatches(text, '검색').ranges).toEqual([
      [3, 5],
      [12, 14],
    ]);
    const hit = findMatches(text, '테스트').ranges[0];
    expect(hit).toBeDefined();
    const [start, end] = hit ?? [0, 0];
    expect(text.slice(start, end)).toBe('테스트');
  });

  it('returns no matches for empty query or query longer than text', () => {
    expect(findMatches('abc', '').ranges).toEqual([]);
    expect(findMatches('ab', 'abc').ranges).toEqual([]);
    expect(findMatches('', 'a').ranges).toEqual([]);
  });

  it('caps at FIND_MAX_MATCHES and flags it', () => {
    const text = 'ab'.repeat(FIND_MAX_MATCHES + 500);
    const result = findMatches(text, 'ab');
    expect(result.ranges.length).toBe(FIND_MAX_MATCHES);
    expect(result.capped).toBe(true);
    // Under the cap the flag stays off.
    expect(findMatches('abab', 'ab').capped).toBe(false);
  });

  it('keeps original-offset integrity when case folding changes length', () => {
    // "İ".toLowerCase() is 2 code units — the slow path must still report
    // offsets into the ORIGINAL string.
    const text = 'İstanbul abc ABC';
    const result = findMatches(text, 'abc');
    expect(result.ranges).toEqual([
      [9, 12],
      [13, 16],
    ]);
    for (const [start, end] of result.ranges) {
      expect(text.slice(start, end).toLowerCase()).toBe('abc');
    }
  });
});

describe('findLineMatches', () => {
  it('maps matches to 1-based line and 0-based col', () => {
    const lines = ['const a = 1;', 'let b = a + a;', ''];
    expect(findLineMatches(lines, 'a').matches).toEqual([
      { line: 1, col: 6, len: 1 },
      { line: 2, col: 8, len: 1 },
      { line: 2, col: 12, len: 1 },
    ]);
  });

  it('is case-insensitive per line', () => {
    expect(findLineMatches(['Foo', 'FOO'], 'foo').matches).toEqual([
      { line: 1, col: 0, len: 3 },
      { line: 2, col: 0, len: 3 },
    ]);
  });

  it('never matches a query containing a newline (per-line surface)', () => {
    expect(findLineMatches(['ab', 'cd'], 'ab\ncd').matches).toEqual([]);
  });

  it('caps across lines and flags it', () => {
    const lines = Array.from({ length: FIND_MAX_MATCHES + 10 }, () => 'x');
    const result = findLineMatches(lines, 'x');
    expect(result.matches.length).toBe(FIND_MAX_MATCHES);
    expect(result.capped).toBe(true);
  });
});
