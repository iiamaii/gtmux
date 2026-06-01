<script lang="ts">
  /**
   * RightPanel — unified floating panel on the right edge.
   *
   * Symmetric counterpart of `LeftPanel.svelte` (ADR-0017 §D2 amend ③).
   * Hosts Inspect and Preview tabs while matching LeftPanel chrome.
   *
   * Collapsed state:
   *   - chromeStore.state.paneInfoCollapsed === true
   *   - 268px panel collapses to a 28px vertical rail on the right
   *     edge. Rail shows an "expand" chevron (◀) plus one icon per tab.
   *     Tab-icon click expands AND switches to that tab.
   */

  import { onDestroy } from 'svelte';
  import { chromeStore, type RightPanelTab } from '$lib/stores/chrome.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import ItemInfoView from './ItemInfoView.svelte';
  import FilePreviewView from './FilePreviewView.svelte';
  import PanelFoldButton from './PanelFoldButton.svelte';

  const collapsed = $derived(chromeStore.state.paneInfoCollapsed);
  const activeTab = $derived(chromeStore.state.rightPanelTab);
  const leftTab = $derived(chromeStore.state.leftPanelTab);
  const panelWidth = $derived(chromeStore.state.rightPanelWidth);
  // No active session 시 inspect tab + body 는 의미 없음. fold/expand 만 유지.
  const noActiveSession = $derived(sessionStore.active === null);
  const inspectEnabled = $derived(!noActiveSession && leftTab !== 'files');
  const previewEnabled = $derived(!noActiveSession && leftTab === 'files');

  let panelEl = $state<HTMLElement | null>(null);
  let resizing = $state(false);

  function selectTab(tab: RightPanelTab): void {
    if (!rightTabEnabled(tab)) return;
    chromeStore.setRightPanelTab(tab);
  }

  function expandAndSelect(tab: RightPanelTab): void {
    if (!rightTabEnabled(tab)) return;
    chromeStore.setRightPanelTab(tab); // also flips paneInfoCollapsed → false
  }

  function rightTabEnabled(tab: RightPanelTab): boolean {
    return tab === 'preview' ? previewEnabled : inspectEnabled;
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
      title={noActiveSession
        ? 'Connect a session to inspect items'
        : inspectEnabled ? 'Inspect' : 'Inspect is available from Layers or Terminals'}
      aria-label="Open Inspect tab"
      disabled={!inspectEnabled}
      onclick={() => expandAndSelect('inspect')}
    >
      <!-- info circle -->
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="9"/>
        <line x1="12" y1="11" x2="12" y2="17"/>
        <line x1="12" y1="7.5" x2="12" y2="7.6"/>
      </svg>
    </button>
    <button
      type="button"
      class="rail-btn"
      class:active={activeTab === 'preview'}
      title={noActiveSession
        ? 'Connect a session to preview files'
        : previewEnabled ? 'Preview' : 'Preview is available from Files'}
      aria-label="Open Preview tab"
      disabled={!previewEnabled}
      onclick={() => expandAndSelect('preview')}
    >
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M4 5.5A1.5 1.5 0 0 1 5.5 4h8L20 10.5v8A1.5 1.5 0 0 1 18.5 20h-13A1.5 1.5 0 0 1 4 18.5v-13z"/>
        <path d="M13 4v7h7"/>
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
          disabled={!inspectEnabled}
          title={noActiveSession
            ? 'Connect a session to inspect items'
            : inspectEnabled ? '' : 'Inspect is available from Layers or Terminals'}
          onclick={() => selectTab('inspect')}
        >Inspect</button>
        <button
          type="button"
          role="tab"
          class="panel-tab"
          class:active={activeTab === 'preview'}
          aria-selected={activeTab === 'preview'}
          disabled={!previewEnabled}
          title={noActiveSession
            ? 'Connect a session to preview files'
            : previewEnabled ? '' : 'Preview is available from Files'}
          onclick={() => selectTab('preview')}
        >Preview</button>
      </div>
      <span class="head-spacer"></span>
      <PanelFoldButton
        direction="right"
        onclick={() => chromeStore.togglePaneInfo()}
        aria-label="Collapse right panel"
      />
    </header>

    <div class="right-panel-body" class:no-session={noActiveSession} inert={noActiveSession}>
      {#if activeTab === 'inspect'}
        <ItemInfoView />
      {:else}
        <FilePreviewView />
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

  .panel-tab:hover:not(:disabled) {
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

  /* No active session — body 는 visible 하되 inert + dimmed (LeftPanel
   * 과 동일 패턴). */
  .right-panel-body.no-session {
    opacity: 0.4;
    pointer-events: none;
  }

  .panel-tab:disabled,
  .rail-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
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

  .rail-btn:hover:not(:disabled) {
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
