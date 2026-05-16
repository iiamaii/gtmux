<script lang="ts">
  // FilePathNode — SvelteFlow custom node for `type: "file_path"` (ADR-0018 D4).
  //
  // 사용자 입력 path 의 visual reference. 실제 OS-level open 은 ADR-0023 의
  // confirm + allowlist 흐름 (FileOpenConfirmModal — BE-NEW-12 의존, P2).
  //
  // 현재: path 표시 + 더블 클릭 인라인 편집만. open icon 은 placeholder
  // (ADR-0023 ship 시 wire).

  import { NodeResizer } from '@xyflow/svelte';
  import InlineEditField from '$lib/common/InlineEditField.svelte';
  import { ensureMutationOk, sessionStore } from '$lib/stores/sessionStore.svelte';
  import { mutateLayout, UnauthorizedError } from '$lib/http/sessions';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import type { FilePathItem, CanvasItem } from '$lib/types/canvas';

  interface FilePathNodeData {
    id: string;
    w: number;
    h: number;
    visibility: boolean;
    locked: boolean;
    path: string;
    kind?: 'directory' | 'file';
  }

  let {
    data,
    selected = false,
  }: {
    data: FilePathNodeData;
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

  let editing = $state(false);
  type ResizeParams = { x: number; y: number; width: number; height: number };

  function onDblClick(e: MouseEvent): void {
    if (isLocked) return;
    e.stopPropagation();
    editing = true;
  }

  async function onCommit(next: string): Promise<void> {
    if (next === data.path) {
      editing = false;
      return;
    }
    const active = sessionStore.active;
    if (active === null) return;
    if (!(await ensureMutationOk('File path edit aborted — session reconnect failed.'))) return;
    try {
      const { layout } = await mutateLayout(active.name, (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'file_path'
            ? ({ ...it, path: next } as FilePathItem)
            : it,
        ),
      }));
      sessionStore.loadLayout(layout);
      editing = false;
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Path commit failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  async function onResizeEnd(_event: unknown, params: ResizeParams): Promise<void> {
    const active = sessionStore.active;
    if (active === null) return;
    if (!(await ensureMutationOk('Resize aborted — session reconnect failed.'))) return;
    try {
      const { layout } = await mutateLayout(active.name, (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'file_path'
            ? ({ ...it, x: params.x, y: params.y, w: Math.max(160, params.width), h: Math.max(32, params.height) } as FilePathItem)
            : it,
        ),
      }));
      sessionStore.loadLayout(layout);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Resize failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

</script>

{#if isVisible}
  <div
    class="file-path-node"
    class:m-single={isInM}
    class:locked={isLocked}
    style="width: 100%; height: 100%;"
    role="group"
    aria-label="File path item"
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked}
      minWidth={160}
      minHeight={32}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeEnd}
    />
    <svg
      class="file-icon"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      {#if data.kind === 'directory'}
        <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
      {:else}
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
        <polyline points="14 2 14 8 20 8" />
      {/if}
    </svg>
    <div
      class="path-host"
      ondblclick={onDblClick}
      role="presentation"
    >
      {#if editing}
        <InlineEditField
          value={data.path}
          editing={true}
          allowEmpty={true}
          placeholder="/path/to/file"
          class="path-edit"
          onCommit={(next: string) => void onCommit(next)}
          onCancel={() => (editing = false)}
        />
      {:else if data.path.length === 0}
        <span class="path-placeholder">Double-click to set path</span>
      {:else}
        <span class="path-text" title={data.path}>{data.path}</span>
      {/if}
    </div>
    <!-- Open icon — ADR-0023 의 FileOpenConfirmModal 시 wire. 현재는 placeholder. -->
    <span class="open-disabled" title="Open — coming with ADR-0023" aria-hidden="true">↗</span>
  </div>
{/if}

<style>
  .file-path-node {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    box-sizing: border-box;
    padding: 0 var(--space-8);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    overflow: hidden;
  }

  .file-path-node.m-single {
    outline: none;
  }

  .file-path-node.locked {
    cursor: default;
  }

  .file-icon {
    flex: 0 0 auto;
    color: var(--color-fg-muted);
  }

  .path-host {
    flex: 1 1 auto;
    min-width: 0;
    cursor: text;
  }

  .path-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    display: block;
  }

  .path-placeholder {
    color: var(--color-fg-subtle);
    font-style: italic;
    user-select: none;
  }

  .open-disabled {
    flex: 0 0 auto;
    color: var(--color-fg-subtle);
    cursor: not-allowed;
    user-select: none;
  }

  :global(.path-edit) {
    width: 100%;
    font-family: inherit;
    font-size: inherit;
  }
</style>
