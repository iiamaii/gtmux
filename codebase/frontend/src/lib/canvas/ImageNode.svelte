<script lang="ts">
  /**
   * ImageNode — SvelteFlow custom node for `type: "image"` (ADR-0018 D4).
   *
   * 현 단계: BE asset endpoint (P2+) 미land — placeholder visual 만.
   * 사용자가 canvas click 으로 빈 image item 생성, BE 의 `/api/assets/*`
   * ship 후 file picker → upload → asset_id wire 후속.
   *
   * 시각: ref/frontend-design/components-v5 §03 Image. Empty 는 dashed drop
   * zone, asset 이 있으면 이미지가 frame 을 채우고 하단 caption / 상단 status
   * pill 을 overlay 한다.
   */

  import { NodeResizer } from '@xyflow/svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { pickLocalFile } from '$lib/files/localFilePicker';
  import { uploadAsset, AssetUploadUnavailableError } from '$lib/http/assets';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import type { CanvasItem, ImageItem } from '$lib/types/canvas';
  import CanvasCloseButton from './CanvasCloseButton.svelte';

  interface ImageNodeData {
    id: string;
    w: number;
    h: number;
    visibility: boolean;
    locked: boolean;
    label?: string;
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
  const imageLabel = $derived(data.label ?? 'image');

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

  const IMAGE_ACCEPT = 'image/png,image/jpeg,image/gif,image/webp,image/svg+xml';

  async function onLoadImageClick(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    if (isLocked) return;
    const file = await pickLocalFile({ accept: IMAGE_ACCEPT });
    if (file === null) return;
    try {
      const uploaded = await uploadAsset(file, 'image');
      await sessionStore.applyMutation(
        (cur) => ({
          ...cur,
          items: cur.items.map((it: CanvasItem) =>
            it.id === data.id && it.type === 'image'
              ? ({
                  ...it,
                  label: uploaded.file_name,
                  asset_id: uploaded.asset_id,
                  mime: uploaded.mime,
                  original_w: uploaded.original_w,
                  original_h: uploaded.original_h,
                } as ImageItem)
              : it,
          ),
        }),
        {
          abortMessage: 'Image file change aborted — session reconnect failed.',
          failMessage: 'Image file change failed',
        },
      );
    } catch (err) {
      toastStore.show({
        message: err instanceof AssetUploadUnavailableError
          ? 'Asset upload API is not available yet. Backend work is required before image upload can complete.'
          : `Image upload failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
        durationMs: 6_000,
      });
    }
  }
</script>

{#if isVisible}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="image-node"
    class:m-single={isInM}
    class:locked={isLocked}
    class:is-empty={!hasAsset}
    style="width: 100%; height: 100%;"
    role="img"
    aria-label={hasAsset ? 'Image' : 'Image (pending — BE asset endpoint required)'}
    onclick={!hasAsset ? (e) => void onLoadImageClick(e) : undefined}
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
    <CanvasCloseButton id={data.id} variant={hasAsset ? 'dark' : 'light'} disabled={isLocked} />
    {#if !isLocked}
      <button
        type="button"
        class="image-change"
        title={hasAsset ? 'Change image' : 'Load image'}
        aria-label={hasAsset ? 'Change image' : 'Load image'}
        onclick={(e) => void onLoadImageClick(e)}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
          <path d="M9 17H7A5 5 0 0 1 7 7h2"/>
          <path d="M15 7h2a5 5 0 1 1 0 10h-2"/>
          <line x1="8" x2="16" y1="12" y2="12"/>
        </svg>
      </button>
    {/if}
    <div class="image-clip" class:is-empty={!hasAsset}>
      {#if hasAsset}
        <img
          src={`/api/assets/${data.asset_id}`}
          alt=""
          class="image-asset"
          draggable="false"
        />
        <div class="img-caption" aria-hidden="true">
          <span class="filename">{imageLabel}</span>
          <span class="right">image</span>
        </div>
      {:else}
        <span class="empty-idle" aria-hidden="true">
          <svg class="empty-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linejoin="round" stroke-linecap="round">
            <rect x="4" y="5" width="16" height="14" rx="1.5"/>
            <path d="M7 16l4.1-4.1 3 3L16 13l3 3"/>
            <circle cx="15.5" cy="9.5" r="1.2"/>
          </svg>
        </span>
      {/if}
    </div>
  </div>
{/if}

<style>
  /* ref/frontend-design/components-v5 §03 — .shape-image. */
  .image-node {
    display: block;
    box-sizing: border-box;
    position: relative;
    isolation: isolate;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06);
    color: var(--color-fg);
    overflow: visible;
  }

  .image-node.is-empty {
    background: var(--color-surface);
    border: 1px dashed var(--color-border-strong);
    box-shadow: none;
    color: var(--color-fg-muted);
    cursor: pointer;
    transition:
      border-color var(--motion-fast) var(--motion-easing),
      background var(--motion-fast) var(--motion-easing);
  }

  .image-node.is-empty:hover {
    border-color: var(--color-accent);
    border-style: solid;
    background: color-mix(in srgb, var(--color-accent) 6%, var(--color-surface));
  }

  .image-clip {
    position: absolute;
    inset: 0;
    display: block;
    overflow: hidden;
    border-radius: calc(var(--radius-md) - 1px);
    z-index: 0;
  }

  .image-clip.is-empty {
    display: grid;
    place-items: center;
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
    object-fit: cover;
    display: block;
  }

  .img-caption {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 28px 12px 10px;
    background: linear-gradient(to bottom, rgba(0, 0, 0, 0) 0%, rgba(0, 0, 0, 0.55) 100%);
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.4px;
    color: #f5f5f5;
    pointer-events: none;
    opacity: 0;
    transition: opacity var(--motion-fast) var(--motion-easing);
  }

  .image-node:hover .img-caption,
  .image-node:focus-within .img-caption {
    opacity: 1;
  }

  .img-caption .filename {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 540;
    color: #ffffff;
  }

  .img-caption .right {
    opacity: 0.7;
  }

  .img-caption .right {
    margin-left: auto;
    flex-shrink: 0;
  }

  :global(.image-node .svelte-flow__resize-control) {
    z-index: 10 !important;
  }

  .empty-idle {
    grid-area: 1 / 1;
    display: grid;
    grid-template-rows: 24px auto;
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

  .image-change {
    position: absolute;
    top: 6px;
    right: 34px;
    z-index: 12;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border: none;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--color-surface-2) 88%, transparent);
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    opacity: 0;
    transition:
      opacity var(--motion-fast) var(--motion-easing),
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .image-node:hover .image-change,
  .image-change:focus-visible {
    opacity: 1;
  }

  .image-change:hover {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }
</style>
