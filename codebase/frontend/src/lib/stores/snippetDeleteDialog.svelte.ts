// snippetDeleteDialog — ADR-0038 (2026-05-24 amend, delete-mode dialog).
//
// Confirms a single Snippet entry deletion when the node is in 'delete'
// viewMode and the user clicks a pill. Mirrors the pattern used by
// panelCloseDialog / sessionDeleteDialog: singleton store with `show()` /
// `confirm()` / `cancel()` + pending callback.
//
// Single-entry scope only (delete mode operates one entry at a time). For
// batch deletion the future ContextMenu path will spawn a different dialog.

interface ShowArgs {
  /** Display label — the entry's key (truncated if long for title). */
  key: string;
  /** Caller-supplied delete action — invoked only on user confirm. */
  onConfirm: () => void | Promise<void>;
}

class SnippetDeleteDialogStore {
  open = $state(false);
  /** Display label (entry.key). */
  entryKey = $state('');

  private pendingOnConfirm: (() => void | Promise<void>) | null = null;

  show(args: ShowArgs): void {
    this.entryKey = args.key;
    this.pendingOnConfirm = args.onConfirm;
    this.open = true;
  }

  confirm(): void {
    const cb = this.pendingOnConfirm;
    this.open = false;
    this.pendingOnConfirm = null;
    if (cb !== null) void cb();
  }

  cancel(): void {
    this.open = false;
    this.pendingOnConfirm = null;
  }
}

export const snippetDeleteDialog = new SnippetDeleteDialogStore();
