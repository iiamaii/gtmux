<script lang="ts">
  // PanelDanglingOverlay — terminal 이 0x85 TERMINAL_DIED 로 dangling 표시되면
  // visual placeholder. 정상 exit 는 기존 정책대로 자동 respawn, explicit kill
  // (Panel+Terminal / SIGTERM) 은 사용자의 종료 의도를 보존해 자동 respawn 하지
  // 않고 명시 CTA 로만 복구한다.
  //
  // 정본:
  // - BE Stage 5-B (0034 §3): UUID-carrying terminal-died broadcast
  // - ADR-0021 D10 / D10.1 (G25.1.b amend): reason 분기 + 명시 [Restart terminal] CTA
  // - 시안: ref/frontend-design/components-v5 §04 Panel · "Not connected — empty state"

  import { onMount } from 'svelte';
  import { danglingTerminals } from '$lib/stores/danglingTerminals.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { respawnTerminal } from '$lib/http/terminals';
  import { UnauthorizedError } from '$lib/http/sessions';
  import { toastStore } from '$lib/ui/toast-store.svelte';

  const { terminalId }: { terminalId: string } = $props();

  const reason = $derived(danglingTerminals.reasonFor(terminalId));
  const visible = $derived(reason !== null);
  const respawning = $derived(danglingTerminals.isRespawning(terminalId));
  const autoRespawn = $derived(reason === 'exit');

  // 자동 respawn — exit reason 만. killed 은 사용자 click 으로만 진입.
  $effect(() => {
    if (!visible) return;
    if (!autoRespawn) return;
    void triggerRespawn();
  });

  async function triggerRespawn(): Promise<void> {
    if (!danglingTerminals.startRespawn(terminalId)) return;
    try {
      await respawnTerminal(terminalId);
      // 명시 clear — 0x88 broadcast 가 dispatcher 로도 도착하지만 visual 즉시
      // 정합 위해 caller 도 clear (idempotent).
      danglingTerminals.clear(terminalId);
      void terminalPool.refresh();
    } catch (err) {
      // Lock 해제 — mark 는 유지. 사용자가 다시 CTA 누르면 재시도.
      danglingTerminals.releaseRespawn(terminalId);
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      // exit (auto) path 는 silent (race) — killed (manual) path 는 surface.
      if (autoRespawn) {
        console.debug('[gtmux] auto-respawn failed (likely race)', err);
        return;
      }
      toastStore.show({
        message: `Restart failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  function onManualRestart(e: MouseEvent): void {
    e.stopPropagation();
    if (respawning) return;
    void triggerRespawn();
  }

  onMount(() => {
    // mount 직후 한 번 명시 호출 — exit reason 만 자동 trigger. killed 은 CTA 대기.
    if (visible && autoRespawn) void triggerRespawn();
  });
</script>

{#if visible}
  {#if reason === 'killed'}
    <!-- Resting empty state — components-v5 §04 "Not connected" 패턴 차용.
         panel-body 의 xterm-bg 와 연결되도록 동일 background 위에 dashed 40×40
         ring (+ double halo) + mono title/msg + primary CTA. -->
    <div class="empty-panel" role="status" aria-live="polite">
      <div class="empty-stack">
        <div class="empty-glyph" aria-hidden="true">
          <svg
            width="18"
            height="18"
            viewBox="0 0 18 18"
            fill="none"
            stroke="currentColor"
            stroke-width="1.3"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <rect x="1.6" y="2.4" width="14.8" height="13.2" rx="1.8" />
            <path d="M4.2 6.8l2.6 2.0-2.6 2.0" />
            <path d="M8.0 12.2h5.4" />
          </svg>
        </div>
        <div class="empty-title">Terminal stopped</div>
        <div class="empty-actions">
          <button
            type="button"
            class="empty-cta primary nodrag"
            onclick={onManualRestart}
            onpointerdown={(e: PointerEvent) => e.stopPropagation()}
            disabled={respawning}
          >
            <svg
              viewBox="0 0 12 12"
              fill="none"
              stroke="currentColor"
              stroke-width="1.4"
              stroke-linecap="round"
              aria-hidden="true"
            >
              <path d="M3 6h6M6 3v6" />
            </svg>
            {respawning ? 'Restarting…' : 'Restart terminal'}
          </button>
        </div>
        <div class="empty-msg">
          Process ended by an <b>explicit kill</b>.<br />
          Restart when you need the panel again.
        </div>
        <div class="empty-hint">
          or press <span class="kbd">⌘</span><span class="kbd">R</span> while focused
        </div>
      </div>
    </div>
  {:else}
    <!-- Transient — exit reason 의 auto respawn 진행. 곧 사라질 상태이므로 v5
         empty 패턴 대신 가벼운 horizontal card + spinner. -->
    <div class="transient-overlay" role="status" aria-live="polite">
      <div class="transient-card">
        <div class="spinner" aria-hidden="true"></div>
        <div class="transient-text">
          <div class="transient-label">Restoring terminal</div>
          <span class="transient-hint">
            {respawning ? 'RESPAWNING…' : 'RECONNECTING…'}
          </span>
        </div>
      </div>
    </div>
  {/if}
{/if}

<style>
  /* ── Empty panel (reason='killed') ─────────────────────────────────
   * components-v5 §04 "Not connected — empty state" 차용. panel-body 와
   * 동일 background (xterm-bg) 위에 subtle radial highlight + 중앙 stack.
   * Token-aware (theme 따라 light/dark 모두 정합). */
  .empty-panel {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 22px 28px;
    text-align: center;
    background:
      radial-gradient(
        circle at 50% 38%,
        color-mix(in srgb, var(--color-fg) 4%, transparent),
        transparent 60%
      ),
      var(--xterm-bg);
    z-index: 5;
    pointer-events: auto;
  }

  .empty-stack {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-8);
    max-width: 320px;
  }

  /* 40×40 dashed ring with secondary halo (v5 ::after pattern). */
  .empty-glyph {
    position: relative;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    border: 1px dashed
      color-mix(in srgb, var(--color-fg) 18%, transparent);
    display: grid;
    place-items: center;
    color: var(--color-fg-muted);
    margin-bottom: 2px;
  }

  .empty-glyph::after {
    content: '';
    position: absolute;
    inset: -5px;
    border-radius: 50%;
    border: 1px dashed
      color-mix(in srgb, var(--color-fg) 6%, transparent);
  }

  .empty-title {
    font-family: var(--font-mono);
    font-size: 11.5px;
    font-weight: var(--weight-semibold);
    letter-spacing: 0.1px;
    color: var(--color-fg);
    margin-top: 2px;
  }

  .empty-msg {
    font-family: var(--font-mono);
    font-size: 10.5px;
    line-height: 1.55;
    color: var(--color-fg-muted);
  }

  .empty-msg b {
    color: var(--color-fg);
    font-weight: var(--weight-semibold);
  }

  .empty-actions {
    display: flex;
    gap: var(--space-6);
    margin-top: var(--space-6);
  }

  /* Pill CTA — v5 spec: 50px radius, mono 10px / 0.2 letter-spacing,
   * 5/12/6 padding, 11×11 icon. */
  .empty-cta {
    display: inline-flex;
    align-items: center;
    gap: var(--space-6);
    height: auto;
    padding: 5px 12px 6px;
    border-radius: var(--radius-pill);
    border: 1px solid
      color-mix(in srgb, var(--color-fg) 18%, transparent);
    background: color-mix(in srgb, var(--color-fg) 4%, transparent);
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.2px;
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing);
  }

  .empty-cta svg {
    width: 11px;
    height: 11px;
  }

  .empty-cta:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-fg) 8%, transparent);
  }

  .empty-cta:disabled {
    cursor: wait;
    opacity: 0.7;
  }

  /* Primary — Button.svelte 의 primary variant 정합 (2026-05-21):
   * --color-accent solid background. 다른 dialog 의 primary button 과 동일.
   * 옛 success(green) tint 는 폐기 — 사용자 UX 통일 요구. */
  .empty-cta.primary {
    border-color: var(--color-accent);
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }

  .empty-cta.primary:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-accent) 88%, white);
  }

  .empty-hint {
    margin-top: var(--space-6);
    font-family: var(--font-mono);
    font-size: 9.5px;
    letter-spacing: 0.3px;
    color: var(--color-fg-subtle);
  }

  .empty-hint .kbd {
    display: inline-block;
    padding: 0 5px;
    margin: 0 1px;
    border: 1px solid
      color-mix(in srgb, var(--color-fg) 14%, transparent);
    background: color-mix(in srgb, var(--color-fg) 4%, transparent);
    border-radius: 3px;
    color: var(--color-fg);
    font-size: 9px;
  }

  /* ── Transient overlay (reason='exit') ──────────────────────────── */
  .transient-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--color-bg) 70%, transparent);
    backdrop-filter: blur(2px);
    z-index: 5;
    pointer-events: none;
  }

  .transient-card {
    display: inline-flex;
    align-items: center;
    gap: var(--space-10);
    padding: var(--space-8) var(--space-12);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06);
  }

  .transient-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    text-align: left;
  }

  .transient-label {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    color: var(--color-fg);
    line-height: 1.2;
  }

  .transient-hint {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
  }

  .spinner {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-accent);
    animation: spin 0.8s linear infinite;
    flex: 0 0 auto;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
