// findRouting — pure ADR-0058 D5 priority resolution for Cmd/Ctrl+F.
//
// `findShortcut.svelte.ts` assembles the context from DOM/stores and executes
// the returned route; keeping the branch order here makes the priority chain
// unit-testable without a DOM. Returning `null` means "fall through to the
// existing ADR-0052 D2 chain unchanged" (D5 branch 5).

export interface FindRouteContext {
  /** D5 (0) — focus inside an xterm keeps the terminal special-case chain. */
  xtermFocused: boolean;
  /** Focus already inside an open FindBar input (re-invoke → select-all). */
  findBarFocused: boolean;
  /** Focus inside the LeftPanel footer search input (ADR-0052 D2 (2)). */
  leftPanelSearchFocused: boolean;
  maximizedItemId: string | null;
  /** Trimmed document Selection text ('' when none). */
  selectionText: string;
  /** Surface key of the `[data-find-surface]` containing the selection anchor. */
  selectionSurfaceKey: string | null;
  /** The sole member of sessionStore.M when exactly one item is selected. */
  singleSelectedItemId: string | null;
  /** Right panel Preview tab active. */
  previewTabActive: boolean;
  /** Registry probe — true when the key has a registered searchable surface. */
  hasSurface: (key: string) => boolean;
}

export type FindRoute =
  | { kind: 'select-all-find-input' }
  | { kind: 'open-surface'; key: string; prefill?: string };

export function resolveDocumentFindRoute(ctx: FindRouteContext): FindRoute | null {
  // (0) Terminal focus → untouched ADR-0052 chain (terminal special-case).
  if (ctx.xtermFocused) return null;

  // Repeated Cmd/Ctrl+F while typing in a FindBar = select-all of its input
  // (ADR-0058 D5 — isomorphic to the left panel's branch (2)).
  if (ctx.findBarFocused) return { kind: 'select-all-find-input' };

  // Typing in the LeftPanel footer search → fall through so the ADR-0052 D2
  // chain step (2) performs its guaranteed select-all. Without this, branches
  // (3)/(4) would hijack the shortcut whenever a document item is selected or
  // the Preview tab holds a searchable file.
  if (ctx.leftPanelSearchFocused) return null;

  const prefill = ctx.selectionText.length > 0 ? ctx.selectionText : undefined;

  // (1) Maximized searchable document → its FindBar. Prefill only when the
  // selection actually lives inside that surface.
  if (ctx.maximizedItemId !== null) {
    const key = `max:${ctx.maximizedItemId}`;
    if (ctx.hasSurface(key)) {
      return {
        kind: 'open-surface',
        key,
        prefill: ctx.selectionSurfaceKey === key ? prefill : undefined,
      };
    }
  }

  // (2) Selection anchored inside a searchable document/preview surface →
  // that surface's FindBar, selection prefilled (IDE convention — wins over
  // the old chain's left-panel routing for in-document highlights).
  if (
    prefill !== undefined &&
    ctx.selectionSurfaceKey !== null &&
    ctx.hasSurface(ctx.selectionSurfaceKey)
  ) {
    return { kind: 'open-surface', key: ctx.selectionSurfaceKey, prefill };
  }

  // (3) Single selected document item with a searchable view → its FindBar.
  if (ctx.singleSelectedItemId !== null) {
    const key = `node:${ctx.singleSelectedItemId}`;
    if (ctx.hasSurface(key)) return { kind: 'open-surface', key };
  }

  // (4) Preview tab active with a searchable kind → the preview FindBar.
  if (ctx.previewTabActive && ctx.hasSurface('preview')) {
    return { kind: 'open-surface', key: 'preview' };
  }

  // (5) Existing ADR-0052 chain.
  return null;
}
