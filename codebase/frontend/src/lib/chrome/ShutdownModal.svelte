<script lang="ts">
  /**
   * Session shutdown confirm modal (plan 0005 Stage C, ADR-0017 §D3).
   *
   * Triggered from SessionMenu. On confirm, sends CTRL `kill-session`
   * (ADR-0013 D10 amend) — backend acks + raises SIGTERM on itself →
   * graceful_shutdown → WS close 1000. ReconnectBanner then surfaces
   * "Session ended" (Stage C banner amend, separate file).
   *
   * Information density (ref §10 Figma style + sketch §13 destructive
   * action prevention):
   *   - Title: "Shutdown session '<name>'?"
   *   - 3 bullets: pane count / layout preservation / exit-code semantics
   *   - Actions: [Cancel] ghost + [Shutdown] danger
   */

  import { getContext } from 'svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { muxStore } from '$lib/stores/mux.svelte';
  import { sendCtrl } from '$lib/ws/ctrl-registry';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import type { WsClient } from '$lib/ws/client';

  interface Props {
    open: boolean;
    sessionName: string;
    onclose: () => void;
  }

  const { open, sessionName, onclose }: Props = $props();

  interface WsClientHolder { current: WsClient | null }
  const wsClientHolder = getContext<WsClientHolder>('wsClient');

  const liveCount = $derived(
    [...muxStore.panes.values()].filter((p) => !p.dead).length
  );

  let inFlight = $state(false);

  async function onShutdown(): Promise<void> {
    const client = wsClientHolder?.current;
    if (!client) {
      toastStore.show({ message: 'WebSocket not ready', tone: 'error' });
      return;
    }
    inFlight = true;
    try {
      const { response } = sendCtrl(client, 'kill-session', [], { timeoutMs: 5_000 });
      const result = await response;
      if (!result.ok) {
        toastStore.show({
          message: `Shutdown failed: ${result.code ?? 'unknown'} ${result.error ?? ''}`,
          tone: 'error',
        });
        return;
      }
      // Backend acked. The SIGTERM-self fires next and the WS will close
      // 1000. ReconnectBanner takes over with the "Session ended" flow.
      onclose();
    } catch (e) {
      toastStore.show({
        message: `Shutdown request failed: ${(e as Error).message ?? e}`,
        tone: 'error',
      });
    } finally {
      inFlight = false;
    }
  }
</script>

<Modal {open} {onclose} title="Shutdown session '{sessionName}'?">
  {#snippet body()}
    <ul class="bullets">
      <li>
        <strong>{liveCount}</strong>
        {liveCount === 1 ? 'active pane' : 'active panes'} will be reaped
      </li>
      <li>Canvas layout will be preserved on disk</li>
      <li>Server process will exit with code 6</li>
    </ul>
    <p class="hint">
      You'll need <code>gtmux start --session {sessionName}</code> to re-enter.
    </p>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={onclose} disabled={inFlight}>Cancel</Button>
    <Button variant="danger" onclick={onShutdown} disabled={inFlight}>
      {inFlight ? 'Shutting down…' : 'Shutdown'}
    </Button>
  {/snippet}
</Modal>

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
