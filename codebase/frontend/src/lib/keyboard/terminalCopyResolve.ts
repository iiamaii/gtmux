// Pure copy-decision core for the terminal Cmd+C shortcut (ADR-0049 D7).
//
// Kept in its own module — free of any Svelte rune store or DOM import — so it
// is unit-testable under the node vitest environment. `terminalCopyShortcut.ts`
// (which transitively pulls in rune stores via the clipboard helper) re-exports
// this and wires it to the real OSC 52 buffer.

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
