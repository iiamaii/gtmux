import { describe, expect, it } from 'vitest';

import { formatPathWithLocation, type SourceRange } from './sourceLocation';

describe('sourceLocation', () => {
  const path = '/workspace/src/main.ts';

  it('formats a plain path when no range is available', () => {
    expect(formatPathWithLocation(path, null)).toBe(path);
  });

  it('formats a single-line range', () => {
    expect(formatPathWithLocation(path, range({
      startLine: 8,
      startCol: 3,
      endLine: 8,
      endCol: 18,
    }))).toBe('/workspace/src/main.ts:8:3-18');
  });

  it('formats a multi-line range', () => {
    expect(formatPathWithLocation(path, range({
      startLine: 8,
      startCol: 3,
      endLine: 10,
      endCol: 5,
    }))).toBe('/workspace/src/main.ts:8:3-10:5');
  });

  it('falls back to line-only output when columns are ambiguous', () => {
    expect(formatPathWithLocation(path, range({
      startLine: 8,
      startCol: 1,
      endLine: 8,
      endCol: 1,
      columnAmbiguous: true,
    }))).toBe('/workspace/src/main.ts:8-8');
    expect(formatPathWithLocation(path, range({
      startLine: 8,
      startCol: 1,
      endLine: 10,
      endCol: 1,
      columnAmbiguous: true,
    }))).toBe('/workspace/src/main.ts:8-10');
  });
});

function range(value: SourceRange): SourceRange {
  return value;
}
