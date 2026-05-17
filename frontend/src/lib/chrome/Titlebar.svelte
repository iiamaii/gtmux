<script lang="ts">
  /**
   * Titlebar — 44px chrome (plan 0005 Stage C, ADR-0017 §D1/D2).
   *
   * Layout (3-col grid):
   *   - left   : SessionMenu (kebab) + brand-mark + "gtmux"
   *   - center : host info — "<bind>:<port> · <mode>"
   *   - right  : ThemeToggle + FocusToggle
   *
   * Stage C scope: chrome shell + actions only. ViewportCtrl / HelpBar /
   * Sidebar floating refactor land in Stage E / F.
   */

  import SessionMenu from './SessionMenu.svelte';
  import ThemeToggle from '$lib/ui/ThemeToggle.svelte';
  import FocusToggle from './FocusToggle.svelte';
  import brandLogoUrl from '$lib/assets/brand.png';

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
    <div class="brand" aria-label="gtmux">
      <img class="brand-mark" src={brandLogoUrl} alt="" aria-hidden="true" />
      <span class="brand-name">gtmux</span>
    </div>
  </div>

  <div class="titlebar-center">
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
    gap: var(--space-10);
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

  /* Brand — titlebar 좌측 identity. brand-mark + "gtmux" 가로 배치. */
  .brand {
    display: inline-flex;
    align-items: center;
    gap: var(--space-8);
    font-weight: var(--weight-semibold);
    font-size: 18px;
    letter-spacing: -0.3px;
    color: var(--color-fg);
    user-select: none;
  }

  .brand-mark {
    width: 30px;
    height: 30px;
    border-radius: var(--radius-md);
    object-fit: cover;
    flex-shrink: 0;
    display: block;
  }

  .brand-name {
    line-height: 1;
    /* cap-height 시각 중심을 brand-mark vertical center 와 정합. auth page
       의 동일 nudge 패턴 — 폰트 ascent 여백 보정. */
    transform: translateY(-1px);
  }

  .sep {
    color: var(--color-fg-subtle);
  }

  .muted {
    color: var(--color-fg-muted);
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
