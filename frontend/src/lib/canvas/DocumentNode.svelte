<script lang="ts">
  /**
   * DocumentNode — SvelteFlow custom node for `type: "document"`.
   *
   * 정본 시안: `ref/frontend-design/components-v3.html §02` — `.shape-document`.
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
  import type { CanvasItem, DocumentItem } from '$lib/types/canvas';

  interface DocumentNodeData {
    id: string;
    w: number;
    h: number;
    visibility: boolean;
    locked: boolean;
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

  /** size_bytes 의 사람용 표기 (KB). */
  const sizeLabel = $derived.by((): string => {
    const bytes = data.size_bytes ?? 0;
    if (bytes < 1024) return `${bytes} B`;
    return `${(bytes / 1024).toFixed(1)} KB`;
  });

  /** content 를 (heading, paragraphs) 로 분할. 첫 줄이 markdown `# `, `## ` 등
   * 으로 시작하면 그 부분을 heading 으로, 아니면 빈 heading. */
  const parsed = $derived.by((): { heading: string; paragraphs: string[] } => {
    const raw = data.content ?? '';
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
  });
  const isEmpty = $derived(isInline && parsed.heading === '' && parsed.paragraphs.length === 0);

  type ResizeParams = { x: number; y: number; width: number; height: number };

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
</script>

{#if isVisible}
  <div
    class="document-node shape-document"
    class:m-single={isInM}
    class:locked={isLocked}
    class:is-empty={isEmpty}
    style="width: 100%; height: 100%;"
    role="article"
    aria-label={`Document ${data.file_name}`}
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked}
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
      <span class="filename" title={data.file_name}>{data.file_name}</span>
      {#if (data.size_bytes ?? 0) > 0}
        <span class="sep">·</span>
        <span>{sizeLabel}</span>
      {/if}
      <span class="right">{isInline ? 'inline' : 'asset'}</span>
    </header>

    <!-- 2. Doc-body: eyebrow + h2 + p (placeholder 시 empty hint) -->
    <div class="doc-body">
      {#if isEmpty}
        <div class="empty-hint">Empty document — double-click to start writing</div>
      {:else}
        <div class="eyebrow">{isInline ? 'Inline document' : 'Asset · ' + (data.mime ?? '')}</div>
        {#if parsed.heading.length > 0}
          <h2>{parsed.heading}</h2>
        {/if}
        {#each parsed.paragraphs.slice(0, 3) as para}
          <p>{para}</p>
        {/each}
        {#if parsed.paragraphs.length > 3}
          <p class="more">… {parsed.paragraphs.length - 3} more</p>
        {/if}
      {/if}
    </div>

    <!-- 3. Doc-foot: page-dot + count + right meta -->
    <footer class="doc-foot">
      <span class="page-dot on" aria-hidden="true"></span>
      <span>Page 1</span>
      <span class="right">{isInline ? 'md · UTF-8' : (data.mime ?? '—')}</span>
    </footer>
  </div>
{/if}

<style>
  /* ref/frontend-design/components-v3.html §02 — .shape-document.
   * grid 30 / 1fr / 26 (head/body/foot). */
  .document-node {
    display: grid;
    grid-template-rows: 30px 1fr 26px;
    box-sizing: border-box;
    background: var(--color-surface);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.06), 0 0 0 1px var(--color-border);
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
  }

  .doc-head {
    border-bottom: 1px solid var(--color-border);
  }

  .doc-foot {
    border-top: 1px solid var(--color-border);
  }

  .doc-head svg {
    flex-shrink: 0;
    opacity: 0.75;
  }

  .doc-head .filename {
    color: var(--color-fg);
    font-weight: 540;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .doc-head .sep {
    color: var(--color-border-strong);
  }

  .doc-head .right,
  .doc-foot .right {
    margin-left: auto;
    flex-shrink: 0;
  }

  /* body — generous padding, eyebrow + h2 + p */
  .doc-body {
    padding: 28px 36px 24px;
    overflow: hidden;
    min-height: 0;
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
  }

  .doc-body p {
    margin: 0 0 8px;
    font-family: var(--font-sans);
    font-size: 12.5px;
    line-height: 1.55;
    letter-spacing: -0.1px;
    color: var(--color-fg-muted);
  }

  .doc-body p.more {
    color: var(--color-fg-subtle);
    font-style: italic;
  }

  .empty-hint {
    color: var(--color-fg-subtle);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.2px;
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
