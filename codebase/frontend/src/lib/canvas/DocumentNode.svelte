<script lang="ts">
  /**
   * DocumentNode — SvelteFlow custom node for `type: "document"`.
   *
   * 정본 시안: `ref/frontend-design/components-v5 §02` — `.shape-document`.
   *
   * 정본 spec:
   * - ADR-0018 D4 amend ② (2026-05-17) — 두 mode: (a) asset-based / (b)
   *   inline-stored. BE schema.rs 가 Item::Document 에 `asset_id: Option`
   *   + `content: Option` ship (DOCUMENT_INLINE_MAX_BYTES 64 KB).
   * - plan-0011 FE Slice-A2 — 본 컴포넌트가 inline-stored mode 의 v3 시안
   *   정합 placeholder. 추후 InlineEdit wire 는 별 후속.
   *
   * 시안 구조 (grid 30px / 1fr / 26px):
   *   1. doc-head — file svg + filename + sep + size + right (Edited Nm ago)
   *   2. doc-body — eyebrow + h2 + p (markdown content 의 첫 줄 = h2)
   *   3. doc-foot — page-dot indicator + count + right (md · UTF-8 · LF)
   *
   * 현 단계: read-only display. content 의 첫 줄 (# 로 시작하면 strip) 을
   * h2 로, 나머지를 p 로 분할 렌더. 인라인 편집 진입 (더블 클릭) 은 후속.
   */

  import { NodeResizer } from '@xyflow/svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { documentViewModeStore } from '$lib/stores/documentViewMode.svelte';
  import InlineEditField from '$lib/common/InlineEditField.svelte';
  import InlineEditTextarea from '$lib/common/InlineEditTextarea.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { uploadAsset, AssetUploadUnavailableError } from '$lib/http/assets';
  import { pickLocalFile } from '$lib/files/localFilePicker';
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
  } from './documentRender';
  import type { CanvasItem, DocumentItem } from '$lib/types/canvas';

  /** Inline content cap = ADR-0018 D4 amend ② / BE DOCUMENT_INLINE_MAX_BYTES. */
  const DOCUMENT_INLINE_MAX_BYTES = 64 * 1024;

  interface DocumentNodeData {
    id: string;
    w: number;
    h: number;
    visibility: boolean;
    locked: boolean;
    minimized?: boolean;
    label?: string;
    asset_id?: string;
    file_name: string;
    content?: string;
    mime?: string;
    size_bytes?: number;
  }

  let {
    data,
    selected = false,
  }: {
    data: DocumentNodeData;
    selected?: boolean;
    id?: string;
    type?: string;
    width?: number;
    height?: number;
    dragHandle?: string;
    sourcePosition?: unknown;
    targetPosition?: unknown;
    dragging?: boolean;
    zIndex?: number;
    selectable?: boolean;
    deletable?: boolean;
    draggable?: boolean;
    parentId?: string;
  } = $props();

  const isVisible = $derived(data.visibility !== false);
  const isLocked = $derived(data.locked === true);
  const isInM = $derived(selected || sessionStore.M.has(data.id));
  const isMultiM = $derived(isInM && sessionStore.M.size > 1);
  const isSingleM = $derived(isInM && sessionStore.M.size <= 1);
  const isMaximized = $derived(sessionStore.maximizedItemId === data.id);
  /** Asset-based vs inline-stored 모드 구분 (ADR-0018 D4 amend ②). */
  const isInline = $derived((data.asset_id ?? '').length === 0);
  let assetPreviewText = $state<string | null>(null);
  let assetPreviewLoading = $state(false);
  let assetPreviewError = $state<string | null>(null);

  /** size_bytes 의 사람용 표기 (KB). */
  const sizeLabel = $derived.by((): string => {
    const bytes = data.size_bytes ?? 0;
    if (bytes < 1024) return `${bytes} B`;
    return `${(bytes / 1024).toFixed(1)} KB`;
  });
  const fileTypeLabel = $derived.by((): string => {
    if (isInline) return 'markdown';
    const name = data.file_name.toLowerCase();
    const ext = name.includes('.') ? name.slice(name.lastIndexOf('.') + 1) : '';
    switch (ext) {
      case 'md':
      case 'markdown':
        return 'markdown';
      case 'html':
      case 'htm':
        return 'html';
      case 'txt':
      case 'text':
      case 'log':
        return 'text';
      case 'json':
        return 'json';
      case 'pdf':
        return 'pdf';
      case 'css':
        return 'css';
      case 'js':
      case 'jsx':
        return 'javascript';
      case 'ts':
      case 'tsx':
        return 'typescript';
      default: {
        const mime = (data.mime ?? '').toLowerCase();
        if (mime === 'application/json') return 'json';
        if (mime === 'application/pdf') return 'pdf';
        if (mime.startsWith('text/html')) return 'html';
        if (mime.startsWith('text/markdown')) return 'markdown';
        if (mime.startsWith('text/')) return 'text';
        return 'document';
      }
    }
  });

  /** ADR-0018 D10 amend ③/④/⑤/⑥ + ADR-0037 — markdown/html viewer + 3-mode
   *  toggle. helper + viewMode store 가 외부화 — DocumentNode (normal) 와
   *  MaximizedItemModal (maximize) 가 *같은 store* 의 같은 itemId 를 구독해
   *  rendering + 전이 + persist 동기화. normal↔maximize 전환 + unmount/remount
   *  도 reset 없음. */
  const viewMode = $derived(documentViewModeStore.get(data.id));
  const canToggleView = $derived(isToggleableFileType(fileTypeLabel));
  const nextViewModeLabel = $derived(getNextViewModeLabel(viewMode, fileTypeLabel));
  /** 'interactive' 는 fileType=html 일 때만 의미. fileType 가 바뀌면 reset. */
  $effect(() => {
    if (viewMode === 'interactive' && fileTypeLabel !== 'html') {
      documentViewModeStore.set(data.id, 'rendered');
    }
  });

  /** ADR-0037 R4 (2단계) — iframe content height auto-fit via postMessage.
   *  iframe 안 inline probe script 가 ResizeObserver + load 시점에 parent
   *  로 height 보내고, parent (본 컴포넌트) 가 받아서 iframe style.height
   *  에 반영. cap = panel 의 max-height (CSS 의 max-height:100%). */
  let iframeRef = $state<HTMLIFrameElement | null>(null);
  let iframeContentHeight = $state<number | null>(null);
  const interactiveSrcdoc = $derived(buildInteractiveSrcdoc(data.content ?? ''));
  const interactiveSrcdocAsset = $derived(buildInteractiveSrcdoc(assetPreviewText ?? ''));

  $effect(() => {
    function onMessage(e: MessageEvent): void {
      // sandbox 의 unique opaque origin 이라 origin 검사 의미 X. source 검증
      // 으로 *우리* iframe 의 postMessage 만 accept — 다른 page 의 message
      // (예: 외부 widget) 차단.
      if (iframeRef === null || e.source !== iframeRef.contentWindow) return;
      const data = e.data as Record<string, unknown> | null;
      if (data === null || typeof data !== 'object') return;
      const h = data[IFRAME_HEIGHT_MESSAGE_TAG];
      if (typeof h === 'number' && h > 0 && h < 50000) {
        iframeContentHeight = h;
      }
    }
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  });

  /** viewMode 가 interactive 가 아닐 때 height state reset. */
  $effect(() => {
    if (viewMode !== 'interactive') {
      iframeContentHeight = null;
    }
  });

  const iframeHeightStyle = $derived(
    iframeContentHeight !== null ? `${iframeContentHeight}px` : '100%',
  );
  // Inline content (data.content) 의 mime 추정은 markdown 으로 기본 — fileTypeLabel
  // 이 'markdown' 이면 marked parse, 'html' 이면 sanitize only.
  function renderContent(raw: string): string {
    if (fileTypeLabel === 'html') return renderHtml(raw);
    return renderMarkdown(raw);
  }
  const inlineHtml = $derived(renderContent(data.content ?? ''));
  const assetHtml = $derived(renderContent(assetPreviewText ?? ''));
  const isEmpty = $derived(isInline && (data.content ?? '').trim().length === 0);
  const fileStem = $derived.by((): string => {
    const base = data.file_name.trim().split('/').pop() ?? data.file_name.trim();
    const dot = base.lastIndexOf('.');
    if (dot <= 0) return base;
    return base.slice(0, dot);
  });
  const documentTitle = $derived((data.label ?? '').trim() || fileStem || 'Untitled');
  const canPreviewAssetText = $derived.by(() => {
    if (isInline) return false;
    const mime = (data.mime ?? '').toLowerCase();
    return mime.startsWith('text/') || mime === 'application/json';
  });

  // svelte-flow 가 selection 변경 시 data prop 의 reactive proxy 를 새 ref 로
  // 갱신 → effect 의 dependency 가 invalidate → fetch 재시작 → "Loading
  // preview…" blink. 회피: id 를 stable derived 로 wrap — *값* 이 같으면
  // svelte 의 derived 가 subscriber notify skip → effect re-fire 안 함.
  const assetFetchId = $derived.by((): string => {
    if (isInline || !canPreviewAssetText) return '';
    return data.asset_id ?? '';
  });
  const assetFetchAccept = $derived(data.mime ?? 'text/plain');

  $effect(() => {
    const id = assetFetchId;
    if (id.length === 0) {
      assetPreviewText = null;
      assetPreviewLoading = false;
      assetPreviewError = null;
      return;
    }

    let cancelled = false;
    assetPreviewText = null;
    assetPreviewError = null;
    assetPreviewLoading = true;

    async function loadPreview(): Promise<void> {
      try {
        const res = await fetch(`/api/assets/${id}`, {
          method: 'GET',
          credentials: 'include',
          headers: { Accept: assetFetchAccept },
        });
        if (!res.ok) throw new Error(`GET /api/assets/${id} returned ${res.status}`);
        const text = await res.text();
        if (!cancelled) assetPreviewText = text;
      } catch (err) {
        if (!cancelled) assetPreviewError = err instanceof Error ? err.message : String(err);
      } finally {
        if (!cancelled) assetPreviewLoading = false;
      }
    }

    void loadPreview();
    return () => {
      cancelled = true;
    };
  });

  // ─ Inline edit (ADR-0018 D4 amend ② / plan-0011 FE Slice-A2) ─
  //   - filename: double-click on .doc-head .filename → InlineEditField.
  //   - content: double-click on .doc-body → InlineEditTextarea. cap 64 KB
  //     (BE DOCUMENT_INLINE_MAX_BYTES 정합).
  //   - commit 시 applyMutation 통과 → history capture (Cmd+Z 자연 정합).
  //   - inline-stored 모드에서만 편집 진입 — asset-based 는 read-only.
  let nameEditing = $state(false);
  let labelEditing = $state(false);
  let contentEditing = $state(false);

  function onLabelDblClick(e: MouseEvent): void {
    if (isLocked) return;
    e.stopPropagation();
    labelEditing = true;
  }

  function onNameDblClick(e: MouseEvent): void {
    if (isLocked || !isInline) return;
    e.stopPropagation();
    nameEditing = true;
  }

  function onBodyDblClick(e: MouseEvent): void {
    if (isLocked || !isInline) return;
    e.stopPropagation();
    contentEditing = true;
  }

  async function commitName(next: string): Promise<void> {
    const trimmed = next.trim();
    if (trimmed === data.file_name || trimmed.length === 0) {
      nameEditing = false;
      return;
    }
    if (sessionStore.active === null) return;
    const oldStem = fileStem;
    const nextBase = trimmed.split('/').pop() ?? trimmed;
    const nextDot = nextBase.lastIndexOf('.');
    const nextStem = nextDot > 0 ? nextBase.slice(0, nextDot) : nextBase;
    const result = await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'document'
            ? ({
                ...it,
                file_name: trimmed,
                label:
                  (it.label ?? '').trim().length === 0 || (it.label ?? '') === oldStem
                    ? nextStem
                    : it.label,
              } as DocumentItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Document filename edit aborted — session reconnect failed.',
        failMessage: 'Document rename failed',
      },
    );
    if (result.ok) nameEditing = false;
  }

  async function commitLabel(next: string): Promise<void> {
    const trimmed = next.trim();
    if (trimmed === (data.label ?? '')) {
      labelEditing = false;
      return;
    }
    if (sessionStore.active === null) return;
    const result = await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'document'
            ? ({ ...it, label: trimmed } as DocumentItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Document title edit aborted — session reconnect failed.',
        failMessage: 'Document title edit failed',
      },
    );
    if (result.ok) labelEditing = false;
  }

  async function commitContent(next: string): Promise<void> {
    if (next === (data.content ?? '')) {
      contentEditing = false;
      return;
    }
    if (sessionStore.active === null) return;
    const byteLength = new TextEncoder().encode(next).length;
    if (byteLength > DOCUMENT_INLINE_MAX_BYTES) {
      toastStore.show({
        message: `Document content too long (${byteLength} / ${DOCUMENT_INLINE_MAX_BYTES} bytes).`,
        tone: 'error',
      });
      return;
    }
    const result = await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'document'
            ? ({ ...it, content: next, size_bytes: byteLength } as DocumentItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Document edit aborted — session reconnect failed.',
        failMessage: 'Document commit failed',
      },
    );
    if (result.ok) contentEditing = false;
  }

  type ResizeParams = { x: number; y: number; width: number; height: number };
  const DOC_MIN_H = 35;
  const DOC_RESTORE_W = 360;
  const DOC_RESTORE_H = 220;

  async function onResizeEnd(_event: unknown, params: ResizeParams): Promise<void> {
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'document'
            ? ({
                ...it,
                x: params.x,
                y: params.y,
                w: Math.max(220, params.width),
                h: Math.max(160, params.height),
              } as DocumentItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Resize aborted — session reconnect failed.',
        failMessage: 'Resize failed',
      },
    );
  }

  async function onMinimizeClick(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    e.preventDefault();
    if (isLocked) return;
    if (sessionStore.active === null) return;
    const cur = sessionStore.items.get(data.id);
    if (cur === undefined) return;
    const next = cur.minimized !== true;
    let nextW = cur.w;
    let nextH = cur.h;
    if (next) {
      sessionStore.backupItemGeom(data.id, { x: cur.x, y: cur.y, w: cur.w, h: cur.h });
      nextH = DOC_MIN_H;
    } else {
      const backup = sessionStore.getRestoredGeom(data.id);
      nextW = backup?.w ?? DOC_RESTORE_W;
      nextH = backup?.h ?? DOC_RESTORE_H;
      sessionStore.clearRestoredGeom(data.id);
    }
    await sessionStore.applyMutation(
      (cur2) => ({
        ...cur2,
        items: cur2.items.map((it) =>
          it.id === data.id && it.type === 'document'
            ? ({ ...it, minimized: next, w: nextW, h: nextH } as DocumentItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Minimize aborted — session reconnect failed.',
        failMessage: 'Minimize failed',
      },
    );
  }

  function onMaximizeClick(e: MouseEvent): void {
    e.stopPropagation();
    e.preventDefault();
    sessionStore.toggleMaximize(data.id);
  }

  async function onCloseClick(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    e.preventDefault();
    if (isLocked) return;
    await sessionStore.applyDeletion([data.id], { killTerminal: false });
  }

  const DOCUMENT_ACCEPT = '.md,.txt,.json,.html,.css,.js,.ts,.tsx,.jsx,.pdf,text/*,application/json,application/pdf';

  async function onLoadFileClick(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    if (isLocked) return;
    const file = await pickLocalFile({ accept: DOCUMENT_ACCEPT });
    if (file === null) return;
    try {
      const uploaded = await uploadAsset(file, 'document');
      await sessionStore.applyMutation(
        (cur) => ({
          ...cur,
          items: cur.items.map((it: CanvasItem) =>
            it.id === data.id && it.type === 'document'
              ? ({
                  ...it,
                  asset_id: uploaded.asset_id,
                  label: uploaded.file_name.replace(/\.[^/.]+$/, ''),
                  file_name: uploaded.file_name,
                  mime: uploaded.mime,
                  size_bytes: uploaded.size_bytes,
                  content: undefined,
                } as DocumentItem)
              : it,
          ),
        }),
        {
          abortMessage: 'Document file change aborted — session reconnect failed.',
          failMessage: 'Document file change failed',
        },
      );
    } catch (err) {
      toastStore.show({
        message: err instanceof AssetUploadUnavailableError
          ? 'Asset upload API is not available yet. Backend work is required before document upload can complete.'
          : `Document upload failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
        durationMs: 6_000,
      });
    }
  }

  function onRootClick(e: MouseEvent): void {
    if (isEmpty) void onLoadFileClick(e);
  }

  function onBodyWheel(e: WheelEvent): void {
    e.stopPropagation();
  }
</script>

{#if isVisible}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="document-node shape-document"
    class:m-single={isSingleM}
    class:m-multi={isMultiM}
    class:locked={isLocked}
    class:is-empty={isEmpty}
    class:is-min={data.minimized === true}
    style="width: 100%; height: 100%;"
    role="article"
    aria-label={`Document ${documentTitle}`}
    onclick={data.minimized !== true && isEmpty ? onRootClick : undefined}
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked && data.minimized !== true}
      minWidth={220}
      minHeight={160}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeEnd}
    />
    <!-- 1. Doc-head: file svg + title + actions. Filename lives in footer. -->
    <header class="doc-head">
      <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
        <path d="M3 1.5h4.5L9.5 3.5V10.5H3V1.5z" />
        <path d="M7.5 1.5V3.5h2" />
      </svg>
      {#if labelEditing}
        <InlineEditField
          value={documentTitle}
          editing={true}
          plain={true}
          placeholder={fileStem || 'Untitled'}
          class="doc-title-edit"
          onCommit={(next: string) => void commitLabel(next)}
          onCancel={() => (labelEditing = false)}
        />
      {:else}
        <span
          class="doc-title"
          title={`${documentTitle} — double-click to rename`}
          ondblclick={onLabelDblClick}
          role="presentation"
        >{documentTitle}</span>
      {/if}
      {#if !isLocked}
        <div class="doc-actions">
          {#if canToggleView}
            <!-- ADR-0037 D1/D4 — markdown 은 2-mode (rendered↔source),
                 html 은 3-mode cyclic (rendered→interactive→source→rendered).
                 icon = next mode 의 힌트 (사용자 click 시 가는 곳). -->
            <button
              type="button"
              class="doc-btn"
              class:is-active={viewMode !== 'rendered'}
              title={nextViewModeLabel}
              aria-label={nextViewModeLabel}
              onclick={(e: MouseEvent) => {
                e.stopPropagation();
                documentViewModeStore.set(data.id, getNextViewMode(viewMode, fileTypeLabel));
              }}
              onmousedown={(e: MouseEvent) => e.stopPropagation()}
            >
              {#if viewMode === 'rendered' && fileTypeLabel === 'html'}
                <!-- play (run interactively) -->
                <svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true">
                  <polygon points="6 4 20 12 6 20"/>
                </svg>
              {:else if viewMode === 'source'}
                <!-- book-open (show rendered) — visibility eye 와 겹침 회피. -->
                <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
                  <path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z"/>
                  <path d="M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z"/>
                </svg>
              {:else}
                <!-- </> code (show source) -->
                <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
                  <polyline points="16 18 22 12 16 6"/>
                  <polyline points="8 6 2 12 8 18"/>
                </svg>
              {/if}
            </button>
          {/if}
          <button
            type="button"
            class="doc-btn"
            title="Change document"
            aria-label="Change document"
            onclick={(e) => void onLoadFileClick(e)}
            onmousedown={(e: MouseEvent) => e.stopPropagation()}
          >
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
              <path d="M9 17H7A5 5 0 0 1 7 7h2"/>
              <path d="M15 7h2a5 5 0 1 1 0 10h-2"/>
              <line x1="8" x2="16" y1="12" y2="12"/>
            </svg>
          </button>
          <button
            type="button"
            class="doc-btn"
            class:is-active={data.minimized === true}
            title={data.minimized === true ? 'Restore' : 'Minimize'}
            aria-label={data.minimized === true ? 'Restore' : 'Minimize'}
            onclick={(e) => void onMinimizeClick(e)}
            onmousedown={(e: MouseEvent) => e.stopPropagation()}
          >
            {#if data.minimized === true}
              <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
                <path d="M3 5h6"/><path d="M3 8h6"/>
              </svg>
            {:else}
              <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
                <path d="M3 8h6"/>
              </svg>
            {/if}
          </button>
          <button
            type="button"
            class="doc-btn"
            class:is-active={isMaximized}
            title={isMaximized ? 'Restore' : 'Maximize'}
            aria-label={isMaximized ? 'Restore' : 'Maximize'}
            onclick={onMaximizeClick}
            onmousedown={(e: MouseEvent) => e.stopPropagation()}
          >
            <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
              <rect x="2.5" y="2.5" width="7" height="7" rx="0.5"/>
            </svg>
          </button>
          <button
            type="button"
            class="doc-btn close"
            title="Close"
            aria-label="Close"
            onclick={(e) => void onCloseClick(e)}
            onmousedown={(e: MouseEvent) => e.stopPropagation()}
          >
            <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
              <path d="M3 3l6 6M9 3l-6 6"/>
            </svg>
          </button>
        </div>
      {/if}
    </header>

    <!-- 2. Doc-body: eyebrow + h2 + p (placeholder 시 empty hint) -->
    <div
      class="doc-body nowheel"
      ondblclick={onBodyDblClick}
      onwheel={onBodyWheel}
      role="presentation"
    >
      {#if contentEditing}
        <InlineEditTextarea
          value={data.content ?? ''}
          editing={true}
          plain={true}
          placeholder={'# Heading\n\nWrite your document — markdown ok.'}
          rows={8}
          class="doc-content-edit"
          onCommit={(next: string) => void commitContent(next)}
          onCancel={() => (contentEditing = false)}
        />
      {:else if isEmpty}
        <div class="empty-hint">
          <span class="empty-idle" aria-hidden="true">
            <svg class="empty-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linejoin="round" stroke-linecap="round">
              <path d="M7 3.5h7l3 3v14H7z"/>
              <path d="M14 3.5v3h3"/>
              <path d="M10 11h4M10 14h5M10 17h3"/>
            </svg>
          </span>
        </div>
      {:else if isInline}
        <div class="eyebrow">Inline document</div>
        <!-- ADR-0018 D10 amend ③/④/⑤ + ADR-0037 — markdown/html rendered or
             interactive (sandboxed iframe, html 만) or source. -->
        {#if viewMode === 'source'}
          <pre class="doc-source">{data.content ?? ''}</pre>
        {:else if viewMode === 'interactive' && fileTypeLabel === 'html'}
          <!-- svelte-ignore a11y_missing_attribute -->
          <iframe
            bind:this={iframeRef}
            class="doc-iframe"
            sandbox={INTERACTIVE_IFRAME_SANDBOX}
            referrerpolicy="no-referrer"
            loading="lazy"
            srcdoc={interactiveSrcdoc}
            style:height={iframeHeightStyle}
          ></iframe>
        {:else}
          <div class="doc-md">{@html inlineHtml}</div>
        {/if}
      {:else}
        <div class="eyebrow">Document file</div>
        {#if assetPreviewLoading}
          <p>Loading preview…</p>
        {:else if assetPreviewText !== null && (assetPreviewText.trim().length > 0)}
          {#if viewMode === 'source'}
            <pre class="doc-source">{assetPreviewText}</pre>
          {:else if viewMode === 'interactive' && fileTypeLabel === 'html'}
            <!-- svelte-ignore a11y_missing_attribute -->
            <iframe
              bind:this={iframeRef}
              class="doc-iframe"
              sandbox={INTERACTIVE_IFRAME_SANDBOX}
              referrerpolicy="no-referrer"
              loading="lazy"
              srcdoc={interactiveSrcdocAsset}
              style:height={iframeHeightStyle}
            ></iframe>
          {:else}
            <div class="doc-md">{@html assetHtml}</div>
          {/if}
        {:else}
          <div class="asset-summary">
            <svg class="asset-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
              <path d="M7 3.5h7l3 3v14H7z"/>
              <path d="M14 3.5v3h3"/>
              <path d="M10 11h4M10 14h5M10 17h3"/>
            </svg>
            <h2>{data.file_name}</h2>
            <p>{assetPreviewError ?? 'Preview is not available for this document type.'}</p>
          </div>
        {/if}
      {/if}
    </div>

    <!-- 3. Doc-foot: page-dot + count + right meta -->
    <footer class="doc-foot">
      <span class="page-dot on" aria-hidden="true"></span>
      <span>Page 1</span>
      {#if nameEditing}
        <InlineEditField
          value={data.file_name}
          editing={true}
          plain={true}
          placeholder="filename.md"
          class="doc-name-edit"
          onCommit={(next: string) => void commitName(next)}
          onCancel={() => (nameEditing = false)}
        />
      {:else}
        <span
          class="filename"
          title={isInline ? `${data.file_name} — double-click to rename` : data.file_name}
          ondblclick={onNameDblClick}
          role="presentation"
        >{data.file_name}</span>
      {/if}
      {#if (data.size_bytes ?? 0) > 0}
        <span class="doc-size" title={sizeLabel}>{sizeLabel}</span>
      {/if}
      <span class="right">{fileTypeLabel}</span>
    </footer>
  </div>
{/if}

<style>
  /* ref/frontend-design/components-v5 §02 — .shape-document.
   * grid 30 / 1fr / 26 (head/body/foot). */
  .document-node {
    display: grid;
    grid-template-rows: 30px 1fr 26px;
    box-sizing: border-box;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.06);
    border-radius: var(--radius-md);
    color: var(--color-fg);
    overflow: hidden;
  }

  .document-node.m-single,
  .document-node.m-multi {
    outline: none;
  }

  .document-node.locked {
    cursor: default;
  }

  /* head + foot — surface-2 mono 10px / 0.4px */
  .doc-head,
  .doc-foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 14px;
    background: var(--color-surface-2);
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.4px;
    color: var(--color-fg-muted);
    min-width: 0;
  }

  .doc-head {
    padding: 0 4px 0 14px;
    border-bottom: 1px solid var(--color-border);
    gap: 6px;
  }

  .doc-foot {
    border-top: 1px solid var(--color-border);
  }

  .doc-head svg {
    flex-shrink: 0;
    opacity: 0.75;
  }

  .doc-head .doc-title,
  .doc-foot .filename {
    flex: 1 1 auto;
    color: var(--color-fg);
    font-size: 9.5px;
    font-weight: var(--weight-medium);
    letter-spacing: 0.4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .doc-foot .filename {
    max-width: 42%;
    text-transform: uppercase;
  }

  .doc-head > span:not(.doc-title),
  .doc-foot > span:not(.page-dot) {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .doc-foot .right {
    margin-left: 0;
    max-width: 34%;
    flex-shrink: 1;
  }

  .doc-size {
    flex: 0 1 auto;
    margin-left: auto;
    margin-right: auto;
    max-width: 72px;
    text-align: center;
  }

  .doc-actions {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    flex: 0 0 auto;
    margin-left: 2px;
  }

  .doc-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border: none;
    background: transparent;
    border-radius: 4px;
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .doc-btn:hover:not(:disabled) {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  .doc-btn.is-active {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  .doc-btn.close:hover:not(:disabled) {
    background: #e5484d;
    color: #ffffff;
  }

  .doc-btn:focus-visible {
    outline: 1px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .document-node.is-min {
    grid-template-rows: 30px;
    border: 0;
    box-shadow: none;
    background: transparent;
    cursor: default;
  }

  .document-node.is-min .doc-head {
    height: 100%;
    box-sizing: border-box;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: 0 1px 10px rgba(0, 0, 0, 0.10);
  }

  .document-node.is-min.m-single .doc-head,
  .document-node.is-min.m-multi .doc-head {
    border-color: var(--color-accent);
    border-width: 1.5px;
  }

  .document-node.is-min .doc-body,
  .document-node.is-min .doc-foot {
    display: none;
  }

  /* body — generous padding, eyebrow + h2 + p */
  .doc-body {
    padding: 28px 36px 24px;
    overflow: auto;
    min-height: 0;
    scrollbar-width: thin;
  }

  .doc-body .eyebrow {
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
    margin-bottom: 18px;
  }

  .doc-body h2 {
    margin: 0 0 12px;
    font-family: var(--font-sans);
    font-size: 26px;
    font-weight: 540;
    letter-spacing: -0.6px;
    line-height: 1.15;
    color: var(--color-fg);
    overflow-wrap: anywhere;
  }

  .doc-body p {
    margin: 0 0 8px;
    font-family: var(--font-sans);
    font-size: 12.5px;
    line-height: 1.55;
    letter-spacing: -0.1px;
    color: var(--color-fg-muted);
    overflow-wrap: anywhere;
  }

  /* ADR-0037 D3 + R4 (2단계) — interactive mode (sandboxed iframe). iframe 가
     doc-body 의 padding 영역 가득 채우고 eyebrow 는 숨김. :has() 로 dedicated
     branch CSS — markup churn 없이 layout 분기. R4: content height 를
     style:height inline 으로 받고 max-height 으로 panel 안에서 cap →
     content 크면 panel 안 scroll, 작으면 content 만큼만 (위 빈 공간 X). */
  .doc-iframe {
    display: block;
    flex: 0 0 auto;
    width: 100%;
    min-height: 200px;
    border: 0;
    background: #ffffff;
  }
  .doc-body:has(.doc-iframe) {
    padding: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
  }
  .doc-body:has(.doc-iframe) .eyebrow {
    display: none;
  }

  /* ADR-0018 D10 amend ④ — source view (raw markdown / html source code). */
  .doc-source {
    margin: 0;
    padding: 0;
    font-family: var(--font-mono);
    font-size: 11.5px;
    line-height: 1.55;
    color: var(--color-fg-muted);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  /* plan 0011 follow-up (2026-05-21) — marked output container styling.
     ADR-0018 D10 의 inline/asset markdown 모두 본 container 안. */
  .doc-md {
    color: var(--color-fg-muted);
    font-family: var(--font-sans);
    font-size: 12.5px;
    line-height: 1.55;
    letter-spacing: -0.1px;
    overflow-wrap: anywhere;
  }
  .doc-md :global(h1),
  .doc-md :global(h2) {
    margin: 0 0 12px;
    font-family: var(--font-sans);
    font-size: 26px;
    font-weight: 540;
    letter-spacing: -0.6px;
    line-height: 1.15;
    color: var(--color-fg);
  }
  .doc-md :global(h3) {
    margin: 16px 0 8px;
    font-size: 18px;
    font-weight: 600;
    color: var(--color-fg);
  }
  .doc-md :global(h4),
  .doc-md :global(h5),
  .doc-md :global(h6) {
    margin: 14px 0 6px;
    font-size: 14px;
    font-weight: 600;
    color: var(--color-fg);
  }
  .doc-md :global(p) {
    margin: 0 0 10px;
  }
  .doc-md :global(ul),
  .doc-md :global(ol) {
    margin: 0 0 10px;
    padding-left: 22px;
  }
  .doc-md :global(li) {
    margin: 2px 0;
  }
  .doc-md :global(blockquote) {
    margin: 0 0 10px;
    padding-left: 12px;
    border-left: 2px solid var(--color-border-strong);
    color: var(--color-fg-subtle);
  }
  .doc-md :global(code) {
    font-family: var(--font-mono);
    font-size: 11.5px;
    padding: 0 4px;
    background: var(--color-surface-2);
    border-radius: var(--radius-sm);
  }
  .doc-md :global(pre) {
    margin: 0 0 10px;
    padding: 10px 12px;
    background: var(--color-surface-2);
    border-radius: var(--radius-sm);
    overflow-x: auto;
    font-family: var(--font-mono);
    font-size: 11.5px;
    line-height: 1.5;
  }
  .doc-md :global(pre code) {
    padding: 0;
    background: transparent;
    border-radius: 0;
  }
  .doc-md :global(a) {
    color: var(--color-accent);
    text-decoration: underline;
  }
  .doc-md :global(table) {
    border-collapse: collapse;
    margin: 0 0 10px;
    font-size: 12px;
  }
  .doc-md :global(th),
  .doc-md :global(td) {
    border: 1px solid var(--color-border);
    padding: 4px 8px;
    text-align: left;
  }
  .doc-md :global(th) {
    background: var(--color-surface-2);
    font-weight: 600;
    color: var(--color-fg);
  }
  .doc-md :global(hr) {
    border: 0;
    border-top: 1px solid var(--color-border);
    margin: 14px 0;
  }
  .doc-md :global(img) {
    max-width: 100%;
    height: auto;
  }

  .asset-summary {
    display: grid;
    justify-items: start;
    gap: 8px;
  }

  .asset-summary h2 {
    margin-bottom: 0;
  }

  .asset-icon {
    width: 24px;
    height: 24px;
    color: var(--color-fg-muted);
  }

  .document-node.is-empty {
    cursor: pointer;
  }

  .document-node.is-empty .doc-head .doc-title {
    color: var(--color-fg-muted);
  }

  .document-node.is-empty .doc-body {
    display: grid;
    place-items: center;
    padding: 14px;
  }

  .empty-hint {
    display: grid;
    grid-template-areas: "stack";
    place-items: center;
    width: 100%;
    height: 100%;
  }

  .empty-idle {
    grid-area: stack;
    display: grid;
    place-items: center;
    gap: 7px;
    color: var(--color-fg-muted);
    opacity: 0.7;
    transition: opacity var(--motion-fast) var(--motion-easing);
  }

  .empty-icon {
    width: 24px;
    height: 24px;
  }

  /* Inline edit chrome — doc-content-edit textarea + doc-name-edit input.
   * Doc-body 의 padding 을 그대로 따라가도록 width 100% + font 정합. */
  :global(.doc-content-edit) {
    width: 100%;
    min-height: 120px;
    font-family: var(--font-mono);
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--color-fg);
  }

  :global(.doc-name-edit),
  :global(.doc-title-edit) {
    flex: 1 1 auto;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 9.5px;
    font-weight: var(--weight-medium);
    letter-spacing: 0.4px;
    text-transform: uppercase;
    color: var(--color-fg);
  }

  :global(.doc-title-edit) {
    text-transform: none;
  }

  /* page-dot indicator — 5px circle */
  .doc-foot .page-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--color-border-strong);
    flex-shrink: 0;
  }

  .doc-foot .page-dot.on {
    background: var(--color-fg);
  }
</style>
