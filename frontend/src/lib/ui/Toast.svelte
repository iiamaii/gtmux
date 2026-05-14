<script lang="ts">
  /**
   * Toast host — non-blocking transient notifications (ADR-0016 §D4).
   *
   * Mount once in `routes/+page.svelte`. Other modules call
   * `toast.show({ message, tone, duration })` from `$lib/ui/toast-store`.
   */

  import { toastStore } from '$lib/ui/toast-store.svelte';
</script>

<div class="toast-host" role="region" aria-label="Notifications" aria-live="polite">
  {#each toastStore.items as item (item.id)}
    <div class="toast toast-{item.tone}">
      <span class="toast-message">{item.message}</span>
      <button
        type="button"
        class="toast-dismiss"
        aria-label="Dismiss notification"
        onclick={() => toastStore.dismiss(item.id)}
      >×</button>
    </div>
  {/each}
</div>

<style>
  .toast-host {
    position: fixed;
    right: var(--space-4);
    bottom: var(--space-4);
    z-index: var(--z-toast);
    display: flex;
    flex-direction: column-reverse;
    gap: var(--space-2);
    pointer-events: none;
    max-width: 360px;
  }

  .toast {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-2);
    color: var(--color-fg);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-3);
    font-size: var(--text-sm);
    pointer-events: auto;
    animation: toast-in var(--motion-normal) var(--motion-easing);
  }

  .toast-info {
    border-left: 3px solid var(--color-info);
  }

  .toast-success {
    border-left: 3px solid var(--color-success);
  }

  .toast-warning {
    border-left: 3px solid var(--color-warning);
  }

  .toast-error {
    border-left: 3px solid var(--color-danger);
  }

  .toast-message {
    flex: 1 1 auto;
    min-width: 0;
    line-height: var(--leading-normal);
  }

  .toast-dismiss {
    flex: 0 0 auto;
    width: 20px;
    height: 20px;
    padding: 0;
    background: transparent;
    color: var(--color-fg-subtle);
    border: 0;
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-size: var(--text-md);
    line-height: 1;
  }

  .toast-dismiss:hover {
    background: var(--color-surface-3);
    color: var(--color-fg);
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateX(16px);
    }
    to {
      opacity: 1;
      transform: translateX(0);
    }
  }
</style>
