<script lang="ts">
  // Svelte Flow custom node — Panel chrome + xterm body.
  //
  // 책임:
  // - `data` (NodeProps의 data prop) = PanelData (canvas-layout-schema §1 Panel JSON) +
  //   Canvas.svelte 가 추가로 주입한 m_multi 플래그 (M.size > 1).
  // - 헤더 바 = drag handle. label + badges (L/M/Min/I).
  // - 본문 = XtermHost.
  // - 선택 시각 (M):
  //     * single  (.m-single) → solid 1.5px accent outline (Figma 정합)
  //     * multi   (.m-multi)  → dashed 2px accent outline + 헤더 색조 변화
  // - resize : NodeResizer (corner + edge handles). onResizeEnd 시 panelsStore
  //   + PUT /api/layout 으로 영속화.
  // - visibility=false → 렌더 X.

  import { getContext } from 'svelte';
  import { NodeResizer } from '@xyflow/svelte';
  import XtermHost from './XtermHost.svelte';
  import { panelsStore } from '$lib/stores/panels.svelte';
  import { ephemeralStore } from '$lib/stores/ephemeral.svelte';
  import { muxStore } from '$lib/stores/mux.svelte';
  import { putLayoutCommitCurrent } from '$lib/http/layout';
  import { sendCtrl } from '$lib/ws/ctrl-registry';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import type { WsClient } from '$lib/ws/client';

  interface WsClientHolder {
    current: WsClient | null;
  }
  const wsClientHolder = getContext<WsClientHolder>('wsClient');

  interface PanelData {
    id: string;
    pane_id?: string;
    x?: number;
    y?: number;
    w?: number;
    h?: number;
    z?: number;
    visibility?: boolean;
    minimized?: boolean;
    locked?: boolean;
    label?: string | null;
    /** Canvas.svelte 가 주입 — 현재 M 선택 개수가 2 이상이면 true. */
    m_multi?: boolean;
  }

  let {
    data,
    selected = false
  }: {
    data: PanelData;
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
  const isStreaming = $derived(isVisible && data.minimized !== true);
  const headerLabel = $derived(data.label ?? data.pane_id ?? data.id);

  const isInM = $derived(selected || ephemeralStore.m.has(data.id));
  const isMultiM = $derived(isInM && data.m_multi === true);
  const isSingleM = $derived(isInM && data.m_multi !== true);

  const isInI = $derived(
    typeof data.pane_id === 'string' && ephemeralStore.i === data.pane_id
  );

  const panelW = $derived(data.w ?? 480);
  const panelH = $derived(data.h ?? 320);
  const isLocked = $derived(data.locked === true);

  const TOKEN_STORAGE_KEY = 'gtmux_token';
  function readToken(): string | null {
    try {
      return sessionStorage.getItem(TOKEN_STORAGE_KEY);
    } catch {
      return null;
    }
  }

  // NodeResizer onResizeEnd — { event, params: { x, y, width, height } }.
  // Resize 도중에는 SvelteFlow 가 controlled width/height 를 자체 업데이트
  // 하므로 본 핸들러는 *최종 값만* store + disk 로 commit (drag 와 동일 패턴).
  type ResizeParams = { x: number; y: number; width: number; height: number };
  function onResizeEnd(_event: unknown, params: ResizeParams) {
    panelsStore.resizePanel(data.id, params.x, params.y, params.width, params.height);
    const token = readToken();
    if (token === null) {
      console.warn('[gtmux] resize commit skipped: no auth token');
      return;
    }
    void putLayoutCommitCurrent(token).catch((e) => {
      console.warn('[gtmux] resize commit failed:', e);
    });
  }

  // ─ Stage G — close button (S7-FE-CLOSE-GUARD) ──────────────────────────
  //
  // CONTEXT.md "Pane lifecycle invariant" — 마지막 살아 있는 child 일 때
  // close 비활성화. 사후 recovery 가 아닌 사전 prevention.
  const liveCount = $derived(
    [...muxStore.panes.values()].filter((p) => p.dead !== true).length
  );

  // Live count = 1 + this panel's pane is the one alive ⇒ disabled.
  // We also disable when the underlying pane is not registered yet
  // (no pane_id) to avoid the close racing the spawn.
  const paneNumeric = $derived.by(() => {
    if (typeof data.pane_id !== 'string' || data.pane_id[0] !== '%') return null;
    const n = Number.parseInt(data.pane_id.slice(1), 10);
    return Number.isNaN(n) ? null : n;
  });
  const closeDisabled = $derived(liveCount <= 1 || paneNumeric === null);
  const closeTooltip = $derived(
    closeDisabled && liveCount <= 1
      ? "Last live pane — use Session shutdown"
      : "Close panel"
  );

  let closing = $state(false);

  async function onClose(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    if (closeDisabled || closing) return;
    const client = wsClientHolder?.current;
    if (!client || paneNumeric === null) {
      toastStore.show({ message: 'WebSocket not ready', tone: 'error' });
      return;
    }
    closing = true;
    try {
      // 1) Remove from layout first — so the visual disappears immediately
      //    (responsiveness UX). PUT commits the new state to disk.
      panelsStore.removePanel(data.id);
      const token = readToken();
      if (token !== null) {
        void putLayoutCommitCurrent(token).catch((e) => {
          console.warn('[gtmux] close commit (layout) failed:', e);
        });
      }
      // 2) Fire CTRL kill-pane — backend kills PTY + child shell,
      //    broadcasts pane-died NOTIFY → dispatcher updates muxStore.
      const { response } = sendCtrl(client, 'kill-pane', [String(paneNumeric)], {
        timeoutMs: 5_000,
      });
      const r = await response;
      if (!r.ok) {
        toastStore.show({
          message: `kill-pane failed: ${r.code ?? '?'} ${r.error ?? ''}`,
          tone: 'error',
        });
      }
    } catch (e) {
      toastStore.show({
        message: `Close failed: ${(e as Error).message ?? e}`,
        tone: 'error',
      });
    } finally {
      closing = false;
    }
  }
</script>

{#if isVisible}
  <div
    class="panel"
    class:m-single={isSingleM}
    class:m-multi={isMultiM}
    class:i-active={isInI}
    class:locked={isLocked}
    style="width: {panelW}px; height: {panelH}px;"
    role="group"
    aria-label={`Panel ${headerLabel}`}
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked}
      minWidth={240}
      minHeight={140}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeEnd}
    />
    <header class="panel-header" aria-label={`Drag handle for ${headerLabel}`}>
      <span class="panel-label">{headerLabel}</span>
      <span class="panel-actions">
        <span class="panel-badges">
          {#if isLocked}
            <span class="badge badge-lock" aria-label="Locked">L</span>
          {/if}
          {#if data.minimized === true}
            <span class="badge badge-min" aria-label="Minimized">M</span>
          {/if}
          {#if isInI}
            <span class="badge badge-input" aria-label="Input target">I</span>
          {/if}
        </span>
        <button
          type="button"
          class="panel-close"
          aria-label={closeTooltip}
          title={closeTooltip}
          disabled={closeDisabled || closing}
          onclick={onClose}
          onmousedown={(e: MouseEvent) => e.stopPropagation()}
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <line x1="18" y1="6" x2="6" y2="18"/>
            <line x1="6" y1="6" x2="18" y2="18"/>
          </svg>
        </button>
      </span>
    </header>
    <div class="panel-body">
      {#if isStreaming && typeof data.pane_id === 'string'}
        <XtermHost paneId={data.pane_id.replace(/^%/, '')} />
      {/if}
    </div>
  </div>
{/if}

<style>
  .panel {
    display: flex;
    flex-direction: column;
    background: var(--color-surface);
    color: var(--color-fg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    overflow: hidden;
    box-sizing: border-box;
    font-family: var(--font-mono);
    font-size: var(--text-lg);
  }

  /* Single-select — solid 1.5px accent (Figma signature). */
  .panel.m-single {
    outline: 1.5px solid var(--color-accent);
    outline-offset: 0;
  }
  .panel.m-single .panel-header {
    background: color-mix(in srgb, var(--color-accent) 12%, var(--color-surface-2));
    border-bottom-color: var(--color-accent);
  }

  /* Multi-select — dashed 2px accent + 헤더 색조 강화. */
  .panel.m-multi {
    outline: 2px dashed var(--color-accent);
    outline-offset: 0;
  }
  .panel.m-multi .panel-header {
    background: color-mix(in srgb, var(--color-accent) 22%, var(--color-surface-2));
    border-bottom-color: var(--color-accent);
  }

  .panel.i-active {
    border-color: var(--color-success);
  }

  .panel.locked .panel-header {
    cursor: default;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-8);
    height: 24px;
    background: var(--color-surface-2);
    border-bottom: 1px solid var(--color-border);
    cursor: grab;
    user-select: none;
    flex: 0 0 auto;
    transition:
      background var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing);
  }

  .panel-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }

  .panel-actions {
    display: inline-flex;
    align-items: center;
    gap: var(--space-4);
  }

  .panel-badges {
    display: inline-flex;
    gap: var(--space-4);
  }

  /* Close button — 16×16 ghost. disabled 시 opacity 낮춤 + cursor 변경. */
  .panel-close {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: var(--radius-sm);
    background: transparent;
    border: 0;
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing),
      opacity var(--motion-fast) var(--motion-easing);
  }

  .panel-close:hover:not(:disabled) {
    background: var(--color-danger);
    color: white;
  }

  .panel-close:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .badge {
    display: inline-block;
    min-width: 16px;
    padding: 0 var(--space-4);
    border-radius: var(--radius-sm);
    text-align: center;
    font-size: var(--text-sm);
    line-height: 16px;
    background: var(--color-glass-2);
    color: var(--color-fg-muted);
  }

  .badge-lock {
    background: var(--color-fg-subtle);
    color: var(--color-bg);
  }

  .badge-min {
    background: var(--color-warning);
    color: var(--color-bg);
  }

  .badge-input {
    background: var(--color-success);
    color: var(--color-bg);
  }

  .panel-body {
    flex: 1 1 auto;
    min-height: 0;
    position: relative;
    background: var(--color-bg);
  }

  /* NodeResizer handle / line styling (Figma white-fill with accent border). */
  :global(.panel-resize-handle) {
    background: var(--color-bg) !important;
    border: 1.5px solid var(--color-accent) !important;
    width: 7px !important;
    height: 7px !important;
    border-radius: 1px !important;
  }
  :global(.panel-resize-line) {
    border-color: var(--color-accent) !important;
  }
</style>
