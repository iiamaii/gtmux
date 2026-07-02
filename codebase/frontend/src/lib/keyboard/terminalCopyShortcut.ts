import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
import { OSC52_FALLBACK_TTL_MS, peekRecentOsc52, takeRecentOsc52 } from '$lib/xterm/osc52';
import { resolveTerminalCopyText } from './terminalCopyResolve';
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
    const provider = focusedProvider() ?? providerWithSelection();
    const selectedText = provider?.getSelection() ?? '';

    // ADR-0049 D7 — a real selection wins; otherwise fall back to a fresh OSC 52
    // buffer (mouse-mode TUI like claude, whose drag never makes an xterm
    // selection). The buffer is drained here, inside the keydown gesture, so the
    // async clipboard write runs under transient activation. The buffer is only
    // ever filled past the OSC 52 gate (consent ON + secure), so consent-off
    // sessions resolve to null and this stays a no-op.
    const copyText = resolveTerminalCopyText(selectedText, () =>
      takeRecentOsc52(OSC52_FALLBACK_TTL_MS, performance.now()),
    );

    // Plain Cmd+C with nothing to copy (no selection AND no fresh buffer):
    // return WITHOUT preventDefault so the browser passthrough is preserved
    // (unchanged from the pre-D7 behavior). Cmd+Shift+C still proceeds to block
    // the browser shortcut even with nothing to copy.
    if (!mustBlockBrowserShortcut && copyText === null) return;

    e.preventDefault();
    e.stopImmediatePropagation();

    if (copyText === null) return; // Cmd+Shift+C: blocked browser shortcut, no copy.

    void copyTextToSystemClipboard(copyText).then((result) => {
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
