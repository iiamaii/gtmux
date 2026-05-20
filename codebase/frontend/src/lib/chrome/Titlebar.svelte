<script lang="ts">
  /**
   * Titlebar — 44px chrome (plan 0005 Stage C, ADR-0017 §D1/D2).
   *
   * Layout (3-col grid):
   *   - left   : SessionMenu (kebab) + brand-mark + "gtmux"
   *   - center : host info — "<bind>:<port> · <mode>"
   *   - right  : (theme toggle 은 SettingsOverlay 안에서만 — 잦은 toggle 시
   *     xterm cell stale 색 회귀 회피 + Settings 변경 마다 자동 reload)
   *
   * Stage C scope: chrome shell + actions only. ViewportCtrl / HelpBar /
   * Sidebar floating refactor land in Stage E / F.
   */

  import SessionMenu from './SessionMenu.svelte';
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
    <!-- 0077 follow-up — 사용자 유도용 명시 새로고침 버튼. 브라우저
         Cmd+R 도 지원하지만 사용자 인지를 위해 chrome 에 노출. session
         switch 의 reload toggle (Settings) 과는 별개 — 사용자 명시 액션. -->
    <button
      type="button"
      class="titlebar-action"
      aria-label="Refresh page"
      title="Refresh page (full reload — resets all state)"
      onclick={() => {
        if (typeof window !== 'undefined') window.location.reload();
      }}
    >
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M2.5 7a5.5 5.5 0 0 1 10.3-2.6"/>
        <path d="M13.5 9a5.5 5.5 0 0 1-10.3 2.6"/>
        <polyline points="11 1.5 13 4.5 10 4.5"/>
        <polyline points="5 14.5 3 11.5 6 11.5"/>
      </svg>
    </button>
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

  .titlebar-action {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-fg-muted);
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .titlebar-action:hover {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .titlebar-action:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 1px;
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
    font-size: 16px;
    letter-spacing: -0.25px;
    color: var(--color-fg);
    user-select: none;
  }

  .brand-mark {
    width: 27px;
    height: 27px;
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
