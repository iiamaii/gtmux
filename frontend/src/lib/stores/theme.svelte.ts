// Theme store — light / dark mode (ADR-0016 v2 amend D2).
//
// Source of truth: in-memory `theme` state. Side effects:
//   1. localStorage `gtmux-theme` ← persistence across reloads
//   2. <html> classList `dark` toggle ← CSS tokens flip
//   3. <meta name="theme-color"> ← browser chrome (optional, P1)
//
// Initial value priority:
//   1. localStorage `gtmux-theme` (user's explicit toggle from prior session)
//   2. `prefers-color-scheme: dark` media query (OS preference)
//   3. fallback: dark (sketch default — terminal workspace skews dark)
//
// The class application also runs in a tiny inline script in `index.html`
// *before* Svelte hydrates, so the first paint never shows the wrong theme
// (FOUC guard). This store then takes over for subsequent toggles.

export type Theme = 'light' | 'dark';

const STORAGE_KEY = 'gtmux-theme';
const DARK_CLASS = 'dark';

class ThemeStore {
  /** Reactive theme value — components subscribe by reading `theme`. */
  theme = $state<Theme>(resolveInitial());

  /** Apply the *current* in-memory value to <html> + persist to localStorage.
   *  Idempotent. Called by setters; expose for unit tests / hydrate. */
  apply(): void {
    if (typeof document === 'undefined') return;
    if (this.theme === 'dark') {
      document.documentElement.classList.add(DARK_CLASS);
    } else {
      document.documentElement.classList.remove(DARK_CLASS);
    }
    try {
      localStorage.setItem(STORAGE_KEY, this.theme);
    } catch (e) {
      console.debug('[gtmux] theme persist failed', e);
    }
  }

  /** Switch to a specific theme. No-op if already there. */
  set(next: Theme): void {
    if (this.theme === next) return;
    this.theme = next;
    this.apply();
  }

  /** Flip the current theme. UI toggle calls this. */
  toggle(): void {
    this.set(this.theme === 'dark' ? 'light' : 'dark');
  }
}

/** Resolve the initial theme honouring the priority chain in the module
 *  docstring. Pure, runnable in SSR (returns the fallback without touching
 *  `localStorage` or `matchMedia` when those are unavailable). */
function resolveInitial(): Theme {
  if (typeof window === 'undefined') return 'dark';
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === 'light' || stored === 'dark') return stored;
  } catch (e) {
    console.debug('[gtmux] theme storage read failed', e);
  }
  try {
    if (window.matchMedia('(prefers-color-scheme: light)').matches) {
      return 'light';
    }
  } catch (e) {
    console.debug('[gtmux] matchMedia failed', e);
  }
  return 'dark';
}

export const themeStore = new ThemeStore();

/** Test helper — re-resolve initial without breaking the runtime singleton.
 *  Used by Stage B unit tests once a vitest harness is wired (P1+). */
export function _resolveInitialForTests(): Theme {
  return resolveInitial();
}
