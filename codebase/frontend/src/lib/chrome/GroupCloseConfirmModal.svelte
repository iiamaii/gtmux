<script lang="ts">
  /**
   * GroupCloseConfirmModal — confirm dialog for closing a group and
   * (optionally) its descendant terminals.
   *
   * 정본:
   * - ADR-0021 D9.3 (Group close = bulk 1 dialog)
   * - frontend-handover-v2 §3.2 GroupCloseConfirmModal
   * - ADR-0010 D7 (Group/Panel parent_id tree)
   *
   * Three options:
   *   [Cancel]               — close dialog, no state change.
   *   [Panels only]          — remove group + descendants from layout
   *                            only. Terminals stay alive in the pool;
   *                            other sessions retain their panels.
   *   [Panels + Terminals]   — additionally `POST /api/terminals/<id>/kill`
   *                            on each descendant terminal. Mirror hint
   *                            warns if any of them are attached to
   *                            other sessions (those panels go dangling).
   *
   * Implementation note: until BE ships `DELETE /api/sessions/<name>/
   * groups/<gid>`, we use `mutateLayout` (PUT) to atomically prune the
   * group + descendants in a single round trip. Kill calls fan out in
   * parallel.
   */

  import { onMount } from 'svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { groupCloseDialog } from '$lib/stores/groupCloseDialog.svelte';
  import { settingsStore } from '$lib/stores/settings.svelte';
  import { UnauthorizedError } from '$lib/http/sessions';
  import { killTerminal } from '$lib/http/terminals';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import type { CanvasItem } from '$lib/types/canvas';
  import { pruneEmptyGroups } from '$lib/types/group';

  let committing = $state(false);
  let autoCommittedGroupId = $state<string | null>(null);

  const open = $derived(groupCloseDialog.open);
  const groupId = $derived(groupCloseDialog.groupId);

  const group = $derived.by(() => {
    if (groupId === null) return null;
    return sessionStore.groups.get(groupId) ?? null;
  });
  const modalOpen = $derived(open && group !== null);

  /** Descendant group + item ids via parent_id BFS. */
  const descendants = $derived.by(() => {
    if (groupId === null) return { groupIds: [] as string[], items: [] as CanvasItem[] };
    const groupIds: string[] = [];
    const items: CanvasItem[] = [];

    // BFS over groups whose parent_id chain leads back to `groupId`.
    const queue: string[] = [groupId];
    const seen = new Set<string>([groupId]);
    while (queue.length > 0) {
      const cur = queue.shift()!;
      for (const g of sessionStore.groups.values()) {
        if (g.parent_id === cur && !seen.has(g.id)) {
          seen.add(g.id);
          groupIds.push(g.id);
          queue.push(g.id);
        }
      }
    }

    // Items whose parent_id is `groupId` or any descendant group.
    const ancestorSet = new Set<string>([groupId, ...groupIds]);
    for (const it of sessionStore.items.values()) {
      if (it.parent_id !== null && ancestorSet.has(it.parent_id)) {
        items.push(it);
      }
    }

    return { groupIds, items };
  });

  const terminalDescendants = $derived(
    descendants.items.filter((it) => it.type === 'terminal'),
  );

  /** Terminals that have other-session attachments — those panels will
   *  go dangling if we kill the terminal here. */
  const mirroredTerminals = $derived.by(() => {
    const active = sessionStore.active?.name ?? null;
    if (active === null) return [];
    const out: { id: string; otherCount: number }[] = [];
    for (const it of terminalDescendants) {
      const pool = terminalPool.byId(it.id);
      if (pool === null) continue;
      const others = pool.attached_sessions.filter((s) => s !== active);
      if (others.length > 0) out.push({ id: it.id, otherCount: others.length });
    }
    return out;
  });

  function shortId(id: string): string {
    return id.replace(/-/g, '').slice(0, 8);
  }

  function close(): void {
    if (committing) return;
    groupCloseDialog.close();
  }

  /** Remove the group + descendants from the layout. Returns true on success. */
  async function pruneLayout(): Promise<boolean> {
    if (sessionStore.active === null || groupId === null) return false;
    const gAll = new Set<string>([groupId, ...descendants.groupIds]);
    const itemIds = new Set<string>(descendants.items.map((it) => it.id));
    const result = await sessionStore.applyMutation(
      (cur) =>
        pruneEmptyGroups({
          ...cur,
          groups: cur.groups.filter((g) => !gAll.has(g.id)),
          items: cur.items.filter((it) => !itemIds.has(it.id)),
        }),
      { failMessage: 'Group prune failed' },
    );
    if (!result.ok) return false;
    for (const id of itemIds) sessionStore.M.delete(id);
    for (const id of gAll) sessionStore.M.delete(id);
    if (sessionStore.drillRootId !== null && gAll.has(sessionStore.drillRootId)) {
      sessionStore.clearDrill();
    }
    return true;
  }

  async function commitPanelsOnly(): Promise<void> {
    if (committing) return;
    const itemCount = descendants.items.length;
    const terminalCount = terminalDescendants.length;
    committing = true;
    try {
      const ok = await pruneLayout();
      if (!ok) return;
      toastStore.show({
        message: `Removed group + ${itemCount} item${itemCount === 1 ? '' : 's'}${
          terminalCount > 0
            ? ` (${terminalCount} terminal${terminalCount === 1 ? '' : 's'} still alive in pool)`
            : ''
        }.`,
        tone: 'success',
      });
      groupCloseDialog.close();
    } finally {
      committing = false;
    }
  }

  async function commitWithTerminals(): Promise<void> {
    if (committing) return;
    const killIds = terminalDescendants.map((it) => it.id);
    const itemCount = descendants.items.length;
    const mirroredCount = mirroredTerminals.length;
    committing = true;
    try {
      const ok = await pruneLayout();
      if (!ok) return;

      // Kill terminals after pruning the canvas layout so the visual removal is
      // immediate; terminal pool refresh follows once process cleanup settles.
      const results = await Promise.allSettled(killIds.map((id) => killTerminal(id)));
      const rejected = results.filter((r) => r.status === 'rejected');
      if (rejected.length > 0) {
        // 401 anywhere → boot user to auth.
        const unauth = rejected.find(
          (r) => r.status === 'rejected' && r.reason instanceof UnauthorizedError,
        );
        if (unauth) {
          window.location.href = '/auth';
          return;
        }
      }

      // PanelNode.performClose 와 동일 — kill 결과를 toast 전에 await 으로 반영해
      // sidebar 의 stale row 노출 회피.
      await terminalPool.refresh();
      const killed = killIds.length - rejected.length;
      const hint =
        mirroredCount > 0
          ? ` — ${mirroredCount} terminal${mirroredCount === 1 ? '' : 's'} mirrored in other session(s); those panels go dangling.`
          : '';
      toastStore.show({
        message: `Removed group, ${itemCount} item${
          itemCount === 1 ? '' : 's'
        }, killed ${killed} terminal${killed === 1 ? '' : 's'}${hint}`,
        tone: 'success',
        durationMs: 6_000,
      });
      groupCloseDialog.close();
    } finally {
      committing = false;
    }
  }

  // Pool subscription so mirrored counts stay fresh while modal is open.
  onMount(() => terminalPool.subscribe());

  $effect(() => {
    if (!open || groupId === null) {
      autoCommittedGroupId = null;
      return;
    }
    if (!settingsStore.behavior.auto_kill_terminal_on_panel_close) return;
    if (terminalDescendants.length === 0) return;
    if (autoCommittedGroupId === groupId) return;
    autoCommittedGroupId = groupId;
    void commitWithTerminals();
  });

  const groupLabel = $derived(group?.label ?? (groupId !== null ? groupId.slice(0, 8) : ''));
</script>

<Modal
  open={modalOpen}
  onclose={close}
  title="Close group"
  dismissOnBackdrop={!committing}
  dismissOnEsc={!committing}
>
  {#snippet body()}
    {#if group !== null}
      <div class="modal-stack">
        <p class="modal-lead">
          Closing <strong>{groupLabel}</strong> removes the group and its
          descendants from this canvas.
        </p>
        <ul class="counts">
          <li>
            <span class="k">descendant groups</span>
            <span class="v mono">{descendants.groupIds.length}</span>
          </li>
          <li>
            <span class="k">descendant items</span>
            <span class="v mono">{descendants.items.length}</span>
          </li>
          <li>
            <span class="k">of which terminals</span>
            <span class="v mono">{terminalDescendants.length}</span>
          </li>
        </ul>

        {#if mirroredTerminals.length > 0}
          <div class="mirror-hint" role="note">
            <strong>{mirroredTerminals.length}</strong>
            terminal{mirroredTerminals.length === 1 ? ' is' : 's are'} also
            attached to other session(s). If you kill the terminals, those
            panels go <em>dangling</em> (respawn-on-click).
            <ul class="mirror-list">
              {#each mirroredTerminals as m (m.id)}
                <li>
                  <span class="mono">{shortId(m.id)}</span>
                  · +{m.otherCount} session{m.otherCount === 1 ? '' : 's'}
                </li>
              {/each}
            </ul>
          </div>
        {/if}
      </div>
    {/if}
  {/snippet}

  {#snippet footer()}
    <Button variant="ghost" onclick={close} disabled={committing}>Cancel</Button>
    <Button
      variant="secondary"
      onclick={() => void commitPanelsOnly()}
      disabled={committing}
      title="Remove group + descendant panels. Terminals stay alive in the pool."
    >Panels only</Button>
    <Button
      variant="danger"
      onclick={() => void commitWithTerminals()}
      disabled={committing || terminalDescendants.length === 0}
      title={terminalDescendants.length === 0
        ? 'No descendant terminals to kill.'
        : `Remove panels + kill ${terminalDescendants.length} terminal(s).`}
    >Panels + Terminals</Button>
  {/snippet}
</Modal>

<style>
  .counts {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: var(--space-4);
  }

  .counts li {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    padding: var(--space-4) var(--space-8);
    background: var(--color-surface-2);
    border-radius: var(--radius-sm);
    font-size: var(--text-base);
  }

  .counts .k {
    color: var(--color-fg-muted);
  }

  .counts .v {
    color: var(--color-fg);
  }

  .mono {
    font-family: var(--font-mono);
  }

  .mirror-hint {
    padding: var(--space-8) var(--space-12);
    background: color-mix(in srgb, var(--color-warning) 12%, transparent);
    border-left: 3px solid var(--color-warning);
    border-radius: var(--radius-sm);
    font-size: var(--text-base);
    line-height: var(--leading-normal);
    color: var(--color-fg);
  }

  .mirror-list {
    margin: var(--space-6) 0 0;
    padding-left: var(--space-16);
    color: var(--color-fg-muted);
  }
</style>
