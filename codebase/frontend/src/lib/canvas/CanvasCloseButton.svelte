<script lang="ts">
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import CanvasGlyph from './CanvasGlyph.svelte';

  interface Props {
    id: string;
    variant?: 'light' | 'dark';
    label?: string;
    disabled?: boolean;
  }

  const {
    id,
    variant = 'light',
    label = 'Close',
    disabled = false,
  }: Props = $props();

  async function onClose(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    e.preventDefault();
    if (disabled) return;
    await sessionStore.applyDeletion([id], { killTerminal: false });
  }
</script>

<button
  type="button"
  class="canvas-close"
  class:dark={variant === 'dark'}
  title={label}
  aria-label={label}
  disabled={disabled}
  onclick={(e) => void onClose(e)}
  onpointerdown={(e: PointerEvent) => e.stopPropagation()}
>
  <CanvasGlyph name="close" />
</button>

<style>
  .canvas-close {
    position: absolute;
    top: 6px;
    right: 6px;
    z-index: 12;
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    opacity: 0;
    transition:
      opacity var(--motion-fast) var(--motion-easing),
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  /* Dark variant is used only over image content (ImageNode). 2026-07-27
     re-spec: resting background TRANSPARENT (note-style) instead of the dark
     chip; a soft drop-shadow keeps the white glyph legible over arbitrary
     images. Red hover fill is unchanged. */
  .canvas-close.dark {
    width: 20px; /* canvas-tier standard box (icon unification 2026-07-27) */
    height: 20px;
    top: 8px;
    right: 8px;
    background: transparent;
    color: #ffffff;
    filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.6));
  }

  :global(.svelte-flow__node:hover) .canvas-close,
  .canvas-close:focus-visible {
    opacity: 1;
  }

  .canvas-close:hover:not(:disabled) {
    background: #e5484d;
    color: #ffffff;
  }

  .canvas-close.dark:hover:not(:disabled) {
    background: rgba(229, 72, 77, 0.92);
  }

  .canvas-close:focus-visible {
    outline: 1px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .canvas-close:disabled {
    cursor: not-allowed;
    opacity: 0.35;
  }
</style>
