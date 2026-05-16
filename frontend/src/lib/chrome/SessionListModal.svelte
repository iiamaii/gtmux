<script lang="ts">
  /**
   * SessionListModal — 기존 session 선택 modal.
   *
   * 정본:
   * - ADR-0019 D6.4 + plan-0007 §14.20 G18 (1s polling while open)
   * - ADR-0019 D3 (single-attach reciprocal — 활성 session 은 disabled)
   * - plan-0007 §14 FE-NEW-1
   *
   * 동작:
   * 1. Modal open 시 즉시 `listSessions()` 1회 + 1s 주기 polling 시작.
   * 2. Modal close 시 polling 즉시 중단 + state reset.
   * 3. 활성 session = 50% opacity + "in use" badge + click disabled + tooltip.
   * 4. 사용자 click → `onSelect(name)` (부모가 attach 호출).
   *
   * 부모 책임:
   * - `onSelect(name)` 안에서 `attachSession()` 호출 → AttachConfirmModal 진입.
   * - 401 시 부모가 `/auth` 로 redirect.
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { listSessions, UnauthorizedError } from '$lib/http/sessions';
  import type { SessionInfo } from '$lib/types/sessions';

  interface Props {
    open: boolean;
    onClose: () => void;
    /** Session 선택 — 부모가 attach 흐름 진행. 비활성 row 는 호출 X. */
    onSelect: (name: string) => void;
    /** 401 시 부모가 redirect 처리 — 호출되면 `/auth` 로 이동. */
    onUnauthorized?: () => void;
    /** Polling 주기 (ms). 기본 1000 (G18). 테스트 시 override. */
    pollIntervalMs?: number;
  }

  const {
    open,
    onClose,
    onSelect,
    onUnauthorized,
    pollIntervalMs = 1000,
  }: Props = $props();

  let sessions = $state<SessionInfo[]>([]);
  let loading = $state(true);
  let errorMessage = $state<string | null>(null);
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  let available = $derived(sessions.filter((s) => !s.active));
  let inUse = $derived(sessions.filter((s) => s.active));

  async function refresh(): Promise<void> {
    try {
      const res = await listSessions();
      sessions = res.sessions;
      loading = false;
      errorMessage = null;
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      errorMessage = err instanceof Error ? err.message : String(err);
      loading = false;
    }
  }

  // Polling lifecycle — modal open/close 정합.
  $effect(() => {
    if (open) {
      loading = true;
      sessions = [];
      errorMessage = null;
      void refresh();
      pollTimer = setInterval(() => {
        void refresh();
      }, pollIntervalMs);
      return () => {
        if (pollTimer !== null) {
          clearInterval(pollTimer);
          pollTimer = null;
        }
      };
    }
  });

  function formatLastUsed(iso: string): string {
    try {
      const d = new Date(iso);
      const now = Date.now();
      const diffMs = now - d.getTime();
      const sec = Math.round(diffMs / 1000);
      if (sec < 60) return 'just now';
      const min = Math.round(sec / 60);
      if (min < 60) return `${min}m ago`;
      const hr = Math.round(min / 60);
      if (hr < 24) return `${hr}h ago`;
      const day = Math.round(hr / 24);
      return `${day}d ago`;
    } catch {
      return iso;
    }
  }
</script>

<Modal
  {open}
  onclose={onClose}
  title="Open existing session"
  dismissOnBackdrop={false}
>
  {#snippet body()}
    {#if loading}
      <p class="state">Loading sessions…</p>
    {:else if errorMessage !== null}
      <p class="state error" role="alert">{errorMessage}</p>
    {:else if sessions.length === 0}
      <p class="state">No sessions yet. Use <em>New session</em> to create one.</p>
    {:else}
      {#if available.length > 0}
        <div class="section">
          <div class="section-head">Available</div>
          <ul class="list" role="listbox" aria-label="Available sessions">
            {#each available as s (s.name)}
              <li>
                <button
                  type="button"
                  class="row"
                  onclick={() => onSelect(s.name)}
                >
                  <span class="row-main">
                    <span class="row-name">{s.name}</span>
                    <span class="row-meta">
                      last used {formatLastUsed(s.last_used_at)}
                      {#if s.item_count !== undefined}
                        · {s.item_count} items
                      {/if}
                    </span>
                  </span>
                  <svg
                    class="row-chevron"
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                  >
                    <polyline points="9 18 15 12 9 6" />
                  </svg>
                </button>
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      {#if inUse.length > 0}
        <div class="section">
          <div class="section-head">In use</div>
          <ul class="list" aria-label="Sessions in use by other webpages">
            {#each inUse as s (s.name)}
              <li>
                <span
                  class="row disabled"
                  aria-disabled="true"
                  title={s.active_server_pid !== undefined
                    ? `In use by server-pid ${s.active_server_pid}`
                    : 'In use by another webpage'}
                >
                  <span class="row-main">
                    <span class="row-name">{s.name}</span>
                    <span class="row-meta">
                      last used {formatLastUsed(s.last_used_at)}
                    </span>
                  </span>
                  <span class="badge">
                    {s.active_server_pid !== undefined
                      ? `pid ${s.active_server_pid}`
                      : 'attached'}
                  </span>
                </span>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    {/if}
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={onClose}>Cancel</Button>
  {/snippet}
</Modal>

<style>
  .state {
    margin: 0;
    padding: var(--space-24) 0;
    text-align: center;
    color: var(--color-fg-muted);
    font-size: var(--text-md);
  }

  .state.error {
    color: var(--color-danger);
  }

  .state em {
    font-style: normal;
    color: var(--color-fg);
    font-weight: var(--weight-medium);
  }

  .section + .section {
    margin-top: var(--space-16);
  }

  .section-head {
    font-family: var(--font-mono);
    font-size: var(--text-base);
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--color-fg-muted);
    margin-bottom: var(--space-8);
    padding: 0 var(--space-4);
  }

  .list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .row {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-12);
    padding: var(--space-10) var(--space-12);
    background: var(--color-surface-2);
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    color: var(--color-fg);
    text-align: left;
    transition:
      background var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing);
    cursor: pointer;
  }

  .row:hover:not(.disabled) {
    background: var(--color-glass-1);
    border-color: var(--color-border-strong);
  }

  .row:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .row.disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .row-main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .row-name {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    color: var(--color-fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-meta {
    font-size: var(--text-base);
    color: var(--color-fg-muted);
  }

  .row-chevron {
    color: var(--color-fg-subtle);
    flex-shrink: 0;
  }

  .badge {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    padding: 2px 8px;
    border-radius: var(--radius-pill);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    color: var(--color-fg-muted);
    text-transform: lowercase;
    letter-spacing: 0.3px;
    flex-shrink: 0;
  }
</style>
