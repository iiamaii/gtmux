<script lang="ts">
  // NoteNode — SvelteFlow custom node for `type: "note"` (ADR-0018 D4).
  //
  // ref/frontend-design/components-v5 §01 정합:
  // - Surface + 1px shared border + 2px note-color left rail
  // - Grid: 18px head (mono uppercase glyph + label + meta + min-btn) / 1fr body (sans 12px)
  // - Minimized state (`.is-min`): 32×32 chip — rounded square icon button (visible bg/hover),
  //   click anywhere to restore. Schema h = w = 32 + minimized=true. Backup geom in sessionStore.

  import { NodeResizer, useSvelteFlow } from '@xyflow/svelte';
  import InlineEditField from '$lib/common/InlineEditField.svelte';
  import InlineEditTextarea from '$lib/common/InlineEditTextarea.svelte';
  import CanvasGlyph from './CanvasGlyph.svelte';
  import { componentSettings } from '$lib/stores/componentSettings.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import type { NoteItem, CanvasItem } from '$lib/types/canvas';
  import {
    constrainResizeAspectIfShift,
    scheduleLiveAspectResize,
  } from './resizeConstraint';
  import { mapDisplayHitToBodyOffset } from './noteCaret';
  import { holdLayoutRefetch, releaseLayoutRefetch } from '$lib/ws/layoutRefetch.svelte';

  interface NoteNodeData {
    id: string;
    x: number;
    y: number;
    w: number;
    h: number;
    visibility: boolean;
    locked: boolean;
    minimized?: boolean;
    title: string;
    body: string;
    color: string;
    /** Canvas.svelte group selection proxy. Descendants must not show own controls. */
    group_selected?: boolean;
  }

  let {
    data,
  }: {
    data: NoteNodeData;
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
  const isMinimized = $derived(data.minimized === true);

  let titleEditing = $state(false);
  let bodyEditing = $state(false);
  type ResizeParams = { x: number; y: number; width: number; height: number };
  const RESIZE_MIN_W = 160;
  const RESIZE_MIN_H = 60;

  // Minimize: schema-driven geom (w=h=32 chip, minimized=true) + in-memory backup.
  // PanelNode 와 동일 패턴 — `sessionStore.restoredItemGeoms` 사용.
  // Note 는 chip (square icon button) 모드 — speech-bubble glyph 만 표시, 클릭 시
  // restore. Inspector minimize 버튼 SVG 토글은 Panel 과 동일 (line ↔ square).
  const MIN_CHIP = 32;
  const RESTORE_DEFAULT_W = 240;
  const RESTORE_DEFAULT_H = 96;

  // ADR-0018 D9 amend 2026-07-23 — caret-at-point body edit entry. The display
  // <pre> renders the raw body string as plain text, so a caret hit in the
  // display DOM maps 1:1 onto a textarea offset (pure mapping in noteCaret).
  let bodyDisplayEl: HTMLElement | undefined = $state();
  let bodyEditCaret: number | null = $state(null);

  function caretHitFromPoint(x: number, y: number): { node: Node; offset: number } | null {
    const doc = document as unknown as {
      caretPositionFromPoint?: (x: number, y: number) => { offsetNode: Node; offset: number } | null;
      caretRangeFromPoint?: (x: number, y: number) => Range | null;
    };
    if (typeof doc.caretPositionFromPoint === 'function') {
      const pos = doc.caretPositionFromPoint(x, y);
      if (pos !== null) return { node: pos.offsetNode, offset: pos.offset };
    }
    if (typeof doc.caretRangeFromPoint === 'function') {
      const range = doc.caretRangeFromPoint(x, y);
      if (range !== null) return { node: range.startContainer, offset: range.startOffset };
    }
    return null;
  }

  function bodyCaretFromPoint(x: number, y: number): number {
    const el = bodyDisplayEl;
    const bodyLength = data.body.length;
    if (!el) return bodyLength;
    const hit = caretHitFromPoint(x, y);
    if (hit === null || !el.contains(hit.node)) return bodyLength;
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    const nodeLengths: number[] = [];
    let hitIndex = -1;
    let node = walker.nextNode();
    while (node !== null) {
      if (node === hit.node) hitIndex = nodeLengths.length;
      nodeLengths.push((node as Text).length);
      node = walker.nextNode();
    }
    return mapDisplayHitToBodyOffset(nodeLengths, hitIndex, hit.offset, bodyLength);
  }

  function onTitleDblClick(e: MouseEvent): void {
    if (isLocked || isMinimized) return;
    e.stopPropagation();
    // ADR-0018 D9 amend 2026-07-23 — grouped-note drill-in dblclick only drills.
    if (sessionStore.consumeSuppressedTextEditDblClick(data.id)) return;
    titleEditing = true;
  }

  // R6 (ADR-0018 D9 amend, batch-5 Grill #13): body dblclick zone 을 root
  // .note-node 까지 확장. body / padding / head-row 의 *비라벨* 영역 모두에서
  // 더블 클릭 → body editing. title 영역은 별 처리 없음 — 기존 .note-label
  // 의 ondblclick (onTitleDblClick) 만이 title editing 진입.
  //
  // 회피 path:
  //  - locked / minimized 시 no-op.
  //  - target 이 button 또는 그 자손 (svg path) 이면 자체 click handler 우선.
  //  - target 이 .note-label (또는 그 자손) 이면 stopPropagation 으로 이미
  //    onTitleDblClick 이 흡수 — root 까지 안 옴.
  function onContentDblClick(e: MouseEvent): void {
    if (isLocked || isMinimized) return;
    const target = e.target as HTMLElement | null;
    if (target === null) return;
    const currentTarget = e.currentTarget as HTMLElement | null;
    let cursor: HTMLElement | null = target;
    while (cursor !== null && cursor !== currentTarget) {
      if (cursor.tagName === 'BUTTON') return;
      cursor = cursor.parentElement;
    }
    e.stopPropagation();
    // ADR-0018 D9 amend 2026-07-23 — grouped-note drill-in dblclick only drills.
    if (sessionStore.consumeSuppressedTextEditDblClick(data.id)) return;
    bodyEditCaret = bodyCaretFromPoint(e.clientX, e.clientY);
    bodyEditing = true;
  }

  async function commit(field: 'title' | 'body', next: string): Promise<void> {
    if (field === 'title' && next === data.title) {
      titleEditing = false;
      return;
    }
    if (field === 'body' && next === data.body) {
      bodyEditing = false;
      return;
    }
    if (sessionStore.active === null) return;
    // Inspector hot-path 와 동일: optimisticMutation 으로 commit 즉시 반영.
    const result = await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'note'
            ? ({ ...it, [field]: next } as NoteItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Note edit aborted — session reconnect failed.',
        failMessage: 'Note commit failed',
      },
    );
    if (result.ok) {
      if (field === 'title') titleEditing = false;
      else bodyEditing = false;
    }
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
      data.w / data.h,
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
      data.w / data.h,
      RESIZE_MIN_W,
      RESIZE_MIN_H,
    );
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'note'
            ? ({
                ...it,
                x: constrained.x,
                y: constrained.y,
                w: Math.max(RESIZE_MIN_W, constrained.width),
                h: Math.max(RESIZE_MIN_H, constrained.height),
              } as NoteItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Resize aborted — session reconnect failed.',
        failMessage: 'Resize failed',
      },
    );
  }

  async function onMinimizeClick(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    e.preventDefault();
    if (isLocked) return;
    if (sessionStore.active === null) return;
    const cur = sessionStore.items.get(data.id);
    if (cur === undefined) return;
    const wasMinimized = cur.minimized === true;
    const next = !wasMinimized;
    let nextW = cur.w;
    let nextH = cur.h;
    if (next === true) {
      sessionStore.backupItemGeom(data.id, { x: cur.x, y: cur.y, w: cur.w, h: cur.h });
      nextW = MIN_CHIP;
      nextH = MIN_CHIP;
    } else {
      const backup = sessionStore.getRestoredGeom(data.id);
      nextW = backup !== null ? backup.w : RESTORE_DEFAULT_W;
      nextH = backup !== null ? backup.h : RESTORE_DEFAULT_H;
      sessionStore.clearRestoredGeom(data.id);
    }
    await sessionStore.applyMutation(
      (cur2) => ({
        ...cur2,
        items: cur2.items.map((it) =>
          it.id === data.id
            ? ({ ...it, minimized: next, w: nextW, h: nextH } as typeof it)
            : it,
        ),
      }),
      {
        abortMessage: 'Minimize aborted — session reconnect failed.',
        failMessage: 'Minimize failed',
      },
    );
  }

  function onChipClick(e: MouseEvent): void {
    if (!isMinimized) return;
    void onMinimizeClick(e);
  }

  // Maximize — PanelNode 와 동일. sessionStore.maximizedItemId 토글 만으로
  // MaximizedPanelModal 이 렌더링.
  const isMaximized = $derived(sessionStore.maximizedItemId === data.id);
  function onMaximizeClick(e: MouseEvent): void {
    e.stopPropagation();
    e.preventDefault();
    sessionStore.toggleMaximize(data.id);
  }

  async function onCloseClick(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    e.preventDefault();
    if (isLocked) return;
    await sessionStore.applyDeletion([data.id], { killTerminal: false });
  }
</script>

{#if isVisible}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="note-node-shell"
    style="width: 100%; height: 100%; --note-accent: {data.color}; --note-content-scale: {componentSettings.noteScale};"
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked && !isMinimized && !isMaximized}
      minWidth={160}
      minHeight={60}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeStart}
      {onResize}
      {onResizeEnd}
    />

    <div
      class="note-node"
      class:m-single={isInM}
      class:locked={isLocked}
      class:is-min={isMinimized}
      style="width: 100%; height: 100%;"
      role={isMinimized ? 'button' : 'group'}
      aria-label={isMinimized ? `Restore note ${data.title || 'Untitled'}` : `Note ${data.title || 'Untitled'}`}
      onclick={isMinimized ? onChipClick : undefined}
      onkeydown={isMinimized ? (e: KeyboardEvent) => { if (e.key === 'Enter' || e.key === ' ') onChipClick(e as unknown as MouseEvent); } : undefined}
      ondblclick={isMinimized ? undefined : onContentDblClick}
      tabindex={isMinimized ? 0 : -1}
      title={isMinimized ? `${data.title || 'Untitled'} — click to restore` : undefined}
    >
      <div class="note-head">
      <!-- Type-identity glyph — unified via CanvasGlyph 'note' (Toolbar2-anchored
           lucide scroll-text; note glyph unification 2026-07-27, ADR-0016 정합). -->
      <span class="note-glyph" aria-hidden="true">
        <CanvasGlyph name="note" />
      </span>
      <span class="note-label" ondblclick={onTitleDblClick} role="presentation">
        {#if titleEditing}
          <InlineEditField
            value={data.title}
            editing={true}
            allowEmpty={true}
            plain={true}
            placeholder="Title…"
            class="note-title-edit"
            onCommit={(next: string) => void commit('title', next)}
            onCancel={() => (titleEditing = false)}
          />
        {:else}
          <b>{data.title.length > 0 ? data.title : 'Untitled'}</b>
        {/if}
      </span>
      {#if isLocked}
        <!-- Locked-state indicator — unified CanvasGlyph 'lock' (lock UX
             unification 2026-07-27, ADR-0018 D9 family). Persistent status
             glyph (NOT hover-reveal like the note's buttons) so the lock state
             is always legible. Unlock stays in the Inspector State section.
             Minimize / close are hidden while locked; maximize is view-only so
             it stays in the hover-reveal cluster below. -->
        <span class="note-lock" title="Locked — unlock in the Inspector" aria-label="Locked">
          <CanvasGlyph name="lock" size={12} />
        </span>
      {/if}
      <!-- Action cluster — 1px unified inter-button gap (icon unification
           2026-07-27); the 6px head gap stays between title and cluster.
           Hover-reveal (NoteNode has no full header strip). -->
      <span class="note-actions" role="presentation">
        {#if !isLocked}
        <button
          type="button"
          class="note-btn nodrag"
          class:is-active={isMinimized}
          title={isMinimized ? 'Restore' : 'Minimize'}
          aria-label={isMinimized ? 'Restore' : 'Minimize'}
          onclick={(e) => void onMinimizeClick(e)}
        >
          {#if isMinimized}
            <!-- restore-from-minimized = square -->
            <CanvasGlyph name="restore-min" size={12} />
          {:else}
            <!-- minimize (underscore) -->
            <CanvasGlyph name="minimize" size={12} />
          {/if}
        </button>
        {/if}
        <!-- Maximize — view-only (ephemeral modal overlay, no layout mutation),
             so it stays available while locked. -->
        <button
          type="button"
          class="note-btn nodrag"
          class:is-active={isMaximized}
          title={isMaximized ? 'Restore' : 'Maximize'}
          aria-label={isMaximized ? 'Restore' : 'Maximize'}
          onclick={onMaximizeClick}
        >
          {#if isMaximized}
            <!-- restore-while-maximized = lucide minimize (corner brackets in) -->
            <CanvasGlyph name="restore-max" size={12} />
          {:else}
            <!-- lucide maximize (corner brackets out) -->
            <CanvasGlyph name="maximize" size={12} />
          {/if}
        </button>
        {#if !isLocked}
        <button
          type="button"
          class="note-btn close nodrag"
          title="Close"
          aria-label="Close"
          onclick={(e) => void onCloseClick(e)}
        >
          <CanvasGlyph name="close" size={12} />
        </button>
        {/if}
      </span>
    </div>

      <div class="note-body-wrap" role="presentation">
      {#if bodyEditing}
        <InlineEditTextarea
          value={data.body}
          editing={true}
          allowEmpty={true}
          plain={true}
          selectOnFocus={false}
          initialCaret={bodyEditCaret}
          placeholder="Body…"
          class="note-body-edit"
          onCommit={(next: string) => void commit('body', next)}
          onCancel={() => (bodyEditing = false)}
        />
      {:else if data.body.length === 0}
        <span class="note-placeholder">Double-click to add body</span>
      {:else}
        <pre class="note-body" bind:this={bodyDisplayEl}>{data.body}</pre>
      {/if}
      </div>

      <!-- 32×32 chip 모드 시 표시되는 type glyph (note-head/body 는 hide).
           Note glyph unification 2026-07-27 — same CanvasGlyph 'note'
           (Toolbar2-anchored lucide scroll-text) as every other note surface. -->
      <span class="note-chip" aria-hidden="true">
        <CanvasGlyph name="note" size={14} />
      </span>
    </div>
  </div>
{/if}

<style>
  .note-node-shell {
    box-sizing: border-box;
    position: relative;
    overflow: visible;
  }

  /* ref/frontend-design/components-v5 §01 — Note. NodeResizer 는 padding/border 가
     있는 visual node 밖 shell 에 위치시켜 bbox corner 와 scaler 기준점을 일치. */
  .note-node {
    box-sizing: border-box;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-left: 2px solid var(--note-accent, var(--color-accent));
    border-radius: var(--radius-sm);
    padding: 8px 6px 12px 12px;
    display: grid;
    grid-template-rows: 18px 1fr;
    gap: 6px;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06);
    color: var(--color-fg);
    overflow: visible;
    font-family: var(--font-sans);
    position: relative;
  }

  .note-node.m-single { outline: none; }
  .note-node.locked { cursor: default; }

  .note-head {
    display: flex; align-items: center; gap: 6px;
    font-family: var(--font-mono);
    font-size: 9.5px;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
    height: 20px; /* fits the 20×20 canvas-tier buttons */
    min-width: 0;
  }
  .note-actions {
    display: inline-flex;
    align-items: center;
    gap: 1px; /* unified inter-button gap (icon unification 2026-07-27) */
    flex-shrink: 0;
  }
  .note-glyph {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--note-accent, var(--color-accent));
  }
  /* Locked-state indicator — 20×20 canvas-tier box matching .note-btn, but
     PERSISTENT (no hover-reveal, unlike the note's buttons) since it carries
     status. Muted fg, non-interactive. */
  .note-lock {
    width: 20px;
    height: 20px;
    flex-shrink: 0;
    display: grid;
    place-items: center;
    color: var(--color-fg-muted);
  }
  .note-label {
    color: var(--color-fg-muted);
    flex: 1; min-width: 0;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    cursor: text;
  }
  .note-label :global(b) {
    color: var(--color-fg);
    font-weight: var(--weight-medium);
    letter-spacing: 0.4px;
    margin-right: 4px;
  }
  /* 20×20 canvas-tier standard box (icon unification 2026-07-27). Hover-
     reveal stays NOTE-ONLY — the note has no full header strip, so its
     controls appear on node hover; other components keep always-visible. */
  .note-btn {
    width: 20px; height: 20px;
    flex-shrink: 0;
    display: grid; place-items: center;
    border: none;
    background: transparent;
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    opacity: 0;
    transition: opacity .12s, background .12s, color .12s;
  }
  .note-node:hover .note-btn { opacity: 1; }
  .note-btn:hover {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }
  /* icon system unification 2026-07-27 (ADR-0016 정합) — active-state treatment
     matches the terminal/document panel reference: neutral glass fill + fg
     color (no accent). Effect/shape only; color scheme unchanged. */
  .note-btn.is-active {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }
  .note-btn.close:hover {
    background: #e5484d;
    color: #ffffff;
  }
  .note-btn:focus-visible {
    opacity: 1;
    outline: 1px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .note-body-wrap {
    min-height: 0;
    overflow: auto;
    cursor: text;
    padding-right: 6px;
  }
  .note-body {
    margin: 0;
    font-family: inherit;
    font-size: calc(12px * var(--note-content-scale, 1));
    line-height: 1.4;
    letter-spacing: -0.1px;
    color: var(--color-fg);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .note-placeholder {
    color: var(--color-fg-subtle);
    font-size: calc(12px * var(--note-content-scale, 1));
    font-style: italic;
    user-select: none;
  }

  .note-chip {
    display: none;
    width: 14px; height: 14px;
    place-items: center;
    color: var(--color-fg);
  }

  /* Minimized — 32×32 chip (square icon button). Wrapper w=h=32 (schema).
     head + body 숨김, chip glyph centered. 전체 chip 클릭으로 restore. */
  .note-node.is-min {
    grid-template-rows: 1fr;
    padding: 0;
    place-items: center;
    cursor: pointer;
    border: 1px solid var(--color-border);
    border-left: 2px solid var(--note-accent, var(--color-accent));
    border-radius: var(--radius-md);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06);
  }
  /* Minimized + selected — wrapper bbox 는 is-minimized 규칙으로 suppressed,
     대신 본 컴포넌트의 rounded-square border 색/두께로 selection 표시.
     NoteNode 의 m-single 은 isInM (single + multi 모두 포함) 이라 multi-select
     도 자연 통합. */
  .note-node.is-min.m-single {
    border-color: var(--color-accent);
    border-width: calc(1.5px / var(--canvas-zoom, 1));
  }
  .note-node.is-min .note-head,
  .note-node.is-min .note-body-wrap { display: none; }
  .note-node.is-min .note-chip { display: grid; }
  .note-node.is-min:hover {
    background: var(--color-surface-2);
  }

  :global(.note-title-edit) {
    width: 100%;
    font-family: var(--font-mono);
    font-size: 9.5px;
    letter-spacing: 0.4px;
    font-weight: var(--weight-medium);
    color: var(--color-fg);
    background: transparent;
    border: 0;
    outline: none;
  }

  :global(.note-body-edit) {
    width: 100%;
    min-height: 48px;
    font-size: 12px;
    background: transparent;
    border: 0;
    resize: none;
    outline: none;
    color: var(--color-fg);
    font-family: var(--font-sans);
    line-height: 1.4;
  }
</style>
