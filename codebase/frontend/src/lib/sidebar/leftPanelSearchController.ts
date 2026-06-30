// leftPanelSearchController — single-controller registry that lets global
// shortcuts (Cmd/Ctrl+F, ADR-0052 D2) drive the LeftPanel search bar without a
// hard import into the component.
//
// Mirrors the terminal-copy provider pattern (`terminalCopyShortcut.ts`): the
// component registers itself on mount and unregisters on destroy; callers go
// through `requestLeftPanelSearch`. Unlike the copy providers, only ONE
// LeftPanel exists, so this is a single-slot registry rather than a Set.

import { tick } from 'svelte';
import { chromeStore, type LeftPanelTab } from '$lib/stores/chrome.svelte';

export interface LeftPanelSearchController {
  /** Set the search query for a given tab (writes `searchByTab[tab]`). */
  setQuery(tab: LeftPanelTab, query: string): void;
  /** Focus the footer search input; optionally select-all its text. */
  focusSearch(opts?: { selectAll?: boolean }): void;
  /** The currently-active left-panel tab. */
  currentTab(): LeftPanelTab;
}

let controller: LeftPanelSearchController | null = null;

/**
 * Register the active LeftPanel controller. The latest registration wins.
 * Returns an unregister callback that only clears the slot if this controller
 * is still the current one (guards against a stale unmount clobbering a fresh
 * remount).
 */
export function registerLeftPanelSearchController(c: LeftPanelSearchController): () => void {
  controller = c;
  return () => {
    if (controller === c) controller = null;
  };
}

/**
 * Drive the left-panel search from a shortcut. No-op (returns `false`) when no
 * LeftPanel is mounted. Otherwise:
 *   1. Resolve the target tab (`req.tab ?? controller.currentTab()`).
 *   2. Switch to it via `chromeStore.setLeftPanelTab` (also expands the rail).
 *   3. If `req.query !== undefined`, set that tab's query.
 *   4. If `req.focus`, focus (and optionally select-all) the search input.
 *
 * Focus timing: when the panel was collapsed, `setLeftPanelTab` expands it and
 * the footer input only mounts on the next tick — so the focus step runs after
 * `await tick()` to guarantee `searchInputEl` exists.
 */
export function requestLeftPanelSearch(req: {
  tab?: LeftPanelTab;
  query?: string;
  focus?: boolean;
  selectAll?: boolean;
}): boolean {
  const c = controller;
  if (c === null) return false;

  const targetTab = req.tab ?? c.currentTab();
  chromeStore.setLeftPanelTab(targetTab); // switches tab AND expands the rail.

  if (req.query !== undefined) c.setQuery(targetTab, req.query);

  if (req.focus) {
    // Defer to the next tick so the footer input is mounted after an expand.
    void tick().then(() => {
      // Re-read the controller in case it was swapped/cleared meanwhile.
      const live = controller;
      if (live === null) return;
      live.focusSearch({ selectAll: req.selectAll });
    });
  }

  return true;
}
