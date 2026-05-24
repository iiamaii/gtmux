<script lang="ts">
  // NoteNode — SvelteFlow custom node for `type: "note"` (ADR-0018 D4).
  //
  // ref/frontend-design/components-v5 §01 정합:
  // - Surface + 1px shared border + 2px note-color left rail
  // - Grid: 18px head (mono uppercase glyph + label + meta + min-btn) / 1fr body (sans 12px)
  // - Minimized state (`.is-min`): 32×32 chip — rounded square icon button (visible bg/hover),
  //   click anywhere to restore. Schema h = w = 32 + minimized=true. Backup geom in sessionStore.

  import { NodeResizer } from '@xyflow/svelte';
  import InlineEditField from '$lib/common/InlineEditField.svelte';
  import InlineEditTextarea from '$lib/common/InlineEditTextarea.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import type { NoteItem, CanvasItem } from '$lib/types/canvas';

  interface NoteNodeData {
    id: string;
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

  const isVisible = $derived(data.visibility !== false);
  const isLocked = $derived(data.locked === true);
  const isInM = $derived(sessionStore.M.has(data.id) && data.group_selected !== true);
  const isMinimized = $derived(data.minimized === true);

  let titleEditing = $state(false);
  let bodyEditing = $state(false);
  type ResizeParams = { x: number; y: number; width: number; height: number };

  // Minimize: schema-driven geom (w=h=32 chip, minimized=true) + in-memory backup.
  // PanelNode 와 동일 패턴 — `sessionStore.restoredItemGeoms` 사용.
  // Note 는 chip (square icon button) 모드 — speech-bubble glyph 만 표시, 클릭 시
  // restore. Inspector minimize 버튼 SVG 토글은 Panel 과 동일 (line ↔ square).
  const MIN_CHIP = 32;
  const RESTORE_DEFAULT_W = 240;
  const RESTORE_DEFAULT_H = 96;

  function onTitleDblClick(e: MouseEvent): void {
    if (isLocked || isMinimized) return;
    e.stopPropagation();
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

  async function onResizeEnd(_event: unknown, params: ResizeParams): Promise<void> {
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'note'
            ? ({ ...it, x: params.x, y: params.y, w: Math.max(160, params.width), h: Math.max(60, params.height) } as NoteItem)
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
    style="width: 100%; height: 100%; --note-accent: {data.color};"
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked && !isMinimized && !isMaximized}
      minWidth={160}
      minHeight={60}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
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
      <!-- lucide scroll-text — simplified for 12-unit small chrome. -->
      <svg class="note-glyph" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
        <rect x="1.5" y="2" width="9" height="8" rx="1.5"/>
        <path d="M3.5 4.5h5M3.5 6.5h5M3.5 8.5h3"/>
      </svg>
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
      {#if !isLocked}
        <button
          type="button"
          class="note-btn nodrag"
          title={isMinimized ? 'Restore' : 'Minimize'}
          aria-label={isMinimized ? 'Restore' : 'Minimize'}
          onclick={(e) => void onMinimizeClick(e)}
        >
          {#if isMinimized}
            <!-- restore (small square) -->
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
              <rect x="2" y="2" width="6" height="6" rx="0.6"/>
            </svg>
          {:else}
            <!-- minimize (underscore) -->
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" aria-hidden="true">
              <path d="M2.5 7h5"/>
            </svg>
          {/if}
        </button>
        <button
          type="button"
          class="note-btn nodrag"
          title={isMaximized ? 'Restore' : 'Maximize'}
          aria-label={isMaximized ? 'Restore' : 'Maximize'}
          onclick={onMaximizeClick}
        >
          {#if isMaximized}
            <!-- restore (two windows) -->
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
              <rect x="1.5" y="3" width="5.4" height="5.3" rx="0.4"/>
              <path d="M3.2 3V1.6h5.3V7H7"/>
            </svg>
          {:else}
            <!-- maximize (square outline) -->
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
              <rect x="2" y="2" width="6" height="6" rx="0.6"/>
            </svg>
          {/if}
        </button>
        <button
          type="button"
          class="note-btn close nodrag"
          title="Close"
          aria-label="Close"
          onclick={(e) => void onCloseClick(e)}
        >
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
            <path d="M2.5 2.5l5 5M7.5 2.5l-5 5"/>
          </svg>
        </button>
      {/if}
    </div>

      <div class="note-body-wrap" role="presentation">
      {#if bodyEditing}
        <InlineEditTextarea
          value={data.body}
          editing={true}
          allowEmpty={true}
          plain={true}
          placeholder="Body…"
          class="note-body-edit"
          onCommit={(next: string) => void commit('body', next)}
          onCancel={() => (bodyEditing = false)}
        />
      {:else if data.body.length === 0}
        <span class="note-placeholder">Double-click to add body</span>
      {:else}
        <pre class="note-body">{data.body}</pre>
      {/if}
      </div>

      <!-- 32×32 chip 모드 시 표시되는 type glyph (note-head/body 는 hide).
           lucide scroll-text — head .note-glyph 와 동일 silhouette,
           14-unit viewBox 에 맞춰 약간 확대. -->
      <svg class="note-chip" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
        <rect x="2" y="2.5" width="10" height="9" rx="1.5"/>
        <path d="M4 5h6M4 7h6M4 9h3.5"/>
      </svg>
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
    height: 18px;
    min-width: 0;
  }
  .note-glyph {
    width: 12px; height: 12px;
    flex-shrink: 0;
    color: var(--note-accent, var(--color-accent));
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
  .note-btn {
    width: 18px; height: 18px;
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
    font-size: 12px;
    line-height: 1.4;
    letter-spacing: -0.1px;
    color: var(--color-fg);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .note-placeholder {
    color: var(--color-fg-subtle);
    font-size: 12px;
    font-style: italic;
    user-select: none;
  }

  .note-chip {
    display: none;
    width: 14px; height: 14px;
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
  .note-node.is-min .note-chip { display: block; }
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
