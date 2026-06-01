<script lang="ts">
  /**
   * Modal primitive (ADR-0016 §D4).
   *
   * Behaviour:
   *   - role=dialog + aria-modal=true
   *   - Esc → close
   *   - Click on backdrop → close (controlled by `dismissOnBackdrop`)
   *   - Focus trap — first focusable element inside the dialog gets
   *     focus on open; Tab cycles within the dialog
   *   - Body scroll lock while open
   *
   * Usage:
   *   <Modal open={showModal} onclose={() => (showModal = false)} title="Confirm">
   *     {#snippet body()}
   *       <div class="modal-stack">
   *         <p class="modal-lead">Are you sure?</p>
   *       </div>
   *     {/snippet}
   *     {#snippet footer()}
   *       <Button variant="ghost" onclick={cancel}>Cancel</Button>
   *       <Button variant="danger" onclick={confirm}>OK</Button>
   *     {/snippet}
   *   </Modal>
   *
   * Common body utility classes: modal-stack, modal-lead/modal-copy,
   * modal-state, modal-note.
   */

  import type { Snippet } from 'svelte';

  interface Props {
    /** Controlled visibility. Parent owns the boolean. */
    open: boolean;
    /** Fired when the modal requests to close (Esc / backdrop). */
    onclose?: () => void;
    /** Title rendered in the header (also wired to aria-labelledby). */
    title?: string;
    /** Whether clicks on the backdrop should close. Default true. */
    dismissOnBackdrop?: boolean;
    /** Whether the Esc key should close. Default true. */
    dismissOnEsc?: boolean;
    /** Width preset for the dialog shell. */
    size?: 'sm' | 'md' | 'wide';
    /** Let complex modal bodies own their own padding/header/footer bands. */
    flushBody?: boolean;
    body: Snippet;
    footer?: Snippet;
  }

  const {
    open,
    onclose,
    title,
    dismissOnBackdrop = true,
    dismissOnEsc = true,
    size = 'md',
    flushBody = false,
    body,
    footer,
  }: Props = $props();

  let dialog: HTMLDivElement | undefined = $state();
  const titleId = `modal-title-${Math.random().toString(36).slice(2, 10)}`;

  function requestClose(): void {
    onclose?.();
  }

  function onBackdropMousedown(e: MouseEvent): void {
    if (!dismissOnBackdrop) return;
    if (e.target === e.currentTarget) requestClose();
  }

  function portal(node: HTMLElement): { destroy: () => void } {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }

  function onKeydown(e: KeyboardEvent): void {
    if (!open) return;
    if (e.key === 'Escape') {
      if (!dismissOnEsc) return;
      e.preventDefault();
      requestClose();
      return;
    }
    if (e.key === 'Tab' && dialog) {
      // Simple focus trap — cycle within focusable elements inside the dialog.
      const focusables = Array.from(
        dialog.querySelectorAll<HTMLElement>(
          'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'
        )
      );
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (!first || !last) return;
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }

  $effect(() => {
    if (typeof window === 'undefined') return;
    if (!open) return;
    window.addEventListener('keydown', onKeydown);
    // Body scroll lock — restore on close.
    const prevOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    // Initial focus — first focusable inside the dialog.
    queueMicrotask(() => {
      if (!dialog) return;
      const focusable = dialog.querySelector<HTMLElement>(
        'button:not([disabled]), [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
      focusable?.focus();
    });
    return () => {
      window.removeEventListener('keydown', onKeydown);
      document.body.style.overflow = prevOverflow;
    };
  });
</script>

{#if open}
  <div
    class="modal-backdrop"
    use:portal
    onmousedown={onBackdropMousedown}
    role="presentation"
  >
    <div
      bind:this={dialog}
      role="dialog"
      aria-modal="true"
      aria-labelledby={title ? titleId : undefined}
      class="modal-dialog modal-dialog-{size}"
    >
      {#if title}
        <header class="modal-header">
          <h2 id={titleId} class="modal-title">{title}</h2>
        </header>
      {/if}
      <div class="modal-body" class:modal-body-flush={flushBody}>
        {@render body()}
      </div>
      {#if footer}
        <footer class="modal-footer">
          {@render footer()}
        </footer>
      {/if}
    </div>
  </div>
{/if}

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: var(--z-modal);
    background: transparent;
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-16);
    animation: backdrop-in var(--motion-fast) var(--motion-easing);
  }

  .modal-dialog {
    box-sizing: border-box;
    width: 100%;
    max-height: min(86vh, 760px);
    background: var(--color-surface);
    color: var(--color-fg);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    animation: dialog-in var(--motion-slow) var(--motion-easing);
  }

  .modal-dialog-sm {
    max-width: 360px;
  }

  .modal-dialog-md {
    max-width: 480px;
  }

  .modal-dialog-wide {
    max-width: 560px;
  }

  .modal-header {
    padding: var(--space-16) var(--space-24) var(--space-8);
  }

  .modal-title {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: var(--weight-semibold);
    line-height: var(--leading-tight);
  }

  .modal-body {
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-12) var(--space-24);
    font-size: var(--text-base);
    line-height: var(--leading-normal);
    color: var(--color-fg-muted);
  }

  .modal-body-flush {
    padding: 0;
  }

  .modal-footer {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--space-8);
    padding: var(--space-12) var(--space-24) var(--space-16);
    border-top: 1px solid var(--color-border);
    background: color-mix(in srgb, var(--color-surface-2) 60%, transparent);
  }

  :global(.modal-stack) {
    display: flex;
    flex-direction: column;
    gap: var(--space-12);
  }

  :global(.modal-copy),
  :global(.modal-lead) {
    margin: 0;
    color: var(--color-fg-muted);
    font-size: var(--text-md);
    line-height: var(--leading-normal);
  }

  :global(.modal-copy strong),
  :global(.modal-lead strong) {
    color: var(--color-fg);
    font-weight: var(--weight-semibold);
  }

  :global(.modal-state) {
    margin: 0;
    padding: var(--space-24) 0;
    color: var(--color-fg-muted);
    font-size: var(--text-md);
    line-height: var(--leading-normal);
    text-align: center;
  }

  :global(.modal-note) {
    margin: 0;
    padding: var(--space-10) var(--space-12);
    background: var(--color-surface-2);
    border-left: 3px solid var(--color-warning);
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    font-size: var(--text-base);
    line-height: var(--leading-normal);
  }

  @keyframes backdrop-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes dialog-in {
    from {
      opacity: 0;
      transform: translateY(8px) scale(0.98);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }
</style>
