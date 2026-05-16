// Theme store — light / dark / system (ADR-0016 v2 + 2026-05-16 G27 amend).
//
// Concepts:
//   - `mode` is the *user choice*: 'system' | 'light' | 'dark'.
//   - `theme` is the *resolved* concrete theme: 'light' | 'dark'.
//     When `mode === 'system'`, it follows the OS preference via
//     `matchMedia('(prefers-color-scheme: dark)')`.
//
// Source of truth: in-memory `mode` state. Side effects:
//   1. localStorage `gtmux-theme` ← persistence across reloads
//   2. <html> classList `dark` toggle ← CSS tokens flip
//
// Initial value priority:
//   1. localStorage `gtmux-theme` (user's explicit choice from prior session;
//      one of 'system' | 'light' | 'dark')
//   2. fallback: 'system' (let OS decide — matches user expectations for
//      modern apps)
//
// The class application also runs in a tiny inline script in `index.html`
// *before* Svelte hydrates, so the first paint never shows the wrong theme
// (FOUC guard). This store then takes over for subsequent toggles.

export type Theme = 'light' | 'dark';
export type ThemeMode = 'system' | 'light' | 'dark';

const STORAGE_KEY = 'gtmux-theme';
const DARK_CLASS = 'dark';
const MEDIA_QUERY = '(prefers-color-scheme: dark)';

function systemPrefersDark(): boolean {
  if (typeof window === 'undefined') return true;
  try {
    return window.matchMedia(MEDIA_QUERY).matches;
  } catch {
    return true;
  }
}

class ThemeStore {
  /** User-chosen mode. `system` defers to the OS preference. */
  mode = $state<ThemeMode>(resolveInitialMode());

  /** Latest known OS preference — updated by the MediaQueryList listener. */
  #systemDark = $state<boolean>(systemPrefersDark());

  /** Resolved theme — what actually applies to `<html>` + tokens. */
  resolved = $derived<Theme>(
    this.mode === 'system' ? (this.#systemDark ? 'dark' : 'light') : this.mode,
  );

  /**
   * @deprecated Read `resolved` for the concrete theme or `mode` for the
   * user choice. Kept so existing callers (`themeStore.theme`) keep
   * compiling — same value as `resolved`.
   */
  get theme(): Theme {
    return this.resolved;
  }

  /** Apply the *resolved* value to <html> + persist `mode` to localStorage.
   *  Idempotent. Called by setters; expose for unit tests / hydrate. */
  apply(): void {
    if (typeof document === 'undefined') return;
    if (this.resolved === 'dark') {
      document.documentElement.classList.add(DARK_CLASS);
    } else {
      document.documentElement.classList.remove(DARK_CLASS);
    }
    try {
      localStorage.setItem(STORAGE_KEY, this.mode);
    } catch (e) {
      console.debug('[gtmux] theme persist failed', e);
    }
  }

  /** Switch to a specific mode. No-op if already there. */
  setMode(next: ThemeMode): void {
    if (this.mode === next) return;
    this.mode = next;
    this.apply();
  }

  /** Backwards-compat — `themeStore.set('light')` continues to work by
   *  mapping a concrete theme onto the corresponding mode. */
  set(next: Theme): void {
    this.setMode(next);
  }

  /** Flip the current resolved theme by setting an explicit mode. */
  toggle(): void {
    this.setMode(this.resolved === 'dark' ? 'light' : 'dark');
  }

  /**
   * Subscribe to OS preference changes. Returns an unsubscribe. Mount
   * once globally (+page.svelte onMount). Safe to mount multiple times
   * — listeners are managed per call.
   */
  bindSystemListener(): () => void {
    if (typeof window === 'undefined') return () => {};
    let mql: MediaQueryList;
    try {
      mql = window.matchMedia(MEDIA_QUERY);
    } catch {
      return () => {};
    }
    const onChange = (e: MediaQueryListEvent): void => {
      this.#systemDark = e.matches;
      if (this.mode === 'system') this.apply();
    };
    // Initial sync — the constructor's snapshot can drift if the OS
    // pref changes between module load and this listener mount.
    this.#systemDark = mql.matches;
    if (this.mode === 'system') this.apply();
    mql.addEventListener('change', onChange);
    return () => mql.removeEventListener('change', onChange);
  }
}

function resolveInitialMode(): ThemeMode {
  if (typeof window === 'undefined') return 'system';
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === 'light' || stored === 'dark' || stored === 'system') return stored;
  } catch (e) {
    console.debug('[gtmux] theme storage read failed', e);
  }
  return 'system';
}

export const themeStore = new ThemeStore();

/** Test helper — re-resolve initial without breaking the runtime singleton. */
export function _resolveInitialForTests(): ThemeMode {
  return resolveInitialMode();
}
