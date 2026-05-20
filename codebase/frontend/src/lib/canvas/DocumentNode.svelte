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
  import InlineEditField from '$lib/common/InlineEditField.svelte';
  import InlineEditTextarea from '$lib/common/InlineEditTextarea.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { uploadAsset, AssetUploadUnavailableError } from '$lib/http/assets';
  import { pickLocalFile } from '$lib/files/localFilePicker';
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

  /** content 를 (heading, paragraphs) 로 분할. 첫 줄이 markdown `# `, `## ` 등
   * 으로 시작하면 그 부분을 heading 으로, 아니면 빈 heading. */
  function parseDocumentText(raw: string): { heading: string; paragraphs: string[] } {
    if (raw.length === 0) return { heading: '', paragraphs: [] };
    const lines = raw.split(/\r?\n/);
    let heading = '';
    let i = 0;
    const headingMatch = lines[0]?.match(/^#{1,6}\s+(.+)$/);
    if (headingMatch) {
      heading = headingMatch[1] ?? '';
      i = 1;
    }
    // skip blank lines after heading
    while (i < lines.length && lines[i]?.trim() === '') i++;
    // group remaining content into paragraphs (blank-line separated).
    const paragraphs: string[] = [];
    let buf: string[] = [];
    for (; i < lines.length; i++) {
      const line = lines[i] ?? '';
      if (line.trim() === '') {
        if (buf.length > 0) {
          paragraphs.push(buf.join(' '));
          buf = [];
        }
      } else {
        buf.push(line);
      }
    }
    if (buf.length > 0) paragraphs.push(buf.join(' '));
    return { heading, paragraphs };
  }

  const parsed = $derived.by((): { heading: string; paragraphs: string[] } => {
    return parseDocumentText(data.content ?? '');
  });
  const assetParsed = $derived.by((): { heading: string; paragraphs: string[] } => {
    return parseDocumentText(assetPreviewText ?? '');
  });
  const isEmpty = $derived(isInline && parsed.heading === '' && parsed.paragraphs.length === 0);
  const canPreviewAssetText = $derived.by(() => {
    if (isInline) return false;
    const mime = (data.mime ?? '').toLowerCase();
    return mime.startsWith('text/') || mime === 'application/json';
  });

  $effect(() => {
    const assetId = data.asset_id ?? '';
    if (isInline || assetId.length === 0 || !canPreviewAssetText) {
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
        const res = await fetch(`/api/assets/${assetId}`, {
          method: 'GET',
          credentials: 'include',
          headers: { Accept: data.mime ?? 'text/plain' },
        });
        if (!res.ok) throw new Error(`GET /api/assets/${assetId} returned ${res.status}`);
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
  let contentEditing = $state(false);

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
    const result = await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'document'
            ? ({ ...it, file_name: trimmed } as DocumentItem)
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
  const DOC_MIN_H = 30;
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
    class:m-single={isInM}
    class:locked={isLocked}
    class:is-empty={isEmpty}
    class:is-min={data.minimized === true}
    style="width: 100%; height: 100%;"
    role="article"
    aria-label={`Document ${data.file_name}`}
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
    <!-- 1. Doc-head: file svg + filename + sep + size + right -->
    <header class="doc-head">
      <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
        <path d="M3 1.5h4.5L9.5 3.5V10.5H3V1.5z" />
        <path d="M7.5 1.5V3.5h2" />
      </svg>
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
      {#if !isLocked}
        <div class="doc-actions">
          <button
            type="button"
            class="doc-btn"
            title="Change document"
            aria-label="Change document"
            onclick={(e) => void onLoadFileClick(e)}
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
            title={data.minimized === true ? 'Restore' : 'Minimize'}
            aria-label={data.minimized === true ? 'Restore' : 'Minimize'}
            onclick={(e) => void onMinimizeClick(e)}
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
            title="Maximize"
            aria-label="Maximize"
            onclick={onMaximizeClick}
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
        {#if parsed.heading.length > 0}
          <h2>{parsed.heading}</h2>
        {/if}
        {#each parsed.paragraphs.slice(0, 3) as para}
          <p>{para}</p>
        {/each}
        {#if parsed.paragraphs.length > 3}
          <p class="more">… {parsed.paragraphs.length - 3} more</p>
        {/if}
      {:else}
        <div class="eyebrow">Document file</div>
        {#if assetPreviewLoading}
          <p>Loading preview…</p>
        {:else if assetPreviewText !== null && (assetParsed.heading.length > 0 || assetParsed.paragraphs.length > 0)}
          {#if assetParsed.heading.length > 0}
            <h2>{assetParsed.heading}</h2>
          {:else}
            <h2>{data.file_name}</h2>
          {/if}
          {#each assetParsed.paragraphs.slice(0, 3) as para}
            <p>{para}</p>
          {/each}
          {#if assetParsed.paragraphs.length > 3}
            <p class="more">… {assetParsed.paragraphs.length - 3} more</p>
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

  .document-node.m-single {
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

  .doc-head .filename {
    flex: 1 1 auto;
    color: var(--color-fg);
    font-size: 9.5px;
    font-weight: var(--weight-medium);
    letter-spacing: 0.4px;
    text-transform: uppercase;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .doc-head > span:not(.filename),
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
    display: flex;
    align-items: center;
    gap: 1px;
    flex-shrink: 0;
    margin-left: 2px;
  }

  .doc-btn {
    width: 18px;
    height: 20px;
    display: grid;
    place-items: center;
    border: none;
    background: transparent;
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .doc-btn:hover {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  .doc-btn.close:hover {
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

  .document-node.is-min.m-single .doc-head {
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

  .doc-body p.more {
    color: var(--color-fg-subtle);
    font-style: italic;
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

  .document-node.is-empty .doc-head .filename {
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

  :global(.doc-name-edit) {
    flex: 1 1 auto;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 9.5px;
    font-weight: var(--weight-medium);
    letter-spacing: 0.4px;
    text-transform: uppercase;
    color: var(--color-fg);
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
