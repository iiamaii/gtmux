<script lang="ts">
  import { terminalActivity } from '$lib/stores/terminalActivity.svelte';
  import { activityLabel, type ActivityRow } from '$lib/stores/terminalActivity';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPoolDisplayName } from '$lib/canvas/terminalLabel';
  let { query = '' }: { query?: string } = $props();
  function name(row: ActivityRow): string {
    return terminalPoolDisplayName(sessionStore.items.get(row.id)?.label, row.id);
  }
  const rows = $derived(terminalActivity.rows.filter((row) =>
    sessionStore.items.get(row.id)?.type === 'terminal' &&
    `${name(row)} ${activityLabel(row, terminalActivity.preferences)}`.toLowerCase().includes(query.toLowerCase()),
  ));
  function locate(row: ActivityRow): void {
    sessionStore.setM([row.id]); sessionStore.zoomToIds([row.id], { mode: 'center' });
  }
</script>

<section class="activity-list" aria-label="Terminal activity">
  <p class="hint">This session. Estimates can be wrong; output silence does not mean a task finished.</p>
  {#if terminalActivity.error}
    <p class="error" role="status">{terminalActivity.error} Retrying…</p>
  {/if}
  {#if terminalActivity.loading}
    <p class="hint">Loading activity…</p>
  {:else if rows.length === 0}
    <p class="hint">{query ? 'No matching terminals.' : 'No live terminals in this session.'}</p>
  {:else}
    <button class="read-all" type="button" disabled={!!terminalActivity.error} onclick={() => rows.forEach((row) => terminalActivity.markRead(row))}>Mark listed terminals read</button>
    <ul>
      {#each rows as row (row.id)}
        {@const notice = terminalActivity.notice(row)}
        <li>
          <button class="locate" type="button" onclick={() => locate(row)} title="Locate terminal on canvas">
            <span class="name">{name(row)}</span>
            <span class="state" class:attention={notice.needs_input} class:done={notice.completed}>
              {activityLabel(row, terminalActivity.preferences)}
            </span>
            {#if terminalActivity.preferences.unread}<span class="read-state">{notice.unread ? 'Unread output' : 'Read'}</span>{/if}
          </button>
          {#if notice.unread || notice.completed || notice.needs_input}
            <button class="ack" type="button" disabled={!!terminalActivity.error} aria-label={`Mark ${name(row)} read`} onclick={() => terminalActivity.markRead(row)}>✓</button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>
<style>
  .activity-list { padding: var(--space-10); color: var(--color-fg); font-size: var(--text-md); }
  .hint { color: var(--color-fg-muted); font-size: var(--text-base); line-height: 1.5; }
  .error { color: var(--color-danger); line-height: 1.5; }
  ul { padding: 0; margin: 8px 0; list-style: none; }
  li { display: flex; align-items: center; border-bottom: 1px solid var(--color-border); }
  button { font: inherit; color: inherit; cursor: pointer; border-radius: var(--radius-sm); }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }
  button:disabled { opacity: .5; cursor: not-allowed; }
  .locate { display: grid; text-align: left; gap: 4px; flex: 1; min-width: 0; border: 0; background: transparent; padding: 10px 6px; }
  .locate:hover { background: var(--color-surface-2); }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .state, .read-state { font-size: var(--text-base); color: var(--color-fg-muted); }
  .attention { color: var(--color-danger); }
  .done { color: var(--color-success); }
  .ack, .read-all { border: 1px solid var(--color-border); background: var(--color-surface-2); padding: 5px 8px; }
</style>
