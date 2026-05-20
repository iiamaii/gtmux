<script lang="ts">
  import { sessionStore } from '$lib/stores/sessionStore.svelte';

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
  <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
    <path d="M3 3l6 6M9 3l-6 6"/>
  </svg>
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

  .canvas-close.dark {
    width: 22px;
    height: 22px;
    top: 8px;
    right: 8px;
    background: rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    color: #ffffff;
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
