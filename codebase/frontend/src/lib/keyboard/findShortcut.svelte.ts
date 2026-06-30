// findShortcut — Cmd/Ctrl+F overrides the browser's native Find and opens the
// left-panel search instead (ADR-0052 D2, 2026-06-30 amend).
//
// Routing (priority):
//   (B) terminal drag-selection present → switch to Files, set Files query =
//       selection, focus + select-all (terminal text is usually a path/name).
//   (A) no selection → focus the CURRENT tab's search input; if the system
//       clipboard has text, autofill it (select-all). Failure/empty clipboard
//       degrades to focus-only.
//
// Registered as a protected default action with `allowInXterm`/`allowInEditable`
// true so it still fires while a terminal or input is focused (case B needs the
// xterm to be focused; case A may fire from anywhere). The registry calls
// `preventDefault()` when a handler returns true, which is what suppresses the
// browser Find dialog.

import { readTextFromSystemClipboard } from '$lib/clipboard/textClipboard';
import { requestLeftPanelSearch } from '$lib/sidebar/leftPanelSearchController';
import { currentTerminalSelection } from './terminalCopyShortcut';
import { shortcutRegistry } from './shortcutRegistry.svelte';

/** Class of the LeftPanel footer search input (LeftPanel.svelte). */
const SEARCH_INPUT_CLASS = 'footer-search-input';

/** True when the left-panel search input already holds focus. */
function searchInputAlreadyFocused(): boolean {
  if (typeof document === 'undefined') return false;
  const el = document.activeElement;
  return el instanceof HTMLInputElement && el.classList.contains(SEARCH_INPUT_CLASS);
}

function handleFind(event: KeyboardEvent): boolean {
  const selection = currentTerminalSelection();

  // (B) Terminal has a drag-selection → route it into the Files search.
  if (selection.length > 0) {
    requestLeftPanelSearch({
      tab: 'files',
      query: selection,
      focus: true,
      selectAll: true,
    });
    // Returning true makes the registry preventDefault the browser Find.
    return true;
  }

  // Edge nicety: already typing in the search input → just select-all the
  // existing query rather than overwriting it from the clipboard.
  if (searchInputAlreadyFocused()) {
    requestLeftPanelSearch({ focus: true, selectAll: true });
    return true;
  }

  // (A) No selection → focus the current tab's search immediately, then try to
  // autofill from the clipboard asynchronously (best-effort; '' = no-op).
  requestLeftPanelSearch({ focus: true });
  void readTextFromSystemClipboard().then((text) => {
    if (text.length > 0) {
      requestLeftPanelSearch({ query: text, focus: true, selectAll: true });
    }
  });

  // Defensive: also preventDefault here. The registry already does this for a
  // consumed shortcut, but Find is a browser-native key we must reliably block.
  event.preventDefault();
  return true;
}

/**
 * Register `find.focus_search` for both Cmd+F (macOS) and Ctrl+F (Win/Linux),
 * mirroring the cross-platform pair pattern in `bindGlobalTerminalCopyShortcut`.
 * Returns an unregister callback.
 */
export function bindGlobalFindShortcut(): () => void {
  if (typeof window === 'undefined') return () => {};
  const unsubs = [
    shortcutRegistry.register({
      actionId: 'find.focus_search',
      key: 'f',
      meta: true,
      allowInXterm: true,
      allowInEditable: true,
      customizable: false,
      protectedReason: 'Browser Find override.',
      category: 'Search',
      description: 'Find / focus search',
      handler: handleFind,
    }),
    shortcutRegistry.register({
      actionId: 'find.focus_search',
      key: 'f',
      ctrl: true,
      allowInXterm: true,
      allowInEditable: true,
      customizable: false,
      protectedReason: 'Browser Find override.',
      category: 'Search',
      description: 'Find / focus search (Win/Linux)',
      handler: handleFind,
    }),
  ];

  return () => {
    for (const unsubscribe of unsubs) unsubscribe();
  };
}
