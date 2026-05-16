<script lang="ts">
  /**
   * RightPanel — unified floating panel on the right edge.
   *
   * Symmetric counterpart of `LeftPanel.svelte` (ADR-0017 §D2 amend ③).
   * Currently a single tab (`Inspect` — Item Info), but the chrome
   * matches LeftPanel exactly so additional tabs (ref `Design /
   * Prototype / Inspect`) can be added without re-shaping the
   * container.
   *
   * Collapsed state:
   *   - chromeStore.state.paneInfoCollapsed === true
   *   - 268px panel collapses to a 28px vertical rail on the right
   *     edge. Rail shows an "expand" chevron (◀) plus one icon per tab.
   *     Tab-icon click expands AND switches to that tab.
   */

  import { onDestroy } from 'svelte';
  import { chromeStore, type RightPanelTab } from '$lib/stores/chrome.svelte';
  import ItemInfoView from './ItemInfoView.svelte';
  import PanelFoldButton from './PanelFoldButton.svelte';

  const collapsed = $derived(chromeStore.state.paneInfoCollapsed);
  const activeTab = $derived(chromeStore.state.rightPanelTab);
  const panelWidth = $derived(chromeStore.state.rightPanelWidth);

  let panelEl = $state<HTMLElement | null>(null);
  let resizing = $state(false);

  function selectTab(tab: RightPanelTab): void {
    chromeStore.setRightPanelTab(tab);
  }

  function expandAndSelect(tab: RightPanelTab): void {
    chromeStore.setRightPanelTab(tab); // also flips paneInfoCollapsed → false
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
    chromeStore.setRightPanelWidth(rect.right - e.clientX);
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
  <aside class="right-rail" aria-label="Right panel (collapsed)">
    <button
      type="button"
      class="rail-btn rail-expand"
      title="Expand right panel"
      aria-label="Expand right panel"
      onclick={() => chromeStore.togglePaneInfo()}
    >
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <polyline points="15 18 9 12 15 6" />
      </svg>
    </button>
    <div class="rail-sep" aria-hidden="true"></div>
    <button
      type="button"
      class="rail-btn"
      class:active={activeTab === 'inspect'}
      title="Inspect"
      aria-label="Open Inspect tab"
      onclick={() => expandAndSelect('inspect')}
    >
      <!-- info circle -->
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="9"/>
        <line x1="12" y1="11" x2="12" y2="17"/>
        <line x1="12" y1="7.5" x2="12" y2="7.6"/>
      </svg>
    </button>
  </aside>
{:else}
  <aside
    bind:this={panelEl}
    class="right-panel"
    class:resizing
    aria-label="Right panel"
    style:width={`${panelWidth}px`}
  >
    <button
      type="button"
      class="resize-handle"
      aria-label="Resize right panel"
      title="Resize right panel"
      onpointerdown={onResizePointerDown}
    ></button>
    <header class="right-panel-head">
      <div class="panel-tabs" role="tablist" aria-label="Right panel tabs">
        <button
          type="button"
          role="tab"
          class="panel-tab"
          class:active={activeTab === 'inspect'}
          aria-selected={activeTab === 'inspect'}
          onclick={() => selectTab('inspect')}
        >Inspect</button>
      </div>
      <span class="head-spacer"></span>
      <PanelFoldButton
        direction="right"
        onclick={() => chromeStore.togglePaneInfo()}
        aria-label="Collapse right panel"
      />
    </header>

    <div class="right-panel-body">
      {#if activeTab === 'inspect'}
        <ItemInfoView />
      {/if}
    </div>
  </aside>
{/if}

<style>
  /* Expanded panel — floating on the right edge, full workspace height. */
  .right-panel {
    position: absolute;
    top: var(--space-8);
    bottom: var(--space-8);
    right: var(--space-8);
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
    left: -5px;
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
    left: 4px;
    bottom: var(--space-8);
    width: 1px;
    border-radius: 999px;
    background: transparent;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .resize-handle:hover::after,
  .right-panel.resizing .resize-handle::after {
    background: var(--color-accent);
  }

  .right-panel-head {
    display: flex;
    align-items: stretch;
    gap: var(--space-6);
    padding: 0 var(--space-8) 0 var(--space-12);
    border-bottom: 1px solid var(--color-border);
    flex: 0 0 auto;
    background: var(--color-surface);
  }

  /* Match LeftPanel underline tabs (ref/frontend-design `.panel-tab`). */
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

  .right-panel-head :global(.fold-btn) {
    align-self: center;
  }

  .right-panel-body {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  /* Collapsed rail — 28px wide vertical bar on the right edge. Mirror
   * of LeftPanel's `.left-rail`. */
  .right-rail {
    position: absolute;
    top: var(--space-8);
    bottom: var(--space-8);
    right: var(--space-8);
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
