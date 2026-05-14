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

  import { ephemeralStore } from '$lib/stores/ephemeral.svelte';
  import { panelsStore } from '$lib/stores/panels.svelte';
  import { muxStore } from '$lib/stores/mux.svelte';

  interface Props {
    collapsed?: boolean;
  }

  const { collapsed = false }: Props = $props();

  // First-selected Panel — Set iteration order is insertion order, which
  // matches the user's click sequence well enough for v0.
  const selectedPanelId = $derived.by(() => {
    const it = ephemeralStore.m.values().next();
    return it.done ? null : (it.value as string);
  });

  const selectedPanel = $derived.by(() => {
    if (selectedPanelId === null) return null;
    return panelsStore.panels.get(selectedPanelId) ?? null;
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
</script>

<aside class="pane-info" class:collapsed aria-label="Pane info panel">
  <header class="pane-info-header">
    <span class="title">Pane Info</span>
  </header>
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
          <span class="k">pane_id</span>
          <span class="v mono">{strOr(selectedPanel['pane_id'], '—')}</span>
        </div>
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
</aside>

<style>
  .pane-info {
    position: absolute;
    top: var(--space-8);
    bottom: var(--space-8);
    right: var(--space-8);
    width: var(--layout-sidebar-right-w);
    background: var(--color-surface);
    color: var(--color-fg);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-md);
    z-index: var(--z-side-panel);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    transition:
      transform var(--motion-slow) var(--motion-easing),
      opacity var(--motion-normal) var(--motion-easing);
  }

  .pane-info.collapsed {
    transform: translateX(calc(var(--layout-sidebar-right-w) + var(--space-12)));
    opacity: 0;
    pointer-events: none;
  }

  .pane-info-header {
    padding: var(--space-10) var(--space-12);
    border-bottom: 1px solid var(--color-border);
    font-family: var(--font-mono);
    font-size: var(--text-base);
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--color-fg-muted);
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

  .kv-pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-6);
  }

  .kv-pair .kv {
    grid-template-columns: 24px 1fr;
  }
</style>
