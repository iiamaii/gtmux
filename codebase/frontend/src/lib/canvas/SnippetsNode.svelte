<script lang="ts">
  // SnippetsNode — SvelteFlow custom node for `type: "snippets"` (ADR-0038).
  // Visual chassis: ref/frontend-design/components-v8.html §12.1
  //
  // Three-strip grid (matches Document's chassis):
  //   head 30px  — clipboard glyph + "Snippets" eyebrow + count + min/close
  //   body 1fr   — pill flow (or empty hover state)
  //   foot 24px  — "{count} items"
  //
  // Editing/adding a snippet entry opens the floating SnippetEditPanel
  // (lib/chrome/SnippetEditPanel.svelte) anchored to the trigger element
  // (pill, [+] add, or empty body). All entry mutations live there.
  //
  // Invariants:
  //   - ADR-0010 D25/D31: chrome (selection ring / NodeResizer / min/close)
  //     shows only when `isInM = M.has(id) && !group_selected`.
  //   - ADR-0028: every entries mutation goes through `applyMutation` (single
  //     entry, priorSnapshot rollback, 1 Cmd+Z step per save/delete/resize).
  //   - ADR-0005 D9: `data.entries` is a reactive proxy — keep `$derived` wraps
  //     stable, no direct proxy reads inside `$effect` side-effect bodies.

  import { NodeResizer } from '@xyflow/svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { snippetEditPanel } from '$lib/stores/snippetEditPanel.svelte';
  import { snippetDeleteDialog } from '$lib/stores/snippetDeleteDialog.svelte';
  import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
  import type { CanvasItem, SnippetEntry, SnippetsItem } from '$lib/types/canvas';

  interface SnippetsNodeData {
    id: string;
    type: 'snippets';
    w: number;
    h: number;
    /** Canvas.svelte effectiveVisibility() output — boolean (true=show).
     *  ItemCommon schema 의 'visible'/'hidden' enum 은 itemToNode 에서 boolean
     *  으로 변환됨. */
    visibility: boolean;
    locked: boolean;
    minimized?: boolean;
    entries: SnippetEntry[];
    /** Optional user-editable display label (ItemCommon.label). Synced with
     *  layer-tree row + inspector identity. Falls back to "Snippets" when null/empty. */
    label?: string | null;
    /** Canvas.svelte group selection proxy. Descendants must not show own chrome. */
    group_selected?: boolean;
  }

  let {
    data,
    width,
    height,
  }: {
    data: SnippetsNodeData;
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

  const isInM = $derived(sessionStore.M.has(data.id) && data.group_selected !== true);
  const isVisible = $derived(data.visibility !== false);
  const isLocked = $derived(data.locked === true);
  const isMinimized = $derived(data.minimized === true);

  // Minimize geometry — same pattern as DocumentNode/PanelNode. backupItemGeom
  // captures pre-minimize w/h; restore replays it. Without h mutation the
  // wrapper stays at original height and only CSS collapses the inner grid,
  // leaving an empty body region below the head (visual bug).
  const SNIPPETS_MIN_H = 35;
  const SNIPPETS_RESTORE_W = 320;
  const SNIPPETS_RESTORE_H = 150;
  const entries = $derived(data.entries);
  const SNIPPETS_CAP = 1000;
  // Sync source for head eyebrow + layer-tree row + inspector identity label.
  // Falls back to 'Snippets' when the user has not set a custom label.
  const displayLabel = $derived(
    typeof data.label === 'string' && data.label.length > 0 ? data.label : 'Snippets',
  );
  const canAdd = $derived(!isLocked && entries.length < SNIPPETS_CAP);

  // Transient copy feedback per entry — `is-copied` 200ms green flash (v8 §12).
  let copiedEntryId = $state<string | null>(null);
  let copiedTimer: ReturnType<typeof setTimeout> | null = null;

  // 2026-05-24 user spec — per-node interaction mode selected via head button
  // group (segmented control). Only one mode active at a time.
  //   idle   : click pill = copy body (default; hover shows ⤢ indicator)
  //   edit   : click pill = open edit panel for that entry; pill tinted; hover shows pencil
  //   delete : click pill = confirm + delete entry; pill tinted danger; hover shows trash
  type ViewMode = 'idle' | 'edit' | 'delete';
  let viewMode = $state<ViewMode>('idle');
  function setViewMode(next: ViewMode): void {
    viewMode = next;
  }

  async function copyBody(entry: SnippetEntry): Promise<void> {
    const result = await copyTextToSystemClipboard(entry.body);
    if (result.ok) {
      // Pill green flash — 200ms (v8 §12).
      if (copiedTimer !== null) clearTimeout(copiedTimer);
      copiedEntryId = entry.id;
      copiedTimer = setTimeout(() => {
        copiedEntryId = null;
        copiedTimer = null;
      }, 200);
      // Toast — short, success tone.
      const shortKey = entry.key.length > 32
        ? entry.key.slice(0, 32) + '…'
        : entry.key;
      toastStore.show({
        message: `Copied: ${shortKey}`,
        tone: 'success',
        durationMs: 2_000,
      });
    } else {
      toastStore.show({
        message: 'Clipboard blocked by browser security. Use HTTPS or select/copy manually.',
        tone: 'error',
      });
    }
  }

  // Mutation responsibility (add/edit/delete) is now owned by the floating
  // SnippetEditPanel (lib/chrome/SnippetEditPanel.svelte) — opened via
  // snippetEditPanel store with an anchor rect. SnippetsNode only gathers the
  // anchor from the trigger element and signals the store.

  function openEditPanel(entry: SnippetEntry, anchorEl: HTMLElement): void {
    if (isLocked) return;
    const r = anchorEl.getBoundingClientRect();
    snippetEditPanel.openFor({
      nodeId: data.id,
      entryId: entry.id,
      prefill: { key: entry.key, body: entry.body },
      anchor: { x: r.left, y: r.top, width: r.width, height: r.height },
      source: 'canvas-pill',
    });
  }

  function deleteEntryConfirmed(entry: SnippetEntry): void {
    if (isLocked) return;
    snippetDeleteDialog.show({
      key: entry.key,
      onConfirm: async () => {
        await sessionStore.applyMutation(
          (cur) => ({
            ...cur,
            items: cur.items.map((it: CanvasItem) => {
              if (it.id !== data.id || it.type !== 'snippets') return it;
              const sit = it as SnippetsItem;
              return { ...sit, entries: sit.entries.filter((e) => e.id !== entry.id) };
            }),
          }),
          {
            abortMessage: 'Snippet delete aborted — session reconnect failed.',
            failMessage: 'Snippet delete failed',
          },
        );
      },
    });
  }

  function onPillClick(entry: SnippetEntry, e: MouseEvent): void {
    if (isLocked) return;
    if (viewMode === 'idle') {
      void copyBody(entry);
    } else if (viewMode === 'edit') {
      openEditPanel(entry, e.currentTarget as HTMLElement);
    } else {
      // delete — opens the styled SnippetDeleteConfirmModal via store.
      deleteEntryConfirmed(entry);
    }
  }

  function openAddPanel(anchorEl: HTMLElement, source: 'canvas-empty' | 'canvas-add'): void {
    if (!canAdd) return;
    // Auto-restore from minimized when adding from a minimized chrome trigger.
    if (isMinimized) void toggleMinimize(false);
    const r = anchorEl.getBoundingClientRect();
    snippetEditPanel.openFor({
      nodeId: data.id,
      entryId: null,
      anchor: { x: r.left, y: r.top, width: r.width, height: r.height },
      source,
    });
  }

  // ── Drag-to-reorder (within node) ─────────────────────────────────────
  // HTML5 native drag — same vocabulary as LayerTreeView row reorder. Drag
  // start fires only after pointer move threshold so a quick click still
  // copies/edits/deletes. Custom MIME type avoids triggering cross-component
  // drops (canvas drop targets only respond to known types).
  const SNIPPET_DRAG_MIME = 'application/x-gtmux-snippet-entry';
  let draggingEntryId = $state<string | null>(null);
  let dropTargetId = $state<string | null>(null);
  let dropBefore = $state(false);

  function onPillDragStart(entry: SnippetEntry, e: DragEvent): void {
    if (isLocked) {
      e.preventDefault();
      return;
    }
    draggingEntryId = entry.id;
    if (e.dataTransfer !== null) {
      e.dataTransfer.effectAllowed = 'move';
      // Custom MIME — opaque to other drop targets. Payload includes node id
      // so a future cross-node reorder could distinguish source. Today we
      // only support intra-node moves.
      e.dataTransfer.setData(SNIPPET_DRAG_MIME, `${data.id}:${entry.id}`);
    }
  }

  function onPillDragOver(entry: SnippetEntry, e: DragEvent): void {
    if (draggingEntryId === null || draggingEntryId === entry.id) return;
    e.preventDefault();
    if (e.dataTransfer !== null) e.dataTransfer.dropEffect = 'move';
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const midX = rect.left + rect.width / 2;
    dropTargetId = entry.id;
    dropBefore = e.clientX < midX;
  }

  function onPillDragLeave(entry: SnippetEntry, e: DragEvent): void {
    if (dropTargetId !== entry.id) return;
    // Only clear when leaving the pill bounds entirely (not when entering
    // child elements like the indicator span).
    const related = e.relatedTarget as Node | null;
    const current = e.currentTarget as Node | null;
    if (related !== null && current !== null && current.contains(related)) return;
    dropTargetId = null;
  }

  async function onPillDrop(entry: SnippetEntry, e: DragEvent): Promise<void> {
    if (draggingEntryId === null) return;
    e.preventDefault();
    const sourceId = draggingEntryId;
    const targetId = entry.id;
    const before = dropBefore;
    draggingEntryId = null;
    dropTargetId = null;
    if (sourceId === targetId) return;
    await reorderEntries(sourceId, targetId, before);
  }

  function onPillDragEnd(): void {
    draggingEntryId = null;
    dropTargetId = null;
  }

  async function reorderEntries(sourceId: string, targetId: string, before: boolean): Promise<void> {
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) => {
          if (it.id !== data.id || it.type !== 'snippets') return it;
          const sit = it as SnippetsItem;
          const source = sit.entries.find((e) => e.id === sourceId);
          if (source === undefined) return sit;
          const remaining = sit.entries.filter((e) => e.id !== sourceId);
          const targetIdx = remaining.findIndex((e) => e.id === targetId);
          if (targetIdx === -1) return sit;
          const insertIdx = before ? targetIdx : targetIdx + 1;
          remaining.splice(insertIdx, 0, source);
          return { ...sit, entries: remaining };
        }),
      }),
      {
        abortMessage: 'Snippet reorder aborted — session reconnect failed.',
        failMessage: 'Snippet reorder failed',
      },
    );
  }

  // Minimum size — body must fit at least one row of badges:
  //   width  = body padding (12+12) + one short badge (~70) + gap (6) + [+] (28) ≈ 130
  //   height = head (30) + body padding (10+10) + pill (22) ≈ 72 → 75 with breathing
  // Below these the body wraps awkwardly or the head clips. NodeResizer prop +
  // applyMutation clamp use the same constants so both code paths agree.
  const SNIPPETS_RESIZE_MIN_W = 140;
  const SNIPPETS_RESIZE_MIN_H = 75;

  type ResizeParams = { x: number; y: number; width: number; height: number };
  async function onResizeEnd(_e: unknown, params: ResizeParams): Promise<void> {
    const nextW = Math.max(SNIPPETS_RESIZE_MIN_W, params.width);
    const nextH = Math.max(SNIPPETS_RESIZE_MIN_H, params.height);
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'snippets'
            ? ({ ...it, x: params.x, y: params.y, w: nextW, h: nextH } as SnippetsItem)
            : it,
        ),
      }),
      { abortMessage: 'Resize aborted', failMessage: 'Resize failed' },
    );
  }

  async function toggleMinimize(next?: boolean): Promise<void> {
    if (isLocked) return;
    if (sessionStore.active === null) return;
    const cur = sessionStore.items.get(data.id);
    if (cur === undefined || cur.type !== 'snippets') return;
    const target = next ?? cur.minimized !== true;
    let nextW = cur.w;
    let nextH = cur.h;
    if (target) {
      sessionStore.backupItemGeom(data.id, { x: cur.x, y: cur.y, w: cur.w, h: cur.h });
      nextH = SNIPPETS_MIN_H;
    } else {
      const backup = sessionStore.getRestoredGeom(data.id);
      nextW = backup?.w ?? SNIPPETS_RESTORE_W;
      nextH = backup?.h ?? SNIPPETS_RESTORE_H;
      sessionStore.clearRestoredGeom(data.id);
    }
    await sessionStore.applyMutation(
      (cur2) => ({
        ...cur2,
        items: cur2.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'snippets'
            ? ({ ...it, minimized: target, w: nextW, h: nextH } as SnippetsItem)
            : it,
        ),
      }),
      { abortMessage: 'Minimize aborted', failMessage: 'Minimize failed' },
    );
  }

  async function onCloseClick(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    e.preventDefault();
    if (isLocked) return;
    await sessionStore.applyDeletion([data.id], { killTerminal: false });
  }

</script>

{#if isVisible}
  <div
    class="snippets-node"
    class:m-single={isInM}
    class:is-locked={isLocked}
    class:is-min={isMinimized}
    style="width: 100%; height: 100%;"
    role="group"
    aria-label="Snippets collection"
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked && !isMinimized}
      minWidth={SNIPPETS_RESIZE_MIN_W}
      minHeight={SNIPPETS_RESIZE_MIN_H}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeEnd}
    />

    <!-- Head strip — same vocabulary as Document's doc-head -->
    <div class="snip-head">
      <!-- ADR-0038 — lucide square-library (scaled 24→12 ×0.5). -->
      <svg class="snip-glyph" width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
        <rect x="1.5" y="1.5" width="9" height="9" rx="1"/>
        <path d="M3.5 3.5v5"/>
        <path d="M5.5 3.5v5"/>
        <path d="m7.5 3.5 1 5"/>
      </svg>
      <span class="eyebrow" title={displayLabel}>{displayLabel}</span>
      <span class="sep">·</span>
      <span class="count">{entries.length} / {SNIPPETS_CAP}</span>
      {#if !isLocked}
        <div class="snip-actions">
          <!-- 2026-05-24 — mode segmented control. 3 buttons, only one active.
               Clicking switches mode; the active button is highlighted. -->
          <div class="snip-mode-group" role="group" aria-label="Snippet interaction mode">
            <button
              type="button"
              class="snip-btn-icon snip-mode-btn nodrag"
              class:is-active={viewMode === 'idle'}
              data-mode="idle"
              title="Copy mode — click a pill to copy"
              aria-label="Copy mode"
              aria-pressed={viewMode === 'idle'}
              onclick={(e: MouseEvent) => { e.stopPropagation(); setViewMode('idle'); }}
              onmousedown={(e: MouseEvent) => e.stopPropagation()}
            >
              <!-- copy / clipboard glyph -->
              <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <rect x="4" y="2.5" width="6" height="7" rx="0.8"/>
                <path d="M2 4.5v5a1 1 0 0 0 1 1h4"/>
              </svg>
            </button>
            <button
              type="button"
              class="snip-btn-icon snip-mode-btn nodrag"
              class:is-active={viewMode === 'edit'}
              data-mode="edit"
              title="Edit mode — click a pill to open the edit panel"
              aria-label="Edit mode"
              aria-pressed={viewMode === 'edit'}
              onclick={(e: MouseEvent) => { e.stopPropagation(); setViewMode('edit'); }}
              onmousedown={(e: MouseEvent) => e.stopPropagation()}
            >
              <!-- pencil -->
              <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M2 10l1-3 5-5 2 2-5 5z"/>
                <path d="M7 3l2 2"/>
              </svg>
            </button>
            <button
              type="button"
              class="snip-btn-icon snip-mode-btn nodrag"
              class:is-active={viewMode === 'delete'}
              data-mode="delete"
              title="Delete mode — click a pill to confirm + delete"
              aria-label="Delete mode"
              aria-pressed={viewMode === 'delete'}
              onclick={(e: MouseEvent) => { e.stopPropagation(); setViewMode('delete'); }}
              onmousedown={(e: MouseEvent) => e.stopPropagation()}
            >
              <!-- trash -->
              <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M3 4h6M5 4V3a1 1 0 0 1 1-1h0a1 1 0 0 1 1 1v1"/>
                <path d="M4 4l.5 6a1 1 0 0 0 1 1h1a1 1 0 0 0 1-1L8 4"/>
              </svg>
            </button>
          </div>
          <button
            type="button"
            class="snip-btn-icon nodrag"
            class:is-active={isMinimized}
            title={isMinimized ? 'Restore' : 'Minimize'}
            aria-label={isMinimized ? 'Restore' : 'Minimize'}
            onclick={(e: MouseEvent) => { e.stopPropagation(); void toggleMinimize(); }}
            onmousedown={(e: MouseEvent) => e.stopPropagation()}
          >
            {#if isMinimized}
              <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
                <path d="M3 5h6"/><path d="M3 8h6"/>
              </svg>
            {:else}
              <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
                <path d="M3 8.5h6"/>
              </svg>
            {/if}
          </button>
          <button
            type="button"
            class="snip-btn-icon close nodrag"
            title="Close"
            aria-label="Close"
            onclick={(e: MouseEvent) => void onCloseClick(e)}
            onmousedown={(e: MouseEvent) => e.stopPropagation()}
          >
            <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
              <path d="M3 3l6 6M9 3l-6 6"/>
            </svg>
          </button>
        </div>
      {/if}
    </div>

    <!-- Body strip — pill collection. When empty, only the `[+]` add badge
         renders (same size/shape as the non-empty case) so the body chrome
         stays consistent. Edit/add form lives in the floating
         SnippetEditPanel, anchored to the trigger element. -->
    <div class="snip-body" role="group">
        {#each entries as entry (entry.id)}
          <!-- Single pill atom. Click action depends on the node's viewMode
               (cycled via head mode button): idle=copy, edit=panel, delete=
               confirm+delete. Hover indicator glyph (⤢/pencil/trash) is
               purely visual feedback — not a separate click target. -->
          <button
            type="button"
            class="snip-pill nodrag"
            class:is-copied={copiedEntryId === entry.id}
            class:mode-edit={viewMode === 'edit'}
            class:mode-delete={viewMode === 'delete'}
            class:is-dragging={draggingEntryId === entry.id}
            class:drop-before={dropTargetId === entry.id && dropBefore && draggingEntryId !== entry.id}
            class:drop-after={dropTargetId === entry.id && !dropBefore && draggingEntryId !== entry.id}
            draggable={!isLocked}
            ondragstart={(e: DragEvent) => onPillDragStart(entry, e)}
            ondragover={(e: DragEvent) => onPillDragOver(entry, e)}
            ondragleave={(e: DragEvent) => onPillDragLeave(entry, e)}
            ondrop={(e: DragEvent) => void onPillDrop(entry, e)}
            ondragend={onPillDragEnd}
            onclick={(e: MouseEvent) => onPillClick(entry, e)}
            title={viewMode === 'idle'
              ? `"${entry.key}" — click to copy · drag to reorder`
              : viewMode === 'edit'
                ? `"${entry.key}" — click to edit · drag to reorder`
                : `"${entry.key}" — click to delete · drag to reorder`}
            aria-label={viewMode === 'idle'
              ? `Copy snippet ${entry.key}`
              : viewMode === 'edit'
                ? `Edit snippet ${entry.key}`
                : `Delete snippet ${entry.key}`}
            disabled={isLocked}
          >
            <span class="snip-pill-text">{entry.key}</span>
            <span class="pill-indicator" aria-hidden="true">
              {#if viewMode === 'edit'}
                <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M2 10l1-3 5-5 2 2-5 5z"/>
                  <path d="M7 3l2 2"/>
                </svg>
              {:else if viewMode === 'delete'}
                <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M3 4h6M5 4V3a1 1 0 0 1 1-1h0a1 1 0 0 1 1 1v1"/>
                  <path d="M4 4l.5 6a1 1 0 0 0 1 1h1a1 1 0 0 0 1-1L8 4"/>
                </svg>
              {:else}
                <!-- idle: ⤢ as indicator-only (not a click target) -->
                <svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M2 8l6-6M8 2H5M8 2v3"/>
                </svg>
              {/if}
            </span>
          </button>
        {/each}
        <button
          type="button"
          class="snip-add nodrag"
          onclick={(e: MouseEvent) => openAddPanel(e.currentTarget as HTMLButtonElement, 'canvas-add')}
          aria-label="Add snippet"
          title="Add snippet"
          disabled={!canAdd}
        >
          <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" aria-hidden="true">
            <path d="M6 2.5v7M2.5 6h7"/>
          </svg>
        </button>
      </div>

  </div>
{/if}

<style>
  /* Container — three-strip grid. Matches Document's chassis pattern:
       - `border` (not box-shadow ring): occupies layout space, no double-paint
         with Canvas wrapper's `.svelte-flow__node.m-selected` ring.
       - `overflow: visible`: NodeResizer handles sit at element corners with
         half-inside/half-outside positioning; overflow:hidden would clip them.
         Body strip handles its own overflow:hidden for pill wrap clipping. */
  .snippets-node {
    box-sizing: border-box;
    position: relative;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.06);
    color: var(--color-fg);
    display: grid;
    grid-template-rows: 30px 1fr;
    overflow: visible;
    font-family: var(--font-sans);
  }
  /* Selection visual is owned by Canvas wrapper (.svelte-flow__node.m-selected
     in Canvas.svelte) — zoom-compensated 1.5px accent ring. Suppress default
     outline like DocumentNode does. Minimized case is handled below with a
     component-local border swap (wrapper ring is suppressed by Canvas for
     .is-minimized.m-selected per ADR / AGENTS.md). */
  .snippets-node.m-single {
    outline: none;
  }
  .snippets-node.is-locked {
    cursor: default;
  }

  /* Head — surface-2 mono strip (borrowed from Document's chassis). Foot
     dropped — count is already in the head eyebrow, separate foot was
     redundant. */
  .snip-head {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--color-surface-2);
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.4px;
    color: var(--color-fg-muted);
    padding: 0 4px 0 12px;
    border-bottom: 1px solid var(--color-border);
    /* Top corners match container — overflow:visible no longer clips for us. */
    border-radius: var(--radius-md) var(--radius-md) 0 0;
  }
  /* Minimized — head is the ONLY visible strip. Same pattern as DocumentNode
     /PanelNode: outer wrapper goes transparent, head takes all four corners
     plus its own border + shadow. Wrapper m-selected ring is suppressed by
     Canvas (.svelte-flow__node.is-minimized.m-selected) so selection visual
     swaps to component-local border color on the head. */
  .snippets-node.is-min {
    grid-template-rows: 1fr;
    border: 0;
    box-shadow: none;
    background: transparent;
  }
  .snippets-node.is-min .snip-head {
    height: 100%;
    box-sizing: border-box;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: 0 1px 10px rgba(0, 0, 0, 0.10);
  }
  .snippets-node.is-min.m-single .snip-head {
    border-color: var(--color-accent);
    border-width: calc(1.5px / var(--canvas-zoom, 1));
  }
  .snip-head .snip-glyph {
    flex-shrink: 0;
    opacity: 0.75;
  }
  .snip-head .eyebrow {
    color: var(--color-fg);
    font-weight: 540;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    font-size: 9.5px;
  }
  .snip-head .sep {
    color: var(--color-border-strong);
  }
  .snip-head .count {
    color: var(--color-fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 160px;
  }

  /* Head action cluster — always visible (header-bearing components convention,
     matches PanelNode .panel-btn / DocumentNode .doc-btn). Note: NoteNode uses
     hover-reveal because it has no full header — just a tiny compact chrome.
     All buttons use the 22×22 canonical chrome size — icons 11×11 centered. */
  .snip-actions {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: auto;
    padding-left: 8px;
    flex-shrink: 0;
  }
  /* 3-button segmented control for viewMode. Same button size as the rest
     of the chrome (22×22) — segmentation comes from the faint container
     surround + 1px inter-button gap, not from a smaller button footprint. */
  .snip-mode-group {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    padding: 1px;
    background: var(--color-glass-1);
    border-radius: var(--radius-sm);
  }
  .snip-btn-icon {
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border: none;
    background: transparent;
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    transition: background 0.12s, color 0.12s;
  }
  .snip-btn-icon:hover:not(:disabled) {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }
  .snip-btn-icon:focus-visible {
    outline: 1px dashed var(--color-accent);
    outline-offset: 1px;
  }
  .snip-btn-icon.is-active {
    color: var(--color-accent);
  }
  .snip-btn-icon.close:hover:not(:disabled) {
    background: var(--color-danger);
    color: #fff;
  }
  /* Mode buttons — share .snip-btn-icon's 22×22 size. Active state per-mode
     swaps to accent / danger fill so the active button reads as "selected"
     within the segmented group. */
  .snip-mode-btn.is-active {
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }
  .snip-mode-btn[data-mode='delete'].is-active {
    background: var(--color-danger);
    color: #fff;
  }
  .snip-mode-btn.is-active:hover:not(:disabled) {
    /* Keep the active fill on hover; don't fall back to ghost hover. */
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }
  .snip-mode-btn[data-mode='delete'].is-active:hover:not(:disabled) {
    background: var(--color-danger);
    color: #fff;
  }

  /* Body — pill flow */
  .snip-body {
    padding: 10px 12px;
    overflow: hidden;
    position: relative;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-content: flex-start;
    min-height: 0;
  }

  /* Single integrated pill. Hover grows right padding to make room for the
     mode-dependent hover indicator glyph (⤢ idle / pencil edit / trash delete).
     Click behavior is driven by parent viewMode — pill itself is the only
     click target (the indicator span is pointer-events:none). */
  .snip-pill {
    position: relative;
    display: inline-flex;
    align-items: center;
    height: 22px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-pill);
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    color: var(--color-accent);
    font-family: var(--font-mono);
    font-size: 11.5px;
    letter-spacing: 0;
    line-height: 1.4;
    cursor: pointer;
    max-width: 200px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    transition: background 0.12s, color 0.12s, padding-right 0.12s;
  }
  .snip-pill:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-accent) 22%, transparent);
    padding-right: 24px;
  }
  .snip-pill:active:not(:disabled) {
    background: color-mix(in srgb, var(--color-accent) 30%, transparent);
  }
  .snip-pill:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 1px;
  }
  .snip-pill:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  /* 200ms copy feedback — green flash (v8 §12 anatomy 12) */
  .snip-pill.is-copied {
    background: color-mix(in srgb, var(--color-success) 18%, transparent);
    color: var(--color-success);
  }
  /* Edit mode — deeper accent tint signals edit affordance. */
  .snip-pill.mode-edit {
    background: color-mix(in srgb, var(--color-accent) 24%, transparent);
  }
  .snip-pill.mode-edit:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-accent) 36%, transparent);
  }
  /* Delete mode — danger color tint signals destructive affordance. */
  .snip-pill.mode-delete {
    background: color-mix(in srgb, var(--color-danger) 16%, transparent);
    color: var(--color-danger);
  }
  .snip-pill.mode-delete:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-danger) 28%, transparent);
  }
  .snip-pill-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Drag-to-reorder visual feedback (HTML5 native drag). The dragged pill
     fades; drop targets show a vertical accent bar on the entering edge. */
  .snip-pill.is-dragging {
    opacity: 0.4;
  }
  .snip-pill.drop-before {
    box-shadow: inset 3px 0 0 var(--color-accent);
  }
  .snip-pill.drop-after {
    box-shadow: inset -3px 0 0 var(--color-accent);
  }

  /* Hover indicator glyph — absolutely positioned inside pill's right padding.
     Pure visual cue; the parent pill button owns the click. */
  .pill-indicator {
    position: absolute;
    right: 6px;
    top: 50%;
    transform: translateY(-50%);
    width: 14px;
    height: 14px;
    display: grid;
    place-items: center;
    color: currentColor;
    opacity: 0;
    transition: opacity 0.12s;
    pointer-events: none;
  }
  .snip-pill:hover .pill-indicator {
    opacity: 1;
  }
  .pill-indicator svg {
    width: 10px;
    height: 10px;
  }

  /* [+] add pill — same vertical mass as content pills, dashed */
  .snip-add {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 22px;
    padding: 0;
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--color-fg-muted);
    border: 1px dashed var(--color-border-strong);
    cursor: pointer;
    transition: background 0.12s, color 0.12s, border-color 0.12s;
  }
  .snip-add:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-accent) 8%, transparent);
    color: var(--color-accent);
    border-color: var(--color-accent);
  }
  .snip-add:focus-visible {
    outline: 1px dashed var(--color-accent);
    outline-offset: 1px;
  }
  .snip-add:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .snip-add svg {
    width: 10px;
    height: 10px;
  }

  /* Empty state intentionally has no special chrome — the `[+]` add badge is
     the only visible affordance and stays the same size/shape as in the
     non-empty case, keeping body proportions stable across states. */

  /* Locked — body + add fade, head/foot stay legible */
  .snippets-node.is-locked .snip-body,
  .snippets-node.is-locked .snip-add {
    opacity: 0.5;
    pointer-events: none;
  }

  /* Minimized — body hidden. (.is-min outer + head visual is defined near
     the container section above.) */
  .snippets-node.is-min .snip-body {
    display: none;
  }

  /* (Inline edit form CSS removed — form now lives in SnippetEditPanel.) */
</style>
