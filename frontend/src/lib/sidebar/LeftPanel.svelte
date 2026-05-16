<script lang="ts">
  /**
   * LeftPanel — unified floating panel on the left edge.
   *
   * Hosts two tabs of content (Layers / Terminals) inside a single
   * floating chrome (ref/frontend-design `panel-tabs` pattern). The
   * previously-split `Sidebar` + `TerminalsPanel` are now embedded as
   * `LayerTreeView` and `TerminalListView` — outer chrome, fold button,
   * and the collapsed rail bar are owned here.
   *
   * Spec: ADR-0017 §D2 amend (2026-05-16 tab-merge revision).
   *
   * Collapsed state:
   *   - chromeStore.state.sidebarCollapsed === true
   *   - The 248px panel is replaced by a 28px vertical rail that shows
   *     an "expand" chevron plus one icon per tab. Clicking a tab icon
   *     expands the panel AND switches to that tab (chromeStore.setLeftPanelTab).
   */

  import { onDestroy } from 'svelte';
  import { chromeStore, type LeftPanelTab } from '$lib/stores/chrome.svelte';
  import LayerTreeView from './LayerTreeView.svelte';
  import TerminalListView from './TerminalListView.svelte';
  import PanelFoldButton from '$lib/chrome/PanelFoldButton.svelte';

  const collapsed = $derived(chromeStore.state.sidebarCollapsed);
  const activeTab = $derived(chromeStore.state.leftPanelTab);
  const panelWidth = $derived(chromeStore.state.leftPanelWidth);

  let panelEl = $state<HTMLElement | null>(null);
  let resizing = $state(false);

  function selectTab(tab: LeftPanelTab): void {
    chromeStore.setLeftPanelTab(tab);
  }

  function expandAndSelect(tab: LeftPanelTab): void {
    chromeStore.setLeftPanelTab(tab); // also flips sidebarCollapsed → false
  }

  function onResizePointerDown(e: PointerEvent): void {
    if (e.button !== 0) return;
    e.preventDefault();
    resizing = true;
    window.addEventListener('pointermove', onResizePointerMove);
    window.addEventListener('pointerup', onResizePointerUp, { once: true });
    window.addEventListener('pointercancel', onResizePointerUp, { once: true });
  }

  function onResizePointerMove(e: PointerEvent): void {
    if (!resizing || panelEl === null) return;
    const rect = panelEl.getBoundingClientRect();
    chromeStore.setLeftPanelWidth(e.clientX - rect.left);
  }

  function onResizePointerUp(): void {
    resizing = false;
    window.removeEventListener('pointermove', onResizePointerMove);
    window.removeEventListener('pointerup', onResizePointerUp);
    window.removeEventListener('pointercancel', onResizePointerUp);
  }

  onDestroy(() => {
    window.removeEventListener('pointermove', onResizePointerMove);
    window.removeEventListener('pointerup', onResizePointerUp);
    window.removeEventListener('pointercancel', onResizePointerUp);
  });
</script>

{#if collapsed}
  <aside class="left-rail" aria-label="Left panel (collapsed)">
    <button
      type="button"
      class="rail-btn rail-expand"
      title="Expand left panel"
      aria-label="Expand left panel"
      onclick={() => chromeStore.toggleSidebar()}
    >
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <polyline points="9 18 15 12 9 6" />
      </svg>
    </button>
    <div class="rail-sep" aria-hidden="true"></div>
    <button
      type="button"
      class="rail-btn"
      class:active={activeTab === 'layers'}
      title="Layers"
      aria-label="Open Layers tab"
      onclick={() => expandAndSelect('layers')}
    >
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <polygon points="12 2 2 7 12 12 22 7 12 2"/>
        <polyline points="2 17 12 22 22 17"/>
        <polyline points="2 12 12 17 22 12"/>
      </svg>
    </button>
    <button
      type="button"
      class="rail-btn"
      class:active={activeTab === 'terminals'}
      title="Terminals"
      aria-label="Open Terminals tab"
      onclick={() => expandAndSelect('terminals')}
    >
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <polyline points="4 17 10 11 4 5"/>
        <line x1="12" y1="19" x2="20" y2="19"/>
      </svg>
    </button>
  </aside>
{:else}
  <aside
    bind:this={panelEl}
    class="left-panel"
    class:resizing
    aria-label="Left panel"
    style:width={`${panelWidth}px`}
  >
    <header class="left-panel-head">
      <div class="panel-tabs" role="tablist" aria-label="Left panel tabs">
        <button
          type="button"
          role="tab"
          class="panel-tab"
          class:active={activeTab === 'layers'}
          aria-selected={activeTab === 'layers'}
          onclick={() => selectTab('layers')}
        >Layers</button>
        <button
          type="button"
          role="tab"
          class="panel-tab"
          class:active={activeTab === 'terminals'}
          aria-selected={activeTab === 'terminals'}
          onclick={() => selectTab('terminals')}
        >Terminals</button>
      </div>
      <span class="head-spacer"></span>
      <PanelFoldButton
        direction="left"
        onclick={() => chromeStore.toggleSidebar()}
        aria-label="Collapse left panel"
      />
    </header>

    <div class="left-panel-body">
      {#if activeTab === 'layers'}
        <LayerTreeView />
      {:else}
        <TerminalListView />
      {/if}
    </div>
    <button
      type="button"
      class="resize-handle"
      aria-label="Resize left panel"
      title="Resize left panel"
      onpointerdown={onResizePointerDown}
    ></button>
  </aside>
{/if}

<style>
  /* Expanded panel — floating on the left edge, full workspace height. */
  .left-panel {
    position: absolute;
    top: var(--space-8);
    bottom: var(--space-8);
    left: var(--space-8);
    box-sizing: border-box;
    background: var(--color-surface);
    color: var(--color-fg);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-md);
    z-index: var(--z-side-panel);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    user-select: none;
  }

  .resize-handle {
    position: absolute;
    top: 0;
    right: -5px;
    bottom: 0;
    width: 10px;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: ew-resize;
    z-index: 2;
    touch-action: none;
  }

  .resize-handle::after {
    content: '';
    position: absolute;
    top: var(--space-8);
    right: 4px;
    bottom: var(--space-8);
    width: 1px;
    border-radius: 999px;
    background: transparent;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .resize-handle:hover::after,
  .left-panel.resizing .resize-handle::after {
    background: var(--color-accent);
  }

  .left-panel-head {
    display: flex;
    align-items: stretch;
    gap: var(--space-6);
    padding: 0 var(--space-8) 0 var(--space-12);
    border-bottom: 1px solid var(--color-border);
    flex: 0 0 auto;
    background: var(--color-surface);
  }

  /* Figma-style underline tabs (ref/frontend-design `.panel-tab`). */
  .panel-tabs {
    display: flex;
    align-items: stretch;
    flex: 1 1 auto;
    min-width: 0;
    gap: var(--space-12);
  }

  .panel-tab {
    border: 0;
    background: transparent;
    color: var(--color-fg-muted);
    padding: var(--space-8) 2px;
    font: inherit;
    font-family: var(--font-mono);
    font-size: var(--text-base);
    text-transform: uppercase;
    letter-spacing: 0.6px;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    transition:
      color var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing);
  }

  .panel-tab:hover {
    color: var(--color-fg);
  }

  .panel-tab.active {
    color: var(--color-fg);
    border-bottom-color: var(--color-fg);
  }

  .head-spacer {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
  }

  .left-panel-head :global(.fold-btn) {
    align-self: center;
  }

  .left-panel-body {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  /* Collapsed rail — 28px wide vertical bar, same vertical span. */
  .left-rail {
    position: absolute;
    top: var(--space-8);
    bottom: var(--space-8);
    left: var(--space-8);
    width: 28px;
    box-sizing: border-box;
    background: var(--color-surface);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-sm);
    z-index: var(--z-side-panel);
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--space-6) 0;
    gap: var(--space-4);
    user-select: none;
  }

  .rail-btn {
    width: 22px;
    height: 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-fg-muted);
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .rail-btn:hover {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  .rail-btn.active {
    color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
  }

  .rail-expand {
    margin-bottom: 2px;
  }

  .rail-sep {
    width: 14px;
    height: 1px;
    background: var(--color-border);
    margin: 2px 0;
  }
</style>
