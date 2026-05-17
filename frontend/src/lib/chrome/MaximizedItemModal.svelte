<script lang="ts">
  // MaximizedItemModal — workspace 전체 (canvas + side panels + ViewportCtrl) 를
  // 덮는 modal overlay. sessionStore.maximizedItemId 가 null 이면 렌더링 X.
  //
  // 설계 정합:
  // - in-flow PanelNode / NoteNode 는 그대로 마운트 유지. modal 의 XtermHost 는
  //   dispatcher 의 multi-subscriber (ADR-0021 D1 mirror) 로 동일 paneId fan-out
  //   → 두 xterm 인스턴스 동시 활성, 모두 PANE_OUT 수신. content 손실 없음.
  // - note 는 sessionStore.items 의 동일 entry 를 양쪽 (in-flow + modal) 이 binding.
  //   InlineEdit / textarea 의 commit 이 store 를 갱신 → 양쪽 sync.
  // - schema item.x/y/w/h 무변경. modal 은 자체 viewport-fill 영역에 렌더.

  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { muxStore } from '$lib/stores/mux.svelte';
  import XtermHost from '$lib/canvas/XtermHost.svelte';
  import PanelDanglingOverlay from '$lib/canvas/PanelDanglingOverlay.svelte';
  import InlineEditField from '$lib/common/InlineEditField.svelte';
  import InlineEditTextarea from '$lib/common/InlineEditTextarea.svelte';
  import type { CanvasItem, NoteItem } from '$lib/types/canvas';

  const itemId = $derived(sessionStore.maximizedItemId);
  const item = $derived(itemId !== null ? sessionStore.items.get(itemId) ?? null : null);
  const isTerminal = $derived(item?.type === 'terminal');
  const isNote = $derived(item?.type === 'note');
  const terminalPaneId = $derived(itemId !== null ? terminalPool.paneIdFor(itemId) : undefined);

  const noteAccent = $derived(item?.type === 'note' ? item.color : null);

  const headerLabel = $derived.by(() => {
    if (item === null) return '—';
    if (item.type === 'note') return item.title.length > 0 ? item.title : 'Untitled';
    const pool = itemId !== null ? terminalPool.byId(itemId) : null;
    const poolLabel = pool?.label?.trim();
    if (poolLabel !== undefined && poolLabel.length > 0) return poolLabel;
    if (item.label !== undefined && item.label !== null && item.label.length > 0) return item.label;
    return itemId !== null ? itemId.slice(0, 8) : '—';
  });

  const isDead = $derived.by(() => {
    if (terminalPaneId === undefined) return false;
    return muxStore.panes.get(terminalPaneId)?.dead === true;
  });

  let titleEditing = $state(false);
  let bodyEditing = $state(false);

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
    if (item === null) return;
    if (e.key === 'Escape' && !titleEditing && !bodyEditing) {
      sessionStore.unmaximize();
    }
  }

  async function commitNoteField(field: 'title' | 'body', next: string): Promise<void> {
    if (item === null || item.type !== 'note') return;
    if (item[field] === next) {
      if (field === 'title') titleEditing = false;
      else bodyEditing = false;
      return;
    }
    if (sessionStore.active === null) return;
    const result = await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === item.id && it.type === 'note'
            ? ({ ...it, [field]: next } as NoteItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Note edit aborted — session reconnect failed.',
        failMessage: 'Note commit failed',
      },
    );
    if (result.ok) {
      if (field === 'title') titleEditing = false;
      else bodyEditing = false;
    }
  }
</script>

<svelte:window onkeydown={onKeyDown} />

{#if item !== null}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="max-modal-backdrop"
    role="dialog"
    aria-modal="true"
    aria-label="Maximized item"
    tabindex="-1"
    onclick={onBackdropClick}
  >
    <div
      class="max-card"
      class:is-note={isNote}
      style:--note-accent={noteAccent ?? 'var(--color-accent)'}
    >
      <header class="max-header">
        {#if isNote}
          <svg class="header-glyph note-glyph" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
            <path d="M1.6 2.5h8.8v5.4H6L3.6 10v-2.1H1.6z"/>
            <path d="M3.6 5.2h4.8"/>
          </svg>
        {:else}
          <svg class="header-glyph" viewBox="0 0 13 13" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="1" y="1.6" width="11" height="9.8" rx="1.4"/>
            <path d="M3 5l1.8 1.4L3 7.8"/>
            <path d="M6 8.4h4"/>
          </svg>
        {/if}
        {#if isNote && titleEditing}
          <span class="header-title-edit">
            <InlineEditField
              value={item.type === 'note' ? item.title : ''}
              editing={true}
              allowEmpty={true}
              placeholder="Title…"
              onCommit={(next: string) => void commitNoteField('title', next)}
              onCancel={() => (titleEditing = false)}
            />
          </span>
        {:else}
          <button
            type="button"
            class="header-title"
            ondblclick={isNote ? () => (titleEditing = true) : undefined}
            disabled={!isNote}
            title={isNote ? 'Double-click to rename' : headerLabel}
          >{headerLabel}</button>
        {/if}
        {#if isTerminal}
          <span class="max-status" aria-label="Panel status">
            <span class="led" class:dead={isDead} aria-hidden="true"></span>
            <span class="status-label">{isDead ? 'dead' : 'running'}</span>
          </span>
        {:else}
          <span class="spacer"></span>
        {/if}
        <div class="max-actions">
          <button
            type="button"
            class="max-btn"
            aria-label="Restore"
            title="Restore (Esc)"
            onclick={onRestoreClick}
          >
            <svg width="14" height="14" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
              <rect x="2" y="3.6" width="6.5" height="6.4" rx="0.5"/>
              <path d="M4 3.6V2.4h6.5v6.4H9"/>
            </svg>
          </button>
        </div>
      </header>

      <div class="max-body">
        {#if isTerminal}
          {#if terminalPaneId !== undefined}
            <XtermHost paneId={String(terminalPaneId)} />
          {:else}
            <div class="max-pending" role="status" aria-live="polite">
              <div class="pending-title">Terminal stream connecting…</div>
              <div class="pending-hint">Waiting for spawn handshake.</div>
            </div>
          {/if}
          {#if itemId !== null}
            <PanelDanglingOverlay terminalId={itemId} />
          {/if}
        {:else if isNote && item.type === 'note'}
          <div class="note-body-host">
            {#if bodyEditing}
              <InlineEditTextarea
                value={item.body}
                editing={true}
                allowEmpty={true}
                placeholder="Body…"
                onCommit={(next: string) => void commitNoteField('body', next)}
                onCancel={() => (bodyEditing = false)}
              />
            {:else}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <pre
                class="note-body-text"
                class:empty={item.body.length === 0}
                ondblclick={() => (bodyEditing = true)}
              >{item.body.length > 0 ? item.body : 'Double-click to add body'}</pre>
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
    background: color-mix(in srgb, var(--color-bg) 72%, transparent);
    backdrop-filter: blur(8px);
    z-index: var(--z-modal);
    display: flex;
    align-items: stretch;
    justify-content: stretch;
  }

  .max-card {
    flex: 1 1 auto;
    margin: var(--space-12);
    background: var(--color-surface);
    color: var(--color-fg);
    display: grid;
    grid-template-rows: 36px 1fr;
    overflow: hidden;
    font-family: var(--font-sans);
    box-shadow: 0 20px 48px rgba(0,0,0,.22), 0 0 0 1px var(--color-border);
    border-radius: var(--radius-md);
  }
  .max-card.is-note {
    border-left: 2px solid var(--note-accent);
  }

  .max-header {
    display: flex; align-items: center; gap: 10px;
    padding: 0 6px 0 12px;
    background: var(--color-surface-2);
    border-bottom: 1px solid var(--color-border);
    height: 36px;
    user-select: none;
  }

  .header-glyph {
    width: 14px; height: 14px;
    flex-shrink: 0;
    color: var(--color-fg);
    opacity: .8;
  }
  .header-glyph.note-glyph {
    color: var(--note-accent);
    opacity: 1;
  }

  .header-title {
    font-family: var(--font-mono);
    font-size: 12px;
    font-weight: var(--weight-medium);
    letter-spacing: 0.2px;
    color: var(--color-fg);
    background: transparent;
    border: 0;
    padding: 0;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    min-width: 0;
    text-align: left;
    cursor: text;
  }
  .header-title:disabled {
    cursor: default;
  }

  .header-title-edit {
    flex: 0 1 auto;
    min-width: 120px;
  }

  .max-status {
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
  .max-status .led {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--color-success);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-success) 28%, transparent);
  }
  .max-status .led.dead {
    background: var(--color-danger);
    box-shadow: none;
  }

  .spacer {
    flex: 1 1 auto;
  }

  .max-actions {
    display: flex; align-items: center; gap: 1px;
    flex-shrink: 0;
  }
  .max-btn {
    width: 24px; height: 24px;
    display: grid; place-items: center;
    border: 0;
    background: transparent;
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
  }
  .max-btn:hover {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }
  .max-btn:focus-visible {
    outline: 1px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .max-body {
    background: var(--color-bg);
    overflow: hidden;
    position: relative;
    min-height: 0;
  }

  .max-pending {
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

  .note-body-host {
    width: 100%; height: 100%;
    background: var(--color-surface);
    padding: 24px 36px;
    overflow: auto;
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
    cursor: text;
  }
  .note-body-text.empty {
    color: var(--color-fg-subtle);
    font-style: italic;
  }
</style>
