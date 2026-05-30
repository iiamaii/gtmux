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
  //
  // ADR-0005 D10 (2026-05-22) — `vector-effect="non-scaling-stroke"` 필수.
  // 이유: drag-resize 중 svelte state (`data.w`/`data.h`) 는 onResizeEnd 까지
  // 갱신 X — viewBox 는 stale + SVG element 는 wrapper 의 새 크기 채움 +
  // preserveAspectRatio="none" → viewBox 내용물이 stretch → stroke 가 비례
  // 따라 두꺼워짐/얇아짐. non-scaling-stroke 가 stroke 의 paint width 를
  // viewport / transform 무관하게 user units 고정 → drag 중에도 안정. dash
  // pattern 도 stretch 안 됨 (부수 개선).

  import { NodeResizer, useSvelteFlow } from '@xyflow/svelte';
  import InlineEditTextarea from '$lib/common/InlineEditTextarea.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import type { CanvasItem, EllipseItem, FigureStrokeDash, FontFamily, FontWeight, RectItem, TextAlign, TextVerticalAlign } from '$lib/types/canvas';
  import { deriveLabel, shouldDeriveLabel } from './labelDerive';
  import { fontFamilyVar } from './fontFamily';
  import { fontWeightCss, textDecorationCss } from './textStyle';
  import {
    constrainResizeAspectIfShift,
    scheduleLiveAspectResize,
  } from './resizeConstraint';
  import { strokeDashArray } from './strokeDash';

  interface ShapeNodeData {
    id: string;
    type: 'rect' | 'ellipse';
    x: number;
    y: number;
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
    text?: string;
    font_size?: number;
    color?: string;
    text_align?: TextAlign;
    text_vertical_align?: TextVerticalAlign;
    font_weight?: FontWeight;
    italic?: boolean;
    underline?: boolean;
    strikethrough?: boolean;
    font_family?: FontFamily;
    label?: string;
    label_auto?: boolean;
    /** Canvas.svelte group selection proxy. Descendants must not show own controls. */
    group_selected?: boolean;
  }

  let {
    data,
    width,
    height,
  }: {
    data: ShapeNodeData;
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
  const isEllipse = $derived(data.type === 'ellipse');
  const liveW = $derived(width ?? data.w);
  const liveH = $derived(height ?? data.h);

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
    const base = Math.min(liveW, liveH) * 0.15;
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
  const shapeText = $derived(data.text ?? '');
  const textAlign = $derived(data.text_align ?? 'center');
  const textVerticalAlign = $derived(data.text_vertical_align ?? 'middle');
  const textFontSize = $derived(data.font_size ?? 14);
  const textColor = $derived(data.color ?? 'var(--color-fg)');
  const textFontWeight = $derived(fontWeightCss(data.font_weight));
  const textFontStyle = $derived(data.italic === true ? 'italic' : 'normal');
  const textDecoration = $derived(textDecorationCss(data));
  const textFontFamily = $derived(fontFamilyVar(data.font_family));
  let editing = $state(false);

  type ResizeParams = { x: number; y: number; width: number; height: number };
  const SHAPE_MIN_SIZE = 20;

  function applyLiveResize(next: ResizeParams): void {
    updateNode(data.id, (node) => ({
      position: { ...node.position, x: next.x, y: next.y },
      width: Math.max(SHAPE_MIN_SIZE, next.width),
      height: Math.max(SHAPE_MIN_SIZE, next.height),
    }));
  }

  function onResize(event: unknown, params: ResizeParams): void {
    scheduleLiveAspectResize(
      event,
      params,
      data,
      data.w / data.h,
      SHAPE_MIN_SIZE,
      SHAPE_MIN_SIZE,
      applyLiveResize,
    );
  }

  async function onResizeEnd(event: unknown, params: ResizeParams): Promise<void> {
    const constrained = constrainResizeAspectIfShift(
      event,
      params,
      data,
      data.w / data.h,
      SHAPE_MIN_SIZE,
      SHAPE_MIN_SIZE,
    );
    const nextW = Math.max(SHAPE_MIN_SIZE, constrained.width);
    const nextH = Math.max(SHAPE_MIN_SIZE, constrained.height);
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && (it.type === 'rect' || it.type === 'ellipse')
            ? ({
                ...it,
                x: constrained.x,
                y: constrained.y,
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

  function isControlSurface(e: MouseEvent): boolean {
    const target = e.target;
    return target instanceof Element && target.closest('.svelte-flow__resize-control') !== null;
  }

  function onDblClick(e: MouseEvent): void {
    if (isLocked || isControlSurface(e)) return;
    e.stopPropagation();
    if (sessionStore.consumeSuppressedTextEditDblClick(data.id)) return;
    editing = true;
  }

  async function onCommit(next: string): Promise<void> {
    if (next === shapeText) {
      editing = false;
      return;
    }
    if (sessionStore.active === null) {
      editing = false;
      return;
    }
    const shouldDerive = shouldDeriveLabel(data.label_auto, data.label, next);
    const result = await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && (it.type === 'rect' || it.type === 'ellipse')
            ? (shouldDerive
                ? ({
                    ...it,
                    text: next,
                    label: deriveLabel(next),
                    label_auto: false,
                  } as RectItem | EllipseItem)
                : ({ ...it, text: next } as RectItem | EllipseItem))
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
</script>

{#if isVisible}
  <div
    class="shape-node"
    class:m-single={isInM}
    class:locked={isLocked}
    role="group"
    aria-label={`${data.type} item`}
    ondblclick={onDblClick}
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked}
      minWidth={SHAPE_MIN_SIZE}
      minHeight={SHAPE_MIN_SIZE}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResize}
      {onResizeEnd}
    />
    <svg
      class="shape-svg"
      width="100%"
      height="100%"
      viewBox={`0 0 ${liveW} ${liveH}`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      {#if isEllipse}
        <ellipse
          cx={liveW / 2}
          cy={liveH / 2}
          rx={Math.max(0, liveW / 2 - inset)}
          ry={Math.max(0, liveH / 2 - inset)}
          fill={svgFill}
          stroke={svgStroke}
          stroke-width={svgStrokeWidth}
          stroke-dasharray={svgDashArray}
          vector-effect="non-scaling-stroke"
          pointer-events={svgPointerEvents}
        />
        {#if needsBorderHitTarget}
          <!-- Invisible thick hit-target — fill-off ellipse 의 border 클릭 영역 확대. -->
          <ellipse
            cx={liveW / 2}
            cy={liveH / 2}
            rx={Math.max(0, liveW / 2 - inset)}
            ry={Math.max(0, liveH / 2 - inset)}
            fill="none"
            stroke="transparent"
            stroke-width={hitStrokeWidth}
            vector-effect="non-scaling-stroke"
            pointer-events="stroke"
            class="shape-hit"
          />
        {/if}
      {:else}
        <rect
          x={inset}
          y={inset}
          width={Math.max(0, liveW - inset * 2)}
          height={Math.max(0, liveH - inset * 2)}
          rx={cornerRadius}
          ry={cornerRadius}
          fill={svgFill}
          stroke={svgStroke}
          stroke-width={svgStrokeWidth}
          stroke-dasharray={svgDashArray}
          vector-effect="non-scaling-stroke"
          pointer-events={svgPointerEvents}
        />
        {#if needsBorderHitTarget}
          <!-- Invisible thick hit-target — fill-off rect 의 border 클릭 영역 확대. -->
          <rect
            x={inset}
            y={inset}
            width={Math.max(0, liveW - inset * 2)}
            height={Math.max(0, liveH - inset * 2)}
            rx={cornerRadius}
            ry={cornerRadius}
            fill="none"
            stroke="transparent"
            stroke-width={hitStrokeWidth}
            vector-effect="non-scaling-stroke"
            pointer-events="stroke"
            class="shape-hit"
          />
        {/if}
      {/if}
    </svg>
    <div
      class="shape-text"
      class:editing
      class:v-top={textVerticalAlign === 'top'}
      class:v-middle={textVerticalAlign === 'middle'}
      class:v-bottom={textVerticalAlign === 'bottom'}
      style="font-size: {textFontSize}px; color: {textColor}; text-align: {textAlign}; font-family: {textFontFamily}; font-weight: {textFontWeight}; font-style: {textFontStyle}; --shape-text-decoration: {textDecoration};"
    >
      <div class="shape-text-cell">
        {#if editing}
          <InlineEditTextarea
            value={shapeText}
            editing={true}
            allowEmpty={true}
            placeholder=""
            class="shape-text-edit"
            plain={true}
            rows={1}
            selectOnFocus={shapeText.length === 0}
            textAlign={textAlign}
            commitOnEnter={true}
            onCommit={(next: string) => void onCommit(next)}
            onCancel={() => (editing = false)}
          />
        {:else if shapeText.length > 0}
          <span class="shape-text-body">{shapeText}</span>
        {/if}
      </div>
    </div>
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

  .shape-node.m-single {
    outline: none;
  }

  .shape-node.locked {
    cursor: default;
  }

  .shape-svg {
    position: absolute;
    inset: 0;
    display: block;
    overflow: visible;
    z-index: 0;
  }

  /* Invisible thick hit-target — fill-off rect/ellipse 의 border 근처 cursor 시각 단서. */
  .shape-hit {
    cursor: pointer;
  }
  .shape-node.locked .shape-hit {
    cursor: default;
  }

  .shape-text {
    box-sizing: border-box;
    position: absolute;
    inset: 8px;
    z-index: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    pointer-events: none;
    line-height: 1.2;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .shape-text.v-top {
    justify-content: flex-start;
  }

  .shape-text.v-middle {
    justify-content: center;
  }

  .shape-text.v-bottom {
    justify-content: flex-end;
  }

  .shape-text.editing {
    overflow: visible;
    pointer-events: auto;
  }

  .shape-text-cell {
    width: 100%;
    line-height: 1.2;
  }

  .shape-text-body {
    display: block;
    width: 100%;
    pointer-events: auto;
    text-decoration: var(--shape-text-decoration);
    text-decoration-skip-ink: auto;
  }

  :global(.shape-text .shape-text-edit) {
    box-sizing: border-box;
    display: block;
    width: 100%;
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
    line-height: 1.2;
    text-decoration: var(--shape-text-decoration);
    text-decoration-skip-ink: auto;
    white-space: pre-wrap;
    word-break: break-word;
    overflow: hidden;
  }
</style>
