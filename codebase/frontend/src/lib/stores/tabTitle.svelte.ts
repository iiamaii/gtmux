import {
  DEFAULT_TAB_TITLE_PREFERENCES,
  TAB_TITLE_STORAGE_KEY,
  loadTabTitlePreferences,
  parseTabTitlePreferences,
  saveTabTitlePreferences,
  type TabTitlePreferences,
} from './tabTitle';

function initialPreferences(): TabTitlePreferences {
  try {
    return loadTabTitlePreferences(window.localStorage);
  } catch {
    return { ...DEFAULT_TAB_TITLE_PREFERENCES };
  }
}

class TabTitleStore {
  preferences = $state<TabTitlePreferences>(initialPreferences());
  saveError = $state<string | null>(null);

  update(patch: Partial<TabTitlePreferences>): boolean {
    const next = { ...this.preferences, ...patch };
    try {
      saveTabTitlePreferences(window.localStorage, next);
      this.preferences = next;
      this.saveError = null;
      return true;
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      this.saveError = `Could not save browser tab preferences: ${reason}`;
      return false;
    }
  }

  // Each tab keeps its own active session; only the format/toggle is shared.
  listen(): () => void {
    const onStorage = (event: StorageEvent): void => {
      if (event.key !== null && event.key !== TAB_TITLE_STORAGE_KEY) return;
      try {
        if (event.storageArea !== window.localStorage) return;
      } catch {
        return;
      }
      this.preferences = parseTabTitlePreferences(event.newValue);
      this.saveError = null;
    };
    window.addEventListener('storage', onStorage);
    return () => window.removeEventListener('storage', onStorage);
  }
}

export const tabTitleStore = new TabTitleStore();
