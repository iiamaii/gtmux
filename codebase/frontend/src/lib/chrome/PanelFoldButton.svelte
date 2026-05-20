<script lang="ts">
  /**
   * PanelFoldButton — header-trailing icon button that collapses a
   * floating chrome panel (LeftPanel / RightPanel).
   *
   * Companion to the in-panel collapsed rail bar:
   *   - PanelFoldButton lives *inside* the panel header (visible when
   *     panel is expanded). One click → collapsed = true.
   *   - When collapsed, the panel's own rail bar (LeftPanel `.left-rail`,
   *     RightPanel `.right-rail`) renders an expand chevron plus
   *     per-tab icons in the panel's footprint.
   *
   * Spec: ADR-0017 §D7 amend ②/③ (panel-tabs + collapsed rail).
   *
   * Direction:
   *   - 'left'  → arrow points left (collapses toward left edge)
   *   - 'right' → arrow points right (collapses toward right edge)
   */

  type Direction = 'left' | 'right';

  interface Props {
    direction: Direction;
    onclick: () => void;
    'aria-label': string;
  }

  const { direction, onclick, 'aria-label': ariaLabel }: Props = $props();
</script>

<button
  type="button"
  class="fold-btn"
  aria-label={ariaLabel}
  title={ariaLabel}
  {onclick}
>
  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#if direction === 'left'}
      <polyline points="15 18 9 12 15 6" />
    {:else}
      <polyline points="9 18 15 12 9 6" />
    {/if}
  </svg>
</button>

<style>
  .fold-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
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

  .fold-btn:hover {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }
</style>
