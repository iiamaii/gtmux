<script lang="ts">
  // MaximizedPanelModal — Panel maximize 시 canvas 영역을 덮는 modal overlay.
  //
  // PanelNode (SvelteFlow 내부) 의 schema geom 을 변경하지 않고, 본 modal 이
  // canvas-root 의 sibling 으로 absolute 렌더링 → viewport pan/zoom 과 무관.
  // sessionStore.maximizedItemId === data.id 인 동안 PanelNode 가 XtermHost 를
  // 마운트하지 않음 (isStreaming guard 에 maximize 추가) → modal 안의 XtermHost
  // 만 활성. ring buffer replay 로 content catch-up.

  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { muxStore } from '$lib/stores/mux.svelte';
  import XtermHost from './XtermHost.svelte';
  import PanelDanglingOverlay from './PanelDanglingOverlay.svelte';

  let { itemId }: { itemId: string } = $props();

  const item = $derived(sessionStore.items.get(itemId));
  const terminalPaneId = $derived(terminalPool.paneIdFor(itemId));

  const isNote = $derived(item?.type === 'note');
  const noteColor = $derived(item?.type === 'note' ? item.color : null);

  const headerLabel = $derived.by(() => {
    const it = sessionStore.items.get(itemId);
    if (it === undefined) return '—';
    // Note 는 title 우선 (Inspector 의 label 매핑과 정합).
    if (it.type === 'note' && it.title.length > 0) return it.title;
    const poolLabel = terminalPool.byId(itemId)?.label?.trim();
    if (poolLabel !== undefined && poolLabel.length > 0) return poolLabel;
    if (it.label !== undefined && it.label !== null && it.label.length > 0) return it.label;
    if (it.type === 'note') return 'Untitled';
    return itemId.slice(0, 8);
  });

  // terminalPool.paneIdFor 는 number | undefined — pane numeric 그대로.
  const isDead = $derived.by(() => {
    if (terminalPaneId === undefined) return false;
    return muxStore.panes.get(terminalPaneId)?.dead === true;
  });

  function onRestoreClick(e: MouseEvent): void {
    e.stopPropagation();
    e.preventDefault();
    sessionStore.unmaximize();
  }

  function onBackdropClick(e: MouseEvent): void {
    if (e.target !== e.currentTarget) return;
    sessionStore.unmaximize();
  }

  function onKeyDown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      sessionStore.unmaximize();
    }
  }
</script>

<svelte:window onkeydown={onKeyDown} />

{#if item !== undefined}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="max-modal-backdrop"
    role="dialog"
    aria-modal="true"
    aria-label="Maximized panel"
    tabindex="-1"
    onclick={onBackdropClick}
  >
    <div class="max-panel" class:is-note={isNote} style:--note-accent={noteColor ?? 'var(--color-accent)'}>
      <header class="panel-header">
        {#if isNote}
          <svg class="panel-glyph note-glyph" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
            <path d="M1.6 2.5h8.8v5.4H6L3.6 10v-2.1H1.6z"/>
            <path d="M3.6 5.2h4.8"/>
          </svg>
        {:else}
          <svg class="panel-glyph" viewBox="0 0 13 13" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="1" y="1.6" width="11" height="9.8" rx="1.4"/>
            <path d="M3 5l1.8 1.4L3 7.8"/>
            <path d="M6 8.4h4"/>
          </svg>
        {/if}
        <span class="panel-title">{headerLabel}</span>
        {#if !isNote}
          <span class="panel-status" aria-label="Panel status">
            <span class="led" class:dead={isDead} aria-hidden="true"></span>
            <span class="status-label">{isDead ? 'dead' : 'running'}</span>
          </span>
        {:else}
          <span class="panel-spacer"></span>
        {/if}
        <div class="panel-actions">
          <button
            type="button"
            class="panel-btn"
            aria-label="Restore"
            title="Restore (Esc)"
            onclick={onRestoreClick}
          >
            <!-- restore (two windows) -->
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
              <rect x="2" y="3.6" width="6.5" height="6.4" rx="0.5"/>
              <path d="M4 3.6V2.4h6.5v6.4H9"/>
            </svg>
          </button>
        </div>
      </header>
      <div class="panel-body">
        {#if item.type === 'terminal'}
          {#if terminalPaneId !== undefined}
            <XtermHost paneId={String(terminalPaneId)} />
          {:else}
            <div class="panel-pending" role="status" aria-live="polite">
              <div class="pending-title">Terminal stream connecting…</div>
              <div class="pending-hint">Waiting for spawn handshake.</div>
            </div>
          {/if}
          <PanelDanglingOverlay terminalId={itemId} />
        {:else if item.type === 'note'}
          <div class="note-body-max">
            {#if item.body.length === 0}
              <span class="note-placeholder">Empty note</span>
            {:else}
              <pre class="note-body-text">{item.body}</pre>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .max-modal-backdrop {
    position: absolute;
    inset: 0;
    background: color-mix(in srgb, var(--color-bg) 70%, transparent);
    backdrop-filter: blur(6px);
    z-index: 1500;
    display: flex;
    align-items: stretch;
    justify-content: stretch;
  }

  .max-panel {
    flex: 1 1 auto;
    background: var(--color-surface);
    color: var(--color-fg);
    display: grid;
    grid-template-rows: 32px 1fr;
    overflow: hidden;
    font-family: var(--font-sans);
    box-shadow: 0 16px 48px rgba(0,0,0,.18), 0 0 0 1px var(--color-border);
    margin: var(--space-12);
    border-radius: var(--radius-md);
  }

  .panel-header {
    display: flex; align-items: center; gap: 10px;
    padding: 0 6px 0 12px;
    background: var(--color-surface-2);
    border-bottom: 1px solid var(--color-border);
    height: 32px;
    user-select: none;
  }

  .panel-glyph {
    width: 13px; height: 13px;
    flex-shrink: 0;
    color: var(--color-fg);
    opacity: .75;
  }

  .panel-title {
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: var(--weight-medium);
    letter-spacing: 0.2px;
    color: var(--color-fg);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    min-width: 0;
  }

  .panel-status {
    display: flex; align-items: center; gap: 6px;
    margin-left: auto;
    margin-right: 4px;
    font-family: var(--font-mono);
    font-size: 9px;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
    flex-shrink: 0;
  }
  .panel-status .led {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--color-success);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-success) 28%, transparent);
  }
  .panel-status .led.dead {
    background: var(--color-danger);
    box-shadow: none;
  }

  .panel-actions {
    display: flex; align-items: center; gap: 1px;
    flex-shrink: 0;
  }

  .panel-btn {
    width: 22px; height: 22px;
    display: grid; place-items: center;
    border: none;
    background: transparent;
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
  }
  .panel-btn:hover {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }
  .panel-btn:focus-visible {
    outline: 1px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .panel-body {
    background: var(--color-bg);
    overflow: hidden;
    position: relative;
    min-height: 0;
  }

  /* Note 변형 — left accent rail + sans body. */
  .max-panel.is-note {
    border-left: 2px solid var(--note-accent);
  }
  .panel-spacer {
    flex: 1 1 auto;
  }
  .note-glyph {
    color: var(--note-accent) !important;
    opacity: 1 !important;
  }
  .note-body-max {
    width: 100%; height: 100%;
    overflow: auto;
    padding: 24px 36px;
    background: var(--color-surface);
  }
  .note-body-text {
    margin: 0;
    font-family: var(--font-sans);
    font-size: var(--text-lg);
    line-height: 1.55;
    letter-spacing: -0.1px;
    color: var(--color-fg);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .note-placeholder {
    color: var(--color-fg-subtle);
    font-size: var(--text-md);
    font-style: italic;
    user-select: none;
  }

  .panel-pending {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    text-align: center;
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.2px;
  }
  .pending-title { color: var(--color-fg); }
  .pending-hint { color: var(--color-fg-subtle); font-size: 10px; margin-top: 4px; }
</style>
