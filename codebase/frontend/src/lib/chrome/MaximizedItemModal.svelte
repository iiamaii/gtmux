<script lang="ts">
  // MaximizedItemModal — workspace 전체를 최상단에서 덮는 modal overlay.
  // sessionStore.maximizedItemId 가 null 이면 렌더링 X.
  //
  // 설계 정합:
  // - in-flow PanelNode / NoteNode / DocumentNode 는 그대로 마운트 유지. modal 의 XtermHost 는
  //   dispatcher 의 multi-subscriber (ADR-0021 D1 mirror) 로 동일 paneId fan-out
  //   → 두 xterm 인스턴스 동시 활성, 모두 PANE_OUT 수신. content 손실 없음.
  // - note 는 sessionStore.items 의 동일 entry 를 양쪽 (in-flow + modal) 이 binding.
  //   InlineEdit / textarea 의 commit 이 store 를 갱신 → 양쪽 sync.
  // - schema item.x/y/w/h 무변경. modal 은 자체 viewport-fill 영역에 렌더.

  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { muxStore } from '$lib/stores/mux.svelte';
  import { documentViewModeStore } from '$lib/stores/documentViewMode.svelte';
  import PanelDanglingOverlay from '$lib/canvas/PanelDanglingOverlay.svelte';
  import InlineEditField from '$lib/common/InlineEditField.svelte';
  import InlineEditTextarea from '$lib/common/InlineEditTextarea.svelte';
  import {
    renderMarkdown,
    renderHtml,
    isToggleableFileType,
    getNextViewMode,
    getNextViewModeLabel,
    INTERACTIVE_IFRAME_SANDBOX,
    IFRAME_HEIGHT_MESSAGE_TAG,
    buildInteractiveSrcdoc,
    type DocumentViewMode,
  } from '$lib/canvas/documentRender';
  import type { CanvasItem, NoteItem } from '$lib/types/canvas';

  const itemId = $derived(sessionStore.maximizedItemId);
  const item = $derived(itemId !== null ? sessionStore.items.get(itemId) ?? null : null);
  const isTerminal = $derived(item?.type === 'terminal');
  const isNote = $derived(item?.type === 'note');
  const isDocument = $derived(item?.type === 'document');
  const terminalPaneId = $derived(itemId !== null ? terminalPool.paneIdFor(itemId) : undefined);

  const noteAccent = $derived(item?.type === 'note' ? item.color : null);

  const headerLabel = $derived.by(() => {
    if (item === null) return '—';
    if (item.type === 'note') return item.title.length > 0 ? item.title : 'Untitled';
    if (item.type === 'document') return item.file_name.length > 0 ? item.file_name : 'document';
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
  let documentAssetText = $state<string | null>(null);
  let documentAssetLoading = $state(false);
  let documentAssetError = $state<string | null>(null);

  /** ADR-0018 D10 amend ③/④ (2026-05-21) — DocumentNode 와 동일 helper 사용
   *  으로 normal / maximize 양쪽 rendering 동기화. 옛 parseDocumentText 의
   *  paragraph slice 폐기. */
  const documentText = $derived.by(() => {
    if (item?.type !== 'document') return '';
    return item.asset_id ? (documentAssetText ?? '') : (item.content ?? '');
  });
  const documentFileTypeLabel = $derived.by(() => {
    if (item?.type !== 'document') return '';
    if (!item.asset_id) return 'markdown';
    const name = item.file_name.toLowerCase();
    const ext = name.includes('.') ? name.slice(name.lastIndexOf('.') + 1) : '';
    if (ext === 'md' || ext === 'markdown') return 'markdown';
    if (ext === 'html' || ext === 'htm') return 'html';
    const mime = (item.mime ?? '').toLowerCase();
    if (mime.startsWith('text/markdown')) return 'markdown';
    if (mime.startsWith('text/html')) return 'html';
    return ext;
  });
  /** ADR-0018 D10 amend ⑥ — viewMode persist via documentViewModeStore.
   *  DocumentNode (normal) 와 같은 itemId 구독 → normal↔maximize 전환 시
   *  reset 없음. */
  const documentViewMode = $derived.by((): DocumentViewMode => {
    if (item?.type !== 'document' || itemId === null) return 'rendered';
    return documentViewModeStore.get(itemId);
  });
  const documentCanToggleView = $derived(isToggleableFileType(documentFileTypeLabel));
  const documentNextViewModeLabel = $derived(
    getNextViewModeLabel(documentViewMode, documentFileTypeLabel),
  );
  /** ADR-0037 — fileType 가 html 이 아니면 interactive 의미 없음. reset. */
  $effect(() => {
    if (
      documentViewMode === 'interactive'
      && documentFileTypeLabel !== 'html'
      && itemId !== null
    ) {
      documentViewModeStore.set(itemId, 'rendered');
    }
  });
  const documentHtml = $derived.by(() => {
    if (documentFileTypeLabel === 'html') return renderHtml(documentText);
    return renderMarkdown(documentText);
  });

  /** ADR-0037 R4 (2단계) — iframe height auto-fit via postMessage.
   *  정본 정합 = DocumentNode 와 동일 패턴. */
  let documentIframeRef = $state<HTMLIFrameElement | null>(null);
  let documentIframeHeight = $state<number | null>(null);
  const documentInteractiveSrcdoc = $derived(buildInteractiveSrcdoc(documentText));

  $effect(() => {
    function onMessage(e: MessageEvent): void {
      if (documentIframeRef === null || e.source !== documentIframeRef.contentWindow) return;
      const data = e.data as Record<string, unknown> | null;
      if (data === null || typeof data !== 'object') return;
      const h = data[IFRAME_HEIGHT_MESSAGE_TAG];
      if (typeof h === 'number' && h > 0 && h < 50000) {
        documentIframeHeight = h;
      }
    }
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  });

  $effect(() => {
    if (documentViewMode !== 'interactive') {
      documentIframeHeight = null;
    }
  });

  const documentIframeHeightStyle = $derived(
    documentIframeHeight !== null ? `${documentIframeHeight}px` : '100%',
  );
  const canPreviewDocumentAsset = $derived.by(() => {
    if (item?.type !== 'document' || !item.asset_id) return false;
    const mime = (item.mime ?? '').toLowerCase();
    return mime.startsWith('text/') || mime === 'application/json';
  });
  /** ADR-0018 D10 amend ⑦ — PDF asset 은 browser-native PDF viewer iframe. */
  const isDocumentPdf = $derived(
    item?.type === 'document'
    && documentFileTypeLabel === 'pdf'
    && (item.asset_id ?? '').length > 0,
  );
  const documentPdfSrc = $derived(
    isDocumentPdf && item?.type === 'document' ? `/api/assets/${item.asset_id}` : '',
  );

  // svelte-flow 의 selection 변경이 item prop 의 reactive proxy 를 새 ref 로
  // 갱신할 때 effect 의 dependency 가 invalidate → fetch 재시작 blink 회피.
  // 정본 = DocumentNode 의 같은 패턴.
  const documentFetchId = $derived.by((): string => {
    if (item?.type !== 'document' || !item.asset_id || !canPreviewDocumentAsset) return '';
    return item.asset_id;
  });

  $effect(() => {
    const assetId = documentFetchId;
    if (assetId.length === 0) {
      documentAssetText = null;
      documentAssetLoading = false;
      documentAssetError = null;
      return;
    }

    let cancelled = false;
    documentAssetText = null;
    documentAssetError = null;
    documentAssetLoading = true;

    async function loadDocumentAsset(): Promise<void> {
      try {
        const res = await fetch(`/api/assets/${assetId}`, {
          method: 'GET',
          credentials: 'include',
          headers: { Accept: 'text/plain,application/json,*/*' },
        });
        if (!res.ok) throw new Error(`GET /api/assets/${assetId} returned ${res.status}`);
        const text = await res.text();
        if (!cancelled) documentAssetText = text;
      } catch (err) {
        if (!cancelled) documentAssetError = err instanceof Error ? err.message : String(err);
      } finally {
        if (!cancelled) documentAssetLoading = false;
      }
    }

    void loadDocumentAsset();
    return () => {
      cancelled = true;
    };
  });

  // ── xterm DOM portal ────────────────────────────────────────────────────
  // Maximize 시 in-flow PanelNode 의 `[data-portal-id={itemId}]` 컨테이너 안의
  // XtermHost DOM (xterm 인스턴스 의 containerEl 트리) 을 modal 의 slot 으로
  // reparent. XtermHost 컴포넌트 자체는 PanelNode 가 계속 mount 유지 → xterm
  // 인스턴스, ResizeObserver, dispatcher 등록 그대로 보존. cleanup 시 inflow
  // 로 다시 reparent. inflow 가 사라진 edge case (session switch 등) 는 child
  // 가 modal 과 함께 GC 되도록 noop.
  let portalSlotEl: HTMLDivElement | undefined = $state(undefined);

  $effect(() => {
    if (portalSlotEl === undefined) return;
    if (!isTerminal || itemId === null) return;
    // closure capture — Svelte 5 의 `bind:this` 는 element teardown 시점에
    // outer-scope `portalSlotEl` 을 undefined 로 reset. cleanup 함수가
    // 호출되는 시점 (modal markup unmount 직전) 에 outer-scope 의 값이
    // 이미 reset 되었으면 `node.parentNode === portalSlotEl` 체크가 false
    // 가 되어 inflow 로 복귀 안 됨 → 사용자 시각: terminal 화면 빈 채로
    // 남고 새로고침 해야 복구. 본 closure 변수 `slot` 은 effect run 시점의
    // reference 를 capture 하므로 reset 와 무관하게 비교 일관.
    const slot = portalSlotEl;
    const sel = `[data-portal-id="${itemId}"]`;
    const inflowHost = document.querySelector(sel) as HTMLElement | null;
    if (inflowHost === null) return;
    // inflow 의 first child (XtermHost containerEl) 만 portalSlot 으로 이동.
    // 다중 child 가능성 (예: pending placeholder) 대비해 looper.
    const moved: ChildNode[] = [];
    while (inflowHost.firstChild) {
      const child = inflowHost.firstChild;
      slot.appendChild(child);
      moved.push(child);
    }
    return () => {
      const home = document.querySelector(sel) as HTMLElement | null;
      if (home === null) return;
      for (const node of moved) {
        if (node.parentNode === slot) {
          home.appendChild(node);
        }
      }
    };
  });

  function onRestoreClick(e: MouseEvent): void {
    e.stopPropagation();
    e.preventDefault();
    sessionStore.unmaximize();
  }

  function blockBackdropEvent(e: Event): void {
    if (e.target !== e.currentTarget) return;
    e.preventDefault();
    e.stopPropagation();
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
    onpointerdown={blockBackdropEvent}
    onpointerup={blockBackdropEvent}
    onmousedown={blockBackdropEvent}
    onmouseup={blockBackdropEvent}
    onclick={blockBackdropEvent}
    ondblclick={blockBackdropEvent}
    oncontextmenu={blockBackdropEvent}
    onwheel={blockBackdropEvent}
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
        {:else if isDocument}
          <svg class="header-glyph" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
            <path d="M3 1.5h4.5L9.5 3.5V10.5H3V1.5z" />
            <path d="M7.5 1.5V3.5h2" />
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
              plain={true}
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
          {#if isDocument && documentCanToggleView}
            <!-- ADR-0037 D1/D4 — DocumentNode 와 동일한 3-mode toggle.
                 markdown: rendered↔source, html: rendered→interactive→source. -->
            <button
              type="button"
              class="max-btn"
              class:is-active={documentViewMode !== 'rendered'}
              aria-label={documentNextViewModeLabel}
              title={documentNextViewModeLabel}
              onclick={(e: MouseEvent) => {
                e.stopPropagation();
                if (itemId !== null) {
                  documentViewModeStore.set(
                    itemId,
                    getNextViewMode(documentViewMode, documentFileTypeLabel),
                  );
                }
              }}
            >
              {#if documentViewMode === 'rendered' && documentFileTypeLabel === 'html'}
                <!-- play (run interactively) -->
                <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true">
                  <polygon points="6 4 20 12 6 20"/>
                </svg>
              {:else if documentViewMode === 'source'}
                <!-- book-open (show rendered) — visibility eye 와 겹침 회피. -->
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
                  <path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z"/>
                  <path d="M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z"/>
                </svg>
              {:else}
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
                  <polyline points="16 18 22 12 16 6"/>
                  <polyline points="8 6 2 12 8 18"/>
                </svg>
              {/if}
            </button>
          {/if}
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
            <!-- xterm DOM portal target — in-flow PanelNode 의 xterm 컨테이너
                 DOM 이 maximize 동안 본 div 로 reparent (JS appendChild).
                 단일 xterm 인스턴스 가 in-flow ↔ modal 사이를 이동 — state /
                 scroll buffer / dispatcher 등록 모두 보존. -->
            <div class="xterm-portal-slot" bind:this={portalSlotEl}></div>
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
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <!--
            R6 (ADR-0018 D9 amend, batch-5 Grill #13): MaximizedItemModal 안의
            note body 도 NoteNode 와 동일하게 host wrapper 전체가 dblclick zone.
            padding / empty area 어디서든 dblclick → body editing 진입.
          -->
          <div
            class="note-body-host"
            ondblclick={() => (bodyEditing = true)}
          >
            {#if bodyEditing}
              <InlineEditTextarea
                value={item.body}
                editing={true}
                allowEmpty={true}
                plain={true}
                placeholder="Body…"
                onCommit={(next: string) => void commitNoteField('body', next)}
                onCancel={() => (bodyEditing = false)}
              />
            {:else}
              <pre
                class="note-body-text"
                class:empty={item.body.length === 0}
              >{item.body.length > 0 ? item.body : 'Double-click to add body'}</pre>
            {/if}
          </div>
        {:else if isDocument && item.type === 'document'}
          <article class="document-body-host nowheel" onwheel={(e) => e.stopPropagation()}>
            {#if isDocumentPdf}
              <!-- ADR-0018 D10 amend ⑦ — PDF iframe (browser-native viewer).
                   sandbox 미지정: PDF plugin 의 same-origin 요구. -->
              <!-- svelte-ignore a11y_missing_attribute -->
              <iframe
                class="document-pdf"
                src={documentPdfSrc}
                title={item.file_name}
                referrerpolicy="no-referrer"
                loading="lazy"
              ></iframe>
            {:else if item.asset_id && documentAssetLoading}
              <div class="document-empty">Loading preview…</div>
            {:else if item.asset_id && !canPreviewDocumentAsset}
              <div class="document-asset-summary">
                <div class="document-eyebrow">Document file</div>
                <h1>{item.file_name}</h1>
                <p>Preview is not available for this document type.</p>
              </div>
            {:else if item.asset_id && documentAssetError !== null}
              <div class="document-asset-summary">
                <div class="document-eyebrow">Document file</div>
                <h1>{item.file_name}</h1>
                <p>{documentAssetError}</p>
              </div>
            {:else if documentText.length === 0}
              <div class="document-empty">Empty document</div>
            {:else}
              <div class="document-eyebrow">{item.asset_id ? 'Document file' : 'Inline document'}</div>
              <!-- ADR-0018 D10 amend ③/④/⑤ + ADR-0037 — DocumentNode 와 동일
                   markdown/html/interactive/source rendering. -->
              {#if documentViewMode === 'source'}
                <pre class="document-source">{documentText}</pre>
              {:else if documentViewMode === 'interactive' && documentFileTypeLabel === 'html'}
                <!-- svelte-ignore a11y_missing_attribute -->
                <iframe
                  bind:this={documentIframeRef}
                  class="document-iframe"
                  sandbox={INTERACTIVE_IFRAME_SANDBOX}
                  referrerpolicy="no-referrer"
                  loading="lazy"
                  srcdoc={documentInteractiveSrcdoc}
                  style:height={documentIframeHeightStyle}
                ></iframe>
              {:else}
                <div class="document-md">{@html documentHtml}</div>
              {/if}
            {/if}
          </article>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .max-modal-backdrop {
    position: absolute;
    inset: 0;
    background: transparent;
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
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

  .max-body {
    background: var(--color-bg);
    overflow: hidden;
    position: relative;
    min-height: 0;
  }

  /* xterm DOM portal target — in-flow PanelNode 의 xterm 컨테이너 가 본 div
     안으로 이동. flex 로 width/height 100% 채움 (xterm 의 ResizeObserver 가
     fit() 자동 호출 → cell 크기 재계산). */
  .xterm-portal-slot {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--xterm-bg);
  }
  :global(.xterm-portal-slot > :first-child) {
    flex: 1 1 auto;
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

  .document-body-host {
    width: 100%;
    height: 100%;
    background: var(--color-surface);
    padding: 42px 58px;
    overflow: auto;
    overscroll-behavior: contain;
    scrollbar-width: thin;
  }

  .document-eyebrow {
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
    margin-bottom: 18px;
  }

  .document-body-host h1 {
    margin: 0 0 18px;
    font-size: 34px;
    font-weight: var(--weight-semibold);
    line-height: 1.12;
    color: var(--color-fg);
  }

  .document-body-host p {
    margin: 0 0 10px;
    max-width: 80ch;
    font-size: 14px;
    line-height: 1.6;
    color: var(--color-fg-muted);
    overflow-wrap: anywhere;
  }

  /* ADR-0018 D10 amend ③/④ — markdown rendered + source view (maximize). */
  .document-md {
    color: var(--color-fg-muted);
    font-family: var(--font-sans);
    font-size: 14px;
    line-height: 1.6;
    max-width: 80ch;
    overflow-wrap: anywhere;
  }
  .document-md :global(h1),
  .document-md :global(h2) {
    margin: 0 0 18px;
    font-size: 34px;
    font-weight: var(--weight-semibold);
    line-height: 1.12;
    color: var(--color-fg);
  }
  .document-md :global(h3) {
    margin: 20px 0 10px;
    font-size: 24px;
    font-weight: 600;
    color: var(--color-fg);
  }
  .document-md :global(h4),
  .document-md :global(h5),
  .document-md :global(h6) {
    margin: 16px 0 8px;
    font-size: 16px;
    font-weight: 600;
    color: var(--color-fg);
  }
  .document-md :global(p) { margin: 0 0 10px; }
  .document-md :global(ul),
  .document-md :global(ol) { margin: 0 0 10px; padding-left: 24px; }
  .document-md :global(li) { margin: 3px 0; }
  .document-md :global(blockquote) {
    margin: 0 0 10px; padding-left: 14px;
    border-left: 3px solid var(--color-border-strong);
    color: var(--color-fg-subtle);
  }
  .document-md :global(code) {
    font-family: var(--font-mono); font-size: 12.5px;
    padding: 1px 5px;
    background: var(--color-surface-2);
    border-radius: var(--radius-sm);
  }
  .document-md :global(pre) {
    margin: 0 0 12px; padding: 12px 14px;
    background: var(--color-surface-2);
    border-radius: var(--radius-sm);
    overflow-x: auto;
    font-family: var(--font-mono); font-size: 12.5px;
    line-height: 1.55;
  }
  .document-md :global(pre code) {
    padding: 0; background: transparent; border-radius: 0;
  }
  .document-md :global(a) { color: var(--color-accent); text-decoration: underline; }
  .document-md :global(table) {
    border-collapse: collapse; margin: 0 0 12px; font-size: 13px;
  }
  .document-md :global(th),
  .document-md :global(td) {
    border: 1px solid var(--color-border); padding: 6px 10px; text-align: left;
  }
  .document-md :global(th) {
    background: var(--color-surface-2); font-weight: 600; color: var(--color-fg);
  }
  .document-md :global(hr) {
    border: 0; border-top: 1px solid var(--color-border); margin: 18px 0;
  }
  .document-md :global(img) { max-width: 100%; height: auto; }

  .document-source {
    margin: 0; padding: 0;
    font-family: var(--font-mono);
    font-size: 13px;
    line-height: 1.6;
    color: var(--color-fg-muted);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  /* ADR-0037 D3 + R4 (2단계) — interactive mode (sandboxed iframe). normal 의
     DocumentNode 와 동일 정책: iframe 가 host 의 padding 영역 가득 + eyebrow
     숨김. R4: style:height inline 으로 content height 받고 host 가 scroll. */
  .document-iframe {
    display: block;
    flex: 0 0 auto;
    width: 100%;
    min-height: 300px;
    border: 0;
    background: #ffffff;
  }
  .document-body-host:has(.document-iframe) {
    padding: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
  }
  .document-body-host:has(.document-iframe) .document-eyebrow {
    display: none;
  }

  /* ADR-0018 D10 amend ⑦ (2026-05-22) — PDF iframe (browser-native viewer).
     interactive HTML iframe 과 달리 height auto-fit 안 함 — PDF plugin 의
     internal scroll + multi-page nav 사용. host 100% 채우고 padding 제거. */
  .document-pdf {
    display: block;
    width: 100%;
    height: 100%;
    border: 0;
    background: #ffffff;
  }
  .document-body-host:has(.document-pdf) {
    padding: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .document-asset-summary h1 {
    margin: 0 0 12px;
  }

  .document-empty {
    height: 100%;
    display: grid;
    place-items: center;
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.7px;
    text-transform: uppercase;
    color: var(--color-fg-subtle);
  }
</style>
