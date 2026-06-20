// Chrome store — floating panel collapse state (plan 0005 Stage E,
// ADR-0017 §D7 + 2026-05-16 amends ② ③ "panel tabs both sides").
//
// Two floating panels — both follow the same shape (header tabs +
// PanelFoldButton + collapsed rail with per-tab icons):
//   - LeftPanel — Layers + Terminals + Files (left edge)
//   - RightPanel — Inspect + Preview (right edge).
// State persists in localStorage so the preference survives reload.
// Web-only state, no backend round-trip.

import { pathEditStore } from '$lib/stores/pathEditStore.svelte';
import { sessionStore } from '$lib/stores/sessionStore.svelte';

export type LeftPanelTab = 'layers' | 'terminals' | 'files';
export type RightPanelTab = 'inspect' | 'preview';

export type ChromeState = {
  sidebarCollapsed: boolean;
  leftPanelTab: LeftPanelTab;
  leftPanelWidth: number;
  paneInfoCollapsed: boolean;
  rightPanelTab: RightPanelTab;
  rightPanelWidth: number;
};

const STORAGE_KEY = 'gtmux-chrome';
const LEFT_PANEL_MIN_WIDTH = 230;
const LEFT_PANEL_MAX_WIDTH = 520;
const RIGHT_PANEL_MIN_WIDTH = 240;
const RIGHT_PANEL_MAX_WIDTH = 560;

const DEFAULT: ChromeState = {
  sidebarCollapsed: false,
  leftPanelTab: 'layers',
  leftPanelWidth: 268,
  paneInfoCollapsed: false,
  rightPanelTab: 'inspect',
  rightPanelWidth: 268,
};

class ChromeStore {
  state = $state<ChromeState>(resolveInitial());

  toggleSidebar(): void {
    this.state = { ...this.state, sidebarCollapsed: !this.state.sidebarCollapsed };
    this.persist();
  }

  /** Switch the active tab in the left panel. Always expands the panel
   *  too (matches the "rail icon click → expand + select" UX). */
  setLeftPanelTab(tab: LeftPanelTab): void {
    if (tab !== this.state.leftPanelTab) clearSelectionsForTabTransition('left', tab);
    this.state = {
      ...this.state,
      leftPanelTab: tab,
      rightPanelTab: rightPanelTabForLeft(tab),
      sidebarCollapsed: false,
    };
    this.persist();
  }

  togglePaneInfo(): void {
    this.state = { ...this.state, paneInfoCollapsed: !this.state.paneInfoCollapsed };
    this.persist();
  }

  /** Switch the active tab in the right panel and sync the left panel domain:
   *  Preview owns Files; Inspect owns Layers/Terminals. */
  setRightPanelTab(tab: RightPanelTab): void {
    const leftPanelTab = leftPanelTabForRight(tab, this.state.leftPanelTab);
    if (tab !== this.state.rightPanelTab || leftPanelTab !== this.state.leftPanelTab) {
      clearSelectionsForTabTransition('right', tab);
    }
    this.state = {
      ...this.state,
      leftPanelTab,
      rightPanelTab: tab,
      sidebarCollapsed: false,
      paneInfoCollapsed: false,
    };
    this.persist();
  }

  setLeftPanelWidth(width: number): void {
    this.state = {
      ...this.state,
      leftPanelWidth: clamp(width, LEFT_PANEL_MIN_WIDTH, LEFT_PANEL_MAX_WIDTH),
    };
    this.persist();
  }

  setRightPanelWidth(width: number): void {
    this.state = {
      ...this.state,
      rightPanelWidth: clamp(width, RIGHT_PANEL_MIN_WIDTH, RIGHT_PANEL_MAX_WIDTH),
    };
    this.persist();
  }

  /** Force a specific state — used by tests / scripted demos. */
  set(next: Partial<ChromeState>): void {
    const normalized = normalizeState({ ...this.state, ...next });
    if (normalized.leftPanelTab !== this.state.leftPanelTab) {
      clearSelectionsForTabTransition('left', normalized.leftPanelTab);
    } else if (normalized.rightPanelTab !== this.state.rightPanelTab) {
      clearSelectionsForTabTransition('right', normalized.rightPanelTab);
    }
    this.state = normalized;
    this.persist();
  }

  private persist(): void {
    if (typeof localStorage === 'undefined') return;
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.state));
    } catch (e) {
      console.debug('[gtmux] chrome persist failed', e);
    }
  }
}

function resolveInitial(): ChromeState {
  if (typeof window === 'undefined') return DEFAULT;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) return DEFAULT;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== 'object' || parsed === null) return DEFAULT;
    const obj = parsed as Record<string, unknown>;
    const leftTab = obj.leftPanelTab;
    const rightTab = obj.rightPanelTab;
    return normalizeState({
      sidebarCollapsed:
        typeof obj.sidebarCollapsed === 'boolean'
          ? obj.sidebarCollapsed
          : DEFAULT.sidebarCollapsed,
      leftPanelTab:
        leftTab === 'layers' || leftTab === 'terminals' || leftTab === 'files'
          ? leftTab
          : DEFAULT.leftPanelTab,
      paneInfoCollapsed:
        typeof obj.paneInfoCollapsed === 'boolean'
          ? obj.paneInfoCollapsed
          : DEFAULT.paneInfoCollapsed,
      rightPanelTab:
        rightTab === 'inspect' || rightTab === 'preview' ? rightTab : DEFAULT.rightPanelTab,
      leftPanelWidth:
        typeof obj.leftPanelWidth === 'number'
          ? obj.leftPanelWidth
          : DEFAULT.leftPanelWidth,
      rightPanelWidth:
        typeof obj.rightPanelWidth === 'number'
          ? obj.rightPanelWidth
          : DEFAULT.rightPanelWidth,
    });
  } catch (e) {
    console.debug('[gtmux] chrome read failed', e);
    return DEFAULT;
  }
}

function normalizeState(state: ChromeState): ChromeState {
  return {
    ...state,
    leftPanelWidth: clamp(state.leftPanelWidth, LEFT_PANEL_MIN_WIDTH, LEFT_PANEL_MAX_WIDTH),
    rightPanelTab: rightPanelTabForLeft(state.leftPanelTab),
    rightPanelWidth: clamp(state.rightPanelWidth, RIGHT_PANEL_MIN_WIDTH, RIGHT_PANEL_MAX_WIDTH),
  };
}

function rightPanelTabForLeft(tab: LeftPanelTab): RightPanelTab {
  return tab === 'files' ? 'preview' : 'inspect';
}

function leftPanelTabForRight(tab: RightPanelTab, current: LeftPanelTab): LeftPanelTab {
  if (tab === 'preview') return 'files';
  return current === 'files' ? 'layers' : current;
}

function clearSelectionsForTabTransition(side: 'left', tab: LeftPanelTab): void;
function clearSelectionsForTabTransition(side: 'right', tab: RightPanelTab): void;
function clearSelectionsForTabTransition(side: 'left' | 'right', tab: LeftPanelTab | RightPanelTab): void {
  // ADR-0046 D6 amend ⑪: tab transitions only clear canvas selection (M + drill).
  // The Files selection (filePreviewStore) PERSISTS across left/right tab transitions
  // and across canvas-component selection, so returning to the Files / Preview tab
  // re-displays the previously-selected item. Clearing the Files selection is the
  // responsibility of an empty-area click (Canvas / Files panel) or a
  // session/workspace change — not of tab switching.
  void side;
  void tab;
  clearCanvasSelectionState();
}

function clearCanvasSelectionState(): void {
  pathEditStore.end();
  sessionStore.clearDrill();
  sessionStore.clearM();
}

function clamp(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min;
  return Math.min(max, Math.max(min, Math.round(value)));
}

export const chromeStore = new ChromeStore();
