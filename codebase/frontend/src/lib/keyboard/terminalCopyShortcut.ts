import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
import { OSC52_FALLBACK_TTL_MS, peekRecentOsc52, takeRecentOsc52 } from '$lib/xterm/osc52';
import { resolveCopyDecision, resolveTerminalCopyText } from './terminalCopyResolve';
import { shortcutRegistry } from './shortcutRegistry.svelte';

export { resolveTerminalCopyText };

export interface TerminalCopyProvider {
  containsFocus: () => boolean;
  getSelection: () => string;
}

const providers = new Set<TerminalCopyProvider>();

export function isTerminalCopyShortcut(e: KeyboardEvent): boolean {
  return (e.ctrlKey || e.metaKey) && e.shiftKey && !e.altKey && e.key.toLowerCase() === 'c';
}

function isTerminalSelectionCopyShortcut(e: KeyboardEvent): boolean {
  return (e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'c';
}

export function registerTerminalCopyProvider(provider: TerminalCopyProvider): () => void {
  providers.add(provider);
  return () => {
    providers.delete(provider);
  };
}

function focusedProvider(): TerminalCopyProvider | null {
  for (const provider of providers) {
    if (provider.containsFocus()) return provider;
  }
  return null;
}

function providerWithSelection(): TerminalCopyProvider | null {
  for (const provider of providers) {
    if (provider.getSelection().length > 0) return provider;
  }
  return null;
}

/**
 * The current non-terminal document selection (`window.getSelection()`) — a note
 * body, Preview surface, or generic chrome highlight. xterm draws its selection
 * on a canvas, so it is never part of the document Selection; a non-empty value
 * here always denotes a non-terminal highlight. Form-control (`<input>` /
 * `<textarea>`) selections are intentionally NOT part of the document Selection,
 * so a focused search box's own text never leaks into a copy decision.
 *
 * Returned raw (untrimmed) so the copy preserves the user's exact selection; the
 * caller trims only for the "is there a selection?" test.
 */
function domSelectionRaw(): string {
  if (typeof window === 'undefined') return '';
  return window.getSelection()?.toString() ?? '';
}

/**
 * Terminal "current highlight" text for the Cmd/Ctrl+F find shortcut
 * (ADR-0052 D2), or `''` when there is none. Mirrors the copy shortcut's source
 * resolution so find and copy agree on what is "selected":
 *   1. a native xterm drag-selection (a normal shell drag), else
 *   2. the gated OSC 52 fallback buffer — a mouse-mode TUI (claude) whose drag
 *      makes no xterm selection but writes its highlight via OSC 52.
 *
 * The OSC 52 buffer is only ever filled past the consent gate (ADR-0049 D3:
 * setting ON + secure context), so this is automatically settings-gated — no
 * extra check. Unlike copy, find PEEKS (non-draining): a search must not consume
 * the one-shot buffer a following Cmd+C copy relies on.
 */
export function currentTerminalSelection(): string {
  const native = providerWithSelection()?.getSelection() ?? '';
  if (native.length > 0) return native;
  return peekRecentOsc52(OSC52_FALLBACK_TTL_MS, performance.now()) ?? '';
}

export function bindGlobalTerminalCopyShortcut(): () => void {
  if (typeof window === 'undefined') return () => {};
  const shortcutUnsubs = [
    shortcutRegistry.register({
      actionId: 'terminal.copy_selection',
      key: 'c',
      meta: true,
      shift: true,
      description: 'Copy terminal selection',
      category: 'Terminal',
      customizable: false,
      protectedReason: 'Capture-phase browser conflict guard; not rebindable.',
      handler: () => false,
    }),
    shortcutRegistry.register({
      actionId: 'terminal.copy_selection',
      key: 'c',
      ctrl: true,
      shift: true,
      description: 'Copy terminal selection (Win/Linux)',
      category: 'Terminal',
      customizable: false,
      protectedReason: 'Capture-phase browser conflict guard; not rebindable.',
      handler: () => false,
    }),
  ];

  const onKeyDown = (e: KeyboardEvent): void => {
    if (!isTerminalSelectionCopyShortcut(e)) return;

    const mustBlockBrowserShortcut = isTerminalCopyShortcut(e);

    // Copy-source parity with the Cmd/Ctrl+F resolver: order by WHERE the user is
    // selecting, not by always preferring the terminal. `focusedProvider()` marks
    // the terminal as the active surface; `providerWithSelection()` is only a
    // lingering-selection convenience consulted when nothing else claims the
    // gesture. A non-terminal DOM selection (note body, Preview, chrome) must not
    // be shadowed by a stale `term.getSelection()` / OSC 52 buffer.
    const focused = focusedProvider();
    const dom = domSelectionRaw();

    // ADR-0049 D7 — the OSC 52 buffer is drained here, inside the keydown gesture,
    // so the async clipboard write runs under transient activation. It is only
    // ever filled past the OSC 52 gate (consent ON + secure), so consent-off
    // sessions resolve to null. `resolveCopyDecision` drains it at most once and
    // leaves it intact when a DOM selection wins (so a later terminal Cmd+C still
    // finds it).
    const decision = resolveCopyDecision({
      terminalHasFocus: focused !== null,
      focusedTerminalSelection: focused?.getSelection() ?? '',
      lingeringTerminalSelection: providerWithSelection()?.getSelection() ?? '',
      // Cmd/Ctrl+Shift+C is the explicit terminal-copy gesture — ignore any DOM
      // selection so it always targets the terminal (and still blocks its browser
      // shortcut below). Plain Cmd/Ctrl+C honors a non-terminal DOM selection.
      domSelection: mustBlockBrowserShortcut ? '' : dom.trim(),
      takeBuffer: () => takeRecentOsc52(OSC52_FALLBACK_TTL_MS, performance.now()),
    });

    if (decision.kind === 'dom') {
      // A live non-terminal DOM selection (note body / Preview / chrome) is what
      // the user just highlighted — it must overwrite the clipboard, not the
      // stale terminal selection / OSC 52 buffer. Write it explicitly (raw, to
      // preserve the exact selection) under the keydown gesture and block the
      // default so there is no double copy.
      e.preventDefault();
      e.stopImmediatePropagation();
      void copyTextToSystemClipboard(dom).then((result) => {
        if (!result.ok) {
          console.debug('[gtmux] note/DOM copy failed', result.reason ?? 'Clipboard copy failed');
        }
      });
      return;
    }

    // Plain Cmd+C with nothing to copy (no selection AND no fresh buffer):
    // return WITHOUT preventDefault so the browser passthrough is preserved
    // (unchanged from the pre-D7 behavior). Cmd+Shift+C still proceeds to block
    // the browser shortcut even with nothing to copy.
    if (decision.kind === 'none') {
      if (!mustBlockBrowserShortcut) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      return; // Cmd+Shift+C: blocked browser shortcut, no copy.
    }

    e.preventDefault();
    e.stopImmediatePropagation();
    void copyTextToSystemClipboard(decision.text).then((result) => {
      if (!result.ok) {
        console.debug('[gtmux] terminal copy failed', result.reason ?? 'Clipboard copy failed');
      }
    });
  };

  window.addEventListener('keydown', onKeyDown, { capture: true });
  return () => {
    window.removeEventListener('keydown', onKeyDown, { capture: true });
    for (const unsubscribe of shortcutUnsubs) unsubscribe();
  };
}
