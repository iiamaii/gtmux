<script lang="ts">
  /**
   * FreeDrawNode — SvelteFlow custom node for `type: "free_draw"` (ADR-0018 D4).
   *
   * Drag-to-stroke 의 결과 — `points` 는 flow-coord sequence. SvelteFlow 의
   * Node wrapper 는 item.x/y 의 좌상단 부터 w×h 박스를 차지하므로, 본 렌더러는
   * 점들을 *node-local* 좌표 (point.x - item.x, point.y - item.y) 로 변환해
   * `<svg viewBox="0 0 w h">` 안에 `<path>` 그린다.
   *
   * Resize 는 미지원 (수기 stroke 의 비례 변경은 의미 약함 — P2+ point
   * simplification 이후 결정).
   */

  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import type { Point } from '$lib/types/canvas';
  // 2026-05-20 figure UX 정리: rect/ellipse/line/free 는 canvas X 버튼 미제공
  // (Backspace / Cmd+Delete / ContextMenu Delete 로만 제거).

  interface FreeDrawNodeData {
    id: string;
    x: number;
    y: number;
    w: number;
    h: number;
    visibility: boolean;
    locked: boolean;
    stroke: string;
    stroke_width: number;
    points: Point[];
  }

  let {
    data,
    selected = false,
  }: {
    data: FreeDrawNodeData;
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

  /** flow-coord points → node-local SVG path. */
  const localPath = $derived.by((): string => {
    const pts = data.points ?? [];
    if (pts.length === 0) return '';
    return pts
      .map((p, i) => `${i === 0 ? 'M' : 'L'}${p.x - data.x} ${p.y - data.y}`)
      .join(' ');
  });
</script>

{#if isVisible}
  <div
    class="free-draw-node"
    class:m-single={isInM}
    class:locked={isLocked}
    style="width: 100%; height: 100%;"
    role="img"
    aria-label="Free drawing"
  >
    <svg
      class="free-draw-svg"
      width="100%"
      height="100%"
      viewBox={`0 0 ${data.w} ${data.h}`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <path
        d={localPath}
        fill="none"
        stroke={data.stroke}
        stroke-width={data.stroke_width}
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
  </div>
{/if}

<style>
  .free-draw-node {
    box-sizing: border-box;
    pointer-events: auto;
    position: relative;
    overflow: visible;
  }

  .free-draw-node.m-single {
    outline: none;
  }

  .free-draw-node.locked {
    cursor: default;
  }

  .free-draw-svg {
    display: block;
    overflow: visible;
  }
</style>
