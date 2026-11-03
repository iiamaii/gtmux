// Browser presentation preference; never part of a session layout or server config.
export interface TabTitlePreferences {
  enabled: boolean;
  format: string;
}

export const TAB_TITLE_STORAGE_KEY = 'gtmux-tab-title:v1';
export const TAB_TITLE_FORMAT_MAX_LENGTH = 120;
export const DEFAULT_TAB_TITLE_PREFERENCES: Readonly<TabTitlePreferences> = {
  enabled: true,
  format: '{app} - {session}',
};

export function validateTabTitleFormat(format: string): string | null {
  if (!format.trim()) return 'Enter a title format.';
  if (format.length > TAB_TITLE_FORMAT_MAX_LENGTH) {
    return `Use at most ${TAB_TITLE_FORMAT_MAX_LENGTH} characters.`;
  }
  if (/[\u0000-\u001f\u007f]/.test(format)) return 'Use a single line without control characters.';
  if (/[{}]/.test(format.replace(/\{(?:app|session)\}/g, ''))) {
    return 'Only {app} and {session} placeholders are supported.';
  }
  if (!format.includes('{session}')) return 'Include {session} to identify this tab.';
  return null;
}

export function parseTabTitlePreferences(raw: string | null): TabTitlePreferences {
  const fallback = { ...DEFAULT_TAB_TITLE_PREFERENCES };
  if (raw === null) return fallback;
  try {
    const value: unknown = JSON.parse(raw);
    if (typeof value !== 'object' || value === null || !('version' in value) || value.version !== 1) {
      return fallback;
    }
    if (!('enabled' in value) || typeof value.enabled !== 'boolean'
      || !('format' in value) || typeof value.format !== 'string'
      || validateTabTitleFormat(value.format) !== null) return fallback;
    return { enabled: value.enabled, format: value.format };
  } catch {
    return fallback;
  }
}

export function loadTabTitlePreferences(storage: Pick<Storage, 'getItem'>): TabTitlePreferences {
  try {
    return parseTabTitlePreferences(storage.getItem(TAB_TITLE_STORAGE_KEY));
  } catch {
    return { ...DEFAULT_TAB_TITLE_PREFERENCES };
  }
}

// Write before the caller applies the state, so a rejected write cannot look saved.
export function saveTabTitlePreferences(
  storage: Pick<Storage, 'setItem'>,
  preferences: TabTitlePreferences,
): void {
  const error = validateTabTitleFormat(preferences.format);
  if (error !== null) throw new Error(error);
  storage.setItem(TAB_TITLE_STORAGE_KEY, JSON.stringify({ version: 1, ...preferences }));
}

export function formatTabTitle(sessionName: string | null | undefined, preferences: TabTitlePreferences): string {
  if (!preferences.enabled || !sessionName) return 'gtmux';
  const format = validateTabTitleFormat(preferences.format) === null
    ? preferences.format : DEFAULT_TAB_TITLE_PREFERENCES.format;
  // A callback keeps session names literal, including '$&' and placeholder-like text.
  return format.replace(/\{(app|session)\}/g, (_, key: string) => key === 'app' ? 'gtmux' : sessionName).trim();
}
