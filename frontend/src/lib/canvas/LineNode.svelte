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
  import type { CanvasItem, LineItem } from '$lib/types/canvas';
  import { LINE_HIT_PADDING, LINE_MIN_LENGTH, lineBoxFromEndpoints } from './itemFactory';

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
    /** Canvas itemToNode 가 주입 — box-local 시작점. 4 방향 모두 처리. */
    _boxX1: number;
    _boxY1: number;
    /** Canvas itemToNode 가 주입 — box-local 끝점. */
    _boxX2: number;
    _boxY2: number;
  }

  let {
    data,
    selected = false,
  }: {
    data: LineNodeData;
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

  const { screenToFlowPosition } = useSvelteFlow();
  const isVisible = $derived(data.visibility !== false);
  const isLocked = $derived(data.locked === true);
  const isInM = $derived(selected || sessionStore.M.has(data.id));
  type Endpoint = 'start' | 'end';
  type DraftLine = { x: number; y: number; x2: number; y2: number };
  let draft = $state<DraftLine | null>(null);
  let pendingCommit = $state<DraftLine | null>(null);
  let activeEndpoint = $state<Endpoint | null>(null);

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

  function onEndpointDown(endpoint: Endpoint, e: PointerEvent): void {
    if (isLocked) return;
    e.preventDefault();
    e.stopPropagation();
    activeEndpoint = endpoint;
    draft = { x: data.x, y: data.y, x2: data.x2, y2: data.y2 };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    window.addEventListener('pointermove', onWindowPointerMove, { capture: true });
    window.addEventListener('pointerup', onWindowPointerUp, { capture: true });
    window.addEventListener('pointercancel', onWindowPointerCancel, { capture: true });
  }

  function moveEndpointToClient(clientX: number, clientY: number): void {
    if (activeEndpoint === null || draft === null) return;
    const flow = screenToFlowPosition({ x: clientX, y: clientY });
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
    moveEndpointToClient(e.clientX, e.clientY);
  }

  function onWindowPointerUp(e: PointerEvent): void {
    if (activeEndpoint === null || draft === null) return;
    e.preventDefault();
    e.stopPropagation();
    moveEndpointToClient(e.clientX, e.clientY);
    const next = draft;
    activeEndpoint = null;
    draft = null;
    pendingCommit = next;
    removeWindowListeners();
    void commitLine(next);
  }

  function onWindowPointerCancel(e?: PointerEvent): void {
    e?.preventDefault();
    e?.stopPropagation();
    activeEndpoint = null;
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
      <line
        x1={startX}
        y1={startY}
        x2={endX}
        y2={endY}
        stroke={data.stroke}
        stroke-width={data.stroke_width}
        stroke-linecap="round"
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
  .line-node {
    box-sizing: border-box;
    position: relative;
    overflow: visible;
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
    pointer-events: none;
  }

  .endpoint {
    position: absolute;
    width: 10px;
    height: 10px;
    padding: 0;
    border: 1.5px solid var(--color-accent);
    border-radius: 999px;
    background: var(--color-surface);
    transform: translate(-50%, -50%);
    cursor: crosshair;
    pointer-events: auto;
  }

  .endpoint:hover {
    background: var(--color-accent);
  }
</style>
