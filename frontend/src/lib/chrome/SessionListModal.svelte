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
  import SessionDeleteConfirmModal from './SessionDeleteConfirmModal.svelte';
  import { deleteSession, listSessions, UnauthorizedError } from '$lib/http/sessions';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
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

  // ADR-0019 D10.1 (G51 amend) — Available row 의 hover-kebab → Delete.
  // 가시성 규칙:
  //   - In use row 는 hover-kebab 없음 (그 자체 <span> non-button row).
  //   - Available 안에서도 본 webpage 의 active row (= sessionStore.active.name
  //     일치) 는 kebab 차단 — 본 entry 는 SessionMenu 의 "Delete current session…"
  //     이 own (entry 분기 혼선 방지).
  //   - 승인 후: deleteSession(name) → 1s polling (D6.4) 의 다음 tick 으로 row
  //     자연 제거. 별도 즉시 refresh 트리거 없음 (race 단순화). 404 (race) 도
  //     동일 (다음 poll 이 row 자연 제거).
  let pendingDeleteName = $state<string | null>(null);

  function canDelete(name: string): boolean {
    return sessionStore.active?.name !== name;
  }

  function onRequestDelete(e: MouseEvent, name: string): void {
    e.stopPropagation();
    pendingDeleteName = name;
  }

  async function onConfirmDelete(): Promise<void> {
    const name = pendingDeleteName;
    if (name === null) return;
    pendingDeleteName = null;
    try {
      await deleteSession(name);
      // 404 (race) 는 deleteSession 가 silent return (sessions.ts:121).
      // 다음 polling tick 에서 row 자연 제거.
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      toastStore.show({
        message: `Delete failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

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
              <li class="row-wrap">
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
                {#if canDelete(s.name)}
                  <button
                    type="button"
                    class="row-kebab"
                    aria-label="Delete session ‘{s.name}’"
                    title="Delete session…"
                    onclick={(e) => onRequestDelete(e, s.name)}
                  >
                    <svg
                      width="14"
                      height="14"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="2"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      aria-hidden="true"
                    >
                      <polyline points="3 6 5 6 21 6" />
                      <path d="M19 6l-2 14a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2L5 6" />
                      <path d="M10 11v6" />
                      <path d="M14 11v6" />
                      <path d="M9 6V4a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v2" />
                    </svg>
                  </button>
                {/if}
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

<SessionDeleteConfirmModal
  open={pendingDeleteName !== null}
  sessionName={pendingDeleteName ?? ''}
  onCancel={() => (pendingDeleteName = null)}
  onConfirm={() => void onConfirmDelete()}
/>

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

  /* hover-kebab overlay (ADR-0019 D10.1) — row 위에 absolute. row 의 click
   * 영역과 분리하기 위해 nested button (HTML invalid) 회피 + li position:relative. */
  .row-wrap {
    position: relative;
  }

  .row-kebab {
    position: absolute;
    top: 50%;
    right: var(--space-12);
    transform: translateY(-50%);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    opacity: 0;
    cursor: pointer;
    transition:
      opacity var(--motion-fast) var(--motion-easing),
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .row-wrap:hover .row-kebab,
  .row-kebab:focus-visible {
    opacity: 1;
  }

  .row-kebab:hover {
    background: color-mix(in srgb, var(--color-danger) 14%, transparent);
    color: var(--color-danger);
  }

  .row-kebab:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 1px;
  }

  /* kebab 노출 시 chevron 가시 영역 안 겹치도록 right padding 보강. */
  .row-wrap:hover .row,
  .row-wrap:focus-within .row {
    padding-right: 44px;
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
