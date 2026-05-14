// Chrome store — floating panel collapse state (plan 0005 Stage E,
// ADR-0017 §D7).
//
// Sidebar (left, 248px) and PaneInfoPanel (right, 268px) are both
// collapsible via RailToggle buttons. State persists in localStorage so
// the user's preference survives page reload. Web-only state, no
// backend round-trip.

export type ChromeState = {
  sidebarCollapsed: boolean;
  paneInfoCollapsed: boolean;
};

const STORAGE_KEY = 'gtmux-chrome';

const DEFAULT: ChromeState = {
  sidebarCollapsed: false,
  paneInfoCollapsed: false,
};

class ChromeStore {
  state = $state<ChromeState>(resolveInitial());

  toggleSidebar(): void {
    this.state = { ...this.state, sidebarCollapsed: !this.state.sidebarCollapsed };
    this.persist();
  }

  togglePaneInfo(): void {
    this.state = { ...this.state, paneInfoCollapsed: !this.state.paneInfoCollapsed };
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
    return {
      sidebarCollapsed:
        typeof obj.sidebarCollapsed === 'boolean'
          ? obj.sidebarCollapsed
          : DEFAULT.sidebarCollapsed,
      paneInfoCollapsed:
        typeof obj.paneInfoCollapsed === 'boolean'
          ? obj.paneInfoCollapsed
          : DEFAULT.paneInfoCollapsed,
    };
  } catch (e) {
    console.debug('[gtmux] chrome read failed', e);
    return DEFAULT;
  }
}

export const chromeStore = new ChromeStore();
