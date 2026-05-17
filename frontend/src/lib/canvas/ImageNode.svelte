<script lang="ts">
  /**
   * ImageNode — SvelteFlow custom node for `type: "image"` (ADR-0018 D4).
   *
   * 현 단계: BE asset endpoint (P2+) 미land — placeholder visual 만.
   * 사용자가 canvas click 으로 빈 image item 생성, BE 의 `/api/assets/*`
   * ship 후 file picker → upload → asset_id wire 후속.
   *
   * 시각: 회색 dashed border + 중앙의 image-icon glyph + "Image (pending)"
   * caption. asset_id 가 set 되면 (P2+) `<img src="/api/assets/{asset_id}">`
   * 로 교체.
   */

  import { NodeResizer } from '@xyflow/svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import type { CanvasItem, ImageItem } from '$lib/types/canvas';

  interface ImageNodeData {
    id: string;
    w: number;
    h: number;
    visibility: boolean;
    locked: boolean;
    asset_id?: string;
    mime?: string;
  }

  let {
    data,
    selected = false,
  }: {
    data: ImageNodeData;
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
  const hasAsset = $derived((data.asset_id ?? '').length > 0);

  type ResizeParams = { x: number; y: number; width: number; height: number };

  async function onResizeEnd(_event: unknown, params: ResizeParams): Promise<void> {
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'image'
            ? ({
                ...it,
                x: params.x,
                y: params.y,
                w: Math.max(120, params.width),
                h: Math.max(80, params.height),
              } as ImageItem)
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
    class="image-node"
    class:m-single={isInM}
    class:locked={isLocked}
    style="width: 100%; height: 100%;"
    role="img"
    aria-label={hasAsset ? 'Image' : 'Image (pending — BE asset endpoint required)'}
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked}
      minWidth={120}
      minHeight={80}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeEnd}
    />
    {#if hasAsset}
      <img
        src={`/api/assets/${data.asset_id}`}
        alt=""
        class="image-asset"
        draggable="false"
      />
    {:else}
      <div class="empty-stub" aria-hidden="true">
        <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3" y="3" width="18" height="18" rx="2" />
          <circle cx="8.5" cy="8.5" r="1.5" />
          <polyline points="21 15 16 10 5 21" />
        </svg>
        <div class="label">Drop an image here</div>
        <span class="hint">Pending upload</span>
      </div>
    {/if}
  </div>
{/if}

<style>
  /* ref/frontend-design/components-v3.html — .shape-image (empty/placeholder
   * variant). Real asset rendering 은 P2+ (/api/assets/* ship 후). 현 단계는
   * is-empty 패턴만 — dashed border + 중앙 stub. */
  .image-node {
    display: grid;
    place-items: center;
    box-sizing: border-box;
    background: var(--color-surface);
    border: 1px dashed var(--color-border-strong);
    border-radius: var(--radius-md);
    color: var(--color-fg-muted);
    overflow: hidden;
  }

  .image-node.m-single {
    outline: none;
  }

  .image-node.locked {
    cursor: default;
  }

  .image-asset {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }

  .empty-stub {
    text-align: center;
    color: var(--color-fg-muted);
    font-size: 12px;
    letter-spacing: -0.1px;
    line-height: 1.45;
    padding: 12px;
  }

  .empty-stub svg {
    opacity: 0.55;
    margin-bottom: 6px;
  }

  .empty-stub .label {
    font-family: var(--font-sans);
    color: var(--color-fg-muted);
  }

  .empty-stub .hint {
    display: block;
    font-family: var(--font-mono);
    font-size: 9px;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    margin-top: 6px;
    color: var(--color-fg-subtle);
  }
</style>
