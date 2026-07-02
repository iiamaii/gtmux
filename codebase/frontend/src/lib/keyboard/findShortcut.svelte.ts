// findShortcut — Cmd/Ctrl+F overrides the browser's native Find and opens the
// left-panel search instead (ADR-0052 D2, 2026-06-30 amend; cross-surface +
// no-clipboard 2026-07-01 amend).
//
// Routing (priority):
//   (1) terminal drag-selection → Files tab, query = selection. Terminal is the
//       ONLY surface that always routes to Files (its text is usually a path);
//       every other surface stays in the current tab.
//   (2) already in the search input → select-all the existing query.
//   (3) any other highlight (Preview / document node / generic DOM) → CURRENT
//       tab, query = highlight.
//   (4) nothing selected → focus the CURRENT tab's search input, no autofill.
//
// Consistency axis (user, 2026-07-01): the query source is always the *selection*
// (never the clipboard); only terminal gets the Files-tab special-case.
//
// Registered as a protected default action with `allowInXterm`/`allowInEditable`
// true so it still fires while a terminal or input is focused (case B needs the
// xterm to be focused; case A may fire from anywhere). The registry calls
// `preventDefault()` when a handler returns true, which is what suppresses the
// browser Find dialog.

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

/**
 * The document's DOM selection (Preview `.preview-surface`, document nodes,
 * generic chrome), trimmed. One catch-all covers every non-terminal surface —
 * they are all plain DOM selections, so no per-source accessors are needed.
 * (xterm renders its selection on a canvas, so it is read via its own provider,
 * not this.)
 *
 * Form-control (`<input>`/`<textarea>`) selections are intentionally excluded:
 * they are not part of the document Selection, so the left-panel search input's
 * own text never round-trips back into a query.
 */
function domSelectionText(): string {
  if (typeof window === 'undefined') return '';
  return window.getSelection()?.toString().trim() ?? '';
}

function handleFind(event: KeyboardEvent): boolean {
  // (1) Terminal drag-selection → Files tab. Terminal is the sole surface that
  // always routes to Files (its text is usually a path/name); every other
  // surface stays in the current tab.
  const terminal = currentTerminalSelection();
  if (terminal.length > 0) {
    requestLeftPanelSearch({ tab: 'files', query: terminal, focus: true, selectAll: true });
    // Returning true makes the registry preventDefault the browser Find.
    return true;
  }

  // (2) Already typing in the search input → just select-all the existing query
  // rather than replacing it (repeated Cmd/Ctrl+F re-selects for easy edit).
  if (searchInputAlreadyFocused()) {
    requestLeftPanelSearch({ focus: true, selectAll: true });
    return true;
  }

  // (3) Any other highlight (Preview / document node / generic DOM) → search the
  // CURRENT tab with the highlighted text (mirrors the terminal path, minus the
  // Files special-case). `requestLeftPanelSearch` defaults to the current tab.
  const highlight = domSelectionText();
  if (highlight.length > 0) {
    requestLeftPanelSearch({ query: highlight, focus: true, selectAll: true });
    event.preventDefault();
    return true;
  }

  // (4) Nothing selected → just focus the current tab's search input. No
  // clipboard autofill (removed 2026-07-01 amend): the query source is always
  // the selection, so with nothing selected we only focus.
  requestLeftPanelSearch({ focus: true });
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
