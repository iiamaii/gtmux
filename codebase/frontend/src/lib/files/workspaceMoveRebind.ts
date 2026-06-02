import type { CanvasItem, CanvasLayout, DocumentItem, FilePathItem, ImageItem } from '$lib/types/canvas';

import { basename, resolveWorkspacePath, workspaceRelativePath } from './workspaceAssets';

export interface WorkspaceMoveMapping {
  source: string;
  path: string;
  name: string;
  kind: 'file' | 'directory';
}

export interface CanvasPathRebindResult {
  layout: CanvasLayout;
  changedItemCount: number;
}

interface MovedPath {
  path: string;
  exact: boolean;
  move: WorkspaceMoveMapping;
}

export function rebindCanvasLayoutPathsForMove(
  layout: CanvasLayout,
  moves: readonly WorkspaceMoveMapping[],
  workspaceRoot: string,
): CanvasPathRebindResult {
  const normalizedMoves = normalizeMoves(moves);
  if (normalizedMoves.length === 0) return { layout, changedItemCount: 0 };

  let changedItemCount = 0;
  const items = layout.items.map((item) => {
    const next = rebindCanvasItemPath(item, normalizedMoves, workspaceRoot);
    if (next !== item) changedItemCount += 1;
    return next;
  });
  if (changedItemCount === 0) return { layout, changedItemCount: 0 };
  return {
    layout: { ...layout, items },
    changedItemCount,
  };
}

function rebindCanvasItemPath(
  item: CanvasItem,
  moves: readonly WorkspaceMoveMapping[],
  workspaceRoot: string,
): CanvasItem {
  if (item.type === 'file_path') {
    const moved = movedAbsolutePath(item.path, moves);
    if (moved === null || moved.path === item.path) return item;
    return {
      ...item,
      path: moved.path,
      kind: moved.exact ? moved.move.kind : item.kind,
    } satisfies FilePathItem;
  }

  if (item.type !== 'image' && item.type !== 'document') return item;
  if (item.path === undefined) return item;
  const absolutePath = resolveWorkspacePath(workspaceRoot, item.path);
  if (absolutePath === null) return item;
  const moved = movedAbsolutePath(absolutePath, moves);
  if (moved === null || moved.path === absolutePath) return item;
  const nextRelativePath = workspaceRelativePath(workspaceRoot, moved.path);
  if (nextRelativePath === null || nextRelativePath === item.path) return item;

  if (item.type === 'image') {
    return {
      ...item,
      path: nextRelativePath,
    } satisfies ImageItem;
  }

  return {
    ...item,
    path: nextRelativePath,
    file_name: basename(moved.path),
  } satisfies DocumentItem;
}

function movedAbsolutePath(
  absolutePath: string,
  moves: readonly WorkspaceMoveMapping[],
): MovedPath | null {
  for (const move of moves) {
    if (absolutePath === move.source) return { path: move.path, exact: true, move };
    if (move.kind !== 'directory') continue;
    const prefix = `${move.source}/`;
    if (!absolutePath.startsWith(prefix)) continue;
    return {
      path: `${move.path}${absolutePath.slice(move.source.length)}`,
      exact: false,
      move,
    };
  }
  return null;
}

function normalizeMoves(moves: readonly WorkspaceMoveMapping[]): WorkspaceMoveMapping[] {
  return moves
    .map((move) => ({
      ...move,
      source: normalizeAbsolutePath(move.source),
      path: normalizeAbsolutePath(move.path),
    }))
    .filter((move) => move.source.length > 0 && move.path.length > 0 && move.source !== move.path)
    .sort((a, b) => b.source.length - a.source.length);
}

function normalizeAbsolutePath(path: string): string {
  if (path === '/') return path;
  return path.replace(/\/+$/, '');
}
