<script lang="ts">
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ReauthModal from './ReauthModal.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { shutdownServer, serverStatus, type ServerStatus } from '$lib/http/shutdown';
  import { UnauthorizedError } from '$lib/http/sessions';
  import {
    InvalidCredentialError,
    CredentialRequiredError,
    RateLimitedError,
  } from '$lib/http/stepup';

  interface Props {
    open: boolean;
    sessionName: string;
    onclose: () => void;
  }

  const { open, onclose }: Props = $props();

  let status = $state<ServerStatus | null>(null);
  let statusError = $state('');
  $effect(() => {
    if (!open) return;
    let disposed = false;
    status = null; statusError = '';
    void serverStatus().then(value => { if (!disposed) status = value; })
      .catch(e => { if (!disposed) statusError = e instanceof Error ? e.message : String(e); });
    return () => { disposed = true; };
  });

  let reauthOpen = $state(false);

  // Close the step-up modal whenever this confirm modal is dismissed so a
  // stale gate never lingers.
  $effect(() => {
    if (!open) reauthOpen = false;
  });

  function onConfirm(): void {
    reauthOpen = true;
  }

  /**
   * Gated action — runs with the credential from ReauthModal. Step-up errors
   * (wrong credential / required / rate limit) are re-thrown so the ReauthModal
   * keeps itself open and shows them inline. Everything else is handled here:
   * UnauthorizedError → /auth redirect, others → toast.
   */
  async function runShutdown(credential: string): Promise<void> {
    try {
      await shutdownServer(credential);
    } catch (e) {
      if (
        e instanceof InvalidCredentialError ||
        e instanceof CredentialRequiredError ||
        e instanceof RateLimitedError
      ) {
        throw e; // ReauthModal branches + stays open for retry.
      }
      if (e instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Shutdown request failed: ${e instanceof Error ? e.message : String(e)}`,
        tone: 'error',
      });
      return;
    }
    // Backend accepted the request. The SERVER_SHUTDOWN WS frame and normal
    // close follow; ReconnectBanner owns the visible end state.
    reauthOpen = false;
    onclose();
  }
</script>

<Modal {open} {onclose} title="Stop server?">
  {#snippet body()}
    {#if status}
      <p class="hint">Server <strong>{status.instance}</strong> at {status.bind}:{status.port}</p>
      <ul class="bullets">
        <li>All <strong>{status.active_terminals}</strong> active terminals on this server will stop, across every session and browser tab.</li>
        <li>Saved session layouts and configuration stay on disk. Running programs will end.</li>
      </ul>
      <p class="hint">Start the server again with <code>gtmux start --name {status.instance}</code> and the same configuration, or your host application.</p>
      {#if !status.can_shutdown}<p class="hint">This host manages the server lifecycle. Stop it from the host application.</p>{/if}
    {:else}
      <p class="hint" role="status">{statusError || 'Checking server status…'}</p>
    {/if}
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={onclose} disabled={reauthOpen}>Cancel</Button>
    <Button variant="danger" onclick={onConfirm} disabled={reauthOpen || !status?.can_shutdown || status.state === 'stopping'}>
      Shutdown
    </Button>
  {/snippet}
</Modal>

<ReauthModal
  open={reauthOpen}
  title="Confirm shutdown"
  description="Re-enter your credential to stop the server."
  confirmLabel="Shutdown"
  confirmVariant="danger"
  onSubmit={runShutdown}
  onCancel={() => (reauthOpen = false)}
/>

<style>
  .bullets {
    margin: 0;
    padding-left: var(--space-18);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .bullets li {
    color: var(--color-fg);
    line-height: var(--leading-normal);
  }

  .bullets strong {
    color: var(--color-fg);
    font-weight: var(--weight-semibold);
  }

  .hint {
    margin: var(--space-12) 0 0;
    color: var(--color-fg-muted);
    font-size: var(--text-base);
  }

  .hint code {
    font-family: var(--font-mono);
    background: var(--color-glass-1);
    padding: var(--space-2) var(--space-6);
    border-radius: var(--radius-sm);
    color: var(--color-fg);
  }
</style>
