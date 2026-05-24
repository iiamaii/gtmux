// snippetEditPanel — ADR-0038 (2026-05-24 amend, v8 §12 follow-up).
//
// Floating edit form for Snippets entries — replaces the inline form that
// previously lived inside SnippetsNode. Singleton: only one panel open at a
// time. The caller (canvas pill, inspector add button, layer list context
// menu) provides an *anchor rect* so the panel can position itself near the
// trigger origin.
//
// State is purely ephemeral UI — not persisted, not layout-affecting. The
// actual mutation (entry add/edit/delete) goes through `sessionStore.apply
// Mutation` from inside the panel component (single applyMutation entry
// per save/delete, Cmd+Z step preserved).

import type { SnippetEntry } from '$lib/types/canvas';

/** Anchor rectangle in viewport coordinates — typically from
 *  `element.getBoundingClientRect()`. The panel anchors to this rect with
 *  smart placement (below preferred; above as fallback). */
export interface PanelAnchor {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface SnippetEditPanelOpenOpts {
  /** SnippetsItem.id — target node whose entries are being edited. */
  nodeId: string;
  /** SnippetEntry.id — null = add new entry; string = edit existing. */
  entryId: string | null;
  /** Optional initial draft values (used for edit; ignored for add). */
  prefill?: Pick<SnippetEntry, 'key' | 'body'> | null;
  /** Where to anchor the panel visually. */
  anchor: PanelAnchor;
  /** Optional source tag for analytics / debug. */
  source?: 'canvas-pill' | 'canvas-empty' | 'canvas-add' | 'inspector' | 'layer-context';
}

class SnippetEditPanelStore {
  open = $state(false);
  nodeId = $state<string | null>(null);
  entryId = $state<string | null>(null);
  prefillKey = $state('');
  prefillBody = $state('');
  anchor = $state<PanelAnchor | null>(null);
  source = $state<SnippetEditPanelOpenOpts['source'] | null>(null);

  openFor(opts: SnippetEditPanelOpenOpts): void {
    this.nodeId = opts.nodeId;
    this.entryId = opts.entryId;
    this.prefillKey = opts.prefill?.key ?? '';
    this.prefillBody = opts.prefill?.body ?? '';
    this.anchor = opts.anchor;
    this.source = opts.source ?? null;
    this.open = true;
  }

  close(): void {
    this.open = false;
    this.nodeId = null;
    this.entryId = null;
    this.prefillKey = '';
    this.prefillBody = '';
    this.anchor = null;
    this.source = null;
  }
}

export const snippetEditPanel = new SnippetEditPanelStore();
