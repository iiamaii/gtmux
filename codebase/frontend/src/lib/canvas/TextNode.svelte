<script lang="ts">
  // TextNode — SvelteFlow custom node for `type: "text"` (ADR-0018 D4).
  //
  // 사용자 free text. minimal rendering — body 만. Inline edit 은 P1+ 의 별
  // InlineEditTextarea wiring (0033 §8.2 InlineEditField consumer wire).
  //
  // 현재 단계: 더블 클릭 → InlineEditTextarea (body) → commit 시 mutateLayout.

  import { NodeResizer } from '@xyflow/svelte';
  import InlineEditTextarea from '$lib/common/InlineEditTextarea.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import type { TextAlign, TextVerticalAlign, TextItem, CanvasItem } from '$lib/types/canvas';
  import CanvasCloseButton from './CanvasCloseButton.svelte';
  // 텍스트 정렬 UI 는 ToolbarSubbar (lib/toolbar/ToolbarSubbar.svelte) 로 이전.
  // 본 컴포넌트는 더 이상 alignment toolbar 를 그리지 않는다.

  interface TextNodeData {
    id: string;
    x: number;
    y: number;
    w: number;
    h: number;
    z: number;
    visibility: boolean;
    locked: boolean;
    text: string;
    font_size: number;
    text_align?: TextAlign;
    text_vertical_align?: TextVerticalAlign;
    color: string;
  }

  let {
    data,
    selected = false,
  }: {
    data: TextNodeData;
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
  const textAlign = $derived(data.text_align ?? 'center');
  const textVerticalAlign = $derived(data.text_vertical_align ?? 'middle');

  let editing = $state(false);
  const minTextHeight = $derived(Math.max(16, Math.ceil(data.font_size)));
  type ResizeParams = { x: number; y: number; width: number; height: number };

  function onDblClick(e: MouseEvent): void {
    if (isLocked) return;
    e.stopPropagation();
    editing = true;
  }

  async function onCommit(next: string): Promise<void> {
    if (next === data.text) {
      editing = false;
      return;
    }
    if (sessionStore.active === null) {
      editing = false;
      return;
    }
    const result = await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'text'
            ? ({ ...it, text: next } as TextItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Text edit aborted — session reconnect failed.',
        failMessage: 'Text commit failed',
      },
    );
    if (result.ok) editing = false;
  }

  async function onResizeEnd(_event: unknown, params: ResizeParams): Promise<void> {
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'text'
            ? ({ ...it, x: params.x, y: params.y, w: Math.max(120, params.width), h: Math.max(minTextHeight, params.height) } as TextItem)
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
    class="text-node"
    class:m-single={isInM}
    class:locked={isLocked}
    style="width: 100%; height: 100%; font-size: {data.font_size}px; color: {data.color}; text-align: {textAlign};"
    role="group"
    aria-label="Text item"
    ondblclick={onDblClick}
  >
      <NodeResizer
        nodeId={data.id}
        isVisible={isInM && !isLocked}
        minWidth={120}
        minHeight={minTextHeight}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeEnd}
    />
    <CanvasCloseButton id={data.id} disabled={isLocked} />
    <div
      class="text-content"
      class:editing
      class:v-top={textVerticalAlign === 'top'}
      class:v-middle={textVerticalAlign === 'middle'}
      class:v-bottom={textVerticalAlign === 'bottom'}
    >
      <div class="text-cell">
        {#if editing}
          <InlineEditTextarea
            value={data.text}
            editing={true}
            allowEmpty={true}
            placeholder="Text…"
            class="text-edit"
            plain={true}
            rows={1}
            selectOnFocus={data.text.length === 0}
            textAlign={textAlign}
            onCommit={(next: string) => void onCommit(next)}
            onCancel={() => (editing = false)}
          />
        {:else if data.text.length === 0}
          <span class="text-placeholder">Double-click to edit</span>
        {:else}
          <span class="text-body">{data.text}</span>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .text-node {
    box-sizing: border-box;
    display: block;
    position: relative;
    padding: 0;
    background: transparent;
    line-height: 1.4;
    white-space: pre-wrap;
    word-break: break-word;
    overflow: visible;
    cursor: text;
  }

  .text-node.m-single {
    outline: none;
  }

  .text-node.locked {
    cursor: default;
  }

  .text-content {
    box-sizing: border-box;
    position: absolute;
    inset: 0 var(--space-8);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    z-index: 0;
  }

  .text-content.v-top {
    justify-content: flex-start;
  }

  .text-content.v-middle {
    justify-content: center;
  }

  .text-content.v-bottom {
    justify-content: flex-end;
  }

  .text-content.editing {
    overflow: visible;
  }

  .text-cell {
    box-sizing: border-box;
    display: block;
    width: 100%;
    word-break: break-word;
    white-space: pre-wrap;
    line-height: 1;
  }

  .text-placeholder {
    display: block;
    width: 100%;
    color: var(--color-fg-subtle);
    font-style: italic;
    line-height: 1;
    user-select: none;
  }

  .text-body {
    display: block;
    width: 100%;
    line-height: 1;
  }

  :global(.text-content .text-edit) {
    box-sizing: border-box;
    display: block;
    width: 100%;
    height: auto;
    min-height: 0;
    margin: 0;
    padding: 0;
    border: 0;
    font-family: inherit;
    font-size: inherit;
    color: inherit;
    background: transparent;
    resize: none;
    outline: none;
    line-height: 1;
    white-space: pre-wrap;
    word-break: break-word;
    overflow: hidden;
  }

  :global(.text-node .svelte-flow__resize-control) {
    z-index: 3;
  }
</style>
