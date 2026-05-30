<script lang="ts">
  import { onDestroy } from 'svelte';
  import { useSvelteFlow } from '@xyflow/svelte';
  import { debugCount } from '$lib/common/debugCounts';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { pathEditStore } from '$lib/stores/pathEditStore.svelte';
  import type {
    Anchor,
    CanvasItem,
    FigureStrokeDash,
    Head,
    PathEndpoint,
    PathItem,
    PathRouting,
    PathWaypoint,
    Point,
  } from '$lib/types/canvas';
  import {
    anchorPoint,
    buildPathD,
    connectableTargetAtPoint,
    connectPathEndpoint,
    insertWaypointNearPoint,
    nearestAnchor,
    removeWaypoints,
    resolveEndpoint,
    updatePathBBoxCache,
  } from './pathGeometry';
  import { strokeDashArray } from './strokeDash';
  import PathHeadMarker from './PathHeadMarker.svelte';

  interface PathNodeData {
    id: string;
    parent_id: string | null;
    x: number;
    y: number;
    w: number;
    h: number;
    z: number;
    visibility: boolean;
    locked: boolean;
    minimized: boolean;
    from: PathEndpoint;
    to: PathEndpoint;
    routing: PathRouting;
    waypoints?: PathWaypoint[];
    head_from?: Head;
    head_to?: Head;
    stroke: string;
    stroke_width: number;
    stroke_dash?: FigureStrokeDash;
    group_selected?: boolean;
  }

  let { data }: { data: PathNodeData } = $props();

  const { screenToFlowPosition } = useSvelteFlow();
  const isVisible = $derived(data.visibility !== false);
  const isLocked = $derived(data.locked === true);
  const isInM = $derived(sessionStore.M.has(data.id) && data.group_selected !== true);
  const isEditing = $derived(pathEditStore.editingPathId === data.id);
  const itemMap = sessionStore.items;
  const svgDashArray = $derived(strokeDashArray(data.stroke_dash, data.stroke_width));
  const startHead = $derived(data.head_from ?? 'none');
  const endHead = $derived(data.head_to ?? 'none');
  const startMarkerId = $derived(`path-${data.id}-marker-start-${startHead}`);
  const endMarkerId = $derived(`path-${data.id}-marker-end-${endHead}`);
  const startMarkerRef = $derived(startHead === 'none' ? undefined : `url(#${startMarkerId})`);
  const endMarkerRef = $derived(endHead === 'none' ? undefined : `url(#${endMarkerId})`);
  const CONNECT_PREVIEW_MARGIN = 36;
  const ANCHOR_HOVER_RADIUS = 18;
  const PREVIEW_ANCHORS: readonly Anchor[] = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW', 'center'];

  type EndpointId = 'from' | 'to';
  type AnchorPreview = {
    box: { x: number; y: number; w: number; h: number };
    anchors: {
      anchor: Anchor;
      x: number;
      y: number;
      nearest: boolean;
      hovered: boolean;
    }[];
  };
  type DragState =
    | {
        kind: 'endpoint';
        pointerId: number;
        endpoint: EndpointId;
      }
    | {
        kind: 'waypoint';
        pointerId: number;
        startFlow: { x: number; y: number };
        startPath: PathItem;
        waypointIds: Set<string>;
      };

  let draft = $state<PathItem | null>(null);
  let dragState = $state<DragState | null>(null);
  let pendingCommit = $state<PathItem | null>(null);
  let anchorPreview = $state<AnchorPreview | null>(null);

  const renderPath = $derived(draft ?? pendingCommit ?? currentPath());
  const renderBox = $derived({ x: data.x, y: data.y, w: data.w, h: data.h });
  const pathD = $derived(buildPathD(renderPath, itemMap, renderBox));
  const fromHandle = $derived(toLocal(resolveEndpoint(renderPath.from, itemMap)));
  const toHandle = $derived(toLocal(resolveEndpoint(renderPath.to, itemMap)));
  const waypointHandles = $derived(
    (renderPath.waypoints ?? []).map((p) => ({
      id: p.id,
      x: p.x - renderBox.x,
      y: p.y - renderBox.y,
      selected: pathEditStore.selectedWaypointIds.has(p.id),
    })),
  );
  const hitStrokeWidth = $derived(Math.max(data.stroke_width + 20, 24));

  function currentPath(): PathItem {
    return {
      id: data.id,
      parent_id: data.parent_id,
      x: data.x,
      y: data.y,
      w: data.w,
      h: data.h,
      z: data.z,
      visibility: data.visibility ? 'visible' : 'hidden',
      locked: data.locked,
      minimized: data.minimized,
      type: 'path',
      from: cloneEndpoint(data.from),
      to: cloneEndpoint(data.to),
      routing: data.routing,
      waypoints: data.waypoints?.map((p) => ({ ...p })),
      head_from: data.head_from ?? 'none',
      head_to: data.head_to ?? 'arrow',
      stroke: data.stroke,
      stroke_width: data.stroke_width,
      stroke_dash: data.stroke_dash,
    };
  }

  function cloneEndpoint(endpoint: PathEndpoint): PathEndpoint {
    return endpoint.kind === 'free'
      ? { kind: 'free', point: { ...endpoint.point } }
      : {
          kind: 'connected',
          item_id: endpoint.item_id,
          anchor: endpoint.anchor,
          ...(endpoint.offset == null ? {} : { offset: { ...endpoint.offset } }),
          fallback_point: { ...endpoint.fallback_point },
        };
  }

  function toLocal(point: { x: number; y: number }): { x: number; y: number } {
    return { x: point.x - renderBox.x, y: point.y - renderBox.y };
  }

  function onPathDblClick(e: MouseEvent): void {
    if (isLocked) return;
    e.preventDefault();
    e.stopPropagation();
    pathEditStore.begin(data.id);
  }

  function isEditableTarget(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    const tag = target.tagName;
    return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable;
  }

  function onEditKeydown(e: KeyboardEvent): void {
    if (!isEditing || isLocked || isEditableTarget(e.target)) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopImmediatePropagation();
      pathEditStore.end(data.id);
      return;
    }
    if (e.key !== 'Delete' && e.key !== 'Backspace') return;
    if (pathEditStore.selectedWaypointIds.size === 0) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    const next = removeWaypoints(currentPath(), pathEditStore.selectedWaypointIds);
    pathEditStore.setSelectedWaypointIds([]);
    pendingCommit = next;
    void commitPath(next);
  }

  $effect(() => {
    if (!isEditing) return;
    window.addEventListener('keydown', onEditKeydown, { capture: true });
    return () => {
      window.removeEventListener('keydown', onEditKeydown, { capture: true });
    };
  });

  function onPathHitPointerUp(e: PointerEvent): void {
    if (!isEditing || isLocked) return;
    e.preventDefault();
    e.stopPropagation();
    const point = screenToFlowPosition({ x: e.clientX, y: e.clientY });
    const next = insertWaypointNearPoint(currentPath(), point, itemMap);
    pendingCommit = next;
    void commitPath(next);
  }

  function setEndpoint(path: PathItem, endpoint: EndpointId, point: Point): PathItem {
    const nextEndpoint: PathEndpoint = { kind: 'free', point };
    return endpoint === 'from'
      ? { ...path, from: nextEndpoint }
      : { ...path, to: nextEndpoint };
  }

  function connectableDropTargetAt(point: Point, path: PathItem, endpoint: EndpointId): CanvasItem | null {
    const other = endpoint === 'from' ? path.to : path.from;
    const blockedId = other.kind === 'connected' ? other.item_id : null;
    debugCount('path.endpointCandidate.scan');
    return connectableTargetAtPoint(point, itemMap, {
      margin: CONNECT_PREVIEW_MARGIN,
      excludeId: data.id,
      excludeSecondId: blockedId,
    });
  }

  function anchorPreviewAt(
    point: Point,
    path: PathItem,
    endpoint: EndpointId,
  ): AnchorPreview | null {
    const target = connectableDropTargetAt(point, path, endpoint);
    if (target === null) return null;
    const nearest = nearestAnchor(target, point);
    return {
      box: {
        x: target.x - renderBox.x,
        y: target.y - renderBox.y,
        w: target.w,
        h: target.h,
      },
      anchors: PREVIEW_ANCHORS.map((anchor) => {
        const pos = anchorPoint(target, anchor);
        const distance = Math.hypot(pos.x - point.x, pos.y - point.y);
        return {
          anchor,
          x: pos.x - renderBox.x,
          y: pos.y - renderBox.y,
          nearest: anchor === nearest,
          hovered: anchor === nearest && distance <= ANCHOR_HOVER_RADIUS,
        };
      }),
    };
  }

  function endpointPathAtPointer(
    path: PathItem,
    endpoint: EndpointId,
    point: Point,
  ): PathItem {
    const target = connectableDropTargetAt(point, path, endpoint);
    if (target === null) return setEndpoint(path, endpoint, point);
    return connectPathEndpoint(path, endpoint, target, point, itemMap)
      ?? setEndpoint(path, endpoint, point);
  }

  function onEndpointDown(endpoint: EndpointId, e: PointerEvent): void {
    if (isLocked) return;
    e.preventDefault();
    e.stopPropagation();
    draft = currentPath();
    anchorPreview = null;
    dragState = { kind: 'endpoint', pointerId: e.pointerId, endpoint };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    window.addEventListener('pointermove', onWindowPointerMove, { capture: true });
    window.addEventListener('pointerup', onWindowPointerUp, { capture: true });
    window.addEventListener('pointercancel', onWindowPointerCancel, { capture: true });
  }

  function onWaypointDown(id: string, e: PointerEvent): void {
    if (isLocked || !isEditing) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.metaKey || e.ctrlKey) {
      pathEditStore.toggleWaypoint(id);
    } else if (!pathEditStore.selectedWaypointIds.has(id)) {
      pathEditStore.setSelectedWaypointIds([id]);
    }
    const ids = new Set(pathEditStore.selectedWaypointIds);
    if (!ids.has(id)) ids.add(id);
    draft = currentPath();
    dragState = {
      kind: 'waypoint',
      pointerId: e.pointerId,
      startFlow: screenToFlowPosition({ x: e.clientX, y: e.clientY }),
      startPath: currentPath(),
      waypointIds: ids,
    };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    window.addEventListener('pointermove', onWindowPointerMove, { capture: true });
    window.addEventListener('pointerup', onWindowPointerUp, { capture: true });
    window.addEventListener('pointercancel', onWindowPointerCancel, { capture: true });
  }

  function removeWindowListeners(): void {
    window.removeEventListener('pointermove', onWindowPointerMove, { capture: true });
    window.removeEventListener('pointerup', onWindowPointerUp, { capture: true });
    window.removeEventListener('pointercancel', onWindowPointerCancel, { capture: true });
  }

  function onWindowPointerMove(e: PointerEvent): void {
    const state = dragState;
    if (state === null || e.pointerId !== state.pointerId) return;
    e.preventDefault();
    e.stopPropagation();
    const flow = screenToFlowPosition({ x: e.clientX, y: e.clientY });
    if (state.kind === 'endpoint') {
      const path = draft ?? currentPath();
      anchorPreview = anchorPreviewAt(flow, path, state.endpoint);
      draft = endpointPathAtPointer(path, state.endpoint, flow);
      return;
    }
    const dx = flow.x - state.startFlow.x;
    const dy = flow.y - state.startFlow.y;
    draft = {
      ...state.startPath,
      waypoints: (state.startPath.waypoints ?? []).map((p) =>
        state.waypointIds.has(p.id) ? { ...p, x: p.x + dx, y: p.y + dy } : p,
      ),
    };
  }

  function onWindowPointerUp(e: PointerEvent): void {
    if (dragState === null || e.pointerId !== dragState.pointerId) return;
    e.preventDefault();
    e.stopPropagation();
    const next =
      dragState.kind === 'endpoint'
        ? endpointPathAtPointer(
            draft ?? currentPath(),
            dragState.endpoint,
            screenToFlowPosition({ x: e.clientX, y: e.clientY }),
          )
        : draft;
    dragState = null;
    draft = null;
    anchorPreview = null;
    removeWindowListeners();
    if (next !== null) {
      pendingCommit = next;
      void commitPath(next);
    }
  }

  function onWindowPointerCancel(e?: PointerEvent): void {
    e?.preventDefault();
    e?.stopPropagation();
    dragState = null;
    draft = null;
    pendingCommit = null;
    anchorPreview = null;
    removeWindowListeners();
  }

  onDestroy(() => {
    if (dragState !== null) onWindowPointerCancel();
  });

  async function commitPath(next: PathItem): Promise<void> {
    const committed = updatePathBBoxCache(next, itemMap);
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'path' ? committed : it,
        ),
      }),
      {
        abortMessage: 'Path edit aborted — session reconnect failed.',
        failMessage: 'Path edit failed',
      },
    );
    pendingCommit = null;
  }
</script>

{#if isVisible}
  <div
    class="path-node"
    class:m-single={isInM}
    class:locked={isLocked}
    style="width: {renderBox.w}px; height: {renderBox.h}px;"
    role="group"
    aria-label="Path item"
    ondblclick={onPathDblClick}
  >
    <svg
      width={renderBox.w}
      height={renderBox.h}
      viewBox={`0 0 ${renderBox.w} ${renderBox.h}`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <defs>
        <PathHeadMarker
          id={startMarkerId}
          head={startHead}
          stroke={data.stroke}
          orient="auto-start-reverse"
        />
        <PathHeadMarker id={endMarkerId} head={endHead} stroke={data.stroke} />
      </defs>
      {#if isEditing && !isLocked}
        <path
          d={pathD}
          fill="none"
          stroke="var(--color-accent)"
          stroke-width={Math.max(data.stroke_width + 8, 10)}
          stroke-linecap="round"
          stroke-linejoin="round"
          vector-effect="non-scaling-stroke"
          pointer-events="none"
          class="edit-path-halo"
        />
      {/if}
      <path
        d={pathD}
        fill="none"
        stroke={data.stroke}
        stroke-width={data.stroke_width}
        stroke-dasharray={svgDashArray}
        stroke-linecap="round"
        stroke-linejoin="round"
        marker-start={startMarkerRef}
        marker-end={endMarkerRef}
        vector-effect="non-scaling-stroke"
        pointer-events="none"
      />
      {#if isEditing && !isLocked}
        <path
          d={pathD}
          fill="none"
          stroke="var(--color-accent)"
          stroke-width={Math.max(data.stroke_width, 2)}
          stroke-linecap="round"
          stroke-linejoin="round"
          vector-effect="non-scaling-stroke"
          pointer-events="none"
          class="edit-path-line"
        />
      {/if}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <path
        d={pathD}
        fill="none"
        stroke="transparent"
        stroke-width={hitStrokeWidth}
        stroke-linecap="round"
        stroke-linejoin="round"
        vector-effect="non-scaling-stroke"
        pointer-events="stroke"
        class="path-hit"
        onpointerup={onPathHitPointerUp}
      />
    </svg>
    {#if isInM && !isLocked}
      {#if anchorPreview !== null}
        <div
          class="anchor-preview-box"
          style="left: {anchorPreview.box.x}px; top: {anchorPreview.box.y}px; width: {anchorPreview.box.w}px; height: {anchorPreview.box.h}px;"
          aria-hidden="true"
        ></div>
        {#each anchorPreview.anchors as previewAnchor (previewAnchor.anchor)}
          <div
            class="anchor-preview"
            class:nearest={previewAnchor.nearest}
            class:hovered={previewAnchor.hovered}
            style="left: {previewAnchor.x}px; top: {previewAnchor.y}px;"
            aria-hidden="true"
          ></div>
        {/each}
      {/if}
      <button
        type="button"
        class="endpoint from"
        style="left: {fromHandle.x}px; top: {fromHandle.y}px;"
        aria-label="Move path start"
        onpointerdown={(e) => onEndpointDown('from', e)}
      ></button>
      <button
        type="button"
        class="endpoint to"
        style="left: {toHandle.x}px; top: {toHandle.y}px;"
        aria-label="Move path end"
        onpointerdown={(e) => onEndpointDown('to', e)}
      ></button>
    {/if}
    {#if isEditing && !isLocked}
      {#each waypointHandles as waypoint}
        <button
          type="button"
          class="waypoint"
          class:selected={waypoint.selected}
          style="left: {waypoint.x}px; top: {waypoint.y}px;"
          aria-label="Move path waypoint"
          onpointerdown={(e) => onWaypointDown(waypoint.id, e)}
        ></button>
      {/each}
    {/if}
  </div>
{/if}

<style>
  .path-node {
    box-sizing: border-box;
    position: relative;
    overflow: visible;
    pointer-events: none;
  }

  .path-node svg {
    display: block;
    overflow: visible;
  }

  .path-hit {
    cursor: pointer;
  }

  .path-node.locked .path-hit {
    cursor: default;
  }

  .edit-path-halo {
    opacity: 0.18;
  }

  .edit-path-line {
    opacity: 0.9;
    stroke-dasharray: 4 5;
  }

  .endpoint,
  .waypoint {
    position: absolute;
    width: var(--canvas-scaler-size, 10px);
    height: var(--canvas-scaler-size, 10px);
    padding: 0;
    border: var(--canvas-scaler-border, 1.5px) solid var(--color-accent);
    border-radius: 999px;
    background: var(--color-surface);
    transform: translate(-50%, -50%) scale(calc(1 / var(--canvas-zoom, 1)));
    transform-origin: center;
    pointer-events: auto;
  }

  .endpoint {
    cursor: crosshair;
  }

  .waypoint {
    cursor: move;
    border-radius: 2px;
  }

  .endpoint::before,
  .waypoint::before {
    content: '';
    position: absolute;
    inset: -4px;
  }

  .endpoint:hover,
  .waypoint:hover,
  .waypoint.selected {
    background: var(--color-accent);
  }

  .anchor-preview-box,
  .anchor-preview {
    position: absolute;
    pointer-events: none;
  }

  .anchor-preview-box {
    box-sizing: border-box;
    border: var(--canvas-scaler-border, 1.5px) dashed var(--color-accent);
    border-radius: 4px;
    opacity: 0.55;
  }

  .anchor-preview {
    width: var(--canvas-scaler-size, 10px);
    height: var(--canvas-scaler-size, 10px);
    border: var(--canvas-scaler-border, 1.5px) solid var(--color-accent);
    border-radius: 999px;
    background: var(--color-surface);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-bg) 80%, transparent);
    opacity: 0.85;
    transform: translate(-50%, -50%) scale(calc(1 / var(--canvas-zoom, 1)));
    transform-origin: center;
  }

  .anchor-preview.nearest {
    opacity: 1;
    border-color: var(--color-accent);
  }

  .anchor-preview.hovered {
    background: var(--color-accent);
    box-shadow:
      0 0 0 2px color-mix(in srgb, var(--color-bg) 80%, transparent),
      0 0 0 6px color-mix(in srgb, var(--color-accent) 22%, transparent);
  }
</style>
