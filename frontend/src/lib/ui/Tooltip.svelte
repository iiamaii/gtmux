<script lang="ts">
  /**
   * Lightweight tooltip primitive (ADR-0016 §D4).
   *
   * Pattern: wrap any element. Hover or keyboard focus on the wrapped
   * element reveals the label above it.
   *
   * For native HTML attributes, prefer `title=` — that gives the
   * platform tooltip without the wrapper. Use this component when a
   * styled / always-shown tooltip is needed (e.g. on a disabled button
   * where the platform tooltip may be suppressed).
   */

  import type { Snippet } from 'svelte';

  interface Props {
    /** Text shown in the tooltip. */
    label: string;
    /** Position relative to the wrapped element. Default `top`. */
    placement?: 'top' | 'bottom' | 'left' | 'right';
    children: Snippet;
  }

  const { label, placement = 'top', children }: Props = $props();

  let open = $state(false);
</script>

<span
  class="tooltip-host"
  onmouseenter={() => (open = true)}
  onmouseleave={() => (open = false)}
  onfocusin={() => (open = true)}
  onfocusout={() => (open = false)}
  role="presentation"
>
  {@render children()}
  {#if open}
    <span class="tooltip tooltip-{placement}" role="tooltip">{label}</span>
  {/if}
</span>

<style>
  .tooltip-host {
    position: relative;
    display: inline-flex;
  }

  .tooltip {
    position: absolute;
    z-index: 10;
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface-3);
    color: var(--color-fg);
    font-size: var(--text-sm);
    line-height: var(--leading-tight);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-2);
    white-space: nowrap;
    pointer-events: none;
    animation: tooltip-in var(--motion-fast) var(--motion-easing);
  }

  .tooltip-top {
    bottom: calc(100% + var(--space-1));
    left: 50%;
    transform: translateX(-50%);
  }

  .tooltip-bottom {
    top: calc(100% + var(--space-1));
    left: 50%;
    transform: translateX(-50%);
  }

  .tooltip-left {
    right: calc(100% + var(--space-1));
    top: 50%;
    transform: translateY(-50%);
  }

  .tooltip-right {
    left: calc(100% + var(--space-1));
    top: 50%;
    transform: translateY(-50%);
  }

  @keyframes tooltip-in {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(2px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }
</style>
