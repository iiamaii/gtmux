<script lang="ts">
  // NoteNode — SvelteFlow custom node for `type: "note"` (ADR-0018 D4).
  //
  // 사용자 메모: title + body. note color 는 accent border + tinted background.
  // Inline edit (P1): 더블 클릭 → title (single line) / body (textarea) 분리 편집.

  import { NodeResizer } from '@xyflow/svelte';
  import InlineEditField from '$lib/common/InlineEditField.svelte';
  import InlineEditTextarea from '$lib/common/InlineEditTextarea.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { mutateLayout, UnauthorizedError } from '$lib/http/sessions';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import type { NoteItem, CanvasItem } from '$lib/types/canvas';

  interface NoteNodeData {
    id: string;
    w: number;
    h: number;
    visibility: boolean;
    locked: boolean;
    title: string;
    body: string;
    color: string;
  }

  let {
    data,
    selected = false,
  }: {
    data: NoteNodeData;
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

  let titleEditing = $state(false);
  let bodyEditing = $state(false);
  type ResizeParams = { x: number; y: number; width: number; height: number };

  function onTitleDblClick(e: MouseEvent): void {
    if (isLocked) return;
    e.stopPropagation();
    titleEditing = true;
  }

  function onBodyDblClick(e: MouseEvent): void {
    if (isLocked) return;
    e.stopPropagation();
    bodyEditing = true;
  }

  async function commit(field: 'title' | 'body', next: string): Promise<void> {
    if (field === 'title' && next === data.title) {
      titleEditing = false;
      return;
    }
    if (field === 'body' && next === data.body) {
      bodyEditing = false;
      return;
    }
    const active = sessionStore.active;
    if (active === null) return;
    try {
      const { layout } = await mutateLayout(active.name, (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'note'
            ? ({ ...it, [field]: next } as NoteItem)
            : it,
        ),
      }));
      sessionStore.loadLayout(layout);
      if (field === 'title') titleEditing = false;
      else bodyEditing = false;
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Note commit failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  async function onResizeEnd(_event: unknown, params: ResizeParams): Promise<void> {
    const active = sessionStore.active;
    if (active === null) return;
    try {
      const { layout } = await mutateLayout(active.name, (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'note'
            ? ({ ...it, x: params.x, y: params.y, w: Math.max(160, params.width), h: Math.max(80, params.height) } as NoteItem)
            : it,
        ),
      }));
      sessionStore.loadLayout(layout);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Resize failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

</script>

{#if isVisible}
  <div
    class="note-node"
    class:m-single={isInM}
    class:locked={isLocked}
    style="width: 100%; height: 100%; --note-accent: {data.color};"
    role="group"
    aria-label="Note item"
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked}
      minWidth={160}
      minHeight={80}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeEnd}
    />
    <div class="note-header" ondblclick={onTitleDblClick} role="presentation">
      {#if titleEditing}
        <InlineEditField
          value={data.title}
          editing={true}
          allowEmpty={true}
          placeholder="Title…"
          class="note-title-edit"
          onCommit={(next: string) => void commit('title', next)}
          onCancel={() => (titleEditing = false)}
        />
      {:else}
        <span class="note-title">
          {data.title.length > 0 ? data.title : 'Untitled'}
        </span>
      {/if}
    </div>
    <div class="note-body-wrap" ondblclick={onBodyDblClick} role="presentation">
      {#if bodyEditing}
        <InlineEditTextarea
          value={data.body}
          editing={true}
          allowEmpty={true}
          placeholder="Body…"
          class="note-body-edit"
          onCommit={(next: string) => void commit('body', next)}
          onCancel={() => (bodyEditing = false)}
        />
      {:else if data.body.length === 0}
        <span class="note-placeholder">Double-click to add body</span>
      {:else}
        <pre class="note-body">{data.body}</pre>
      {/if}
    </div>
  </div>
{/if}

<style>
  .note-node {
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    background: color-mix(in srgb, var(--note-accent) 12%, var(--color-surface));
    border: 1px solid var(--note-accent);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-sm);
    overflow: hidden;
    font-family: var(--font-sans);
  }

  .note-node.m-single {
    outline: none;
  }

  .note-node.locked {
    cursor: default;
  }

  .note-header {
    padding: var(--space-4) var(--space-8);
    border-bottom: 1px solid color-mix(in srgb, var(--note-accent) 40%, transparent);
    cursor: text;
    flex: 0 0 auto;
  }

  .note-title {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    color: var(--color-fg);
  }

  .note-body-wrap {
    flex: 1 1 auto;
    min-height: 0;
    padding: var(--space-4) var(--space-8);
    overflow: auto;
    cursor: text;
  }

  .note-body {
    margin: 0;
    font-family: inherit;
    font-size: var(--text-sm);
    color: var(--color-fg);
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.45;
  }

  .note-placeholder {
    color: var(--color-fg-subtle);
    font-size: var(--text-sm);
    font-style: italic;
    user-select: none;
  }

  :global(.note-title-edit) {
    width: 100%;
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }

  :global(.note-body-edit) {
    width: 100%;
    min-height: 60px;
    font-size: var(--text-sm);
    background: transparent;
    border: 0;
    resize: none;
    outline: none;
    color: var(--color-fg);
    font-family: inherit;
    line-height: 1.45;
  }
</style>
