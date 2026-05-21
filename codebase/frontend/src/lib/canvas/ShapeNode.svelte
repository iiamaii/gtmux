<script lang="ts">
  // ShapeNode — rect / ellipse 공용 renderer (ADR-0018 D4, batch-5 amend ①).
  //
  // 두 type 의 payload 동일: stroke / fill / stroke_width
  //   + fill_enabled / stroke_enabled / stroke_dash (batch-5 amend ①)
  //   + rect 만 corner_rounded (batch-5 Grill #5 — 자동 radius FE 계산).
  //
  // SVG migration: 옛 CSS border 기반 → inline SVG `<rect>` / `<ellipse>`.
  // 이유:
  //  - dash_dot pattern 표현 (CSS `border-style` 는 dashed / dotted 만)
  //  - SVG `pointer-events` attribute 로 fill/stroke 별 hit-test 분기
  //  - corner_rounded + dash_dot 동시 표현 (CSS border-radius + stroke-dasharray
  //    결합 시 corner rendering 깨짐)
  //
  // SvelteFlow wrapper div (`.shape-node`) 는 그대로 NodeResizer 의 layer host
  // 역할. SVG element 가 fill 영역 / stroke ring 의 시각 + hit-test 책임.
  // 2026-05-20 figure UX 정리: rect/ellipse/line/free 는 canvas X 버튼 미제공
  // (Backspace / Cmd+Delete / ContextMenu 의 Delete 로만 제거 — sketching 도구
  // 의 일관된 axis-aligned 시각).

  import { NodeResizer } from '@xyflow/svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import type { CanvasItem, EllipseItem, FigureStrokeDash, RectItem } from '$lib/types/canvas';
  import { strokeDashArray } from './strokeDash';

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
    fill_enabled?: boolean;
    stroke_enabled?: boolean;
    corner_rounded?: boolean;
    stroke_dash?: FigureStrokeDash;
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

  // ADR-0018 D4 amend ① — fill/stroke on-off + dash.
  const fillEnabled = $derived(data.fill_enabled !== false);
  const strokeEnabled = $derived(data.stroke_enabled !== false);
  const strokeWidth = $derived(data.stroke_width);

  // SVG attribute values. 둘 다 off 면 fill/stroke 모두 "none" → 완전 invisible.
  const svgFill = $derived(fillEnabled ? data.fill : 'none');
  const svgStroke = $derived(strokeEnabled ? data.stroke : 'none');
  const svgStrokeWidth = $derived(strokeEnabled ? strokeWidth : 0);
  const svgDashArray = $derived(strokeEnabled ? strokeDashArray(data.stroke_dash, strokeWidth) : 'none');

  // pointer-events 분기 (R1 hit-test):
  //  - fill on  + stroke on  → visiblePainted (둘 다 hit)
  //  - fill on  + stroke off → visibleFill    (fill 만 hit)
  //  - fill off + stroke on  → visibleStroke  (stroke 만 hit — ring band)
  //  - 둘 다 off              → none           (완전 non-hittable; M 안 진입은 NodeResizer 로 인지)
  const svgPointerEvents = $derived.by(() => {
    if (!fillEnabled && !strokeEnabled) return 'none';
    if (!fillEnabled) return 'visibleStroke';
    if (!strokeEnabled) return 'visibleFill';
    return 'visiblePainted';
  });

  // Grill #5 — rect 의 corner_rounded 가 true 면 FE 가 자동 radius 계산:
  //   clamp(min(w, h) * 0.15, 4, 16)
  // ellipse 는 SVG <ellipse> 가 native 라 corner_radius 무관.
  const cornerRadius = $derived.by(() => {
    if (data.type !== 'rect' || data.corner_rounded !== true) return 0;
    const base = Math.min(data.w, data.h) * 0.15;
    return Math.max(4, Math.min(16, base));
  });

  // SVG inset — stroke 중심선이 viewBox edge 와 일치하지 않게 stroke_width / 2
  // 만큼 안쪽으로. stroke_enabled=false 면 inset 0 (fill 이 box 전체).
  const inset = $derived(strokeEnabled ? strokeWidth / 2 : 0);

  // 2026-05-20 — fill-off + stroke-on 일 때 stroke 두께 2px 만으로는 border
  // 클릭 hit 영역이 너무 좁다는 사용자 보고. LineNode 와 동일한 invisible
  // thick hit-target overlay 패턴 — 같은 geometry 의 transparent stroke 를
  // 24px 두께로 덮어 border 근처에서 catch. fill 이 on 일 땐 fill 자체가
  // 내부 전체 hit 라 필요 없음.
  const HIT_TOLERANCE_PX = 20;
  const MIN_HIT_PX = 24;
  const needsBorderHitTarget = $derived(!fillEnabled && strokeEnabled);
  const hitStrokeWidth = $derived(Math.max(strokeWidth + HIT_TOLERANCE_PX, MIN_HIT_PX));

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
  <!--
    Wrapper pointer-events 는 fillEnabled 분기:
     - fillEnabled=true → 'auto' (div + SVG 둘 다 hit; interior 면 div 가 catch,
       stroke ring 도 SVG 가 catch). 옛 동작과 정합 — 회귀 0.
     - fillEnabled=false → 'none' (div 통과, SVG element 의 pointer-events 만
       authoritative). interior 클릭은 뒤 panel/canvas 로 전달. SVG 의 ring 만
       hit 가능 (stroke_enabled=true 면).

    [정정 2026-05-21] 직전 버전 comment 의 "NodeResizer handle 은 자체
    pointer-events:all 이라 wrapper none 와 무관" 가정은 *틀렸음*. xyflow 의
    `.svelte-flow__resize-control` 의 CSS 는 `position: absolute` 만 명시 —
    `pointer-events` 미지정이라 inheritable property 가 wrapper 로부터
    `none` 상속 → fill-off shape 의 resize handle 이 클릭 불가가 되어
    scale 변경 불가 회귀. `position: absolute` 는 시각 위치만 분리할 뿐 DOM
    상속은 유지. 본 component 의 `:global(.svelte-flow__resize-control)`
    rule 로 resize handle 에 한해 `pointer-events: all` 명시 override.
  -->
  <div
    class="shape-node"
    class:m-single={isInM}
    class:locked={isLocked}
    class:pass-through={!fillEnabled}
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
    <svg
      class="shape-svg"
      width="100%"
      height="100%"
      viewBox={`0 0 ${data.w} ${data.h}`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      {#if isEllipse}
        <ellipse
          cx={data.w / 2}
          cy={data.h / 2}
          rx={Math.max(0, data.w / 2 - inset)}
          ry={Math.max(0, data.h / 2 - inset)}
          fill={svgFill}
          stroke={svgStroke}
          stroke-width={svgStrokeWidth}
          stroke-dasharray={svgDashArray}
          pointer-events={svgPointerEvents}
        />
        {#if needsBorderHitTarget}
          <!-- Invisible thick hit-target — fill-off ellipse 의 border 클릭 영역 확대. -->
          <ellipse
            cx={data.w / 2}
            cy={data.h / 2}
            rx={Math.max(0, data.w / 2 - inset)}
            ry={Math.max(0, data.h / 2 - inset)}
            fill="none"
            stroke="transparent"
            stroke-width={hitStrokeWidth}
            pointer-events="stroke"
            class="shape-hit"
          />
        {/if}
      {:else}
        <rect
          x={inset}
          y={inset}
          width={Math.max(0, data.w - inset * 2)}
          height={Math.max(0, data.h - inset * 2)}
          rx={cornerRadius}
          ry={cornerRadius}
          fill={svgFill}
          stroke={svgStroke}
          stroke-width={svgStrokeWidth}
          stroke-dasharray={svgDashArray}
          pointer-events={svgPointerEvents}
        />
        {#if needsBorderHitTarget}
          <!-- Invisible thick hit-target — fill-off rect 의 border 클릭 영역 확대. -->
          <rect
            x={inset}
            y={inset}
            width={Math.max(0, data.w - inset * 2)}
            height={Math.max(0, data.h - inset * 2)}
            rx={cornerRadius}
            ry={cornerRadius}
            fill="none"
            stroke="transparent"
            stroke-width={hitStrokeWidth}
            pointer-events="stroke"
            class="shape-hit"
          />
        {/if}
      {/if}
    </svg>
  </div>
{/if}

<style>
  .shape-node {
    box-sizing: border-box;
    position: relative;
    width: 100%;
    height: 100%;
    overflow: visible;
  }

  /* Fill off — wrapper pass-through. SVG element 의 pointer-events attribute 가 authoritative hit-test. */
  .shape-node.pass-through {
    pointer-events: none;
  }

  /* xyflow NodeResizer handle 의 inherit 차단 — fill-off shape 도 resize 가능.
     `.svelte-flow__resize-control` 는 xyflow internal class (style.css 기본은
     position:absolute 만 명시, pointer-events 미지정). wrapper 의 `none` 이
     inherit 되면 handle 클릭 불가 → scale 변경 불가 회귀 (2026-05-21 fix). */
  .shape-node.pass-through :global(.svelte-flow__resize-control) {
    pointer-events: all;
  }

  .shape-node.m-single {
    outline: none;
  }

  .shape-node.locked {
    cursor: default;
  }

  .shape-svg {
    display: block;
    overflow: visible;
  }

  /* Invisible thick hit-target — fill-off rect/ellipse 의 border 근처 cursor 시각 단서. */
  .shape-hit {
    cursor: pointer;
  }
  .shape-node.locked .shape-hit {
    cursor: default;
  }
</style>
