<script lang="ts">
  // ShapeNode — rect / ellipse 공용 renderer (ADR-0018 D4).
  //
  // 두 type 의 payload 동일: stroke / fill / stroke_width.
  // 시각 차이는 border-radius 만 — `data.type` 으로 분기.

  import { NodeResizer } from '@xyflow/svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import type { CanvasItem, EllipseItem, RectItem } from '$lib/types/canvas';
  import CanvasCloseButton from './CanvasCloseButton.svelte';

  interface ShapeNodeData {
    id: string;
    type: 'rect' | 'ellipse';
    w: number;
    h: number;
    visibility: boolean;
    locked: boolean;
    stroke: string;
    fill: string;
    stroke_width: number;
  }

  let {
    data,
    selected = false,
  }: {
    data: ShapeNodeData;
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
  const isEllipse = $derived(data.type === 'ellipse');

  type ResizeParams = { x: number; y: number; width: number; height: number };

  async function onResizeEnd(_event: unknown, params: ResizeParams): Promise<void> {
    const nextW = Math.max(20, params.width);
    const nextH = Math.max(20, params.height);
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && (it.type === 'rect' || it.type === 'ellipse')
            ? ({
                ...it,
                x: params.x,
                y: params.y,
                w: nextW,
                h: nextH,
              } as RectItem | EllipseItem)
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
    class="shape-node"
    class:m-single={isInM}
    class:locked={isLocked}
    class:ellipse={isEllipse}
    style="
      width: 100%;
      height: 100%;
      border-color: {data.stroke};
      border-width: {data.stroke_width}px;
      background: {data.fill};
    "
    role="group"
    aria-label={`${data.type} item`}
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked}
      minWidth={20}
      minHeight={20}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeEnd}
    />
    <CanvasCloseButton id={data.id} disabled={isLocked} />
  </div>
{/if}

<style>
  .shape-node {
    box-sizing: border-box;
    border-style: solid;
    position: relative;
    overflow: visible;
  }

  .shape-node.ellipse {
    border-radius: 50%;
  }

  .shape-node.m-single {
    outline: none;
  }

  .shape-node.locked {
    cursor: default;
  }
</style>
