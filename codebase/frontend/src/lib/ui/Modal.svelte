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
   *       <p>Are you sure?</p>
   *     {/snippet}
   *     {#snippet footer()}
   *       <Button variant="ghost" onclick={cancel}>Cancel</Button>
   *       <Button variant="danger" onclick={confirm}>OK</Button>
   *     {/snippet}
   *   </Modal>
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
    body: Snippet;
    footer?: Snippet;
  }

  const {
    open,
    onclose,
    title,
    dismissOnBackdrop = true,
    dismissOnEsc = true,
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
      class="modal-dialog"
    >
      {#if title}
        <header class="modal-header">
          <h2 id={titleId} class="modal-title">{title}</h2>
        </header>
      {/if}
      <div class="modal-body">
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
    width: 100%;
    max-width: 480px;
    background: var(--color-surface);
    color: var(--color-fg);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    overflow: hidden;
    animation: dialog-in var(--motion-slow) var(--motion-easing);
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
    padding: var(--space-12) var(--space-24);
    font-size: var(--text-base);
    color: var(--color-fg-muted);
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-8);
    padding: var(--space-12) var(--space-24) var(--space-16);
    border-top: 1px solid var(--color-border);
    background: color-mix(in srgb, var(--color-surface-2) 60%, transparent);
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
