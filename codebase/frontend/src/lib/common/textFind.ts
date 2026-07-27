// textFind — pure match computation for in-document find (ADR-0058 D2).
//
// Matching is a literal substring, case-insensitive (IDE Cmd+F convention —
// deliberately NOT the left panel's token-AND filter, see ADR-0058 R2).
// Offsets are UTF-16 code units, directly usable for DOM Range endpoints.
// Matches are non-overlapping: after a hit the scan resumes past its end
// (adjacent hits still count — "aaaa" ~ "aa" → 2 matches).

/**
 * Hard cap on computed matches (ADR-0058 D2) — computation stops here and the
 * result is flagged `capped` so the UI can show "5000+" instead of freezing on
 * a huge log file. No silent cap.
 */
export const FIND_MAX_MATCHES = 5000;

export interface FindMatchesResult {
  /** `[start, end)` offsets into the haystack, in document order. */
  ranges: Array<readonly [number, number]>;
  /** True when the scan stopped at `FIND_MAX_MATCHES`. */
  capped: boolean;
}

/** One match inside a line-addressed text (CodeViewer surface, ADR-0058 D3). */
export interface LineMatch {
  /** 1-based line number — matches CodeViewer's `data-line` anchors. */
  line: number;
  /** 0-based column (UTF-16 code units) within the line. */
  col: number;
  /** Match length in UTF-16 code units. */
  len: number;
}

export interface LineMatchesResult {
  matches: LineMatch[];
  capped: boolean;
}

/**
 * Case-fold for matching. `toLowerCase` can change string length for a few
 * exotic code points (e.g. "İ" → "i̇"); when that happens the folded offsets
 * would drift, so callers must compare folded lengths before trusting offsets.
 */
function fold(text: string): string {
  return text.toLowerCase();
}

/**
 * All case-insensitive literal occurrences of `query` in `text` as
 * `[start, end)` offset pairs, capped at `FIND_MAX_MATCHES`.
 */
export function findMatches(text: string, query: string): FindMatchesResult {
  const ranges: Array<readonly [number, number]> = [];
  if (query.length === 0 || text.length === 0 || query.length > text.length) {
    return { ranges, capped: false };
  }

  const foldedQuery = fold(query);
  const foldedText = fold(text);

  // Fast path — folding preserved lengths (true for ASCII/Korean/virtually all
  // real input), so folded offsets equal original offsets.
  if (foldedText.length === text.length && foldedQuery.length === query.length) {
    let from = 0;
    while (from <= foldedText.length - foldedQuery.length) {
      const hit = foldedText.indexOf(foldedQuery, from);
      if (hit < 0) break;
      ranges.push([hit, hit + foldedQuery.length]);
      if (ranges.length >= FIND_MAX_MATCHES) return { ranges, capped: true };
      from = hit + foldedQuery.length;
    }
    return { ranges, capped: false };
  }

  // Slow path — length-changing fold. Compare per-position slices so reported
  // offsets always index the ORIGINAL text (length-shifting characters simply
  // fail to match case-insensitively, which is acceptable for v1).
  let i = 0;
  while (i <= text.length - query.length) {
    if (fold(text.slice(i, i + query.length)) === foldedQuery) {
      ranges.push([i, i + query.length]);
      if (ranges.length >= FIND_MAX_MATCHES) return { ranges, capped: true };
      i += query.length;
    } else {
      i += 1;
    }
  }
  return { ranges, capped: false };
}

/**
 * Line-scoped matching for the CodeViewer surface (ADR-0058 D3): each match is
 * addressed as `(line, col, len)` so it can be mapped onto the rendered
 * `.cv-line[data-line]` rows. Queries containing a newline never match (the
 * surface matches per line by design).
 */
export function findLineMatches(lines: readonly string[], query: string): LineMatchesResult {
  const matches: LineMatch[] = [];
  if (query.length === 0 || query.includes('\n')) {
    return { matches, capped: false };
  }
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (line === undefined) continue;
    const { ranges, capped } = findMatches(line, query);
    for (const [start, end] of ranges) {
      matches.push({ line: index + 1, col: start, len: end - start });
      if (matches.length >= FIND_MAX_MATCHES) return { matches, capped: true };
    }
    // A single line hitting the cap internally also caps the whole result.
    if (capped) return { matches, capped: true };
  }
  return { matches, capped: false };
}
