<script lang="ts">
  /**
   * PaneInfoPanel — 268px floating right panel (plan 0005 Stage E,
   * ADR-0017 §D2).
   *
   * v0 read-only — shows the *first* M-selected Panel's properties:
   *   - pane_id (mono)
   *   - label (or — fallback)
   *   - position / size (x, y, w, h)
   *   - z-index
   *   - locked / visibility / minimized booleans
   *   - dead status (from muxStore)
   *
   * Editable controls (rename / lock toggle / visibility toggle) land
   * in a later phase. The empty state ("No selection") matches Figma's
   * Design tab when nothing is selected on the canvas.
   */

  import { onMount } from 'svelte';
  import { muxStore } from '$lib/stores/mux.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';

  // First-selected Panel — Set iteration order is insertion order, which
  // matches the user's click sequence well enough for v0.
  const selectedPanelId = $derived.by(() => {
    const it = sessionStore.M.values().next();
    return it.done ? null : (it.value as string);
  });

  const selectedPanel = $derived.by((): Record<string, unknown> | null => {
    if (selectedPanelId === null) return null;
    const it = sessionStore.items.get(selectedPanelId);
    if (!it) return null;
    return {
      id: it.id,
      type: it.type,
      pane_id: it.type === 'terminal' ? it.id : null,
      x: it.x,
      y: it.y,
      w: it.w,
      h: it.h,
      z: it.z,
      visibility: it.visibility === 'visible',
      locked: it.locked,
      minimized: it.minimized,
      label: it.label ?? null,
    };
  });

  const paneNumeric = $derived.by(() => {
    if (!selectedPanel) return null;
    const pid = selectedPanel['pane_id'];
    if (typeof pid !== 'string' || pid[0] !== '%') return null;
    const n = Number.parseInt(pid.slice(1), 10);
    return Number.isNaN(n) ? null : n;
  });

  const isDead = $derived.by(() => {
    if (paneNumeric === null) return false;
    return muxStore.panes.get(paneNumeric)?.dead === true;
  });

  // Pool 폴링 구독 (mount 동안 유지).
  onMount(() => terminalPool.subscribe());

  /**
   * Selected panel 의 terminal item pool lookup — selected id 가 terminal type
   * 이면 그 UUID 의 pool entry 표시.
   */
  const terminalPoolEntry = $derived.by(() => {
    if (selectedPanelId === null) return null;
    const it = sessionStore.items.get(selectedPanelId);
    if (it?.type !== 'terminal') return null;
    return terminalPool.byId(selectedPanelId);
  });

  const sessionItem = $derived.by(() => {
    if (selectedPanelId === null) return null;
    return sessionStore.items.get(selectedPanelId) ?? null;
  });

  function numOr(value: unknown, fallback: string): string {
    if (typeof value === 'number') return String(Math.round(value));
    return fallback;
  }

  function boolStr(value: unknown): string {
    if (typeof value !== 'boolean') return '—';
    return value ? 'true' : 'false';
  }

  function strOr(value: unknown, fallback: string): string {
    if (typeof value === 'string' && value.length > 0) return value;
    return fallback;
  }

  const isSelectedTerminal = $derived(sessionItem?.type === 'terminal');
</script>

<div class="item-info-view" aria-label="Item info">
  <div class="pane-info-body">
    {#if selectedPanel === null}
      <div class="empty">
        <p>No selection</p>
        <p class="hint">Click a panel on the canvas to inspect.</p>
      </div>
    {:else}
      <section class="section">
        <h4 class="section-head">Identity</h4>
        <div class="kv">
          <span class="k">type</span>
          <span class="v mono">{strOr(selectedPanel['type'], 'panel')}</span>
        </div>
        {#if isSelectedTerminal}
          <div class="kv">
            <span class="k">terminal</span>
            <span class="v mono">{strOr(selectedPanel['pane_id'], '—')}</span>
          </div>
        {/if}
        <div class="kv">
          <span class="k">label</span>
          <span class="v">{strOr(selectedPanel['label'], '—')}</span>
        </div>
        <div class="kv">
          <span class="k">id</span>
          <span class="v mono">{selectedPanel.id}</span>
        </div>
      </section>

      <section class="section">
        <h4 class="section-head">Geometry</h4>
        <div class="kv-pair">
          <div class="kv">
            <span class="k">x</span>
            <span class="v mono">{numOr(selectedPanel['x'], '0')}</span>
          </div>
          <div class="kv">
            <span class="k">y</span>
            <span class="v mono">{numOr(selectedPanel['y'], '0')}</span>
          </div>
        </div>
        <div class="kv-pair">
          <div class="kv">
            <span class="k">w</span>
            <span class="v mono">{numOr(selectedPanel['w'], '—')}</span>
          </div>
          <div class="kv">
            <span class="k">h</span>
            <span class="v mono">{numOr(selectedPanel['h'], '—')}</span>
          </div>
        </div>
        <div class="kv">
          <span class="k">z</span>
          <span class="v mono">{numOr(selectedPanel['z'], '0')}</span>
        </div>
      </section>

      {#if isSelectedTerminal && (terminalPoolEntry !== null || sessionItem !== null)}
        <section class="section">
          <h4 class="section-head">Terminal · Pool</h4>
          {#if terminalPoolEntry !== null}
            <div class="kv">
              <span class="k">attach</span>
              <span class="v mono">×{terminalPoolEntry.attach_count}</span>
            </div>
            {#if terminalPoolEntry.attached_sessions.length > 0}
              <div class="kv">
                <span class="k">sessions</span>
                <span class="v">
                  {#each terminalPoolEntry.attached_sessions as s, i (s)}
                    {#if i > 0}, {/if}<span
                      class="session-chip"
                      class:current={sessionStore.active?.name === s}
                    >{s}</span>
                  {/each}
                </span>
              </div>
            {/if}
            <div class="kv">
              <span class="k">alive</span>
              <span class="v mono" class:dead={!terminalPoolEntry.alive}>
                {terminalPoolEntry.alive ? 'live' : 'dangling'}
              </span>
            </div>
          {:else if sessionItem !== null && sessionItem.type === 'terminal'}
            <div class="kv">
              <span class="k">pool</span>
              <span class="v">
                <span class="warn">missing</span> — terminal not in server pool
              </span>
            </div>
          {/if}
        </section>
      {/if}

      {#if sessionItem !== null && (sessionItem.type === 'rect' || sessionItem.type === 'ellipse' || sessionItem.type === 'line' || sessionItem.type === 'text' || sessionItem.type === 'note' || sessionItem.type === 'file_path')}
        <section class="section">
          <h4 class="section-head">Item Payload</h4>
          {#if sessionItem.type === 'rect' || sessionItem.type === 'ellipse'}
            <div class="kv">
              <span class="k">stroke</span>
              <span class="v mono">{sessionItem.stroke}</span>
            </div>
            <div class="kv">
              <span class="k">fill</span>
              <span class="v mono">{sessionItem.fill}</span>
            </div>
          {:else if sessionItem.type === 'line'}
            <div class="kv-pair">
              <div class="kv">
                <span class="k">x2</span>
                <span class="v mono">{Math.round(sessionItem.x2)}</span>
              </div>
              <div class="kv">
                <span class="k">y2</span>
                <span class="v mono">{Math.round(sessionItem.y2)}</span>
              </div>
            </div>
            <div class="kv">
              <span class="k">stroke</span>
              <span class="v mono">{sessionItem.stroke}</span>
            </div>
          {:else if sessionItem.type === 'text'}
            <div class="kv">
              <span class="k">chars</span>
              <span class="v mono">{sessionItem.text.length}</span>
            </div>
            <div class="kv">
              <span class="k">align</span>
              <span class="v mono">{sessionItem.text_align ?? 'center'}</span>
            </div>
            <div class="kv">
              <span class="k">v-align</span>
              <span class="v mono">{sessionItem.text_vertical_align ?? 'middle'}</span>
            </div>
          {:else if sessionItem.type === 'note'}
            <div class="kv">
              <span class="k">title</span>
              <span class="v">{strOr(sessionItem.title, 'Untitled')}</span>
            </div>
          {:else if sessionItem.type === 'file_path'}
            <div class="kv">
              <span class="k">path</span>
              <span class="v mono">{strOr(sessionItem.path, '—')}</span>
            </div>
          {/if}
        </section>
      {/if}

      <section class="section">
        <h4 class="section-head">State</h4>
        <div class="kv">
          <span class="k">visible</span>
          <span class="v mono">{boolStr(selectedPanel['visibility'] ?? true)}</span>
        </div>
        <div class="kv">
          <span class="k">locked</span>
          <span class="v mono">{boolStr(selectedPanel['locked'])}</span>
        </div>
        <div class="kv">
          <span class="k">minimized</span>
          <span class="v mono">{boolStr(selectedPanel['minimized'])}</span>
        </div>
        <div class="kv">
          <span class="k">alive</span>
          <span class="v mono" class:dead={isDead}>
            {isDead ? 'dead' : 'live'}
          </span>
        </div>
      </section>
    {/if}
  </div>
</div>

<style>
  /* Embedded view — host (RightPanel) owns outer chrome + tabs + fold. */
  .item-info-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .pane-info-body {
    flex: 1 1 auto;
    overflow-y: auto;
    padding: var(--space-6) 0;
  }

  .empty {
    padding: var(--space-12);
    color: var(--color-fg-muted);
  }

  .empty p {
    margin: 0 0 var(--space-4);
    font-size: var(--text-md);
  }

  .empty .hint {
    font-size: var(--text-base);
    color: var(--color-fg-subtle);
  }

  .section {
    padding: var(--space-8) var(--space-12) var(--space-12);
    border-bottom: 1px solid var(--color-border);
  }

  .section:last-child {
    border-bottom: 0;
  }

  .section-head {
    margin: 0 0 var(--space-8);
    font-family: var(--font-mono);
    font-size: var(--text-base);
    text-transform: uppercase;
    letter-spacing: 0.6px;
    font-weight: var(--weight-regular);
    color: var(--color-fg-muted);
  }

  .kv {
    display: grid;
    grid-template-columns: 64px 1fr;
    align-items: center;
    gap: var(--space-6);
    padding: var(--space-4) 0;
    font-size: var(--text-md);
  }

  .kv .k {
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-base);
  }

  .kv .v {
    color: var(--color-fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .kv .v.mono {
    font-family: var(--font-mono);
    font-size: var(--text-base);
  }

  .kv .v.dead {
    color: var(--color-warning);
  }

  .session-chip {
    display: inline-block;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    padding: 1px 6px;
    margin-right: 2px;
    border-radius: var(--radius-pill);
    background: var(--color-surface-2);
    color: var(--color-fg-muted);
    border: 1px solid var(--color-border);
  }

  .session-chip.current {
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    color: var(--color-accent);
    border-color: color-mix(in srgb, var(--color-accent) 30%, transparent);
  }

  .warn {
    color: var(--color-warning);
    font-weight: var(--weight-medium);
  }

  .kv-pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-6);
  }

  .kv-pair .kv {
    grid-template-columns: 24px 1fr;
  }
</style>
