// ADR-0052 D3 — left-panel search matching (pure, no DOM, no deps).
//
// Shared matching semantics for the three left-panel tabs (Files / Layers /
// Terminals) and mirrored by the BE `fs_search` endpoint so that server and
// client results stay consistent.
//
// Rules:
//   - Tokenize the query on whitespace AND `/`, drop empty tokens, lowercase.
//   - `matched` is true iff EVERY token is a case-insensitive substring of
//     (`name` OR `relpath`) — i.e. token-AND across the two candidate keys.
//   - Empty query (no tokens) is treated as "no filter": matched, no ranges.
//   - `ranges` are highlight ranges into `name` only (half-open [start, end)
//     index pairs). A token that matched only via `relpath` contributes no name
//     ranges. Overlapping ranges are merged; the result is sorted ascending.

export interface MatchResult {
  matched: boolean;
  ranges: [number, number][];
}

/**
 * Split a query into lowercased tokens on whitespace and `/`, dropping empties.
 */
export function tokenizeQuery(query: string): string[] {
  return query
    .toLowerCase()
    .split(/[\s/]+/)
    .filter((token) => token.length > 0);
}

/**
 * Match `query` against a candidate `name` + `relpath` pair using token-AND
 * semantics, returning whether it matched and the highlight ranges on `name`.
 */
export function matchNamePath(query: string, name: string, relpath: string): MatchResult {
  const tokens = tokenizeQuery(query);
  if (tokens.length === 0) {
    // No filter — everything matches, nothing to highlight.
    return { matched: true, ranges: [] };
  }

  const lowerName = name.toLowerCase();
  const lowerPath = relpath.toLowerCase();
  const ranges: [number, number][] = [];

  for (const token of tokens) {
    const inName = lowerName.includes(token);
    const inPath = lowerPath.includes(token);
    if (!inName && !inPath) {
      // Token matched neither key → token-AND fails for the whole candidate.
      return { matched: false, ranges: [] };
    }
    if (inName) {
      // Collect every occurrence of the token within `name`; a token can appear
      // multiple times. Overlaps are resolved by the merge step below.
      collectOccurrences(lowerName, token, ranges);
    }
  }

  return { matched: true, ranges: mergeRanges(ranges) };
}

/**
 * Push half-open [start, end) ranges for every occurrence of `token` in
 * `haystack` (both expected lowercased) into `out`.
 */
function collectOccurrences(haystack: string, token: string, out: [number, number][]): void {
  let from = 0;
  for (;;) {
    const index = haystack.indexOf(token, from);
    if (index === -1) break;
    out.push([index, index + token.length]);
    from = index + 1; // advance by 1 so overlapping occurrences are captured
  }
}

/**
 * Sort ranges ascending and merge any that overlap or touch.
 */
export function mergeRanges(ranges: [number, number][]): [number, number][] {
  if (ranges.length <= 1) return ranges.slice();

  const sorted = ranges
    .slice()
    .sort((a, b) => (a[0] - b[0] !== 0 ? a[0] - b[0] : a[1] - b[1]));

  const merged: [number, number][] = [];
  for (const [start, end] of sorted) {
    const last = merged[merged.length - 1];
    if (last !== undefined && start <= last[1]) {
      // Overlapping or adjacent — extend the previous range.
      if (end > last[1]) last[1] = end;
    } else {
      merged.push([start, end]);
    }
  }
  return merged;
}
