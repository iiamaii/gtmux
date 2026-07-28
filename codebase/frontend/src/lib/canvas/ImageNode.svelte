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

  import { NodeResizer, useSvelteFlow } from '@xyflow/svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { filePicker } from '$lib/stores/filePicker.svelte';
  import CanvasGlyph from './CanvasGlyph.svelte';
  import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
  import { fsFileUrl } from '$lib/http/fs';
  import {
    IMAGE_EXTENSIONS,
    basename,
    guessMimeFromPath,
    resolveWorkspacePath,
    workspaceRelativePath,
  } from '$lib/files/workspaceAssets';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import type { CanvasItem, ImageItem } from '$lib/types/canvas';
  import CanvasCloseButton from './CanvasCloseButton.svelte';
  import {
    constrainResizeAspectIfShift,
    scheduleLiveAspectResize,
  } from './resizeConstraint';
  import { holdLayoutRefetch, releaseLayoutRefetch } from '$lib/ws/layoutRefetch.svelte';

  interface ImageNodeData {
    id: string;
    x: number;
    y: number;
    w: number;
    h: number;
    visibility: boolean;
    locked: boolean;
    label?: string;
    path?: string;
    asset_id?: string;
    mime?: string;
    original_w?: number;
    original_h?: number;
    /** Canvas.svelte group selection proxy. Descendants must not show own controls. */
    group_selected?: boolean;
  }

  let {
    data,
  }: {
    data: ImageNodeData;
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

  const { updateNode } = useSvelteFlow();
  const isVisible = $derived(data.visibility !== false);
  const isLocked = $derived(data.locked === true);
  const isInM = $derived(sessionStore.M.has(data.id) && data.group_selected !== true);
  const workspaceRoot = $derived(sessionStore.effectiveWorkspaceRoot);
  const resolvedWorkspacePath = $derived(
    data.path !== undefined ? resolveWorkspacePath(workspaceRoot, data.path) : null,
  );
  const imageSrc = $derived(
    resolvedWorkspacePath !== null
      ? fsFileUrl(resolvedWorkspacePath)
      : (data.asset_id ?? '').length > 0
        ? `/api/assets/${data.asset_id}`
        : '',
  );
  const hasAsset = $derived(imageSrc.length > 0);
  const imageLabel = $derived(data.label ?? 'image');
  // Copy-path target — resolved ABSOLUTE workspace path (same source as
  // DocumentNode's documentCopyPath). null → no copy button (asset_id-only
  // or empty image has no filesystem path to copy).
  const imageCopyPath = $derived(resolvedWorkspacePath);

  async function onCopyPathClick(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    const path = imageCopyPath;
    if (path === null) return;
    const result = await copyTextToSystemClipboard(path);
    toastStore.show({
      message: result.ok ? 'Copied file path.' : (result.reason ?? 'Copy failed.'),
      tone: result.ok ? 'success' : 'error',
    });
  }

  type ResizeParams = { x: number; y: number; width: number; height: number };
  const RESIZE_MIN_W = 120;
  const RESIZE_MIN_H = 80;

  function sourceAspect(): number {
    return data.original_w !== undefined && data.original_h !== undefined && data.original_h > 0
      ? data.original_w / data.original_h
      : data.w / data.h;
  }

  function applyLiveResize(next: ResizeParams): void {
    updateNode(data.id, (node) => ({
      position: { ...node.position, x: next.x, y: next.y },
      width: Math.max(RESIZE_MIN_W, next.width),
      height: Math.max(RESIZE_MIN_H, next.height),
    }));
  }

  function onResize(event: unknown, params: ResizeParams): void {
    scheduleLiveAspectResize(
      event,
      params,
      data,
      sourceAspect(),
      RESIZE_MIN_W,
      RESIZE_MIN_H,
      applyLiveResize,
    );
  }

  // ADR-0053 D7 — resize gesture 동안 외부발 0x80 refetch defer (node drag 와
  // 동일 가드). NodeResizer 는 d3-drag 기반 — start/end 는 항상 짝으로 발화.
  function onResizeStart(): void {
    holdLayoutRefetch();
  }

  async function onResizeEnd(event: unknown, params: ResizeParams): Promise<void> {
    releaseLayoutRefetch();
    const constrained = constrainResizeAspectIfShift(
      event,
      params,
      data,
      sourceAspect(),
      RESIZE_MIN_W,
      RESIZE_MIN_H,
    );
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'image'
            ? ({
                ...it,
                x: constrained.x,
                y: constrained.y,
                w: Math.max(RESIZE_MIN_W, constrained.width),
                h: Math.max(RESIZE_MIN_H, constrained.height),
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

  const IMAGE_ACCEPT = IMAGE_EXTENSIONS.join(',');

  function initialImageDir(): string {
    if (resolvedWorkspacePath === null) return workspaceRoot;
    const slash = resolvedWorkspacePath.lastIndexOf('/');
    return slash <= 0 ? workspaceRoot : resolvedWorkspacePath.slice(0, slash);
  }

  function onLoadImageClick(e: MouseEvent): void {
    e.stopPropagation();
    if (isLocked) return;
    if (workspaceRoot.length === 0) {
      toastStore.show({
        message: 'Workspace root is not available yet.',
        tone: 'error',
      });
      return;
    }
    filePicker.openFor(initialImageDir(), (absolutePath) => {
      const nextPath = workspaceRelativePath(workspaceRoot, absolutePath);
      if (nextPath === null) {
        toastStore.show({
          message: 'Image files must be inside the active project workspace.',
          tone: 'error',
        });
        return;
      }
      void sessionStore.applyMutation(
        (cur) => ({
          ...cur,
          items: cur.items.map((it: CanvasItem) =>
            it.id === data.id && it.type === 'image'
              ? ({
                  ...it,
                  label: basename(absolutePath),
                  path: nextPath,
                  asset_id: undefined,
                  mime: guessMimeFromPath(absolutePath),
                  original_w: undefined,
                  original_h: undefined,
                } as ImageItem)
              : it,
          ),
        }),
        {
          abortMessage: 'Image file change aborted — session reconnect failed.',
          failMessage: 'Image file change failed',
        },
      );
    }, {
      accept: { extensions: [...IMAGE_EXTENSIONS], description: 'image files' },
      rootKind: 'workspace',
      rootPath: workspaceRoot,
    });
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
    onclick={!hasAsset ? onLoadImageClick : undefined}
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked}
      minWidth={120}
      minHeight={80}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeStart}
      {onResize}
      {onResizeEnd}
    />
    <CanvasCloseButton id={data.id} variant={hasAsset ? 'dark' : 'light'} disabled={isLocked} />
    {#if imageCopyPath !== null}
      <!-- Copy path — view-only (never mutates), so it stays VISIBLE while
           locked (SoT §5 philosophy, document precedent). Copies the resolved
           ABSOLUTE workspace path with the same toast UX as DocumentNode.
           Cluster order: copy · change · close (1px gaps, 20×20). -->
      <button
        type="button"
        class="image-copy"
        title="Copy path"
        aria-label="Copy path"
        onclick={(e) => void onCopyPathClick(e)}
      >
        <CanvasGlyph name="copy" />
      </button>
    {/if}
    {#if isLocked}
      <!-- Locked-state indicator — unified CanvasGlyph 'lock' (lock UX
           unification 2026-07-27, ADR-0018 D9 family). ImageNode has no chrome
           header, so a persistent top-left corner badge carries the lock state
           (change button is hidden below; close is disabled). Static indicator —
           unlock stays in the Inspector State section. -->
      <span
        class="image-lock"
        class:on-asset={hasAsset}
        title="Locked — unlock in the Inspector"
        aria-label="Locked"
      >
        <CanvasGlyph name="lock" />
      </span>
    {:else}
      <button
        type="button"
        class="image-change"
        title={hasAsset ? 'Change image' : 'Load image'}
        aria-label={hasAsset ? 'Change image' : 'Load image'}
        onclick={onLoadImageClick}
      >
        <CanvasGlyph name="change" />
      </button>
    {/if}
    <div class="image-clip" class:is-empty={!hasAsset}>
      {#if hasAsset}
        <img
          src={imageSrc}
          alt=""
          class="image-asset"
          draggable="false"
        />
        <div class="img-caption" aria-hidden="true">
          <!-- Type-identity glyph + filename — bottom caption (2026-07-27
               re-spec: restored to the original bottom placement, HOVER-REVEAL
               in sync with the overlay buttons). Unified via CanvasGlyph
               'image' (ADR-0016 정합); canvas-tier 12px, sits left of the
               filename so the caption reads as the image's identity row. -->
          <span class="caption-glyph"><CanvasGlyph name="image" /></span>
          <span class="filename">{imageLabel}</span>
          <span class="right">image</span>
        </div>
      {:else}
        <span class="empty-idle" aria-hidden="true">
          <!-- Type-identity glyph — unified via CanvasGlyph 'image'
               (icon unification 2026-07-27, ADR-0016 정합). -->
          <CanvasGlyph name="image" size={24} />
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

  /* Asset state — TRANSPARENT body (2026-07-27 spec) so the contain letterbox /
     padding region reveals the canvas behind the node instead of a colored
     backdrop. Border + shadow (node chrome) stay from the base rule; only the
     fill is dropped. Border + drop shadow (chrome) stay. Empty state keeps its
     dashed drop-zone surface. */
  .image-node:not(.is-empty) {
    background: transparent;
  }

  .image-node.m-single {
    outline: none;
  }

  .image-node.locked {
    cursor: default;
  }

  /* Contain-scaling (2026-07-27, user request) — the ENTIRE image must always
     be visible regardless of node aspect. Was object-fit: cover, which cropped
     the image to fill the frame. `contain` letterboxes the image; `--space-8`
     (8px) padding keeps a consistent inset so the image never touches the frame
     edge (or the hover caption). box-sizing: border-box so the padding lives
     inside the 100%×100% clip. The letterbox/padding region is TRANSPARENT
     (2026-07-27 spec addition) — the asset-state node body paints no fill (see
     `.image-node:not(.is-empty)` below), so the busy canvas shows through
     around the contained image. */
  .image-asset {
    width: 100%;
    height: 100%;
    object-fit: contain;
    padding: var(--space-8);
    box-sizing: border-box;
    display: block;
  }

  /* Bottom identity caption (2026-07-27 re-spec — restored to original bottom
     placement). Glyph + filename over a bottom-up dark gradient so it reads
     over any image. HOVER-REVEAL: opacity 0 → 1 on node hover / focus-within,
     in sync with the overlay buttons. pointer-events:none so it never blocks
     the image. */
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

  /* Caption filename — NoteNode-anchored micro-label family (ADR-0016 정합).
     NO uppercase — filename is case-bearing. */
  .img-caption .filename {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 9.5px;
    font-weight: 540;
    letter-spacing: 0.6px;
    color: #ffffff;
  }

  /* Identity glyph in the caption — canvas-tier 12px, aligned with the
     filename baseline. currentColor inherits the caption's white text. */
  .img-caption .caption-glyph {
    display: inline-flex;
    flex-shrink: 0;
    color: #ffffff;
    opacity: 0.85;
  }

  .img-caption .right {
    margin-left: auto;
    flex-shrink: 0;
    opacity: 0.7;
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

  .image-change {
    position: absolute;
    /* 1px gap to the close button (SoT §1 canvas cluster gap). Empty state uses
       the light CanvasCloseButton at 6/6 → change at 6+20+1 = 27px; the asset
       state uses the dark close at 8/8 (see :not(.is-empty) override below).
       Was right:34px / top:6px (8px gap + 2px vertical drift), 2026-07-27. */
    top: 6px;
    right: 27px;
    z-index: 12;
    width: 20px; /* canvas-tier standard box (icon unification 2026-07-27) */
    height: 20px;
    display: grid;
    place-items: center;
    border: none;
    border-radius: var(--radius-sm);
    /* Resting background TRANSPARENT — note-style (NoteNode .note-btn parity,
       2026-07-27 re-spec). Glass fill on hover only. Glyph carries a soft
       drop-shadow so it stays legible over arbitrary image content while
       resting transparent. */
    background: transparent;
    color: var(--color-fg-muted);
    filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.55));
    cursor: pointer;
    padding: 0;
    opacity: 0;
    transition:
      opacity var(--motion-fast) var(--motion-easing),
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  /* Asset state — the close button switches to the dark variant at 8/8, so the
     change button follows it: 8+20+1 = 29px, top 8px (1px gap, aligned). */
  .image-node:not(.is-empty) .image-change {
    top: 8px;
    right: 29px;
  }

  .image-node:hover .image-change,
  .image-change:focus-visible {
    opacity: 1;
  }

  .image-change:hover {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  /* Copy-path button — one 20px slot left of change (cluster order
     copy · change · close, 1px gaps). Empty state: close 6 → change 27 →
     copy 48; asset state (dark close at 8/8): 8 → 29 → 50. While locked the
     change button is hidden, so copy compacts into the change slot. Same
     hover-reveal + chip style as .image-change. */
  .image-copy {
    position: absolute;
    top: 6px;
    right: 48px;
    z-index: 12;
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    border: none;
    border-radius: var(--radius-sm);
    /* Resting background TRANSPARENT — note-style (NoteNode .note-btn parity,
       2026-07-27 re-spec). Glass fill on hover only. Soft glyph drop-shadow
       for legibility over arbitrary image content. */
    background: transparent;
    color: var(--color-fg-muted);
    filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.55));
    cursor: pointer;
    padding: 0;
    opacity: 0;
    transition:
      opacity var(--motion-fast) var(--motion-easing),
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .image-node:not(.is-empty) .image-copy {
    top: 8px;
    right: 50px;
  }

  .image-node.locked .image-copy {
    right: 27px;
  }

  .image-node.locked:not(.is-empty) .image-copy {
    right: 29px;
  }

  .image-node:hover .image-copy,
  .image-copy:focus-visible {
    opacity: 1;
  }

  .image-copy:hover {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  /* Locked badge — persistent (status, not hover-reveal), top-left corner so it
     never collides with the top-right close/change cluster. Canvas-tier 20×20
     box like the sibling controls. Empty state = surface-2 chip; asset state =
     dark translucent chip so the glyph reads over any image. */
  .image-lock {
    position: absolute;
    top: 6px;
    left: 6px;
    z-index: 12;
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--color-surface-2) 88%, transparent);
    color: var(--color-fg-muted);
    pointer-events: none;
  }

  .image-lock.on-asset {
    top: 8px;
    left: 8px;
    background: rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    color: #ffffff;
  }
</style>
