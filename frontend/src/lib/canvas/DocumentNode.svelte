<script lang="ts">
  /**
   * DocumentNode — SvelteFlow custom node for `type: "document"`.
   *
   * 정본:
   * - ADR-0018 D4 amend ② (2026-05-17) — 두 mode: (a) asset-based / (b)
   *   inline-stored. BE schema.rs 가 Item::Document 에 `asset_id: Option`
   *   + `content: Option` ship (DOCUMENT_INLINE_MAX_BYTES 64 KB).
   * - plan-0011 FE Slice-A2 (caption/document FE wire) — 본 컴포넌트가
   *   inline-stored mode 의 시안 §02 정합 placeholder. 추후 InlineEdit
   *   wire 는 별 후속.
   *
   * 현 단계: read-only display 만 — file_name 헤더 + content preview (cap
   * 200자 미리보기). 더블 클릭 inline edit 진입은 후속 (Slice-A2).
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
  const contentPreview = $derived((data.content ?? '').slice(0, 240));
  const isEmpty = $derived(isInline && contentPreview.length === 0);

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
                w: Math.max(200, params.width),
                h: Math.max(120, params.height),
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
    class="document-node"
    class:m-single={isInM}
    class:locked={isLocked}
    style="width: 100%; height: 100%;"
    role="article"
    aria-label={`Document ${data.file_name}`}
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked}
      minWidth={200}
      minHeight={120}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeEnd}
    />
    <header class="doc-head">
      <svg class="doc-head-icon" width="13" height="13" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
        <path d="M3.5 1.5h4.5L11 4.5V12.5H3.5V1.5z" />
        <path d="M8 1.5v3h3" />
      </svg>
      <span class="doc-head-name" title={data.file_name}>{data.file_name}</span>
      {#if isInline}
        <span class="doc-head-mode">inline</span>
      {/if}
    </header>
    <div class="doc-body">
      {#if isEmpty}
        <span class="doc-body-empty">Empty document</span>
      {:else if isInline}
        <pre class="doc-body-content">{contentPreview}</pre>
      {:else}
        <span class="doc-body-asset" title={data.asset_id}>
          asset: {(data.asset_id ?? '').slice(0, 12)}…
        </span>
      {/if}
    </div>
  </div>
{/if}

<style>
  /* 시안 §02 — grid 30/1fr/26 (head/body/foot). 본 placeholder 는 foot 생략. */
  .document-node {
    display: grid;
    grid-template-rows: 30px 1fr;
    box-sizing: border-box;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    color: var(--color-fg);
    font-family: var(--font-mono);
    overflow: hidden;
  }

  .document-node.m-single {
    outline: none;
  }

  .document-node.locked {
    cursor: default;
  }

  .doc-head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 var(--space-12);
    background: var(--color-surface-2);
    border-bottom: 1px solid var(--color-border);
    font-size: 11px;
    font-weight: 540;
    letter-spacing: 0.2px;
    color: var(--color-fg);
  }

  .doc-head-icon {
    color: var(--color-fg-muted);
    flex: 0 0 13px;
  }

  .doc-head-name {
    flex: 1 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .doc-head-mode {
    flex: 0 0 auto;
    font-size: 9px;
    color: var(--color-fg-subtle);
    text-transform: uppercase;
    letter-spacing: 0.6px;
  }

  .doc-body {
    padding: var(--space-10) var(--space-12);
    overflow: hidden;
    min-height: 0;
  }

  .doc-body-empty {
    color: var(--color-fg-subtle);
    font-style: italic;
    font-size: 12px;
  }

  .doc-body-content {
    margin: 0;
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.4;
    color: var(--color-fg);
    white-space: pre-wrap;
    word-break: break-word;
    overflow: hidden;
  }

  .doc-body-asset {
    font-size: 11px;
    color: var(--color-fg-muted);
  }
</style>
