<script lang="ts">
  // PanelDanglingOverlay — terminal 이 0x85 TERMINAL_DIED 로 dangling 표시되면
  // visual placeholder. 정상 exit 는 기존 정책대로 자동 respawn, explicit kill
  // (Panel+Terminal / SIGTERM) 은 사용자의 종료 의도를 보존하기 위해 자동
  // respawn 하지 않는다.
  //
  // 정본:
  // - BE Stage 5-B (0034 §3): UUID-carrying terminal-died broadcast
  // - ADR-0021 D10: respawn preserves UUID, fresh PaneId
  // - 사용자 요구 (2026-05-17): "session 간 동일 terminal auto respawn 으로 전환"

  import { onMount } from 'svelte';
  import { danglingTerminals } from '$lib/stores/danglingTerminals.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { respawnTerminal } from '$lib/http/terminals';
  import { UnauthorizedError } from '$lib/http/sessions';

  const { terminalId }: { terminalId: string } = $props();

  const reason = $derived(danglingTerminals.reasonFor(terminalId));
  const visible = $derived(reason !== null);
  const respawning = $derived(danglingTerminals.isRespawning(terminalId));
  const autoRespawn = $derived(reason === 'exit');

  // 자동 respawn — overlay mount 또는 reason 갱신 시 lock 확보 시도. 다른
  // panel / webpage 가 먼저 잡았으면 false (spinner 만 표시). 0x88 도착 시
  // dispatcher 가 clear → overlay 자연 사라짐.
  $effect(() => {
    if (!visible) return;
    if (!autoRespawn) return;
    void triggerAutoRespawn();
  });

  async function triggerAutoRespawn(): Promise<void> {
    if (!danglingTerminals.startRespawn(terminalId)) return;
    try {
      await respawnTerminal(terminalId);
      // 명시 clear — 0x88 broadcast 가 dispatcher 로도 도착하지만 visual 즉시
      // 정합 위해 caller 도 clear (idempotent).
      danglingTerminals.clear(terminalId);
      void terminalPool.refresh();
    } catch (err) {
      // Lock 해제 — mark 는 유지. 사용자가 회복 시도 트리거 (mount remount 등).
      danglingTerminals.releaseRespawn(terminalId);
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      // multi-webpage race: 다른 webpage 가 먼저 respawn 했고 BE 가 conflict
      // 응답한 경우 noisy toast 회피 — 0x88 가 곧 도착 (또는 이미 도착) 하면
      // overlay 자연 해제. error message 만 console.
      console.debug('[gtmux] auto-respawn failed (likely race)', err);
    }
  }

  onMount(() => {
    // mount 직후 한 번 명시 호출 — $effect 가 reason set 보다 먼저 mount 된
    // 경우 대비. visible derived 가 false 면 noop.
    if (visible && autoRespawn) void triggerAutoRespawn();
  });
</script>

{#if visible}
  <div class="overlay" role="status" aria-live="polite">
    <div class="card">
      <div class="title">
        {reason === 'killed' ? 'Terminal killed' : 'Terminal exited'}
      </div>
      <div class="hint">
        {#if reason === 'killed'}
          Terminal is stopped.
        {:else}
          {respawning ? 'Respawning…' : 'Re-creating terminal…'}
        {/if}
      </div>
      {#if reason !== 'killed'}
        <div class="spinner" aria-hidden="true"></div>
      {/if}
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
    pointer-events: none;
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

  .spinner {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-accent);
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
