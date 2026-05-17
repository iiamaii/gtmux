<script lang="ts">
  /**
   * TerminalsPanel — server-wide terminal pool, as an independent
   * floating panel on the bottom-left (split out from Sidebar).
   *
   * 정본:
   * - ADR-0017 §D2 amend (Sidebar = Layers only; Terminals is its own
   *   floating chrome — sibling of Sidebar / PaneInfoPanel)
   * - ADR-0021 D7 (server-wide Terminal pool)
   * - plan-0007 §14 FE-NEW-3
   * - BE Phase 4-B / BE-NEW-10 (`GET /api/terminals`)
   *
   * 동작:
   * - 5s 폴링 (mount 동안) — terminalPool.subscribe().
   * - 각 row: alive dot + label/short id + attach count badge + sessions hint.
   * - Attach 버튼 → 현 active session 의 layout 에 추가 (PUT /api/sessions/<name>/layout).
   * - Attach/Kill 버튼:
   *   - 현재 session panel 과 연결됨: 액션 숨김.
   *   - 다른 session panel 과만 연결됨: Attach 만 표시.
   *   - 어느 panel 과도 연결되지 않음: Attach + Kill 표시.
   *   Panel 과 연결된 terminal 종료는 panel close 의 [Panel + Terminal] 경로로 수행한다.
   * - 빈 pool → "No terminals" placeholder.
   * - Header 우측 fold 버튼 → chromeStore.toggleTerminals().
   *
   * Cross-session leak filter (0039 §3.2 step 4):
   * - Default: 현 active session 에 attach 됐거나 unplaced (attach_count === 0)
   *   인 것만 표시. 다른 session 에만 attach 된 것은 hide — 사용자가 의도치
   *   않게 cross-session terminal 을 현 canvas 에 attach 하는 leak 차단.
   * - Toggle [All]: server-wide pool 전체 노출 (debug / admin).
   */

  import { onMount } from 'svelte';
  import { UnauthorizedError } from '$lib/http/sessions';
  import { killTerminal } from '$lib/http/terminals';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { danglingTerminals } from '$lib/stores/danglingTerminals.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import type { TerminalInfo } from '$lib/types/terminals';
  import type { CanvasItem, TerminalItem } from '$lib/types/canvas';

  const PANEL_DEFAULT_W = 480;
  const PANEL_DEFAULT_H = 320;
  const CASCADE_STEP = 40;

  let attaching = $state<Set<string>>(new Set());
  let killing = $state<Set<string>>(new Set());
  let showAllSessions = $state<boolean>(false);

  let allTerminals = $derived(terminalPool.terminals);
  let loading = $derived(terminalPool.loading);
  let errorMessage = $derived(terminalPool.errorMessage);

  // Cross-session leak filter (0039 §3.2 step 4) — default 시 다른 session 에만
  // attach 된 entry 를 hide. unplaced (attach_count === 0) 와 현 active session
  // attach 는 모두 노출.
  let terminals = $derived.by<TerminalInfo[]>(() => {
    if (showAllSessions) return allTerminals;
    const active = sessionStore.active;
    if (active === null) return allTerminals;
    return allTerminals.filter(
      (t) =>
        t.attach_count === 0 ||
        t.attached_sessions.includes(active.name),
    );
  });

  let hiddenCount = $derived(
    showAllSessions ? 0 : Math.max(0, allTerminals.length - terminals.length),
  );

  onMount(() => {
    return terminalPool.subscribe();
  });

  function shortId(id: string): string {
    return id.replace(/-/g, '').slice(0, 8);
  }

  function displayName(t: TerminalInfo): string {
    if (t.label.length > 0) return t.label;
    return `t${shortId(t.id)}`;
  }

  function ago(unixSec: number): string {
    if (unixSec <= 0) return '';
    const diff = Math.max(0, Math.floor(Date.now() / 1000) - unixSec);
    if (diff < 60) return `${diff}s ago`;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  let activeSessionName = $derived(sessionStore.active?.name ?? null);

  function isOnCurrentCanvas(uuid: string): boolean {
    if (activeSessionName === null) return false;
    return sessionStore.items.has(uuid);
  }

  async function attachToCanvas(uuid: string): Promise<void> {
    if (activeSessionName === null) {
      toastStore.show({
        message: 'No active session — attach a session first.',
        tone: 'warning',
      });
      return;
    }
    if (isOnCurrentCanvas(uuid)) {
      toastStore.show({
        message: 'Terminal already on this canvas.',
        tone: 'info',
      });
      return;
    }
    const name = activeSessionName;
    attaching.add(uuid);
    attaching = new Set(attaching);
    try {
      const result = await sessionStore.applyMutation(
        (cur) => {
          const n = cur.items.length;
          const x = n * CASCADE_STEP;
          const y = n * CASCADE_STEP;
          const maxZ = cur.items.reduce(
            (m: number, it: CanvasItem) => (it.z > m ? it.z : m),
            0,
          );
          const item: TerminalItem = {
            id: uuid,
            type: 'terminal',
            parent_id: null,
            x,
            y,
            w: PANEL_DEFAULT_W,
            h: PANEL_DEFAULT_H,
            z: maxZ + 1,
            visibility: 'visible',
            locked: false,
            minimized: false,
          };
          return { ...cur, items: [...cur.items, item] };
        },
        {
          abortMessage: 'Session reconnect failed — attach aborted.',
          failMessage: 'Attach failed',
        },
      );
      if (!result.ok) return;
      void terminalPool.refresh();
      toastStore.show({
        message: `Attached terminal to "${name}".`,
        tone: 'success',
      });
    } finally {
      attaching.delete(uuid);
      attaching = new Set(attaching);
    }
  }

  async function killOne(t: TerminalInfo): Promise<void> {
    const uuid = t.id;
    if (killing.has(uuid)) return;
    if (t.attach_count > 0) {
      toastStore.show({
        message: 'Remove the linked panel with "Panel + Terminal" to stop this terminal.',
        tone: 'info',
      });
      return;
    }
    const guard = await sessionStore.guardOutgoingMutation();
    if (!guard.ok) {
      toastStore.show({
        message: 'Session reconnect failed — kill aborted.',
        tone: 'error',
      });
      return;
    }
    killing.add(uuid);
    killing = new Set(killing);
    try {
      await killTerminal(uuid);
      toastStore.show({
        message: `Killed terminal ${uuid.slice(0, 8)}.`,
        tone: 'info',
        durationMs: 5_000,
      });
      void terminalPool.refresh();
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Kill failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    } finally {
      killing.delete(uuid);
      killing = new Set(killing);
    }
  }
</script>

<div class="terminal-list-view" aria-label="Server-wide terminals">
  <div class="terminals-toolbar">
    <span class="count-text">
      <span class="count-num">{terminals.length}</span>
      <span class="count-suffix">terminal{terminals.length === 1 ? '' : 's'}</span>
      {#if hiddenCount > 0}
        <span class="hidden-hint" title={`${hiddenCount} 개가 다른 session 에만 attach 됨 — [All] 토글로 노출`}>
          (+{hiddenCount} hidden)
        </span>
      {/if}
    </span>
    <button
      type="button"
      class="filter-toggle"
      class:active={showAllSessions}
      title={showAllSessions
        ? 'Currently showing all sessions — click to filter to current session + unplaced'
        : 'Currently filtered to current session + unplaced — click to show all'}
      aria-pressed={showAllSessions}
      onclick={() => (showAllSessions = !showAllSessions)}
    >
      {showAllSessions ? 'All' : 'Mine'}
    </button>
  </div>

  <div class="terminals-body">
    {#if loading}
      <p class="state">Loading…</p>
    {:else if errorMessage !== null}
      <p class="state error" role="alert">{errorMessage}</p>
    {:else if terminals.length === 0}
      <p class="state placeholder">No terminals running</p>
    {:else}
      <ul class="term-list">
        {#each terminals as t (t.id)}
          {@const onCanvas = isOnCurrentCanvas(t.id)}
          {@const busy = attaching.has(t.id)}
          {@const unplaced = t.attach_count === 0}
          <li
            class="term-row"
            class:on-canvas={onCanvas}
            title={`id: ${t.id}\nattached: ${t.attached_sessions.join(', ') || '(none)'}`}
          >
            <!-- alive dot — BE 의 alive 만으로는 현재 사실상 항상 on (dead → unregister
                 으로 row 자체가 빠짐) 이라 인디케이터 가치가 낮다. 0x85 TERMINAL_DIED
                 후 같은 id 가 dangling 상태로 잠시 살아있는 window 에서 grey 로 가라
                 앉히려 dangling 합산. (PanelNode 의 4-state LED 와 달리 sidebar 는
                 binary 만 유지 — 시급성은 panel header 의 red 가 담당.) -->
            <span
              class="alive"
              class:on={t.alive && !danglingTerminals.has(t.id)}
              aria-hidden="true"
            ></span>
            <span class="name">{displayName(t)}</span>
            {#if t.attach_count > 0}
              <span class="badge" title="{t.attach_count} session reference(s)">
                ×{t.attach_count}
              </span>
            {:else}
              <span class="badge muted" title="not yet placed on any canvas">unplaced</span>
            {/if}
            {#if t.created_at > 0}
              <span class="meta">{ago(t.created_at)}</span>
            {/if}
            {#if !onCanvas}
              <span class="row-actions">
                <button
                  type="button"
                  class="attach-btn"
                  disabled={onCanvas || busy || activeSessionName === null}
                  aria-label={onCanvas ? 'Already on canvas' : 'Attach to canvas'}
                  title={onCanvas
                    ? 'Already on this canvas'
                    : activeSessionName === null
                      ? 'Attach a session first'
                      : `Attach to "${activeSessionName}"`}
                  onclick={() => void attachToCanvas(t.id)}
                >
                  {#if busy}
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" aria-hidden="true">
                      <circle cx="12" cy="12" r="9" stroke-opacity="0.25" />
                      <path d="M12 3a9 9 0 0 1 9 9" />
                    </svg>
                  {:else if onCanvas}
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                      <polyline points="5 12 10 17 19 7" />
                    </svg>
                  {:else}
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                      <line x1="12" y1="5" x2="12" y2="19" />
                      <line x1="5" y1="12" x2="19" y2="12" />
                    </svg>
                  {/if}
                </button>
                {#if unplaced}
                  <button
                    type="button"
                    class="kill-btn"
                    disabled={killing.has(t.id)}
                    aria-label="Kill terminal"
                    title="Kill terminal"
                    onclick={() => void killOne(t)}
                  >
                    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                      <line x1="6" y1="6" x2="18" y2="18" />
                      <line x1="18" y1="6" x2="6" y2="18" />
                    </svg>
                  </button>
                {/if}
              </span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  /* Embedded view — host (LeftPanel) owns floating chrome + tabs. */
  .terminal-list-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .terminals-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-6);
    padding: var(--space-6) var(--space-12);
    border-bottom: 1px solid var(--color-border);
    flex: 0 0 auto;
    font-family: var(--font-mono);
    font-size: var(--text-base);
    color: var(--color-fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.6px;
  }

  .count-text {
    display: inline-flex;
    align-items: center;
    gap: var(--space-4);
  }

  .hidden-hint {
    color: var(--color-fg-subtle);
    text-transform: lowercase;
    letter-spacing: 0;
    font-size: var(--text-sm);
  }

  .filter-toggle {
    padding: 1px var(--space-6);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    color: var(--color-fg-muted);
    font: inherit;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    text-transform: uppercase;
    letter-spacing: 0.4px;
    cursor: pointer;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .filter-toggle:hover {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .filter-toggle.active {
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    color: var(--color-accent);
    border-color: color-mix(in srgb, var(--color-accent) 30%, transparent);
  }

  .count-num {
    padding: 1px 6px;
    border-radius: var(--radius-pill);
    background: var(--color-surface-2);
    color: var(--color-fg);
    letter-spacing: 0.2px;
    text-transform: none;
    font-size: var(--text-sm);
  }

  .count-suffix {
    text-transform: lowercase;
    letter-spacing: 0;
  }

  .terminals-body {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
  }

  .state {
    margin: 0;
    padding: var(--space-8) var(--space-12);
    color: var(--color-fg-muted);
    font-size: var(--text-base);
  }

  .state.placeholder {
    color: var(--color-fg-subtle);
    font-style: italic;
  }

  .state.error {
    color: var(--color-danger);
  }

  .term-list {
    list-style: none;
    padding: var(--space-4) 0;
    margin: 0;
  }

  .term-row {
    display: grid;
    grid-template-columns: 8px 1fr auto auto auto;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-4) var(--space-8) var(--space-4) var(--space-12);
    font-size: var(--text-md);
    line-height: var(--leading-normal);
    cursor: default;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .row-actions {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }

  .kill-btn {
    width: 18px;
    height: 18px;
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    background: transparent;
    display: grid;
    place-items: center;
    opacity: 0;
    transition:
      opacity var(--motion-fast) var(--motion-easing),
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
    cursor: pointer;
    border: 0;
  }

  .term-row:hover .kill-btn {
    opacity: 1;
  }

  .kill-btn:hover:not(:disabled) {
    background: var(--color-danger);
    color: white;
    opacity: 1;
  }

  .kill-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .term-row:hover {
    background: var(--color-glass-1);
  }

  .term-row.on-canvas .name {
    color: var(--color-accent);
  }

  .attach-btn {
    width: 18px;
    height: 18px;
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    background: transparent;
    display: grid;
    place-items: center;
    opacity: 0;
    transition:
      opacity var(--motion-fast) var(--motion-easing),
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
    cursor: pointer;
    border: 0;
  }

  .term-row:hover .attach-btn,
  .attach-btn:disabled {
    opacity: 1;
  }

  .attach-btn:hover:not(:disabled) {
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }

  .attach-btn:disabled {
    color: var(--color-success);
    cursor: not-allowed;
  }

  .alive {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-fg-subtle);
  }

  .alive.on {
    background: var(--color-success);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-success) 28%, transparent);
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: var(--text-md);
  }

  .badge {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    padding: 1px 6px;
    border-radius: var(--radius-pill);
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    color: var(--color-accent);
    letter-spacing: 0.2px;
  }

  .badge.muted {
    background: var(--color-surface-2);
    color: var(--color-fg-subtle);
  }

  .meta {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--color-fg-subtle);
    letter-spacing: 0.2px;
  }
</style>
