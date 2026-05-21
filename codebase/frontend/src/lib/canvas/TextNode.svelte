<script lang="ts">
  // TextNode — SvelteFlow custom node for `type: "text"` (ADR-0018 D4).
  //
  // 사용자 free text. minimal rendering — body 만. Inline edit 은 P1+ 의 별
  // InlineEditTextarea wiring (0033 §8.2 InlineEditField consumer wire).
  //
  // 현재 단계: 더블 클릭 → InlineEditTextarea (body) → commit 시 mutateLayout.

  import { untrack } from 'svelte';
  import { NodeResizer } from '@xyflow/svelte';
  import InlineEditTextarea from '$lib/common/InlineEditTextarea.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import type { CanvasItem, FontWeight, TextAlign, TextItem, TextVerticalAlign } from '$lib/types/canvas';
  import CanvasCloseButton from './CanvasCloseButton.svelte';
  // 텍스트 정렬 UI 는 ToolbarSubbar (lib/toolbar/ToolbarSubbar.svelte) 로 이전.
  // 본 컴포넌트는 더 이상 alignment toolbar 를 그리지 않는다.

  interface TextNodeData {
    id: string;
    x: number;
    y: number;
    w: number;
    h: number;
    z: number;
    visibility: boolean;
    locked: boolean;
    text: string;
    font_size: number;
    text_align?: TextAlign;
    text_vertical_align?: TextVerticalAlign;
    color: string;
    label?: string;
    /** batch-5 R3 — font weight (Light/Normal/Bold). default 'normal'. */
    font_weight?: FontWeight;
    /** batch-5 R3 — italic toggle. default false. */
    italic?: boolean;
    /** batch-5 R3 — underline toggle. default false. */
    underline?: boolean;
    /** batch-5 R3 — strikethrough toggle. default false. */
    strikethrough?: boolean;
  }

  // R7 (batch-5 Grill #18) — label auto-derive 알고리즘. 첫 줄만 + trim +
  // 기존 label cap (4096B, ADR-0018 D8) 자연 활용. 4 KB slice 는 byte 가 아닌
  // char 기준 (byte-cap 은 BE validation 이 강제) — UI-friendly conservative.
  const TEXT_LABEL_CHAR_CAP = 4000;
  function deriveLabel(text: string): string {
    const firstLine = text.split('\n', 1)[0] ?? '';
    return firstLine.trim().slice(0, TEXT_LABEL_CHAR_CAP);
  }

  let {
    data,
    selected = false,
  }: {
    data: TextNodeData;
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
  const textAlign = $derived(data.text_align ?? 'center');
  const textVerticalAlign = $derived(data.text_vertical_align ?? 'middle');

  // batch-5 R3 — font weight / italic / underline / strikethrough.
  // FontWeight: light=300 / normal=400 / bold=700 (Grill #6, 3-bucket).
  const fontWeight = $derived(data.font_weight ?? 'normal');
  const fontWeightCss = $derived(
    fontWeight === 'light' ? 300 : fontWeight === 'bold' ? 700 : 400,
  );
  const fontStyleCss = $derived(data.italic === true ? 'italic' : 'normal');
  const textDecorationCss = $derived.by(() => {
    const parts: string[] = [];
    if (data.underline === true) parts.push('underline');
    if (data.strikethrough === true) parts.push('line-through');
    return parts.length === 0 ? 'none' : parts.join(' ');
  });

  let editing = $state(false);
  const minTextHeight = $derived(Math.max(16, Math.ceil(data.font_size)));
  type ResizeParams = { x: number; y: number; width: number; height: number };

  // R7 (batch-5) — text item spawn 직후 auto-edit 진입. itemFactory 의 성공
  // path 가 sessionStore.justSpawnedTextId 를 set. mount 시 self id 와 일치
  // 하면 editing=true + flag clear (untrack 으로 read/write 분리, $effect 의
  // dependency 추적 제외).
  $effect(() => {
    if (sessionStore.justSpawnedTextId === data.id) {
      untrack(() => {
        editing = true;
        sessionStore.justSpawnedTextId = null;
      });
    }
  });

  function onDblClick(e: MouseEvent): void {
    if (isLocked) return;
    e.stopPropagation();
    editing = true;
  }

  async function onCommit(next: string): Promise<void> {
    if (next === data.text) {
      editing = false;
      return;
    }
    if (sessionStore.active === null) {
      editing = false;
      return;
    }
    // R7 (batch-5 Grill #18) — label-empty trigger derive. label 이 비어있고
    // next 가 비지 않은 경우에만 deriveLabel(next) 로 갱신. 이후 사용자가
    // Inspector 에서 label 을 따로 입력하면 자동 derive 가 비활성 (자율성).
    const curLabel = data.label ?? '';
    const shouldDerive = curLabel === '' && next.length > 0;
    // Inspector hot-path 와 같은 패턴: optimisticMutation 으로 commit 즉시
    // 반영 + PUT 실패 시 priorSnapshot 으로 자동 rollback. server 부하 변화
    // 0 — InlineEditTextarea 가 이미 commit-based (Enter/blur 1회).
    const result = await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'text'
            ? (shouldDerive
                ? ({ ...it, text: next, label: deriveLabel(next) } as TextItem)
                : ({ ...it, text: next } as TextItem))
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

  async function onResizeEnd(_event: unknown, params: ResizeParams): Promise<void> {
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'text'
            ? ({ ...it, x: params.x, y: params.y, w: Math.max(120, params.width), h: Math.max(minTextHeight, params.height) } as TextItem)
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
    class="text-node"
    class:m-single={isInM}
    class:locked={isLocked}
    style="width: 100%; height: 100%; font-size: {data.font_size}px; color: {data.color}; text-align: {textAlign}; font-weight: {fontWeightCss}; font-style: {fontStyleCss}; text-decoration: {textDecorationCss};"
    role="group"
    aria-label="Text item"
    ondblclick={onDblClick}
  >
      <NodeResizer
        nodeId={data.id}
        isVisible={isInM && !isLocked}
        minWidth={120}
        minHeight={minTextHeight}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeEnd}
    />
    <CanvasCloseButton id={data.id} disabled={isLocked} />
    <div
      class="text-content"
      class:editing
      class:v-top={textVerticalAlign === 'top'}
      class:v-middle={textVerticalAlign === 'middle'}
      class:v-bottom={textVerticalAlign === 'bottom'}
    >
      <div class="text-cell">
        {#if editing}
          <InlineEditTextarea
            value={data.text}
            editing={true}
            allowEmpty={true}
            placeholder="Text…"
            class="text-edit"
            plain={true}
            rows={1}
            selectOnFocus={data.text.length === 0}
            textAlign={textAlign}
            onCommit={(next: string) => void onCommit(next)}
            onCancel={() => (editing = false)}
          />
        {:else if data.text.length === 0}
          <span class="text-placeholder">Double-click to edit</span>
        {:else}
          <span class="text-body">{data.text}</span>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .text-node {
    box-sizing: border-box;
    display: block;
    position: relative;
    padding: 0;
    background: transparent;
    line-height: 1.4;
    white-space: pre-wrap;
    word-break: break-word;
    overflow: visible;
    cursor: text;
  }

  .text-node.m-single {
    outline: none;
  }

  .text-node.locked {
    cursor: default;
  }

  .text-content {
    box-sizing: border-box;
    position: absolute;
    inset: 0 var(--space-8);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    z-index: 0;
  }

  .text-content.v-top {
    justify-content: flex-start;
  }

  .text-content.v-middle {
    justify-content: center;
  }

  .text-content.v-bottom {
    justify-content: flex-end;
  }

  .text-content.editing {
    overflow: visible;
  }

  .text-cell {
    box-sizing: border-box;
    display: block;
    width: 100%;
    word-break: break-word;
    white-space: pre-wrap;
    line-height: 1;
  }

  .text-placeholder {
    display: block;
    width: 100%;
    color: var(--color-fg-subtle);
    font-style: italic;
    line-height: 1;
    user-select: none;
  }

  .text-body {
    display: block;
    width: 100%;
    line-height: 1;
  }

  :global(.text-content .text-edit) {
    box-sizing: border-box;
    display: block;
    width: 100%;
    height: auto;
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
    line-height: 1;
    white-space: pre-wrap;
    word-break: break-word;
    overflow: hidden;
  }

  :global(.text-node .svelte-flow__resize-control) {
    z-index: 3;
  }
</style>
