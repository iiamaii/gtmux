export interface SourceRange {
  startLine: number;
  startCol: number;
  endLine: number;
  endCol: number;
  columnAmbiguous?: boolean;
}

interface SourceEndpoint {
  line: number;
  col: number | null;
}

export function selectionToRange(root: HTMLElement, sel: Selection | null): SourceRange | null {
  if (sel === null || sel.rangeCount === 0 || sel.isCollapsed) return null;
  const range = sel.getRangeAt(0);
  if (!nodeBelongsToRoot(root, range.commonAncestorContainer)) return null;

  const start = endpointFromNode(root, range.startContainer, range.startOffset);
  const end = endpointFromNode(root, range.endContainer, range.endOffset);
  if (start === null || end === null) return null;

  const columnAmbiguous = start.col === null || end.col === null;
  return normalizeRange({
    startLine: start.line,
    startCol: start.col ?? 1,
    endLine: end.line,
    endCol: end.col ?? 1,
    columnAmbiguous,
  });
}

export function formatPathWithLocation(absPath: string, range: SourceRange | null): string {
  if (range === null) return absPath;
  if (range.columnAmbiguous === true) {
    return `${absPath}:${range.startLine}-${range.endLine}`;
  }
  if (range.startLine === range.endLine) {
    return `${absPath}:${range.startLine}:${range.startCol}-${range.endCol}`;
  }
  return `${absPath}:${range.startLine}:${range.startCol}-${range.endLine}:${range.endCol}`;
}

function endpointFromNode(
  root: HTMLElement,
  container: Node,
  offset: number,
): SourceEndpoint | null {
  const element = elementForNode(container);
  const lineEl = element?.closest<HTMLElement>('[data-line]');
  if (lineEl === null || lineEl === undefined || !root.contains(lineEl)) return null;
  const line = Number(lineEl.dataset.line);
  if (!Number.isInteger(line) || line < 1) return null;

  const codeEl = lineEl.querySelector<HTMLElement>('[data-code]');
  if (codeEl === null || !codeEl.contains(container)) return { line, col: null };
  return { line, col: columnWithinCode(codeEl, container, offset) };
}

function columnWithinCode(codeEl: HTMLElement, container: Node, offset: number): number | null {
  const range = document.createRange();
  try {
    range.selectNodeContents(codeEl);
    range.setEnd(container, offset);
    return Math.max(1, range.toString().length + 1);
  } catch {
    return null;
  } finally {
    range.detach();
  }
}

function normalizeRange(range: SourceRange): SourceRange {
  if (range.startLine < range.endLine) return range;
  if (range.startLine > range.endLine) return flipRange(range);
  if (range.columnAmbiguous === true || range.startCol <= range.endCol) return range;
  return flipRange(range);
}

function flipRange(range: SourceRange): SourceRange {
  return {
    startLine: range.endLine,
    startCol: range.endCol,
    endLine: range.startLine,
    endCol: range.startCol,
    columnAmbiguous: range.columnAmbiguous,
  };
}

function nodeBelongsToRoot(root: HTMLElement, node: Node): boolean {
  return node === root || root.contains(node);
}

function elementForNode(node: Node): Element | null {
  return node instanceof Element ? node : node.parentElement;
}
