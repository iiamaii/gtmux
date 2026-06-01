<script lang="ts">
  /**
   * Project-standard button primitive (ADR-0016 §D4).
   *
   * Variants:
   *   - primary  — accent fill, used for the single "main" action in a surface
   *   - secondary— surface fill with strong border, default for most buttons
   *   - danger   — destructive (shutdown, delete) — confirm modal required
   *   - ghost    — transparent + hover surface tint, for low-emphasis cancel
   *
   * Sizes:
   *   - sm (24px h, 11px font) — toolbar, panel header
   *   - md (32px h, 12px font) — default
   */

  import type { Snippet } from 'svelte';

  type Variant = 'primary' | 'secondary' | 'danger' | 'ghost';
  type Size = 'sm' | 'md';

  interface Props {
    variant?: Variant;
    size?: Size;
    type?: 'button' | 'submit' | 'reset';
    disabled?: boolean;
    onclick?: (event: MouseEvent) => void;
    'aria-label'?: string;
    title?: string;
    children: Snippet;
  }

  const {
    variant = 'secondary',
    size = 'md',
    type = 'button',
    disabled = false,
    onclick,
    'aria-label': ariaLabel,
    title,
    children,
  }: Props = $props();
</script>

<button
  {type}
  {disabled}
  {onclick}
  {title}
  aria-label={ariaLabel}
  class="btn btn-{variant} btn-{size}"
>
  {@render children()}
</button>

<style>
  .btn {
    box-sizing: border-box;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-6);
    border-radius: var(--radius-md);
    font-family: inherit;
    font-weight: var(--weight-medium);
    line-height: var(--leading-normal);
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
    white-space: nowrap;
    user-select: none;
  }

  .btn :global(svg) {
    flex: 0 0 auto;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-sm {
    height: 24px;
    min-width: 44px;
    padding: 0 var(--space-8);
    font-size: var(--text-sm);
  }

  .btn-md {
    height: 32px;
    min-width: 64px;
    padding: 0 var(--space-12);
    font-size: var(--text-base);
  }

  .btn-primary {
    background: var(--color-accent);
    color: var(--color-bg);
    border: 1px solid var(--color-accent);
  }

  .btn-primary:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-accent) 88%, white);
  }

  .btn-secondary {
    background: var(--color-surface-2);
    color: var(--color-fg);
    border: 1px solid var(--color-border-strong);
  }

  .btn-secondary:hover:not(:disabled) {
    background: var(--color-glass-2);
    border-color: var(--color-fg-subtle);
  }

  .btn-danger {
    background: var(--color-danger);
    color: var(--color-fg);
    border: 1px solid var(--color-danger);
  }

  .btn-danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-danger) 88%, black);
  }

  .btn-ghost {
    background: transparent;
    color: var(--color-fg-muted);
    border: 1px solid transparent;
  }

  .btn-ghost:hover:not(:disabled) {
    background: var(--color-surface-2);
    color: var(--color-fg);
  }
</style>
