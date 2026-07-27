// filePreviewEdit — pure decision logic for the Preview edit mode (ADR-0057 D3/D4).
//
// This module isolates the save / conflict / dirty branches from the Svelte
// component so they can be unit-tested (vitest). The component wires DOM,
// stores and the fetch call; everything reasoned about here is pure.

/** Kinds whose source text can be edited in place (ADR-0057 D2). */
export type EditablePreviewKind = 'text' | 'markdown' | 'html';

export function isEditableKind(kind: string): kind is EditablePreviewKind {
  return kind === 'text' || kind === 'markdown' || kind === 'html';
}

/**
 * Whether the pencil (edit entry) affordance should be shown (ADR-0057 D1/D2).
 * Size gate is intentionally omitted here — the FE has no config surface that
 * exposes `assets.max_size_bytes`, so entry is not size-gated and an oversized
 * write is surfaced via the 413 response instead (see report / D2 note).
 */
export function canEnterEdit(args: {
  multiSelection: boolean;
  hasSelection: boolean;
  loading: boolean;
  hasError: boolean;
  contentLoaded: boolean;
  kind: string;
}): boolean {
  return (
    !args.multiSelection &&
    args.hasSelection &&
    !args.loading &&
    !args.hasError &&
    args.contentLoaded &&
    isEditableKind(args.kind)
  );
}

/** Dirty = the draft diverges from the last-saved / loaded baseline. */
export function isDraftDirty(draft: string, baseline: string): boolean {
  return draft !== baseline;
}

// Write-result types + status classification (WriteResult, classifyWriteStatus,
// writeErrorMessage) live in the transport layer now — `$lib/http/fsWriteResult`
// (keeps `http/fs.ts` free of a `$lib/chrome` import). See Fix 3 / ADR-0057 D3.

// ── 412 conflict-resolution decision (ADR-0057 D4) ──

export type ConflictChoice = 'reload' | 'overwrite';

/**
 * Describes what the caller must do for each 412-dialog choice. Kept pure so
 * the two-branch flow is testable without DOM/fetch.
 *
 * - `reload`   → re-GET server content, drop the local draft (a copy-to-clipboard
 *                affordance is offered in the dialog before discarding).
 * - `overwrite`→ re-GET only to obtain the fresh ETag, then PUT again with it
 *                (explicit user-confirmed last-writer-wins).
 */
export interface ConflictPlan {
  /** Must a fresh GET be issued first? True for both branches. */
  refetch: boolean;
  /** After the GET, replace the editor draft with server content? */
  replaceDraft: boolean;
  /** After the GET, re-issue the PUT with the fresh ETag? */
  rewrite: boolean;
  /** Does the local draft survive this choice? */
  keepDraft: boolean;
}

export function planConflictResolution(choice: ConflictChoice): ConflictPlan {
  if (choice === 'reload') {
    return { refetch: true, replaceDraft: true, rewrite: false, keepDraft: false };
  }
  // overwrite
  return { refetch: true, replaceDraft: false, rewrite: true, keepDraft: true };
}
