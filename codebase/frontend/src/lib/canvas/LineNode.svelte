<script lang="ts">
  // LineNode — `type: "line"` (ADR-0018 D4).
  //
  // line item 의 payload: stroke / stroke_width + (x, y) 시작, (x2, y2) 끝
  // (canvas 절대 좌표).
  //
  // SvelteFlow 의 노드는 *positive width/height 의 박스* 만 지원하므로 wrap 박스
  // (`w × h`) 안에 SVG 를 채워 그린다. 박스 좌상단 = (item.x, item.y), SVG 안
  // 좌표 = (0,0) ↔ (data.w, data.h) — 시작/끝 점을 박스 내부 좌표로 변환.
  //
  // 음수 방향 (좌→우, 우→좌, 위→아래, 아래→위) 모두 지원: item.x2-item.x 의 부호로
  // SVG 좌표 결정.

  import { onDestroy } from 'svelte';
  import { useSvelteFlow } from '@xyflow/svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import type { CanvasItem, FigureStrokeDash, Head, LineItem } from '$lib/types/canvas';
  import { LINE_HIT_PADDING, LINE_MIN_LENGTH, lineBoxFromEndpoints } from './itemFactory';
  // 2026-05-20 figure UX 정리: rect/ellipse/line/free 는 canvas X 버튼 미제공
  // (Backspace / Cmd+Delete / ContextMenu Delete 로만 제거).
  import { strokeDashArray } from './strokeDash';
  import { projectPointToAngle } from './resizeConstraint';
  import PathHeadMarker from './PathHeadMarker.svelte';

  interface LineNodeData {
    id: string;
    x: number;
    y: number;
    w: number;
    h: number;
    visibility: boolean;
    locked: boolean;
    stroke: string;
    stroke_width: number;
    x2: number;
    y2: number;
    stroke_dash?: FigureStrokeDash;
    head_from?: Head;
    head_to?: Head;
    /** Canvas itemToNode 가 주입 — box-local 시작점. 4 방향 모두 처리. */
    _boxX1: number;
    _boxY1: number;
    /** Canvas itemToNode 가 주입 — box-local 끝점. */
    _boxX2: number;
    _boxY2: number;
    /** Canvas.svelte group selection proxy. Descendants must not show own controls. */
    group_selected?: boolean;
  }

  let {
    data,
  }: {
    data: LineNodeData;
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

  const { screenToFlowPosition } = useSvelteFlow();
  const isVisible = $derived(data.visibility !== false);
  const isLocked = $derived(data.locked === true);
  const isInM = $derived(sessionStore.M.has(data.id) && data.group_selected !== true);
  type Endpoint = 'start' | 'end';
  type DraftLine = { x: number; y: number; x2: number; y2: number };
  let draft = $state<DraftLine | null>(null);
  let pendingCommit = $state<DraftLine | null>(null);
  let activeEndpoint = $state<Endpoint | null>(null);
  let endpointShiftAngle = $state<number | null>(null);

  // Render 기준점은 편집 중에도 기존 SvelteFlow node bbox 로 고정한다.
  // draft endpoint 는 이 기준점 밖의 음수/초과 좌표가 될 수 있으며, SVG/DOM
  // overflow 를 visible 로 둬 Excalidraw/Figma 처럼 bbox 밖까지 자유롭게 편집한다.
  const nodeBoxLeft = $derived(Math.min(data.x, data.x2) - LINE_HIT_PADDING);
  const nodeBoxTop = $derived(Math.min(data.y, data.y2) - LINE_HIT_PADDING);
  const renderLine = $derived(draft ?? pendingCommit);
  const startX = $derived(renderLine === null ? data._boxX1 : renderLine.x - nodeBoxLeft);
  const startY = $derived(renderLine === null ? data._boxY1 : renderLine.y - nodeBoxTop);
  const endX = $derived(renderLine === null ? data._boxX2 : renderLine.x2 - nodeBoxLeft);
  const endY = $derived(renderLine === null ? data._boxY2 : renderLine.y2 - nodeBoxTop);

  // ADR-0018 D4 amend ① (batch-5) — stroke dash pattern.
  const svgDashArray = $derived(strokeDashArray(data.stroke_dash, data.stroke_width));
  const startHead = $derived(data.head_from ?? 'none');
  const endHead = $derived(data.head_to ?? 'none');
  const startMarkerId = $derived(`line-${data.id}-marker-start-${startHead}`);
  const endMarkerId = $derived(`line-${data.id}-marker-end-${endHead}`);
  const startMarkerRef = $derived(startHead === 'none' ? undefined : `url(#${startMarkerId})`);
  const endMarkerRef = $derived(endHead === 'none' ? undefined : `url(#${endMarkerId})`);

  // 2026-05-20 figure UX 정리 — 본 wrapper bbox 가 클릭 영역으로 잡히는 옛
  // 동작 (line 을 포함한 큰 사각형이 hit-test 흡수) 을 폐기. 시각적 line 은
  // pointer-events:none 으로 두고, 같은 좌표의 *invisible thick line* 을
  // hit-target 으로 둠 — 사용자가 line 근처만 클릭/cursor 변경 가능.
  //
  // hit-target 두께 = max(stroke_width + HIT_TOLERANCE_PX, MIN_HIT_PX).
  // 매우 얇은 stroke 도 안정적으로 hit 가능 + 너무 두꺼우면 SvelteFlow 의
  // marquee selection 영역과 시각 충돌. 2026-05-20 사용자 보고로 24px 까지
  // 확대 (옛 16px 가 좁다는 피드백).
  const HIT_TOLERANCE_PX = 20;
  const MIN_HIT_PX = 24;
  const hitStrokeWidth = $derived(
    Math.max(data.stroke_width + HIT_TOLERANCE_PX, MIN_HIT_PX),
  );

  function onEndpointDown(endpoint: Endpoint, e: PointerEvent): void {
    if (isLocked) return;
    e.preventDefault();
    e.stopPropagation();
    activeEndpoint = endpoint;
    endpointShiftAngle = null;
    draft = { x: data.x, y: data.y, x2: data.x2, y2: data.y2 };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    window.addEventListener('pointermove', onWindowPointerMove, { capture: true });
    window.addEventListener('pointerup', onWindowPointerUp, { capture: true });
    window.addEventListener('pointercancel', onWindowPointerCancel, { capture: true });
  }

  function moveEndpointToClient(clientX: number, clientY: number, shiftKey: boolean): void {
    if (activeEndpoint === null || draft === null) return;
    let flow = screenToFlowPosition({ x: clientX, y: clientY });
    if (shiftKey) {
      const fixed =
        activeEndpoint === 'start'
          ? { x: draft.x2, y: draft.y2 }
          : { x: draft.x, y: draft.y };
      if (endpointShiftAngle === null) {
        endpointShiftAngle = Math.atan2(flow.y - fixed.y, flow.x - fixed.x);
      }
      flow = projectPointToAngle(fixed, flow, endpointShiftAngle);
    } else {
      endpointShiftAngle = null;
    }
    draft =
      activeEndpoint === 'start'
        ? { ...draft, x: flow.x, y: flow.y }
        : { ...draft, x2: flow.x, y2: flow.y };
  }

  function removeWindowListeners(): void {
    window.removeEventListener('pointermove', onWindowPointerMove, { capture: true });
    window.removeEventListener('pointerup', onWindowPointerUp, { capture: true });
    window.removeEventListener('pointercancel', onWindowPointerCancel, { capture: true });
  }

  function onWindowPointerMove(e: PointerEvent): void {
    if (activeEndpoint === null) return;
    e.preventDefault();
    e.stopPropagation();
    moveEndpointToClient(e.clientX, e.clientY, e.shiftKey);
  }

  function onWindowPointerUp(e: PointerEvent): void {
    if (activeEndpoint === null || draft === null) return;
    e.preventDefault();
    e.stopPropagation();
    moveEndpointToClient(e.clientX, e.clientY, e.shiftKey);
    const next = draft;
    activeEndpoint = null;
    endpointShiftAngle = null;
    draft = null;
    pendingCommit = next;
    removeWindowListeners();
    void commitLine(next);
  }

  function onWindowPointerCancel(e?: PointerEvent): void {
    e?.preventDefault();
    e?.stopPropagation();
    activeEndpoint = null;
    endpointShiftAngle = null;
    draft = null;
    removeWindowListeners();
  }

  // Drag 중 component unmount (session switch / item delete / layout reload)
  // 흐름에서 window listener 누수 차단. pointerup/cancel 정상 종료 시는 함수가
  // 자체적으로 remove 하므로 idempotent.
  onDestroy(() => {
    if (activeEndpoint !== null) {
      removeWindowListeners();
      activeEndpoint = null;
      endpointShiftAngle = null;
      draft = null;
      pendingCommit = null;
    }
  });

  async function commitLine(next: DraftLine): Promise<void> {
    if (Math.hypot(next.x2 - next.x, next.y2 - next.y) < LINE_MIN_LENGTH) {
      toastStore.show({
        message: 'Line is too short. Drag an endpoint at least 5px.',
        tone: 'warning',
      });
      pendingCommit = null;
      return;
    }
    if (sessionStore.active === null) {
      pendingCommit = null;
      return;
    }
    const box = lineBoxFromEndpoints(
      { x: next.x, y: next.y },
      { x: next.x2, y: next.y2 },
    );
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'line'
            ? ({
                ...it,
                x: next.x,
                y: next.y,
                x2: next.x2,
                y2: next.y2,
                w: box.w,
                h: box.h,
              } as LineItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Line edit aborted — session reconnect failed.',
        failMessage: 'Line edit failed',
      },
    );
    pendingCommit = null;
  }
</script>

{#if isVisible}
  <div
    class="line-node"
    class:m-single={isInM}
    class:locked={isLocked}
    style="width: {data.w}px; height: {data.h}px;"
    role="group"
    aria-label="Line item"
  >
    <svg
      width={data.w}
      height={data.h}
      viewBox={`0 0 ${data.w} ${data.h}`}
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
      <!-- Visible stroke — pointer-events:none, hit-target line 이 catch.
           ADR-0005 D10 — vector-effect=non-scaling-stroke. drag-resize 중
           viewBox stale 의 stroke stretch 회피. -->
      <line
        x1={startX}
        y1={startY}
        x2={endX}
        y2={endY}
        stroke={data.stroke}
        stroke-width={data.stroke_width}
        stroke-dasharray={svgDashArray}
        stroke-linecap="round"
        marker-start={startMarkerRef}
        marker-end={endMarkerRef}
        vector-effect="non-scaling-stroke"
        pointer-events="none"
      />
      <!--
        Invisible hit-target — line 근처에서만 click / cursor 변경. SVG
        `pointer-events="stroke"` 는 paint 의 가시 여부와 무관하게 stroke
        geometry 안의 hit 를 catch. transparent paint 라 시각 없음. hit zone
        의 user-units 안정 보장 위해 non-scaling-stroke 적용 (drag 중에도
        24px hit band 유지).
      -->
      <line
        x1={startX}
        y1={startY}
        x2={endX}
        y2={endY}
        stroke="transparent"
        stroke-width={hitStrokeWidth}
        stroke-linecap="round"
        vector-effect="non-scaling-stroke"
        pointer-events="stroke"
        class="line-hit"
      />
    </svg>
    {#if isInM && !isLocked}
      <button
        type="button"
        class="endpoint start"
        style="left: {startX}px; top: {startY}px;"
        aria-label="Move line start"
        onpointerdown={(e) => onEndpointDown('start', e)}
      ></button>
      <button
        type="button"
        class="endpoint end"
        style="left: {endX}px; top: {endY}px;"
        aria-label="Move line end"
        onpointerdown={(e) => onEndpointDown('end', e)}
      ></button>
    {/if}
  </div>
{/if}

<style>
  /*
   * 2026-05-20 hit-test 정리 — wrapper bbox 가 line 을 둘러싼 사각형 전체
   * 에서 click / cursor 변경을 catch 하던 옛 동작을 폐기. wrapper 는
   * pointer-events:none 으로 통과, SVG 안의 invisible hit-target line 만
   * authoritative.
   *  - .line-node       → pointer-events:none (bbox 통과)
   *  - svg              → 상속 받지만 child line 의 pointer-events attribute 가 우선
   *  - .line-hit  (SVG) → pointer-events="stroke" + transparent paint
   *  - .endpoint        → 자체 pointer-events:auto (button 의 hit-target)
   */
  .line-node {
    box-sizing: border-box;
    position: relative;
    overflow: visible;
    pointer-events: none;
  }

  .line-node.m-single {
    outline: none;
  }

  .line-node.locked {
    cursor: default;
  }

  .line-node svg {
    display: block;
    overflow: visible;
  }

  /* Invisible hit-target — cursor 만 시각 단서. */
  .line-hit {
    cursor: pointer;
  }
  .line-node.locked .line-hit {
    cursor: default;
  }

  .endpoint {
    position: absolute;
    width: var(--canvas-scaler-size, 10px);
    height: var(--canvas-scaler-size, 10px);
    padding: 0;
    border: var(--canvas-scaler-border, 1.5px) solid var(--color-accent);
    border-radius: 999px;
    background: var(--color-surface);
    transform: translate(-50%, -50%) scale(calc(1 / var(--canvas-zoom, 1)));
    transform-origin: center;
    cursor: crosshair;
    pointer-events: auto;
  }

  .endpoint::before {
    content: '';
    position: absolute;
    inset: -4px;
    border-radius: 999px;
  }

  .endpoint:hover {
    background: var(--color-accent);
  }
</style>
