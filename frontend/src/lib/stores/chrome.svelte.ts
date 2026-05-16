// Chrome store — floating panel collapse state (plan 0005 Stage E,
// ADR-0017 §D7 + 2026-05-16 amends ② ③ "panel tabs both sides").
//
// Two floating panels — both follow the same shape (header tabs +
// PanelFoldButton + collapsed rail with per-tab icons):
//   - LeftPanel — Layers + Terminals (left edge)
//   - RightPanel — Inspect (right edge, single tab for now; ref leaves
//     room for Design / Prototype / Inspect — currently we only need
//     Inspect, but the tab chrome stays for future growth).
// State persists in localStorage so the preference survives reload.
// Web-only state, no backend round-trip.

export type LeftPanelTab = 'layers' | 'terminals';
export type RightPanelTab = 'inspect';

export type ChromeState = {
  sidebarCollapsed: boolean;
  leftPanelTab: LeftPanelTab;
  paneInfoCollapsed: boolean;
  rightPanelTab: RightPanelTab;
};

const STORAGE_KEY = 'gtmux-chrome';

const DEFAULT: ChromeState = {
  sidebarCollapsed: false,
  leftPanelTab: 'layers',
  paneInfoCollapsed: false,
  rightPanelTab: 'inspect',
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
    this.state = { ...this.state, leftPanelTab: tab, sidebarCollapsed: false };
    this.persist();
  }

  togglePaneInfo(): void {
    this.state = { ...this.state, paneInfoCollapsed: !this.state.paneInfoCollapsed };
    this.persist();
  }

  /** Switch the active tab in the right panel. Always expands the panel
   *  too (matches the LeftPanel rail UX). Currently a single tab, but
   *  the chrome stays symmetric so adding tabs later is purely additive. */
  setRightPanelTab(tab: RightPanelTab): void {
    this.state = { ...this.state, rightPanelTab: tab, paneInfoCollapsed: false };
    this.persist();
  }

  /** Force a specific state — used by tests / scripted demos. */
  set(next: Partial<ChromeState>): void {
    this.state = { ...this.state, ...next };
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
    return {
      sidebarCollapsed:
        typeof obj.sidebarCollapsed === 'boolean'
          ? obj.sidebarCollapsed
          : DEFAULT.sidebarCollapsed,
      leftPanelTab:
        leftTab === 'layers' || leftTab === 'terminals' ? leftTab : DEFAULT.leftPanelTab,
      paneInfoCollapsed:
        typeof obj.paneInfoCollapsed === 'boolean'
          ? obj.paneInfoCollapsed
          : DEFAULT.paneInfoCollapsed,
      rightPanelTab: rightTab === 'inspect' ? rightTab : DEFAULT.rightPanelTab,
    };
  } catch (e) {
    console.debug('[gtmux] chrome read failed', e);
    return DEFAULT;
  }
}

export const chromeStore = new ChromeStore();
