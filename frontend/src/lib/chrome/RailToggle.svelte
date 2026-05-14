<script lang="ts">
  /**
   * RailToggle — thin 16×64 collapse button for floating side panels
   * (plan 0005 Stage E, ADR-0017 §D2/D7).
   *
   * Sits next to the side panel's outer edge. When the panel is open
   * the rail anchors just outside the panel (e.g. left: 264px for a
   * left-side panel). When the panel is collapsed the rail moves to
   * the viewport edge (left: 8px) so the user has a re-entry control.
   *
   * Chevron icon rotates 180° when collapsed to indicate the direction
   * the panel will reappear from.
   */

  type Side = 'left' | 'right';

  interface Props {
    /** Which panel this toggle belongs to. Determines anchor side and
     *  chevron direction. */
    side: Side;
    /** Current collapsed state of the partner panel. */
    collapsed: boolean;
    /** Click handler — wraps a single toggle action. */
    onclick: () => void;
    /** Accessibility label (e.g. "Toggle layer panel"). */
    'aria-label': string;
  }

  const { side, collapsed, onclick, 'aria-label': ariaLabel }: Props = $props();
</script>

<button
  type="button"
  class="rail-toggle rail-{side}"
  class:collapsed
  aria-label={ariaLabel}
  aria-expanded={!collapsed}
  {onclick}
>
  <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#if side === 'left'}
      <polyline points="15 18 9 12 15 6"/>
    {:else}
      <polyline points="9 18 15 12 9 6"/>
    {/if}
  </svg>
</button>

<style>
  .rail-toggle {
    position: absolute;
    top: 50%;
    transform: translateY(-50%);
    width: 16px;
    height: 64px;
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    box-shadow: var(--shadow-sm);
    border: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    z-index: var(--z-rail);
    color: var(--color-fg-muted);
    transition:
      left var(--motion-slow) var(--motion-easing),
      right var(--motion-slow) var(--motion-easing),
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .rail-toggle:hover {
    background: var(--color-surface-2);
    color: var(--color-fg);
  }

  /* Left rail — anchors just outside the open Sidebar (264 = 8 + 248 + 8).
   * Collapsed pulls it back to the viewport edge. */
  .rail-toggle.rail-left {
    left: calc(var(--space-8) + var(--layout-sidebar-w));
  }
  .rail-toggle.rail-left.collapsed {
    left: var(--space-8);
  }
  .rail-toggle.rail-left.collapsed svg {
    transform: rotate(180deg);
  }

  /* Right rail — mirror of the left side. */
  .rail-toggle.rail-right {
    right: calc(var(--space-8) + var(--layout-sidebar-right-w));
  }
  .rail-toggle.rail-right.collapsed {
    right: var(--space-8);
  }
  .rail-toggle.rail-right.collapsed svg {
    transform: rotate(180deg);
  }

  .rail-toggle svg {
    transition: transform var(--motion-normal) var(--motion-easing);
  }
</style>
