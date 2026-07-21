// Pure copy-decision core for the terminal Cmd+C shortcut (ADR-0049 D7, ADR-0052
// copy-source parity).
//
// Kept in its own module — free of any Svelte rune store or DOM import — so it
// is unit-testable under the node vitest environment. `terminalCopyShortcut.ts`
// (which transitively pulls in rune stores via the clipboard helper) re-exports
// this and wires it to the real OSC 52 buffer / DOM selection / focus.

/**
 * Decide what text a terminal copy gesture should put on the clipboard.
 *
 * A real xterm selection always wins; the OSC 52 fallback buffer is consulted
 * (and drained) ONLY when the selection is empty — the mouse-mode TUI case
 * (claude), whose drag never creates an xterm selection.
 *
 * @param selectedText current `term.getSelection()` (`''` when none).
 * @param takeBuffer drains the OSC 52 fallback buffer (one-shot). Injected so
 *   tests can stub it; production passes a `takeRecentOsc52` closure.
 * @returns the text to copy, or `null` for a no-op. When a selection exists the
 *   buffer is left untouched (takeBuffer is not called), so an unrelated copy
 *   does not consume a stale OSC 52 payload.
 */
export function resolveTerminalCopyText(
  selectedText: string,
  takeBuffer: () => string | null,
): string | null {
  if (selectedText.length > 0) return selectedText;
  const buffered = takeBuffer();
  return buffered !== null && buffered.length > 0 ? buffered : null;
}

/**
 * The surface a Cmd/Ctrl+C gesture should copy from:
 *  - `terminal` — write `text` (an xterm selection or drained OSC 52 buffer).
 *  - `dom`      — a non-terminal document selection (note body / preview /
 *                 generic chrome) is the live selection; the caller copies it.
 *  - `none`     — nothing to copy.
 */
export type CopyDecision =
  | { kind: 'terminal'; text: string }
  | { kind: 'dom' }
  | { kind: 'none' };

export interface CopyDecisionInput {
  /**
   * A terminal surface currently holds DOM focus (its helper textarea is the
   * `document.activeElement`). This is the primary discriminator: it marks the
   * terminal as the surface the user last interacted with, so its selection /
   * OSC 52 buffer reflect the *current* highlight rather than a stale one.
   */
  terminalHasFocus: boolean;
  /** `getSelection()` of the focused terminal (`''` when none / not focused). */
  focusedTerminalSelection: string;
  /**
   * `getSelection()` of any terminal that still holds a lingering selection
   * (`''` when none). Consulted only when the terminal is NOT focused and there
   * is no competing DOM selection — a convenience so "copy the visible terminal
   * highlight" still works without re-focusing the pane.
   */
  lingeringTerminalSelection: string;
  /**
   * Trimmed non-terminal document selection (`window.getSelection()`), `''` when
   * none. xterm renders its selection on a canvas, so it is never part of the
   * document Selection — a non-empty value here is always a non-terminal (note /
   * preview / chrome) highlight.
   */
  domSelection: string;
  /** Drains the one-shot OSC 52 fallback buffer (see `takeRecentOsc52`). */
  takeBuffer: () => string | null;
}

/**
 * Resolve the copy source for a Cmd/Ctrl+C gesture, ordering by *where the user
 * is actually selecting* rather than always preferring the terminal.
 *
 * Priority (mirrors the Cmd/Ctrl+F unified resolver's principle — the live
 * selection wins; the terminal is not privileged just because it once held a
 * selection):
 *   (1) terminal focused → terminal source (its selection, else drained OSC 52
 *       buffer). The terminal is the active surface.
 *   (2) terminal NOT focused but a live non-terminal DOM selection exists → that
 *       DOM selection. This is the bug fix: a note-body drag moves focus off the
 *       terminal, so a stale `term.getSelection()` / OSC 52 buffer must NOT
 *       shadow it. The buffer is deliberately left intact (not drained) so a
 *       later terminal Cmd+C still finds it.
 *   (3) no DOM selection → a terminal's lingering selection, else drained OSC 52
 *       buffer, else nothing.
 *
 * `takeBuffer` is drained at most once (only one branch reaches it), so a stale
 * OSC 52 payload is never double-consumed.
 */
export function resolveCopyDecision(input: CopyDecisionInput): CopyDecision {
  if (input.terminalHasFocus) {
    return terminalOrBuffer(input.focusedTerminalSelection, input.takeBuffer);
  }
  if (input.domSelection.length > 0) return { kind: 'dom' };
  return terminalOrBuffer(input.lingeringTerminalSelection, input.takeBuffer);
}

function terminalOrBuffer(
  selection: string,
  takeBuffer: () => string | null,
): CopyDecision {
  if (selection.length > 0) return { kind: 'terminal', text: selection };
  const buffered = takeBuffer();
  return buffered !== null && buffered.length > 0
    ? { kind: 'terminal', text: buffered }
    : { kind: 'none' };
}
