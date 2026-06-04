import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
import { shortcutRegistry } from './shortcutRegistry.svelte';

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
    if (!mustBlockBrowserShortcut && selectedText.length === 0) return;

    e.preventDefault();
    e.stopImmediatePropagation();

    if (selectedText.length === 0) return;

    void copyTextToSystemClipboard(selectedText).then((result) => {
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
