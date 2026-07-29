// changeWebViewDialog — global store for the Web View "change address" modal
// (ADR-0059 D3). Mirrors changeTerminalDialog: a single ChangeWebViewModal
// instance (mounted in +page.svelte) binds to this store's state.
//
// Two callers open it: the WebViewNode header change button and the
// MaximizedItemModal change button — both target an item id and commit a new
// `url` through the history path (applyMutation, D8).

class ChangeWebViewDialogStore {
  open = $state(false);
  /** Edit target — the existing web_view item id, or null in create mode. */
  itemId = $state<string | null>(null);
  /**
   * Create mode (ADR-0059 D3) — the canvas position for a *new* web_view node.
   * The web_view tool defers spawning to this modal because the BE rejects an
   * empty `url` (WebViewUrlInvalid), so a node cannot be persisted url-less; the
   * modal collects a valid address first, then commits the node. Non-null =
   * create mode; null = edit mode.
   */
  createAt = $state<{ x: number; y: number } | null>(null);

  /** Open in EDIT mode for an existing item. */
  show(itemId: string): void {
    this.itemId = itemId;
    this.createAt = null;
    this.open = true;
  }

  /** Open in CREATE mode — commit will spawn a new node at `pos`. */
  showCreate(pos: { x: number; y: number }): void {
    this.itemId = null;
    this.createAt = pos;
    this.open = true;
  }

  close(): void {
    this.open = false;
    this.itemId = null;
    this.createAt = null;
  }
}

export const changeWebViewDialog = new ChangeWebViewDialogStore();
