<script lang="ts">
  // FilePathNode — SvelteFlow custom node for `type: "file_path"` (ADR-0018 D4).
  //
  // 사용자 입력 path 의 visual reference. 실제 OS-level open 은 ADR-0023 의
  // confirm + allowlist 흐름 (FileOpenConfirmModal — BE-NEW-12 의존, P2).
  //
  // ADR-0035 D1 (2026-05-17 amend) — path 의 *직접 입력 제거*. 더블 클릭 →
  // 전역 filePicker 으로 picker modal 진입 → 선택 시 applyMutation.
  // InlineEdit 패턴 폐기 (path 의 free-form typing 은 traversal / typo risk).

  import { NodeResizer } from '@xyflow/svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { filePicker } from '$lib/stores/filePicker.svelte';
  import type { FilePathItem, CanvasItem } from '$lib/types/canvas';
  import CanvasCloseButton from './CanvasCloseButton.svelte';

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

  type ResizeParams = { x: number; y: number; width: number; height: number };

  // ref/frontend-design/components-v5 §03 — display 시 path/name 분리:
  //   path = "/foo/bar/baz.ts" → fp-path "foo/bar/" + fp-name "baz.ts"
  //   path = "/foo/bar/"        → fp-path "foo/" + fp-name "bar/"
  //   path = "baz.ts"           → fp-path "" + fp-name "baz.ts"
  const splitPath = $derived.by(() => {
    const raw = data.path ?? '';
    const trimmed = raw.replace(/^\/+/, '');
    const lastSlash = trimmed.replace(/\/+$/, '').lastIndexOf('/');
    if (lastSlash < 0) return { dir: '', name: trimmed };
    return { dir: trimmed.slice(0, lastSlash + 1), name: trimmed.slice(lastSlash + 1) };
  });

  // 확장자 → lang badge token. 시안 §03 의 per-lang palette.
  type LangBadge = { label: string; cls: string };
  const langBadge = $derived.by((): LangBadge | null => {
    const { name } = splitPath;
    if (data.kind === 'directory') return null;
    const ext = name.includes('.') ? name.slice(name.lastIndexOf('.') + 1).toLowerCase() : '';
    switch (ext) {
      case 'ts': return { label: 'TS', cls: 'ts' };
      case 'tsx': return { label: 'TSX', cls: 'tsx' };
      case 'js': return { label: 'JS', cls: 'js' };
      case 'jsx': return { label: 'JSX', cls: 'jsx' };
      case 'css': return { label: 'CSS', cls: 'css' };
      case 'md': return { label: 'MD', cls: 'md' };
      case 'svg': return { label: 'SVG', cls: 'svg' };
      case 'json': return { label: 'JSON', cls: 'json' };
      case 'rs': return { label: 'RS', cls: 'rs' };
      case 'svelte': return { label: 'SV', cls: 'svelte' };
      case 'html': return { label: 'HTML', cls: 'html' };
      case 'toml': return { label: 'TOML', cls: 'toml' };
      case 'yml':
      case 'yaml': return { label: 'YAML', cls: 'yaml' };
      case '': return null;
      default: return { label: ext.slice(0, 4).toUpperCase(), cls: 'generic' };
    }
  });

  function onPickClick(e: MouseEvent): void {
    if (isLocked) return;
    e.stopPropagation();
    filePicker.openFor('', (path) => {
      void onCommit(path);
    });
  }

  async function onCommit(next: string): Promise<void> {
    if (next === data.path) return;
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'file_path'
            ? ({ ...it, path: next, kind: 'file' } as FilePathItem)
            : it,
        ),
      }),
      {
        abortMessage: 'File path edit aborted — session reconnect failed.',
        failMessage: 'Path commit failed',
      },
    );
  }

  async function onResizeEnd(_event: unknown, params: ResizeParams): Promise<void> {
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'file_path'
            ? ({ ...it, x: params.x, y: params.y, w: Math.max(200, params.width), h: Math.max(80, params.height) } as FilePathItem)
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
    class="file-path-node shape-filepath"
    class:m-single={isInM}
    class:locked={isLocked}
    style="width: 100%; height: 100%;"
    role="group"
    aria-label="File path item"
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked}
      minWidth={200}
      minHeight={80}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeEnd}
    />
    <CanvasCloseButton id={data.id} disabled={isLocked} />
    {#if !isLocked}
      <button
        type="button"
        class="fp-change"
        title="Change file"
        aria-label="Change file"
        onclick={onPickClick}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
          <path d="M9 17H7A5 5 0 0 1 7 7h2"/>
          <path d="M15 7h2a5 5 0 1 1 0 10h-2"/>
          <line x1="8" x2="16" y1="12" y2="12"/>
        </svg>
      </button>
    {/if}
    <div class="fp-card">
      <!-- Main row — icon + meta (path / name) (시안 §03 fp-main). -->
      <div class="fp-main" ondblclick={onPickClick} role="presentation">
        <div class="fp-icon" aria-hidden="true">
          <svg width="13" height="13" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" stroke-linecap="round">
            {#if data.kind === 'directory'}
              <path d="M1.5 3.5a1 1 0 0 1 1-1h3l1.2 1.5h5.3a1 1 0 0 1 1 1V11a1 1 0 0 1-1 1H2.5a1 1 0 0 1-1-1V3.5z" />
            {:else}
              <path d="M3.5 1.5h4.5L11 4.5V12.5H3.5V1.5z"/>
              <path d="M8 1.5v3h3"/>
            {/if}
          </svg>
        </div>
        <div class="fp-meta">
          {#if data.path.length === 0}
            <span class="path-placeholder">Double-click to pick a file…</span>
          {:else}
            {#if splitPath.dir.length > 0}
              <div class="fp-path" title={data.path}>{splitPath.dir}</div>
            {/if}
            <div class="fp-name" title={data.path}>{splitPath.name}</div>
          {/if}
        </div>
      </div>
      <!-- Foot row — badge (per-lang) + placeholder meta (lines / size /
           branch). 실 데이터 wire 는 BE file-stat endpoint (ADR-0034 의
           별 fp-foot wire 가 다른 worker 의 62fc743 에 ship). placeholder
           em-dash 는 *항상 표시* — visual frame 으로 file_path 즉시 인지. -->
      <div class="fp-foot">
        {#if langBadge !== null}
          <span class="fp-badge {langBadge.cls}">{langBadge.label}</span>
        {/if}
        <span class="fp-meta-dim">— lines</span>
        <span class="sep">·</span>
        <span class="fp-meta-dim">— KB</span>
        <span class="right fp-meta-dim">—</span>
      </div>
    </div>
  </div>
{/if}

<style>
  /* ref/frontend-design/components-v5 §03 — file path tile. mono throughout. */
  .file-path-node {
    display: block;
    box-sizing: border-box;
    border-radius: var(--radius-md);
    color: var(--color-fg);
    font-family: var(--font-mono);
    position: relative;
    overflow: visible;
  }

  .fp-card {
    position: absolute;
    inset: 0;
    z-index: 0;
    display: grid;
    grid-template-rows: 1fr auto;
    box-sizing: border-box;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: inherit;
    overflow: hidden;
  }

  :global(.file-path-node .svelte-flow__resize-control) {
    z-index: 10 !important;
  }

  .file-path-node.m-single {
    outline: none;
  }

  .file-path-node.locked {
    cursor: default;
  }

  .fp-change {
    position: absolute;
    top: 6px;
    right: 34px;
    z-index: 12;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    opacity: 0;
    transition:
      opacity var(--motion-fast) var(--motion-easing),
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .file-path-node:hover .fp-change,
  .fp-change:focus-visible {
    opacity: 1;
  }

  .fp-change:hover {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  /* ref/frontend-design/components-v5 §03 — .shape-filepath. */
  .fp-main {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 11px 12px 10px;
    min-width: 0;
    cursor: text;
  }

  .fp-icon {
    width: 24px;
    height: 24px;
    flex: 0 0 24px;
    display: grid;
    place-items: center;
    border-radius: var(--radius-sm);
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  .fp-meta {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
    flex: 1 1 auto;
  }

  .fp-path {
    font-size: 10px;
    letter-spacing: 0.2px;
    color: var(--color-fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fp-name {
    font-size: 13px;
    font-weight: 540;
    letter-spacing: -0.1px;
    color: var(--color-fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .path-placeholder {
    color: var(--color-fg-subtle);
    font-style: italic;
    font-size: 12px;
    user-select: none;
  }

  /* Foot row — surface-2 strip with 1px top border. v3 §03 정합. */
  .fp-foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px 7px;
    background: var(--color-surface-2);
    border-top: 1px solid var(--color-border);
    font-size: 9.5px;
    letter-spacing: 0.5px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
  }

  .fp-foot-spacer {
    flex: 1 1 auto;
  }

  /* v3 시안 §03 — .sep / .right 의 visual 정합. 실 데이터 wire 전까지
   * placeholder em-dash 들을 그대로 보여 frame 만 갖추는 패턴. */
  .fp-foot .sep {
    opacity: 0.5;
  }

  .fp-foot .right {
    margin-left: auto;
  }

  .fp-foot .fp-meta-dim {
    color: var(--color-fg-subtle);
  }

  /* Lang badge — per-lang background color (시안 §03 palette). */
  .fp-badge {
    display: inline-flex;
    align-items: center;
    height: 14px;
    padding: 0 5px;
    border-radius: 3px;
    font-size: 9px;
    font-weight: 540;
    letter-spacing: 0.8px;
    color: #ffffff;
    background: var(--color-fg-muted);
  }

  .fp-badge.ts { background: #3178c6; }
  .fp-badge.tsx { background: #61dafb; color: #002233; }
  .fp-badge.js { background: #f7df1e; color: #1a1a00; }
  .fp-badge.jsx { background: #61dafb; color: #002233; }
  .fp-badge.css { background: #2965f1; }
  .fp-badge.md { background: #555555; }
  .fp-badge.svg { background: #ff9a3c; color: #2a1500; }
  .fp-badge.json { background: #2b2b2b; }
  .fp-badge.rs { background: #ce422b; }
  .fp-badge.svelte { background: #ff3e00; }
  .fp-badge.html { background: #e34c26; }
  .fp-badge.toml { background: #9c4221; }
  .fp-badge.yaml { background: #cb171e; }
  .fp-badge.generic { background: var(--color-fg-muted); }

  :global(.path-edit) {
    width: 100%;
    font-family: inherit;
    font-size: 13px;
  }
</style>
