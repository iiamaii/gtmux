<script lang="ts">
  /**
   * ReconnectModal — page entry blocking modal (ADR-0019 D5.4, plan-0008 §1.3/§4.3).
   *
   * 4 mode: loading / in_use / not_found / unreachable
   *
   * Behaviour:
   * - Esc / backdrop click 비활성 — Modal primitive 의 `dismissOnBackdrop=false` +
   *   `onclose` 미지정. 사용자 cancel 은 [Switch session…] 의 명시 click 만 valid.
   * - 100ms grace — `loading` mount 시점에 visible 을 100ms 지연하여 빠른 200
   *   케이스 (BE 응답 < 100ms) 의 modal flicker 방지.
   * - State transition (loading → in_use 등) 시점에는 grace 적용 안 함 — content
   *   swap 만. ARIA 의 focus 가 새 actionable 버튼으로 이동.
   * - Focus trap + role=dialog + aria-modal=true — Modal primitive 가 제공.
   * - [Retry] 는 unreachable state 만 의미. [Switch session…] 은 모든 state.
   *
   * Less-emphasis vs primary 의 결정 (plan-0008 D-I.2):
   * - loading: [Switch session…] = ghost (text-link 톤) — 정상 흐름은 attempt 우선
   * - in_use / not_found: [Switch session…] = primary — 사용자 선택이 다음 액션
   * - unreachable: [Retry] = primary (default), [Switch session…] = secondary
   */

  import { untrack } from 'svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import type { ReconnectModalState } from '$lib/stores/reconnectGate.svelte';

  interface Props {
    /**
     * ReconnectGate 의 active state — 'idle' / 'success' 는 caller (AppPage)
     * 가 자체 unmount 처리하므로 본 prop 의 union 에서 제외.
     * Prop 이름은 `mode` — svelte-check 의 legacy `$state` store-prefix
     * heuristic 회피.
     */
    mode: ReconnectModalState;
    name: string;
    attempt: number;
    error: string | null;
    onSwitchSession: () => void;
    onRetry: () => void;
  }

  const { mode, name, attempt, error, onSwitchSession, onRetry }: Props =
    $props();

  // 100ms grace — mount 시점부터 timer 시작, expire 시 visible=true.
  // state 가 loading 외로 전이되면 grace skip (즉시 visible).
  //
  // ⚠️ 0045 fix — graceTimer 는 *plain let* (NOT $state)!
  //   $state 로 두면 effect 의 `if (graceTimer !== null)` read + `graceTimer = ...`
  //   write 가 self-trigger → effect_update_depth_exceeded. effect deps 에서
  //   graceTimer 를 제외해야 안정.
  let visible = $state(false);
  let graceTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    // 의존성: mode 만. visible 은 untrack 으로 read.
    const currentMode = mode;
    const isVisible = untrack(() => visible);
    if (currentMode === 'loading' && !isVisible) {
      // first mount in loading — start grace
      if (graceTimer !== null) clearTimeout(graceTimer);
      graceTimer = setTimeout(() => {
        graceTimer = null;
        visible = true;
      }, 100);
      return () => {
        if (graceTimer !== null) {
          clearTimeout(graceTimer);
          graceTimer = null;
        }
      };
    }
    // non-loading state — show immediately
    if (currentMode !== 'loading') {
      if (graceTimer !== null) {
        clearTimeout(graceTimer);
        graceTimer = null;
      }
      visible = true;
    }
  });

  const title = $derived.by(() => {
    switch (mode) {
      case 'loading':
        return 'Reconnecting session';
      case 'in_use':
        return 'Session in use';
      case 'not_found':
        return 'Session not found';
      case 'unreachable':
        return 'Reconnect failed';
    }
  });
</script>

<Modal
  open={visible}
  title={undefined}
  dismissOnBackdrop={false}
>
  {#snippet body()}
    <header class="modal-header" class:warn={mode !== 'loading'}>
      <h2 class="modal-title">
        {title}
        {#if mode !== 'loading'}
          <span class="warn-icon" aria-hidden="true">⚠</span>
        {/if}
      </h2>
    </header>

    <div class="modal-body">
      {#if mode === 'loading'}
        <div class="loading-row">
          <span class="spinner" aria-hidden="true"></span>
          <span class="loading-text">
            Restoring <code>"{name}"</code>…
          </span>
        </div>
      {:else if mode === 'in_use'}
        <p>
          <code>"{name}"</code> is now in use by another window.
        </p>
        <p class="hint">
          Idle timeout 후 다른 webpage 가 가져갔을 수 있어요.
        </p>
      {:else if mode === 'not_found'}
        <p>
          <code>"{name}"</code> no longer exists.
        </p>
        <p class="hint">다른 session 으로 진입하세요.</p>
      {:else if mode === 'unreachable'}
        <p>Couldn't reach the server.</p>
        <p class="hint">
          Attempt {attempt}.
          {#if error}
            Last error: <code class="err">{error}</code>
          {/if}
        </p>
      {/if}
    </div>
  {/snippet}

  {#snippet footer()}
    {#if mode === 'loading'}
      <Button variant="ghost" onclick={onSwitchSession}>Switch session…</Button>
    {:else if mode === 'unreachable'}
      <Button variant="secondary" onclick={onSwitchSession}>Switch session…</Button>
      <Button variant="primary" onclick={onRetry}>Retry</Button>
    {:else}
      <!-- in_use / not_found -->
      <Button variant="primary" onclick={onSwitchSession}>Switch session…</Button>
    {/if}
  {/snippet}
</Modal>

<style>
  .modal-header {
    padding: 0 0 var(--space-8);
  }

  .modal-title {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: var(--weight-semibold);
    line-height: var(--leading-tight);
    color: var(--color-fg);
    display: inline-flex;
    align-items: center;
    gap: var(--space-8);
  }

  .modal-header.warn .modal-title {
    color: var(--color-fg);
  }

  .warn-icon {
    color: var(--color-warning);
    font-size: var(--text-lg);
  }

  .modal-body {
    font-size: var(--text-base);
    color: var(--color-fg-muted);
    line-height: var(--leading-normal);
    display: flex;
    flex-direction: column;
    gap: var(--space-8);
  }

  .modal-body p {
    margin: 0;
  }

  .modal-body p.hint {
    color: var(--color-fg-subtle);
    font-size: var(--text-sm);
  }

  .modal-body code {
    font-family: var(--font-mono);
    background: var(--color-surface-2);
    padding: 1px 4px;
    border-radius: var(--radius-sm);
    color: var(--color-fg);
  }

  .modal-body code.err {
    color: var(--color-danger);
  }

  .loading-row {
    display: inline-flex;
    align-items: center;
    gap: var(--space-12);
    padding: var(--space-8) 0;
  }

  .loading-text {
    font-size: var(--text-base);
    color: var(--color-fg);
  }

  .spinner {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-accent);
    animation: spin 800ms linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
