<script lang="ts">
  /**
   * Session shutdown confirm modal (plan 0005 Stage C, ADR-0017 §D3).
   *
   * Triggered from SessionMenu. The shutdown path is HTTP: on confirm we run a
   * step-up re-auth (ADR-0020 D16) — the ReauthModal collects the mode-aware
   * credential (password if `auth.password_set`, else token) and calls
   * `POST /api/shutdown` with `{ credential }`. The backend re-verifies the
   * credential inline, then schedules graceful shutdown, emits SERVER_SHUTDOWN
   * over WS, and exits with code 6. ReconnectBanner surfaces the intentional
   * shutdown branch.
   *
   * Information density (ref §10 Figma style + sketch §13 destructive
   * action prevention):
   *   - Title: "Shutdown session '<name>'?"
   *   - 3 bullets: pane count / layout preservation / exit-code semantics
   *   - Actions: [Cancel] ghost + [Shutdown] danger → step-up re-auth
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ReauthModal from './ReauthModal.svelte';
  import { muxStore } from '$lib/stores/mux.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { shutdownServer } from '$lib/http/shutdown';
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

  const { open, sessionName, onclose }: Props = $props();

  const liveCount = $derived(
    [...muxStore.panes.values()].filter((p) => !p.dead).length
  );

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
    <Button variant="ghost" onclick={onclose} disabled={reauthOpen}>Cancel</Button>
    <Button variant="danger" onclick={onConfirm} disabled={reauthOpen}>
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
