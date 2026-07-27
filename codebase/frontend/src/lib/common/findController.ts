// findController — registry that lets the global Cmd/Ctrl+F shortcut open a
// document surface's FindBar without hard imports into the components
// (ADR-0058 D5). Mirrors `leftPanelSearchController.ts`: each searchable
// surface registers an opener on mount and unregisters on destroy/gate-off.
//
// Surface keys:
//   `node:<itemId>` — DocumentNode body on the canvas
//   `max:<itemId>`  — MaximizedItemModal document body
//   `preview`       — FilePreviewView surface (inline or maximized overlay)

export interface DocumentFindOpenRequest {
  /** Selection text to prefill as the query (ADR-0058 D5 branch 2). */
  prefill?: string;
}

export type DocumentFindOpener = (req?: DocumentFindOpenRequest) => void;

const openers = new Map<string, DocumentFindOpener>();

/**
 * Register a surface's opener. Latest registration wins per key; the returned
 * unregister only clears the slot when this opener is still current (guards a
 * stale unmount clobbering a fresh remount).
 */
export function registerDocumentFindSurface(key: string, opener: DocumentFindOpener): () => void {
  openers.set(key, opener);
  return () => {
    if (openers.get(key) === opener) openers.delete(key);
  };
}

/** True when a searchable surface is currently registered under `key`. */
export function hasDocumentFindSurface(key: string): boolean {
  return openers.has(key);
}

/** Open (or re-focus) the FindBar of the surface registered under `key`. */
export function openDocumentFind(key: string, req?: DocumentFindOpenRequest): boolean {
  const opener = openers.get(key);
  if (opener === undefined) return false;
  opener(req);
  return true;
}

/**
 * Attribute searchable surfaces set on their body element so the shortcut can
 * resolve "the selection anchor lives inside surface X" (ADR-0058 D5 branch 2).
 */
export const FIND_SURFACE_ATTR = 'data-find-surface';

/** Surface key of the closest `[data-find-surface]` ancestor, if any. */
export function findSurfaceKeyFromNode(node: Node | null): string | null {
  if (node === null) return null;
  const el = node instanceof Element ? node : node.parentElement;
  const host = el?.closest(`[${FIND_SURFACE_ATTR}]`) ?? null;
  return host?.getAttribute(FIND_SURFACE_ATTR) ?? null;
}
