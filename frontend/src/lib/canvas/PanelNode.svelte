<script lang="ts">
  // Svelte Flow custom node — Panel chrome + xterm body.
  //
  // 책임:
  // - `data` (NodeProps의 data prop) = PanelData (canvas-layout-schema §1 Panel JSON) +
  //   Canvas.svelte 가 추가로 주입한 m_multi 플래그 (M.size > 1).
  // - 헤더 바 = drag handle. label + badges (L/M/Min/I).
  // - 본문 = XtermHost.
  // - 선택 시각 (M):
  //     * outline 은 wrapper (.svelte-flow__node.selected) 가 단일 source —
  //       1.5px accent box-shadow ring (Canvas.svelte §05 shared rules B).
  //     * single (.m-single) / multi (.m-multi) 는 *비-outline* 시각 단서:
  //       header 색조 변화 (.m-single .panel-header / .m-multi .panel-header).
  //       multi 시 multi-drag affordance 강조.
  // - resize : NodeResizer (corner + edge handles). onResizeEnd 시 sessionStore
  //   + PUT /api/sessions/<name>/layout 으로 영속화.
  // - visibility=false → 렌더 X.

  import { getContext } from 'svelte';
  import { NodeResizer } from '@xyflow/svelte';
  import PanelDanglingOverlay from './PanelDanglingOverlay.svelte';
  import XtermHost from './XtermHost.svelte';
  import InlineEditField from '$lib/common/InlineEditField.svelte';
  import PanelCloseConfirmModal from '$lib/chrome/PanelCloseConfirmModal.svelte';
  import { ensureMutationOk, sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { danglingTerminals } from '$lib/stores/danglingTerminals.svelte';
  import { UnauthorizedError } from '$lib/http/sessions';
  import { patchTerminalLabel, TERMINAL_LABEL_MAX_BYTES } from '$lib/http/terminals';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import type { CanvasItem, TerminalItem } from '$lib/types/canvas';

  // ContextMenu summoner — header (…) button opens the same menu the
  // right-click would (ADR-0017 §D2 amend: panel header more menu).
  interface ContextMenuHolder {
    openAt: (args: { clientX: number; clientY: number; paneId?: string | null; panelId?: string | null }) => void;
  }
  const contextMenuHolder = getContext<ContextMenuHolder | undefined>('contextMenu');

  function onMoreClick(e: MouseEvent): void {
    e.stopPropagation();
    if (!contextMenuHolder) return;
    const btn = e.currentTarget as HTMLElement | null;
    const rect = btn?.getBoundingClientRect();
    contextMenuHolder.openAt({
      clientX: rect ? rect.right : e.clientX,
      clientY: rect ? rect.bottom + 4 : e.clientY,
      paneId: typeof data.pane_id === 'string' ? data.pane_id : null,
      panelId: data.id,
    });
  }

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
  // Maximize 는 in-flow geom override 패턴 — schema 변경 없이 wrapper 의
  // position/dimensions 만 override. XtermHost 는 정상 유지 (content/스크롤 보존).
  // pan/zoom 및 resize 는 Canvas.svelte / NodeResizer.isVisible 에서 disable.
  const isStreaming = $derived(isVisible && data.minimized !== true);
  // Label source priority (Task 2 fix):
  //   1) terminalPool 의 terminal_meta label (server-wide, PATCH /api/terminals 의
  //      single source of truth — ADR-0021 D7 + terminals.rs:46-48). 빈 문자열은
  //      미설정 으로 간주.
  //   2) layout item.label (legacy — disk 의 layout file 안 stale 가능)
  //   3) pane_id / id fallback.
  // 옛 우선 (data.label → pane_id) 은 session 진입 시 회귀 — layout 안 label 이
  // PATCH 와 join 되지 않아 stale. terminal_meta 우선이 정답.
  const headerLabel = $derived(
    terminalPool.byId(data.id)?.label?.trim() || data.label || data.pane_id || data.id,
  );

  const isInM = $derived(selected || sessionStore.M.has(data.id));
  const isMultiM = $derived(isInM && data.m_multi === true);
  const isSingleM = $derived(isInM && data.m_multi !== true);

  const isInI = $derived(
    typeof data.pane_id === 'string' && sessionStore.I === data.pane_id
  );

  type ResizeParams = { x: number; y: number; width: number; height: number };
  const isLocked = $derived(data.locked === true);

  // NodeResizer onResizeEnd — { event, params: { x, y, width, height } }.
  // Resize 도중에는 SvelteFlow 가 controlled width/height 를 자체 업데이트
  // 하므로 본 핸들러는 *최종 값만* store + disk 로 commit (drag 와 동일 패턴).
  function onResizeEnd(_event: unknown, params: ResizeParams) {
    const nextW = Math.max(240, params.width);
    const nextH = Math.max(140, params.height);
    void sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'terminal'
            ? ({
                ...it,
                x: params.x,
                y: params.y,
                w: nextW,
                h: nextH,
              } as TerminalItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Resize aborted — session reconnect failed.',
        failMessage: 'Resize failed',
      },
    );
  }

  /**
   * Terminal item 의 UUID→PaneId binding (0x88 TERMINAL_SPAWNED 가 source).
   * undefined → spawn 직후 또는 dangling 상태 → connecting placeholder.
   * 정수 → XtermHost mount 가능.
   */
  const terminalPaneId = $derived(terminalPool.paneIdFor(data.id));

  /**
   * Header LED 의 4-state — terminalPool.alive / paneId binding /
   * danglingTerminals 결합.
   *   running    — pool 에 alive + paneId bound. 정상 streaming.
   *   connecting — pool 에 alive 인데 paneId 미 binding (0x88 대기) — spawn 직후.
   *   dangling   — danglingTerminals 에 있음 (0x85 terminal-died 수신 / respawn 대기).
   *   offline    — pool 에 없음 (loading 중 또는 backend 측 unknown).
   */
  type StatusKind = 'running' | 'connecting' | 'dangling' | 'offline';
  const statusKind = $derived.by((): StatusKind => {
    if (danglingTerminals.has(data.id)) return 'dangling';
    const t = terminalPool.byId(data.id);
    if (t === null) return 'offline';
    if (!t.alive) return 'offline';
    if (terminalPaneId === undefined) return 'connecting';
    return 'running';
  });

  // Multi-session terminal kill 은 mirror 보호를 위해 PanelCloseConfirmModal
  // 안에서 사용자 명시 선택. close 버튼 자체는 항상 enabled.
  const closeTooltip = 'Close panel';

  let closing = $state(false);
  let confirmOpen = $state(false);

  /** 현 panel 의 terminal 이 다른 session 에서 reference 되는 list (현 session 제외). */
  const otherSessions = $derived.by((): string[] => {
    const active = sessionStore.active;
    if (active === null) return [];
    const t = terminalPool.byId(data.id);
    if (t === null) return [];
    return t.attached_sessions.filter((s) => s !== active.name);
  });

  const attachCount = $derived.by((): number => {
    const t = terminalPool.byId(data.id);
    return t?.attach_count ?? 0;
  });

  function onClose(e: MouseEvent): void {
    e.stopPropagation();
    if (closing) return;
    confirmOpen = true;
  }

  // ─ Inline label rename (0033 §8.2 P1 — InlineEditField consumer wire) ─
  //
  // terminal panel header label 을 더블 클릭 → 인라인 편집 → commit 시
  // PATCH /api/terminals/:id { label }. terminalPool 즉시 refresh 로 다른
  // surface (TerminalsPanel, PaneInfoPanel) 와 정합.
  let labelEditing = $state(false);
  let labelCommitting = $state(false);

  function validateLabel(s: string): string | null {
    const bytes = new TextEncoder().encode(s).length;
    if (bytes > TERMINAL_LABEL_MAX_BYTES) {
      return `Label too long (${bytes} / ${TERMINAL_LABEL_MAX_BYTES} bytes).`;
    }
    return null;
  }

  function onLabelStartEdit(e: MouseEvent): void {
    // 더블 클릭만 trigger — 일반 클릭은 drag handle 로 통과.
    e.stopPropagation();
    labelEditing = true;
  }

  async function onLabelCommit(next: string): Promise<void> {
    const trimmed = next.trim();
    if (trimmed === (data.label ?? '')) {
      labelEditing = false;
      return;
    }
    if (!(await ensureMutationOk('Label rename aborted — session reconnect failed.'))) return;
    labelCommitting = true;
    try {
      await patchTerminalLabel(data.id, trimmed);
      // sessionStore.items 안 label 도 갱신 — layout 의 다음 GET 으로 정합되지만
      // immediate visual feedback 을 위해 in-memory 도 동시 set.
      const cur = sessionStore.items.get(data.id);
      if (cur !== undefined) {
        sessionStore.items.set(data.id, { ...cur, label: trimmed });
      }
      void terminalPool.refresh();
      labelEditing = false;
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Rename failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    } finally {
      labelCommitting = false;
    }
  }

  async function performClose(killTerminal: boolean): Promise<void> {
    confirmOpen = false;
    if (sessionStore.active === null) return;
    closing = true;
    try {
      const { ok, fail } = await sessionStore.applyDeletion([data.id], {
        killTerminal,
        abortMessage: 'Session reconnect failed — close aborted.',
      });
      if (ok > 0) {
        void terminalPool.refresh();
        toastStore.show({
          message: killTerminal
            ? `Panel + terminal closed.${otherSessions.length > 0 ? ` ${otherSessions.length} mirror panel(s) now dangling.` : ''}`
            : 'Panel removed. Terminal still in pool.',
          tone: 'success',
        });
      } else if (fail > 0) {
        toastStore.show({
          message: 'Close failed.',
          tone: 'error',
        });
      }
    } finally {
      closing = false;
    }
  }

  // ref/frontend-design/components.html §04 — min/max button 핸들러.
  // min: items[].minimized toggle + mutateLayout PUT (Layer list 와 동일 path).
  // max: sessionStore.toggleMaximize (in-memory ephemeral).
  const isMaximized = $derived(sessionStore.maximizedItemId === data.id);

  // Minimize / Maximize 모두 schema item geometry (x, y, w, h) 변경 패턴.
  // 옛 값은 sessionStore.restoredItemGeoms 에 in-memory backup. restore 시 복원.
  //
  // - Minimize: h 만 32 으로 set, x/y/w 도 함께 백업 (단순화 — restore 시 일괄 복원)
  // - Maximize: 전체 (x, y, w, h) 를 canvas viewport 의 visible extent 로 set.
  //   flow coord = canvas DOM client size / zoom + viewport offset.
  const MIN_HEADER_H = 32;
  const RESTORE_DEFAULT_H = 220;

  async function onMinimizeClick(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    e.preventDefault();
    if (sessionStore.active === null) return;
    const cur = sessionStore.items.get(data.id);
    if (cur === undefined) return;
    const wasMinimized = cur.minimized === true;
    const next = !wasMinimized;
    let nextH = cur.h;
    if (next === true) {
      sessionStore.backupItemGeom(data.id, { x: cur.x, y: cur.y, w: cur.w, h: cur.h });
      nextH = MIN_HEADER_H;
    } else {
      const backup = sessionStore.getRestoredGeom(data.id);
      nextH = backup !== null ? backup.h : RESTORE_DEFAULT_H;
      sessionStore.clearRestoredGeom(data.id);
    }
    await sessionStore.applyMutation(
      (cur2) => ({
        ...cur2,
        items: cur2.items.map((it) =>
          it.id === data.id
            ? ({ ...it, minimized: next, h: nextH } as typeof it)
            : it,
        ),
      }),
      {
        abortMessage: 'Minimize aborted — session reconnect failed.',
        failMessage: 'Minimize failed',
      },
    );
  }

  function onMaximizeClick(e: MouseEvent): void {
    e.stopPropagation();
    e.preventDefault();
    // Maximize 는 modal overlay (MaximizedPanelModal) 로 처리 — schema 의 geom
    // 변경 없음. sessionStore.maximizedItemId 토글 만으로 modal 가시성 결정.
    // Canvas.svelte 가 maximizedItemId 를 watch 해 modal 렌더링 + pan/zoom 잠금.
    sessionStore.toggleMaximize(data.id);
  }
</script>

{#if isVisible}
  <div
    class="panel"
    class:m-single={isSingleM}
    class:m-multi={isMultiM}
    class:i-active={isInI}
    class:locked={isLocked}
    class:minimized={data.minimized === true}
    style="width: 100%; height: 100%;"
    role="group"
    aria-label={`Panel ${headerLabel}`}
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked && !isMaximized}
      minWidth={240}
      minHeight={140}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResizeEnd}
    />
    <header class="panel-header" aria-label={`Drag handle for ${headerLabel}`}>
      <!-- ref/frontend-design/components.html §04 — panel glyph (terminal icon). -->
      <svg class="panel-glyph" viewBox="0 0 13 13" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="1" y="1.6" width="11" height="9.8" rx="1.4"/>
        <path d="M3 5l1.8 1.4L3 7.8"/>
        <path d="M6 8.4h4"/>
      </svg>
      {#if labelEditing}
        <span class="panel-label-host" role="presentation">
          <InlineEditField
            value={data.label ?? ''}
            editing={true}
            allowEmpty={true}
            plain={true}
            placeholder={data.id.slice(0, 8)}
            class="panel-label-edit"
            validate={validateLabel}
            onCommit={(next: string) => void onLabelCommit(next)}
            onCancel={() => (labelEditing = false)}
          />
          {#if labelCommitting}
            <span class="panel-label-saving" aria-hidden="true">…</span>
          {/if}
        </span>
      {:else}
        <span
          class="panel-title panel-label-editable"
          title="Double-click to rename"
          ondblclick={onLabelStartEdit}
          role="presentation"
        >{headerLabel}</span>
      {/if}
      <!-- Status LED — terminalPool.alive + paneId binding + danglingTerminals
           결합으로 running / connecting / dangling / offline 4-state. -->
      <span
        class="panel-status"
        data-status={statusKind}
        aria-label={`Panel status: ${statusKind}`}
      >
        <span class="led" aria-hidden="true"></span>
        <span class="status-label">{statusKind}</span>
      </span>
      <div class="panel-actions">
        {#if isInI}
          <span class="badge badge-input" aria-label="Input target" title="Input target">I</span>
        {/if}
        {#if isLocked}
          <span class="badge badge-lock" aria-label="Locked" title="Locked">L</span>
        {/if}
        <button
          type="button"
          class="panel-btn"
          class:is-active={data.minimized === true}
          aria-label={data.minimized === true ? 'Restore' : 'Minimize'}
          title={data.minimized === true ? 'Restore' : 'Minimize'}
          onclick={(e) => void onMinimizeClick(e)}
          onmousedown={(e: MouseEvent) => e.stopPropagation()}
        >
          {#if data.minimized === true}
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
              <path d="M3 5.5h6"/><path d="M3 8.5h6"/>
            </svg>
          {:else}
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
              <path d="M3 8.5h6"/>
            </svg>
          {/if}
        </button>
        <button
          type="button"
          class="panel-btn"
          class:is-active={isMaximized}
          aria-label={isMaximized ? 'Restore' : 'Maximize'}
          title={isMaximized ? 'Restore' : 'Maximize'}
          onclick={onMaximizeClick}
          onmousedown={(e: MouseEvent) => e.stopPropagation()}
        >
          {#if isMaximized}
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
              <rect x="2" y="3.6" width="6.5" height="6.4" rx="0.5"/>
              <path d="M4 3.6V2.4h6.5v6.4H9"/>
            </svg>
          {:else}
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
              <rect x="2.5" y="2.5" width="7" height="7" rx="0.6"/>
            </svg>
          {/if}
        </button>
        <button
          type="button"
          class="panel-btn"
          aria-label="Panel actions"
          aria-haspopup="menu"
          title="Panel actions (arrange · change terminal · remove)"
          onclick={onMoreClick}
          onmousedown={(e: MouseEvent) => e.stopPropagation()}
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="currentColor" aria-hidden="true">
            <circle cx="2.4" cy="6" r="1"/>
            <circle cx="6" cy="6" r="1"/>
            <circle cx="9.6" cy="6" r="1"/>
          </svg>
        </button>
        <button
          type="button"
          class="panel-btn close"
          aria-label={closeTooltip}
          title={closeTooltip}
          disabled={closing}
          onclick={onClose}
          onmousedown={(e: MouseEvent) => e.stopPropagation()}
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
            <path d="M3 3l6 6M9 3l-6 6"/>
          </svg>
        </button>
      </div>
    </header>
    <div class="panel-body">
      {#if isStreaming}
        {#if terminalPaneId !== undefined}
          <!-- xterm-portal-host: MaximizedItemModal 이 maximize 시 본 div 의
               XtermHost DOM (containerEl) 을 modal 의 slot 으로 reparent.
               XtermHost 인스턴스는 PanelNode 가 계속 마운트 유지 — 단일 xterm
               인스턴스 가 in-flow ↔ modal 로 DOM 만 이동, state/scroll 보존. -->
          <div class="xterm-portal-host" data-portal-id={data.id}>
            <XtermHost paneId={String(terminalPaneId)} />
          </div>
        {:else}
          <div class="panel-pending" role="status" aria-live="polite">
            <div class="pending-title">Terminal stream connecting…</div>
            <div class="pending-hint">Waiting for spawn handshake.</div>
          </div>
        {/if}
      {/if}
      <PanelDanglingOverlay terminalId={data.id} />
    </div>
  </div>
{/if}

<PanelCloseConfirmModal
  open={confirmOpen}
  panelLabel={typeof data.label === 'string' && data.label.length > 0 ? data.label : data.id.slice(0, 8)}
  {attachCount}
  {otherSessions}
  onCancel={() => (confirmOpen = false)}
  onPanelOnly={() => void performClose(false)}
  onPanelAndTerminal={() => void performClose(true)}
/>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    background: var(--color-surface);
    color: var(--color-fg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    /* NoteNode 의 §6 fix 와 동일 패턴 — corner resize handle 의 negative
       offset 이 panel 의 edge 밖으로 나가도 clip 되지 않게. inner content
       (xterm) 의 overflow 는 .panel-body 의 overflow:hidden 으로 격리. */
    overflow: visible;
    box-sizing: border-box;
    font-family: var(--font-mono);
    font-size: var(--text-lg);
  }

  /* Single-select — solid 1.5px accent (Figma signature). */
  .panel.m-single {
    outline: none;
  }
  .panel.m-single .panel-header {
    background: color-mix(in srgb, var(--color-accent) 12%, var(--color-surface-2));
    border-bottom-color: var(--color-accent);
  }

  /* Multi-select — dashed 2px accent + 헤더 색조 강화. */
  .panel.m-multi {
    outline: none;
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

  /* ref/frontend-design/components.html §04 — panel head 정합 */
  .panel-header {
    display: flex;
    align-items: center;
    gap: var(--space-6);
    padding: 0 6px 0 12px;
    height: 32px;
    background: var(--color-surface-2);
    border-bottom: 1px solid var(--color-border);
    cursor: grab;
    user-select: none;
    flex: 0 0 auto;
    transition:
      background var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing);
  }

  .panel-glyph {
    width: 13px;
    height: 13px;
    color: var(--color-fg-muted);
    flex: 0 0 auto;
  }

  .panel-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 540;
    letter-spacing: 0.2px;
    color: var(--color-fg);
    flex: 0 1 auto;
    min-width: 0;
  }

  .panel-label-editable {
    cursor: text;
  }

  .panel-label-host {
    display: inline-flex;
    align-items: center;
    gap: var(--space-4);
    flex: 1 1 auto;
    min-width: 0;
  }

  .panel-label-host :global(.panel-label-edit) {
    /* InlineEditField input — panel-title typography 동기. */
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 540;
    letter-spacing: 0.2px;
    height: 22px;
    min-width: 0;
  }

  .panel-label-saving {
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
    flex: 0 0 auto;
  }

  /* Status — LED + uppercase label. margin-left: auto 로 좌측 title 과 우측 actions
   * 사이의 공간 점유. 색은 data-status 분기:
   *   running    → success (green)
   *   connecting → warning (amber)
   *   dangling   → danger (red)
   *   offline    → fg-muted (grey, default)
   */
  .panel-status {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-left: auto;
    flex: 0 0 auto;
    font-family: var(--font-mono);
    font-size: 9px;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
  }

  .panel-status .led {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-fg-muted);
    flex: 0 0 auto;
  }

  .panel-status[data-status='running'] .led {
    background: var(--color-success);
  }

  .panel-status[data-status='connecting'] .led {
    background: var(--color-warning);
  }

  .panel-status[data-status='dangling'] .led {
    background: var(--color-danger);
  }

  .panel-actions {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    flex: 0 0 auto;
  }

  /* 22×22 ghost button — panel-btn (시안 §04). close 변형: red on hover. */
  .panel-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 4px;
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

  .panel-btn:hover:not(:disabled) {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  .panel-btn.is-active {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  .panel-btn.close:hover:not(:disabled) {
    background: #e5484d;
    color: #ffffff;
  }

  .panel-btn:disabled {
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

  /* .badge-min 제거 — minimize 상태는 panel-btn 의 is-active 로 표시. */

  .badge-input {
    background: var(--color-success);
    color: var(--color-bg);
  }

  .panel-body {
    flex: 1 1 auto;
    min-height: 0;
    position: relative;
    /* xterm theme.background 와 동기 — .xterm-screen 의 cell-정수배수 px
     * height 와 컨테이너 사이 잔여 영역이 같은 색이라 resize 중에도
     * 검은색 갭이 노출되지 않음. */
    background: var(--xterm-bg);
    /* xterm 의 .xterm-viewport / .xterm-screen 이 cell-정수배 inline-px
     * height 를 가져 panel-body 보다 클 수 있음 → 하단 resize handle 영역
     * 침범. panel 의 overflow:visible 은 corner handle visibility 보장용 —
     * inner content overflow 는 본 .panel-body 가 격리. */
    overflow: hidden;
  }

  /* xterm DOM portal — XtermHost 의 containerEl 가 본 div 의 직접 child.
     Modal 이 active 시 본 div 가 비어있고, 내부 child 는 modal 의 slot 으로
     reparent (JS appendChild). flex/size 는 maximized 시에도 동일하게 활용. */
  .xterm-portal-host {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  :global(.xterm-portal-host > :first-child) {
    flex: 1 1 auto;
    min-height: 0;
  }

  /* Minimized — header only, body hide, header bottom border 제거 (시안 §04). */
  .panel.minimized .panel-body {
    display: none;
  }

  .panel.minimized .panel-header {
    border-bottom: 0;
  }

  .panel-pending {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-4);
    padding: var(--space-12);
    color: var(--color-fg-muted);
    text-align: center;
    font-family: var(--font-sans);
    background:
      repeating-linear-gradient(
        135deg,
        transparent 0,
        transparent 14px,
        color-mix(in srgb, var(--color-fg-muted) 6%, transparent) 14px,
        color-mix(in srgb, var(--color-fg-muted) 6%, transparent) 16px
      );
  }

  .pending-title {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    color: var(--color-fg);
  }

  .pending-hint {
    font-size: var(--text-sm);
    max-width: 28ch;
    line-height: 1.4;
  }

  /* NodeResizer handle / line styling (Figma white-fill with accent border).
     line 은 wrapper selection 의 box-shadow (1.5px accent) 와 시각 중복 — 비활성.
     edge resize 는 cursor 만 표시되고 line 은 그리지 않음. */
  :global(.panel-resize-handle) {
    background: var(--color-bg) !important;
    border: 1.5px solid var(--color-accent) !important;
    width: 7px !important;
    height: 7px !important;
    border-radius: 1px !important;
  }
  :global(.panel-resize-line) {
    border-color: transparent !important;
  }
</style>
