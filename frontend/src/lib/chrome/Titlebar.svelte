<script lang="ts">
  /**
   * Titlebar — 44px chrome (plan 0005 Stage C, ADR-0017 §D1/D2).
   *
   * Layout (3-col grid):
   *   - left   : SessionMenu (kebab) + "Workspace" tab
   *   - center : session info — "gtmux · <session> · <bind>:<port> · <mode>"
   *   - right  : ThemeToggle + FocusToggle
   *
   * Stage C scope: chrome shell + actions only. ViewportCtrl / HelpBar /
   * Sidebar floating refactor land in Stage E / F.
   */

  import SessionMenu from './SessionMenu.svelte';
  import ThemeToggle from '$lib/ui/ThemeToggle.svelte';
  import FocusToggle from './FocusToggle.svelte';

  const TOKEN_STORAGE_KEY = 'gtmux_token';
  const SESSION_STORAGE_KEY = 'gtmux_session';

  /** Session name surface — bootstrap landing inline-script injects this
   *  into sessionStorage alongside the token (ADR-0017 §D4). Falls back to
   *  "unknown" if the user reached the page through a non-bootstrap path
   *  (manual paste, dev `?token=` etc.). */
  function readSession(): string {
    try {
      const v = sessionStorage.getItem(SESSION_STORAGE_KEY);
      if (v && v.length > 0) return v;
    } catch {
      // ignored
    }
    return 'unknown';
  }

  const sessionName = $state(readSession());

  // Host info — derived from `window.location`. Backend's banner emits the
  // bind/port pair in the CLI but we can reconstruct it client-side from
  // the same origin the browser is on (single-port serve, per ADR-0007).
  const hostInfo = $derived.by(() => {
    if (typeof window === 'undefined') return '';
    return window.location.host;
  });

  // Mode — Local is the only currently supported mode (Cloud is ADR-0003
  // future-only). Hard-code for now; future could read from /api/server.
  const mode = 'Local';
</script>

<header class="titlebar" aria-label="gtmux titlebar">
  <div class="titlebar-left">
    <SessionMenu {sessionName} />
    <span class="title-tab active">Workspace</span>
  </div>

  <div class="titlebar-center">
    <span class="brand">gtmux</span>
    <span class="sep">·</span>
    <strong class="session">{sessionName}</strong>
    <span class="sep">·</span>
    <span class="muted">{hostInfo}</span>
    <span class="sep">·</span>
    <span class="muted">{mode}</span>
  </div>

  <div class="titlebar-right">
    <ThemeToggle />
    <FocusToggle />
  </div>
</header>

<style>
  .titlebar {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    height: var(--layout-titlebar-h);
    padding: 0 var(--space-12);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    z-index: var(--z-titlebar);
    flex: 0 0 auto;
    user-select: none;
    color: var(--color-fg);
  }

  .titlebar-left {
    display: inline-flex;
    align-items: center;
    gap: var(--space-6);
  }

  .titlebar-right {
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-4);
  }

  .titlebar-center {
    display: inline-flex;
    align-items: center;
    gap: var(--space-8);
    color: var(--color-fg-muted);
    font-size: var(--text-md);
    letter-spacing: -0.1px;
    overflow: hidden;
    white-space: nowrap;
  }

  .brand {
    font-family: var(--font-mono);
    font-size: var(--text-md);
    color: var(--color-fg);
    letter-spacing: 0.2px;
  }

  .session {
    color: var(--color-fg);
    font-weight: var(--weight-medium);
  }

  .sep {
    color: var(--color-fg-subtle);
  }

  .muted {
    color: var(--color-fg-muted);
  }

  .title-tab {
    padding: var(--space-4) var(--space-10);
    border-radius: var(--radius-sm);
    font-size: var(--text-md);
    color: var(--color-fg-muted);
    cursor: pointer;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .title-tab:hover {
    background: var(--color-glass-1);
  }

  .title-tab.active {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  /* Responsive — narrow viewport hides center session info to keep
   * actions visible. Stage E 의 PaneInfoPanel collapse 정합. */
  @media (max-width: 720px) {
    .titlebar-center .muted,
    .titlebar-center .sep {
      display: none;
    }
  }

  @media (max-width: 480px) {
    .titlebar-center {
      display: none;
    }
  }
</style>
