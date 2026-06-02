// @vitest-environment jsdom
//
// DOM-walk coverage for `selectionToRange` — the column/line mapping that the
// pure-formatter test (`sourceLocation.test.ts`) cannot exercise. Uses a real
// jsdom Range with a minimal Selection stub (selectionToRange only reads
// rangeCount / isCollapsed / getRangeAt). Mirrors CodeViewer's DOM contract:
//   .cv-line[data-line] > (.cv-gutter + <code data-code>)

import { afterEach, describe, expect, it } from 'vitest';

import { formatPathWithLocation, selectionToRange } from './sourceLocation';

const PATH = '/workspace/src/main.ts';

interface LineSpec {
  /** Plain single text node, or pre-tokenized spans (highlighted mode). */
  text?: string;
  tokens?: string[];
}

let mounted: HTMLElement | null = null;

afterEach(() => {
  mounted?.remove();
  mounted = null;
});

function buildViewer(lines: LineSpec[]): HTMLElement {
  const root = document.createElement('div');
  root.className = 'code-viewer';
  lines.forEach((line, i) => {
    const lineEl = document.createElement('div');
    lineEl.className = 'cv-line';
    lineEl.dataset.line = String(i + 1);

    const gutter = document.createElement('span');
    gutter.className = 'cv-gutter';
    gutter.textContent = String(i + 1);

    const code = document.createElement('code');
    code.className = 'cv-code';
    code.dataset.code = '';
    if (line.tokens !== undefined) {
      for (const tok of line.tokens) {
        const span = document.createElement('span');
        span.textContent = tok;
        code.appendChild(span);
      }
    } else {
      code.textContent = line.text ?? '';
    }
    lineEl.append(gutter, code);
    root.appendChild(lineEl);
  });
  document.body.appendChild(root);
  mounted = root;
  return root;
}

/** First text node inside the `<code data-code>` of line `n` (1-based). */
function codeTextNode(root: HTMLElement, n: number): Text {
  const code = root.querySelectorAll<HTMLElement>('[data-code]')[n - 1];
  if (code === undefined) throw new Error(`no code cell for line ${n}`);
  const walker = document.createTreeWalker(code, NodeFilter.SHOW_TEXT);
  const node = walker.nextNode();
  if (node === null) throw new Error(`no text node in line ${n}`);
  return node as Text;
}

function gutterNode(root: HTMLElement, n: number): Node {
  const gutter = root.querySelectorAll<HTMLElement>('.cv-gutter')[n - 1];
  if (gutter === undefined || gutter.firstChild === null) {
    throw new Error(`no gutter for line ${n}`);
  }
  return gutter.firstChild;
}

function selectionOf(range: Range): Selection {
  return {
    rangeCount: 1,
    isCollapsed: range.collapsed,
    getRangeAt: () => range,
  } as unknown as Selection;
}

describe('selectionToRange (DOM)', () => {
  it('maps a single-line selection to 1-based columns', () => {
    const root = buildViewer([{ text: 'const x = 1;' }]);
    const node = codeTextNode(root, 1);
    const range = document.createRange();
    range.setStart(node, 6); // before "x"
    range.setEnd(node, 11); // before ";"

    const result = selectionToRange(root, selectionOf(range));
    expect(result).toEqual({
      startLine: 1,
      startCol: 7,
      endLine: 1,
      endCol: 12,
      columnAmbiguous: false,
    });
    expect(formatPathWithLocation(PATH, result)).toBe(`${PATH}:1:7-12`);
  });

  it('counts columns across token spans (highlighted mode)', () => {
    const root = buildViewer([{ tokens: ['const ', 'x = 1;'] }]);
    const secondSpan = root.querySelectorAll<HTMLElement>('[data-code] span')[1];
    if (secondSpan?.firstChild == null) throw new Error('missing second span');
    const secondSpanText = secondSpan.firstChild as Text;
    const range = document.createRange();
    range.setStart(secondSpanText, 0); // start of "x = 1;" → column 7 overall
    range.setEnd(secondSpanText, 5); // before ";"

    const result = selectionToRange(root, selectionOf(range));
    expect(result?.startCol).toBe(7);
    expect(result?.endCol).toBe(12);
    expect(result?.columnAmbiguous).toBe(false);
  });

  it('maps a multi-line selection', () => {
    const root = buildViewer([{ text: 'const x = 1;' }, { text: 'return x;' }]);
    const range = document.createRange();
    range.setStart(codeTextNode(root, 1), 6);
    range.setEnd(codeTextNode(root, 2), 3); // before second char span boundary

    const result = selectionToRange(root, selectionOf(range));
    expect(result?.startLine).toBe(1);
    expect(result?.startCol).toBe(7);
    expect(result?.endLine).toBe(2);
    expect(result?.endCol).toBe(4);
    expect(formatPathWithLocation(PATH, result)).toBe(`${PATH}:1:7-2:4`);
  });

  it('falls back to line-only when an endpoint lands in the gutter', () => {
    const root = buildViewer([{ text: 'const x = 1;' }, { text: 'return x;' }]);
    const range = document.createRange();
    range.setStart(gutterNode(root, 1), 0); // gutter → column unresolved
    range.setEnd(codeTextNode(root, 2), 3);

    const result = selectionToRange(root, selectionOf(range));
    expect(result?.columnAmbiguous).toBe(true);
    expect(formatPathWithLocation(PATH, result)).toBe(`${PATH}:1-2`);
  });

  it('returns null for a collapsed selection', () => {
    const root = buildViewer([{ text: 'const x = 1;' }]);
    const range = document.createRange();
    range.setStart(codeTextNode(root, 1), 3);
    range.collapse(true);
    expect(selectionToRange(root, selectionOf(range))).toBeNull();
  });

  it('returns null when the selection is outside the viewer root', () => {
    const root = buildViewer([{ text: 'const x = 1;' }]);
    const outside = document.createElement('p');
    outside.textContent = 'elsewhere';
    document.body.appendChild(outside);
    const range = document.createRange();
    range.selectNodeContents(outside);
    expect(selectionToRange(root, selectionOf(range))).toBeNull();
    outside.remove();
  });
});
