<script lang="ts">
  // PanelDanglingOverlay — overlay rendered when this panel's terminal has been
  // marked dead via BE 0x85 TERMINAL_DIED (0034 §3) but the layout item still
  // exists. Click → respawn the same UUID (ADR-0021 D10 dangling recovery).
  //
  // 정본:
  // - BE Stage 5-B (0034 §3): UUID-carrying terminal-died broadcast
  // - 0033 §8.1: "terminal_died 수신 → 그 UUID 의 모든 panel 에 overlay → click → respawnTerminal(id)"
  // - ADR-0021 D10: respawn preserves UUID, fresh PaneId

  import { danglingTerminals } from '$lib/stores/danglingTerminals.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { respawnTerminal } from '$lib/http/terminals';
  import { UnauthorizedError } from '$lib/http/sessions';
  import { toastStore } from '$lib/ui/toast-store.svelte';

  const { terminalId }: { terminalId: string } = $props();

  const reason = $derived(danglingTerminals.reasonFor(terminalId));
  const visible = $derived(reason !== null);

  let respawning = $state(false);

  async function onRespawn(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    if (respawning) return;
    respawning = true;
    try {
      await respawnTerminal(terminalId);
      danglingTerminals.clear(terminalId);
      void terminalPool.refresh();
      toastStore.show({ message: 'Terminal respawned.', tone: 'success' });
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Respawn failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    } finally {
      respawning = false;
    }
  }
</script>

{#if visible}
  <div class="overlay" role="status" aria-live="polite">
    <div class="card">
      <div class="title">
        {reason === 'killed' ? 'Terminal killed' : 'Terminal exited'}
      </div>
      <div class="hint">Restart the same terminal — UUID is preserved.</div>
      <button
        type="button"
        class="respawn"
        disabled={respawning}
        onclick={onRespawn}
        onmousedown={(e: MouseEvent) => e.stopPropagation()}
      >
        {respawning ? 'Respawning…' : 'Respawn'}
      </button>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--color-bg) 78%, transparent);
    backdrop-filter: blur(2px);
    z-index: 5;
    pointer-events: auto;
  }

  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-8);
    padding: var(--space-12) var(--space-16);
    background: var(--color-surface);
    border: 1px solid var(--color-warning);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    text-align: center;
    max-width: 80%;
  }

  .title {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    color: var(--color-warning);
  }

  .hint {
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
  }

  .respawn {
    margin-top: var(--space-4);
    padding: var(--space-4) var(--space-12);
    background: var(--color-accent);
    color: var(--color-bg);
    border: 0;
    border-radius: var(--radius-sm);
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    cursor: pointer;
    transition: opacity var(--motion-fast) var(--motion-easing);
  }

  .respawn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .respawn:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }
</style>
