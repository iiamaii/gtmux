<script lang="ts">
  /**
   * Icon-only button primitive (ADR-0016 §D5/D6).
   *
   * Required: `aria-label` — there is no visible text label.
   * Square hit target, sizes match Button heights for chrome alignment.
   *
   * Pass a lucide-svelte icon as a child via the default snippet or via
   * the `icon` prop (component reference). Most callers use the snippet.
   */

  import type { Snippet } from 'svelte';

  type Variant = 'secondary' | 'ghost' | 'danger';
  type Size = 'sm' | 'md';

  interface Props {
    variant?: Variant;
    size?: Size;
    disabled?: boolean;
    onclick?: (event: MouseEvent) => void;
    /** Required for screen-reader semantics — there's no visible label. */
    'aria-label': string;
    /** Optional tooltip text shown on hover/focus. */
    title?: string;
    children: Snippet;
  }

  const {
    variant = 'ghost',
    size = 'md',
    disabled = false,
    onclick,
    'aria-label': ariaLabel,
    title,
    children,
  }: Props = $props();
</script>

<button
  type="button"
  {disabled}
  {onclick}
  aria-label={ariaLabel}
  {title}
  class="icon-btn icon-btn-{variant} icon-btn-{size}"
>
  {@render children()}
</button>

<style>
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-md);
    color: var(--color-fg-muted);
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .icon-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .icon-btn-sm {
    width: 24px;
    height: 24px;
  }

  .icon-btn-md {
    width: 32px;
    height: 32px;
  }

  .icon-btn-ghost {
    background: transparent;
  }

  .icon-btn-ghost:hover:not(:disabled) {
    background: var(--color-surface-2);
    color: var(--color-fg);
  }

  .icon-btn-secondary {
    background: var(--color-surface-2);
  }

  .icon-btn-secondary:hover:not(:disabled) {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  .icon-btn-danger {
    background: transparent;
    color: var(--color-fg-muted);
  }

  .icon-btn-danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-danger) 16%, transparent);
    color: var(--color-danger);
  }
</style>
