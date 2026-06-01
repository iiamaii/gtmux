<script lang="ts">
  /**
   * ChangeTerminalModal — picker to rebind a terminal panel to a different
   * terminal UUID (FE-NEW-4, ADR-0021 D8, frontend-handover-v2).
   *
   * Approach (until BE ships `PUT /api/sessions/<name>/items/<id>/terminal`):
   *   layout PUT 의 atomic mutate — 기존 terminal item 제거 + 새 UUID 로
   *   동일 position/size/z/parent_id/label/visibility/locked/minimized 의
   *   terminal item 추가. `mutateLayout` 한 round-trip 으로 처리.
   *
   * Constraints:
   *   - 새 terminal 은 *alive* (terminalPool 의 alive=true 만 노출).
   *   - 새 terminal id 가 이미 같은 layout 에 있으면 disabled (panel 중복 금지).
   *   - 현재 terminal 은 list 에서 제외.
   *
   * UX:
   *   - Esc / backdrop / [Cancel] 닫힘. Enter = 첫 row commit.
   *   - Empty pool → "No other terminals" placeholder.
   *   - 5s polling 은 terminalPool 가 이미 구독 카운팅.
   */

  import { onMount } from 'svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { changeTerminalDialog } from '$lib/stores/changeTerminalDialog.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import type { CanvasItem, TerminalItem } from '$lib/types/canvas';
  import type { TerminalInfo } from '$lib/types/terminals';

  let committing = $state(false);

  const open = $derived(changeTerminalDialog.open);
  const panelId = $derived(changeTerminalDialog.panelId);

  const currentItem = $derived.by((): TerminalItem | null => {
    if (panelId === null) return null;
    const it = sessionStore.items.get(panelId);
    if (!it || it.type !== 'terminal') return null;
    return it;
  });
  const modalOpen = $derived(open && currentItem !== null);

  // Alive pool minus the current binding; sorted by recency (newest first).
  const candidates = $derived.by((): TerminalInfo[] => {
    const cur = currentItem?.id ?? null;
    return terminalPool.terminals
      .filter((t) => t.alive && t.id !== cur)
      .sort((a, b) => b.created_at - a.created_at);
  });

  function isAlreadyOnCanvas(uuid: string): boolean {
    return sessionStore.items.has(uuid);
  }

  function shortId(id: string): string {
    return id.replace(/-/g, '').slice(0, 8);
  }

  function displayName(t: TerminalInfo): string {
    return t.label.length > 0 ? t.label : `t${shortId(t.id)}`;
  }

  function close(): void {
    if (committing) return;
    changeTerminalDialog.close();
  }

  async function commit(nextId: string): Promise<void> {
    if (committing) return;
    const active = sessionStore.active;
    const cur = currentItem;
    if (active === null || cur === null) {
      close();
      return;
    }
    if (nextId === cur.id) {
      close();
      return;
    }
    if (isAlreadyOnCanvas(nextId)) {
      toastStore.show({
        message: 'That terminal is already on this canvas.',
        tone: 'warning',
      });
      return;
    }
    committing = true;
    try {
      const result = await sessionStore.applyMutation(
        (curLayout) => {
          const replaced: TerminalItem = {
            ...cur,
            id: nextId,
          };
          const items: CanvasItem[] = curLayout.items.map((it) =>
            it.id === cur.id ? replaced : it,
          );
          return { ...curLayout, items };
        },
        { failMessage: 'Rebind failed' },
      );
      if (!result.ok) return;
      // M follows the new id so the rebind doesn't leave a phantom selection.
      if (sessionStore.M.has(cur.id)) {
        sessionStore.M.delete(cur.id);
        sessionStore.M.add(nextId);
      }
      void terminalPool.refresh();
      toastStore.show({
        message: `Rebound panel to ${shortId(nextId)}.`,
        tone: 'success',
      });
      changeTerminalDialog.close();
    } finally {
      committing = false;
    }
  }

  onMount(() => terminalPool.subscribe());

  function onWindowKey(e: KeyboardEvent): void {
    if (!modalOpen) return;
    if (e.key === 'Enter') {
      const first = candidates[0];
      if (first && !isAlreadyOnCanvas(first.id)) {
        e.preventDefault();
        void commit(first.id);
      }
    }
  }

  $effect(() => {
    if (typeof window === 'undefined') return;
    if (!modalOpen) return;
    window.addEventListener('keydown', onWindowKey);
    return () => window.removeEventListener('keydown', onWindowKey);
  });
</script>

<Modal
  open={modalOpen}
  onclose={close}
  title="Change terminal"
  dismissOnBackdrop={!committing}
  dismissOnEsc={!committing}
>
  {#snippet body()}
    {#if currentItem !== null}
      <div class="modal-stack">
        <p class="modal-copy">
          Replace the binding of this panel with a different terminal. The
          previous terminal stays alive in the pool; only this panel's stream
          switches.
        </p>
        <div class="current">
          <span class="k mono">current</span>
          <span class="v mono">{shortId(currentItem.id)}</span>
        </div>

        {#if candidates.length === 0}
          <p class="placeholder">No other terminals in the pool.</p>
        {:else}
          <ul class="pick-list">
            {#each candidates as t (t.id)}
              {@const dup = isAlreadyOnCanvas(t.id)}
              <li>
                <button
                  type="button"
                  class="pick-row"
                  disabled={dup || committing}
                  title={dup ? 'Already on this canvas' : `Bind to ${shortId(t.id)}`}
                  onclick={() => void commit(t.id)}
                >
                  <span class="alive on" aria-hidden="true"></span>
                  <span class="name">{displayName(t)}</span>
                  <span class="id mono">{shortId(t.id)}</span>
                  {#if t.attach_count > 0}
                    <span class="badge" title="{t.attach_count} session reference(s)">
                      ×{t.attach_count}
                    </span>
                  {/if}
                  {#if dup}
                    <span class="badge muted">on canvas</span>
                  {/if}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  {/snippet}

  {#snippet footer()}
    <Button variant="ghost" onclick={close} disabled={committing}>Cancel</Button>
  {/snippet}
</Modal>

<style>
  .current {
    display: inline-flex;
    align-items: center;
    gap: var(--space-6);
    padding: var(--space-4) var(--space-8);
    align-self: flex-start;
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
    font-size: var(--text-base);
  }

  .current .k {
    color: var(--color-fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.4px;
    font-size: var(--text-sm);
  }

  .current .v {
    color: var(--color-fg);
  }

  .placeholder {
    margin: 0;
    padding: var(--space-16) 0;
    text-align: center;
    color: var(--color-fg-subtle);
    font-style: italic;
    font-size: var(--text-md);
  }

  .pick-list {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .pick-row {
    display: grid;
    grid-template-columns: 8px 1fr auto auto auto;
    align-items: center;
    gap: var(--space-6);
    width: 100%;
    padding: var(--space-6) var(--space-8);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: var(--text-base);
    text-align: left;
    cursor: pointer;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .pick-row:hover:not(:disabled) {
    background: var(--color-glass-1);
  }

  .pick-row:disabled {
    opacity: 0.5;
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
  }

  .id {
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }

  .mono {
    font-family: var(--font-mono);
  }

  .badge {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    padding: 1px 6px;
    border-radius: var(--radius-pill);
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    color: var(--color-accent);
  }

  .badge.muted {
    background: var(--color-surface-2);
    color: var(--color-fg-subtle);
  }
</style>
