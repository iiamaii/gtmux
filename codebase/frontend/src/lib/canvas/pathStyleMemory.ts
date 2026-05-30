import type { FigureStrokeDash, Head, LineItem, PathItem, PathRouting } from '$lib/types/canvas';

const STORAGE_KEY = 'gtmux.pathStyleMemory.v1';

export interface PathStyleMemory {
  routing: PathRouting;
  head_from: Head;
  head_to: Head;
  stroke: string;
  stroke_width: number;
  stroke_dash?: FigureStrokeDash;
}

const DEFAULT_MEMORY: PathStyleMemory = {
  routing: 'orthogonal',
  head_from: 'none',
  head_to: 'arrow',
  stroke: 'var(--color-fg)',
  stroke_width: 2,
  stroke_dash: 'solid',
};

let memory: PathStyleMemory = loadMemory();

function loadMemory(): PathStyleMemory {
  if (typeof localStorage === 'undefined') return { ...DEFAULT_MEMORY };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) return { ...DEFAULT_MEMORY };
    const parsed = JSON.parse(raw) as Partial<PathStyleMemory>;
    return sanitizeMemory({ ...DEFAULT_MEMORY, ...parsed });
  } catch {
    return { ...DEFAULT_MEMORY };
  }
}

function saveMemory(): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(memory));
  } catch {
    // Preference persistence is best-effort.
  }
}

function sanitizeMemory(next: PathStyleMemory): PathStyleMemory {
  return {
    routing: isRouting(next.routing) ? next.routing : DEFAULT_MEMORY.routing,
    head_from: isHead(next.head_from) ? next.head_from : DEFAULT_MEMORY.head_from,
    head_to: isHead(next.head_to) ? next.head_to : DEFAULT_MEMORY.head_to,
    stroke: typeof next.stroke === 'string' && next.stroke.length > 0 ? next.stroke : DEFAULT_MEMORY.stroke,
    stroke_width: Number.isFinite(next.stroke_width)
      ? Math.max(1, Math.min(32, Math.round(next.stroke_width)))
      : DEFAULT_MEMORY.stroke_width,
    stroke_dash: isDash(next.stroke_dash) ? next.stroke_dash : DEFAULT_MEMORY.stroke_dash,
  };
}

function isRouting(value: unknown): value is PathRouting {
  return value === 'orthogonal' || value === 'straight' || value === 'bezier';
}

function isHead(value: unknown): value is Head {
  return value === 'none' || value === 'arrow' || value === 'circle' || value === 'diamond';
}

function isDash(value: unknown): value is FigureStrokeDash | undefined {
  return (
    value === undefined ||
    value === 'solid' ||
    value === 'dash' ||
    value === 'dot' ||
    value === 'dash_dot'
  );
}

export function getPathStyleMemory(): PathStyleMemory {
  return { ...memory };
}

export function rememberPathStyle(update: Partial<PathStyleMemory>): void {
  memory = sanitizeMemory({ ...memory, ...update });
  saveMemory();
}

export function rememberStyleFromPath(path: PathItem): void {
  rememberPathStyle({
    routing: path.routing,
    head_from: path.head_from,
    head_to: path.head_to,
    stroke: path.stroke,
    stroke_width: path.stroke_width,
    stroke_dash: path.stroke_dash ?? 'solid',
  });
}

export function rememberStyleFromLine(line: LineItem): void {
  rememberPathStyle({
    head_from: line.head_from ?? 'none',
    head_to: line.head_to ?? 'none',
    stroke: line.stroke,
    stroke_width: line.stroke_width,
    stroke_dash: line.stroke_dash ?? 'solid',
  });
}
