import { describe, expect, it } from 'vitest';
import {
  DEFAULT_TAB_TITLE_PREFERENCES as defaults,
  TAB_TITLE_STORAGE_KEY,
  formatTabTitle,
  loadTabTitlePreferences,
  parseTabTitlePreferences,
  saveTabTitlePreferences,
  validateTabTitleFormat,
} from './tabTitle';

describe('browser tab titles', () => {
  it('follows the active session and returns to the app title after detach', () => {
    expect(formatTabTitle('build', defaults)).toBe('gtmux - build');
    expect(formatTabTitle('review', defaults)).toBe('gtmux - review');
    expect(formatTabTitle(null, defaults)).toBe('gtmux');
    expect(formatTabTitle(undefined, defaults)).toBe('gtmux');
  });

  it('can hide session names without losing the chosen format', () => {
    const preferences = { enabled: false, format: '{session} | {app}' };
    expect(formatTabTitle('build', preferences)).toBe('gtmux');
    expect(formatTabTitle('build', { ...preferences, enabled: true })).toBe('build | gtmux');
  });

  it('keeps independent session names when tabs share preferences', () => {
    const preferences = { enabled: true, format: '{session} | {app}' };
    expect(formatTabTitle('alpha', preferences)).toBe('alpha | gtmux');
    expect(formatTabTitle('beta', preferences)).toBe('beta | gtmux');
  });

  it('substitutes names literally without expanding replacement tokens or markup', () => {
    expect(formatTabTitle('$& {app} <b>한글</b>', defaults)).toBe('gtmux - $& {app} <b>한글</b>');
  });

  it('supports repeated placeholders and surrounding whitespace', () => {
    expect(formatTabTitle('work', { enabled: true, format: ' {session} / {session} / {app} ' }))
      .toBe('work / work / gtmux');
  });

  it('rejects formats that cannot identify a session or use unsupported placeholders', () => {
    for (const format of ['', '   ', '{app}', '{unknown} {session}', '{{session}}', '{session}\n']) {
      expect(validateTabTitleFormat(format)).not.toBeNull();
    }
    expect(validateTabTitleFormat('{session}')).toBeNull();
    expect(validateTabTitleFormat('x'.repeat(111) + '{session}')).toBeNull();
    expect(validateTabTitleFormat('x'.repeat(112) + '{session}')).not.toBeNull();
  });

  it('uses a usable title when supplied a damaged format', () => {
    expect(formatTabTitle('work', { enabled: true, format: '{unknown}' })).toBe('gtmux - work');
  });
});

describe('browser title preference persistence', () => {
  it('round-trips the toggle and custom format through storage', () => {
    const entries = new Map<string, string>();
    const storage = {
      getItem: (key: string) => entries.get(key) ?? null,
      setItem: (key: string, value: string) => { entries.set(key, value); },
    };
    const preferences = { enabled: false, format: '{session} | {app}' };
    saveTabTitlePreferences(storage, preferences);
    expect(loadTabTitlePreferences(storage)).toEqual(preferences);
    expect(JSON.parse(entries.get(TAB_TITLE_STORAGE_KEY)!)).toEqual({ version: 1, ...preferences });
  });

  it('recovers from missing, corrupted, unsupported and incorrectly typed data', () => {
    for (const raw of [null, '', 'null', '[]', 'true', '{', '{}',
      '{"version":2,"enabled":false,"format":"{session}"}',
      '{"version":1,"enabled":"false","format":"{session}"}',
      '{"version":1,"enabled":true,"format":"{unknown}"}',
    ]) expect(parseTabTitlePreferences(raw)).toEqual(defaults);
  });

  it('loads defaults when storage reads are denied', () => {
    expect(loadTabTitlePreferences({ getItem: () => { throw new Error('Access denied'); } })).toEqual(defaults);
  });

  it('propagates write failures so callers do not claim a preference was saved', () => {
    expect(() => saveTabTitlePreferences({ setItem: () => { throw new Error('Quota exceeded'); } }, defaults))
      .toThrow('Quota exceeded');
  });

  it('rejects invalid formats before touching storage', () => {
    let writes = 0;
    expect(() => saveTabTitlePreferences({ setItem: () => { writes++; } }, { enabled: true, format: '' }))
      .toThrow('Enter a title format.');
    expect(writes).toBe(0);
  });
});
