<script lang="ts">
  // Svelte Flow 캔버스 host — R3 보고서 채택, ADR-0012 D2/D5/D6.
  //
  // 책임:
  // - `sessionStore.items` (SvelteMap<string, CanvasItem>) → Svelte Flow `nodes` 매핑.
  //   `$derived`로 entry-level fine-grain reactivity (R8 §F3).
  // - viewport (`sessionStore.viewport`) bind — pan/zoom 양방향 sync. PUT 은
  //   updateViewport 의 500ms debounce.
  // - 노드 드래그 → sessionStore.items 갱신 + mutateLayout PUT.
  // - 노드 클릭 → M selection 갱신.
  //     * plain click       : M = [id] (single — Figma 컨벤션)
  //     * Cmd click          : M.toggle(id) (multi-select 추가/제거)
  // - 캔버스 dot grid 는 token-driven (--canvas-bg, --canvas-grid).
  // - panOnDrag = [1, 2] — middle/right 마우스 버튼만 pan (left는 selection/drag용).

  import { onDestroy, onMount, untrack } from 'svelte';
  import { SvelteFlow, Background, BackgroundVariant, useSvelteFlow } from '@xyflow/svelte';
  import type { Node, Viewport } from '@xyflow/svelte';
  import '@xyflow/svelte/dist/style.css';
  import { debugCount } from '$lib/common/debugCounts';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { toolStore } from '$lib/stores/toolStore.svelte';
  import { chromeStore } from '$lib/stores/chrome.svelte';
  import { attachConfirm, UnauthorizedError } from '$lib/http/sessions';
  import { killTerminal } from '$lib/http/terminals';
  import { uploadAsset, AssetUploadUnavailableError } from '$lib/http/assets';
  import type { Anchor, CanvasItem, CanvasLayout, PathEndpoint, Point } from '$lib/types/canvas';
  import {
    descendantGroups,
    descendantItems,
    effectiveLocked,
    effectiveVisibility,
    pruneEmptyGroups,
    targetAtDrillLevel,
  } from '$lib/types/group';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { panelCloseDialog } from '$lib/stores/panelCloseDialog.svelte';
  import FilePickerModal from '$lib/chrome/FilePickerModal.svelte';
  import { filePicker } from '$lib/stores/filePicker.svelte';
  import { pickLocalFile } from '$lib/files/localFilePicker';
  import PanelNode from './PanelNode.svelte';
  import TextNode from './TextNode.svelte';
  import NoteNode from './NoteNode.svelte';
  import FilePathNode from './FilePathNode.svelte';
  import ShapeNode from './ShapeNode.svelte';
  import LineNode from './LineNode.svelte';
  import PathNode from './PathNode.svelte';
  import ImageNode from './ImageNode.svelte';
  import DocumentNode from './DocumentNode.svelte';
  import FreeDrawNode from './FreeDrawNode.svelte';
  import SnippetsNode from './SnippetsNode.svelte';
  import GroupOverlay from './GroupOverlay.svelte';
  import { groupHover } from '$lib/stores/groupHover.svelte';
  import {
    commitNewItem,
    createCanvasItem,
    createShapeItem,
    createLineItem,
    createPathItem,
    createTerminalItem,
    createImageItem,
    createDocumentItem,
    createFreeDrawItem,
    createSnippetsItem,
    lineBoxFromEndpoints,
    DEFAULT_TERMINAL_SIZE,
    DEFAULT_NOTE_SIZE,
    DEFAULT_FILE_PATH_SIZE,
    DEFAULT_IMAGE_SIZE,
    DEFAULT_DOCUMENT_SIZE,
    DEFAULT_SNIPPETS_SIZE,
  } from './itemFactory';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { projectPointToAngle } from './resizeConstraint';
  import {
    anchorPoint,
    autoRoutePath,
    connectableTargetAtPoint,
    computePathBBox,
    hasConnectedEndpoint,
    isPathConnectedToAny,
    nearestAnchor,
    translatePath,
    updatePathBBoxCache,
  } from './pathGeometry';
  import { pathEditStore } from '$lib/stores/pathEditStore.svelte';

  interface CanvasProps {
    /** ContextMenu trigger — `+page.svelte` 가 호스팅하는 ContextMenu
     *  싱글톤의 `openAt` 으로 wire. `null/undefined` 시 컨텍스트 메뉴
     *  비활성 — Canvas 내부 동작에 영향 없음. */
    onContextMenuRequest?: (args: {
      clientX: number;
      clientY: number;
      paneId?: string | null;
      panelId?: string | null;
      /** ADR-0010 D16 + plan-0012 §3.4 D.6 — group entity ContextMenu 진입. */
      groupId?: string | null;
    }) => void;
  }

  const { onContextMenuRequest }: CanvasProps = $props();

  // SvelteFlow viewport projection — onpaneclick 의 screen 좌표를 canvas 좌표로 변환.
  // useSvelteFlow 는 SvelteFlowProvider 컨텍스트가 있어야 동작 (+page.svelte 에서 마운트됨).
  const { screenToFlowPosition, setViewport, getViewport } = useSvelteFlow();
  let applyingStoreViewport = false;

  /** Drag-to-create state — rect/ellipse/line + free_draw. */
  type DragShape = 'rect' | 'ellipse' | 'line' | 'free_draw';
  interface DragState {
    tool: DragShape;
    /** Flow-coord start point (commit 시 기준). */
    startFlow: { x: number; y: number };
    /** Container-local screen coord — ghost overlay 의 left/top 계산용. */
    startLocal: { x: number; y: number };
    currentLocal: { x: number; y: number };
    shiftKey: boolean;
    lineShiftAngle: number | null;
  }
  let dragState = $state<DragState | null>(null);
  interface PathCreateStart {
    endpoint: PathEndpoint;
    point: Point;
    local: Point;
  }
  let pathCreateStart = $state<PathCreateStart | null>(null);
  type PathAnchorCandidate = {
    endpoint: PathEndpoint;
    point: Point;
    local: Point;
    box: { x: number; y: number; w: number; h: number };
    anchors: {
      anchor: Anchor;
      x: number;
      y: number;
      nearest: boolean;
      hovered: boolean;
    }[];
  };
  interface GroupDragState {
    pointerId: number;
    startFlow: { x: number; y: number };
    currentFlow: { x: number; y: number };
    startClient: { x: number; y: number };
    currentClient: { x: number; y: number };
    originals: Map<string, CanvasItem>;
    priorSnapshot: CanvasLayout;
    moved: boolean;
  }
  let groupDragState = $state<GroupDragState | null>(null);
  interface LassoState {
    pointerId: number;
    startFlow: { x: number; y: number };
    currentFlow: { x: number; y: number };
    startClient: { x: number; y: number };
    currentClient: { x: number; y: number };
    startLocal: { x: number; y: number };
    currentLocal: { x: number; y: number };
  }
  let lassoState = $state<LassoState | null>(null);
  let canvasRootEl: HTMLElement | null = null;
  let windowGesturePointerId: number | null = null;
  let suppressNextPaneClickAfterLasso = false;
  let suppressPaneClickTimer: ReturnType<typeof setTimeout> | null = null;

  /** ADR-0018 D4 — free_draw point cap (저장 상한). */
  const FREE_DRAW_MAX_POINTS = 5000;

  /** 0065 FE-1 — free_draw 입력 최소거리 prune (screen px²). 0.5 px 미만 sample
   *  drop → typical 100-1000 Hz pointer event 가 1/4~1/100 로 압축. */
  const FREE_DRAW_MIN_POINT_DELTA_SQ = 0.5 * 0.5;

  /**
   * 0065 FE-1 — free_draw 입력 buffer. **비반응** plain array — pointermove
   * 마다의 spread copy + $state flush 비용 제거. ghostPreview 재계산 trigger
   * 는 `freeDrawFrame` 의 rAF-당-1회 bump 로 coalesce.
   */
  let freeDrawPoints: { x: number; y: number }[] = [];
  let freeDrawPointsLocal: { x: number; y: number }[] = [];
  let freeDrawFrame = $state(0);
  let freeDrawRafId: number | null = null;

  function resetFreeDrawBuffers(): void {
    freeDrawPoints = [];
    freeDrawPointsLocal = [];
    if (freeDrawRafId !== null) {
      cancelAnimationFrame(freeDrawRafId);
      freeDrawRafId = null;
    }
  }

  function scheduleFreeDrawFrame(): void {
    if (freeDrawRafId !== null) return;
    freeDrawRafId = requestAnimationFrame(() => {
      freeDrawRafId = null;
      freeDrawFrame += 1;
    });
  }

  /**
   * Cursor hover preview — 점-spawn 도구 (terminal/note/file_path/image/
   * document) 가 active 일 때 cursor 위치에 새 item 의 *default 크기* 의
   * 윤곽선을 center-aligned 로 미리보기. zoom 에 따라 screen px 비례
   * (`size_screen = size_flow * zoom`). cursor 가 .canvas-root 밖이거나
   * 노드 위면 null 로 hide. text 는 작아서 (160×56) ghost 의미 약함 — 제외.
   */
  let hoverScreen = $state<{ x: number; y: number } | null>(null);
  let hoverFlow = $state<Point | null>(null);
  const PATH_CONNECT_PREVIEW_MARGIN = 36;
  const PATH_ANCHOR_HOVER_RADIUS = 18;
  const PATH_PREVIEW_ANCHORS: readonly Anchor[] = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW', 'center'];

  /** 점-spawn 도구 중 ghost 표시 + cursor=center spawn 적용 대상. */
  const POINT_SPAWN_DEFAULTS = {
    terminal: DEFAULT_TERMINAL_SIZE,
    note: DEFAULT_NOTE_SIZE,
    file_path: DEFAULT_FILE_PATH_SIZE,
    image: DEFAULT_IMAGE_SIZE,
    document: DEFAULT_DOCUMENT_SIZE,
    snippets: DEFAULT_SNIPPETS_SIZE,
  } as const;
  type GhostTool = keyof typeof POINT_SPAWN_DEFAULTS;
  const isGhostTool = $derived.by((): GhostTool | null => {
    const t = toolStore.current;
    return t in POINT_SPAWN_DEFAULTS ? (t as GhostTool) : null;
  });
  const pointSpawnGhost = $derived.by(() => {
    const t = isGhostTool;
    if (t === null || hoverScreen === null) return null;
    const size = POINT_SPAWN_DEFAULTS[t];
    const zoom = sessionStore.viewport.zoom;
    const w = size.w * zoom;
    const h = size.h * zoom;
    // cursor=center — ghost 의 좌상단 = cursor - (w/2, h/2).
    return {
      tool: t,
      x: hoverScreen.x - w / 2,
      y: hoverScreen.y - h / 2,
      w,
      h,
    };
  });

  const isDragTool = $derived(
    toolStore.current === 'rect' ||
      toolStore.current === 'ellipse' ||
      toolStore.current === 'line' ||
      toolStore.current === 'free_draw',
  );
  const isPathTool = $derived(toolStore.current === 'path');

  // Text tool 은 drag-to-create 아닌 click-to-create — cursor 만 text I-beam.
  const isTextTool = $derived(toolStore.current === 'text');

  $effect(() => {
    if (!isPathTool) pathCreateStart = null;
  });

  /* ── G29: Space-hold pan modifier ────────────────────────────────────
   * Figma convention — Space 를 누르면 cursor=grab, 그 상태에서 left-drag =
   * viewport pan. 평소 left-drag 은 selection box / node move 용이라
   * panOnDrag=[1, 2] (middle/right only). Space 누름 동안 panOnDrag 를
   * [0, 1, 2] 로 동적 전환.
   *
   * 가드 (editable focus 시 skip — InlineEditField / xterm 등):
   *   - activeElement 가 INPUT / TEXTAREA / contenteditable / xterm canvas
   *   - 이 때 Space 는 텍스트 입력 → pan 모드 진입 안 함
   * 동작:
   *   - Space keydown → isSpacePressed=true, preventDefault (페이지 스크롤 방지)
   *   - Space keyup → false
   *   - blur (window focus 잃음) → false (sticky 방지)
   */
  let isSpacePressed = $state(false);

  function isEditableFocused(): boolean {
    if (typeof document === 'undefined') return false;
    const el = document.activeElement as HTMLElement | null;
    if (el === null) return false;
    const tag = el.tagName;
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true;
    if (el.isContentEditable) return true;
    // xterm.js renders a textarea overlay (`.xterm-helper-textarea`).
    if (el.classList.contains('xterm-helper-textarea')) return true;
    return false;
  }

  function onWindowKeyDown(e: KeyboardEvent): void {
    if (e.key === 'Escape' && pathCreateStart !== null) {
      if (isEditableFocused()) return;
      e.preventDefault();
      pathCreateStart = null;
      return;
    }

    // ── Undo / Redo (ADR-0028 D8) — Cmd+Z / Cmd+Shift+Z (mac) / Ctrl+Z /
    // Ctrl+Y (others). editable / xterm focus 시 무시 (native input undo
    // 우선). active session 없으면 sessionStore.undo/redo 가 자체 noop.
    if ((e.metaKey || e.ctrlKey) && !e.altKey) {
      if (e.key === 'z' || e.key === 'Z') {
        if (isEditableFocused()) return;
        e.preventDefault();
        if (e.shiftKey) void sessionStore.redo();
        else void sessionStore.undo();
        return;
      }
      if ((e.key === 'y' || e.key === 'Y') && !e.shiftKey) {
        if (isEditableFocused()) return;
        e.preventDefault();
        void sessionStore.redo();
        return;
      }
    }

    // ── Delete/Backspace — remove selected items (multi-session only) ────
    // SvelteFlow 의 builtin delete (deleteKey={null}) 는 비활성 — store 와
    // 미동기 상태로 nodes 만 임시 제거되어 "사라졌다 돌아오는" 회귀 야기.
    // 본 핸들러가 단독으로 BE `DELETE /api/sessions/.../items/:id` 호출 +
    // sessionStore 동기. terminal item 은 kill_terminal=false 기본 (G25 —
    // panel 제거만, terminal pool 유지). xterm/editable focus 시 무시.
    if (e.key === 'Delete' || e.key === 'Backspace') {
      // Hand mode is viewport-only. Do not allow selected element mutation from
      // keyboard while the cursor tool is active.
      if (isHandTool) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (isEditableFocused()) return;
      if (sessionStore.M.size === 0) return;
      e.preventDefault();
      void deleteSelected();
      return;
    }

    if (e.code !== 'Space') return;
    if (e.repeat) {
      // Already in pan mode — just swallow the default to keep the page from scrolling.
      if (isSpacePressed) e.preventDefault();
      return;
    }
    if (isEditableFocused()) return;
    isSpacePressed = true;
    e.preventDefault();
  }

  /**
   * Remove every item in `sessionStore.M` — ADR-0032 Amend ⑥ 정합. Terminal
   * 포함 시 PanelCloseConfirmModal 경유 (Panel(s) only / Panel(s) + Terminal(s)
   * 선택). ContextMenu 의 Delete / Cut / Clear all path 와 동일 dispatch.
   */
  async function deleteSelected(): Promise<void> {
    const ids = Array.from(sessionStore.M);
    if (ids.length === 0) return;
    const groupIds = ids.filter((id) => sessionStore.groups.has(id));
    if (groupIds.length > 0) {
      const groupsArr = [...sessionStore.groups.values()];
      const itemsArr = [...sessionStore.items.values()];
      const deletedGroupIds = new Set<string>();
      const deletedItemIds = new Set<string>();
      for (const groupId of groupIds) {
        deletedGroupIds.add(groupId);
        for (const g of descendantGroups(groupId, groupsArr)) deletedGroupIds.add(g.id);
        for (const it of descendantItems(groupId, groupsArr, itemsArr)) deletedItemIds.add(it.id);
      }
      for (const id of ids) {
        if (sessionStore.items.has(id)) deletedItemIds.add(id);
      }
      const items = [...deletedItemIds]
        .map((id) => sessionStore.items.get(id))
        .filter((it): it is NonNullable<typeof it> => it !== undefined);
      panelCloseDialog.show({
        items,
        onConfirm: async (killSelectedTerminals) => {
          const itemCount = deletedItemIds.size;
          const result = await sessionStore.applyMutation(
            (cur) =>
              pruneEmptyGroups({
                ...cur,
                groups: cur.groups.filter((g) => !deletedGroupIds.has(g.id)),
                items: cur.items.filter((it) => !deletedItemIds.has(it.id)),
              }),
            { failMessage: 'Delete selected groups failed' },
          );
          if (result.ok) {
            if (
              sessionStore.drillRootId !== null &&
              deletedGroupIds.has(sessionStore.drillRootId)
            ) {
              sessionStore.clearDrill();
            }
            sessionStore.clearM();
            toastStore.show({
              message: `Removed ${deletedGroupIds.size} group${deletedGroupIds.size === 1 ? '' : 's'} + ${itemCount} item${itemCount === 1 ? '' : 's'}${
                killSelectedTerminals ? ' (terminals killed)' : ''
              }.`,
              tone: 'success',
            });
          }
          if (result.ok && killSelectedTerminals) {
            const terminals = items.filter((it) => it.type === 'terminal');
            const results = await Promise.allSettled(terminals.map((it) => killTerminal(it.id)));
            const unauthorized = results.some(
              (r) => r.status === 'rejected' && r.reason instanceof UnauthorizedError,
            );
            if (unauthorized) {
              window.location.href = '/auth';
              return;
            }
            void terminalPool.refresh();
          }
        },
      });
      return;
    }
    const items = ids
      .map((id) => sessionStore.items.get(id))
      .filter((it): it is NonNullable<typeof it> => it !== undefined);
    panelCloseDialog.show({
      items,
      onConfirm: async (killTerminal) => {
        const { ok, fail } = await sessionStore.applyDeletion(ids, {
          killTerminal,
        });
        if (ok === 0 && fail === 0) return;
        if (fail === 0) {
          toastStore.show({
            message: killTerminal
              ? `Removed ${ok} item${ok === 1 ? '' : 's'} (terminals killed).`
              : `Removed ${ok} item${ok === 1 ? '' : 's'}.`,
            tone: 'success',
          });
        } else {
          toastStore.show({
            message: `Removed ${ok}, failed ${fail}.`,
            tone: 'error',
          });
        }
      },
    });
  }

  function onWindowKeyUp(e: KeyboardEvent): void {
    if (e.code !== 'Space') return;
    if (!isSpacePressed) return;
    isSpacePressed = false;
  }

  function onWindowBlur(): void {
    isSpacePressed = false;
  }

  $effect(() => {
    if (typeof window === 'undefined') return;
    window.addEventListener('keydown', onWindowKeyDown);
    window.addEventListener('keyup', onWindowKeyUp);
    window.addEventListener('blur', onWindowBlur);
    return () => {
      window.removeEventListener('keydown', onWindowKeyDown);
      window.removeEventListener('keyup', onWindowKeyUp);
      window.removeEventListener('blur', onWindowBlur);
    };
  });

  /**
   * Reactive SvelteFlow panOnDrag — left button included when Space is held
   * (G29) *or* when Hand tool is active (Figma convention: H = pan mode).
   */
  const isHandTool = $derived(toolStore.current === 'hand');
  // Select mode 만 SvelteFlow 의 selection/drag 활성 — 다른 mode (hand / 도구) 는
  // 캔버스 위 component 선택 금지. (사용자 요구: 입력 중 클릭 시 selection 회귀)
  const isSelectMode = $derived(toolStore.current === 'select');
  // Maximize 동안 viewport pan/zoom 잠금 — panel 이 100% scale 로 캔버스 전면 유지.
  const isMaximizedActive = $derived(sessionStore.maximizedItemId !== null);
  const panOnDragMask = $derived(
    isSpacePressed || isHandTool ? [0, 1, 2] : [1, 2],
  );

  const GHOST_LINE_PADDING = 8;

  /** Live preview overlay geometry (container-local, screen px). */
  const ghostPreview = $derived.by(() => {
    if (dragState === null) return null;
    if (dragState.tool === 'free_draw') {
      // 0065 FE-1 — freeDrawFrame 만 reactive dep. pointermove 마다의
      // dragState reassign 없이 rAF tick (16ms) 마다 1회 재계산.
      void freeDrawFrame;
      const pts = freeDrawPointsLocal;
      const first = pts[0];
      if (first === undefined) return null;
      let minX = first.x, minY = first.y, maxX = first.x, maxY = first.y;
      for (const p of pts) {
        if (p.x < minX) minX = p.x;
        if (p.y < minY) minY = p.y;
        if (p.x > maxX) maxX = p.x;
        if (p.y > maxY) maxY = p.y;
      }
      const left = minX - GHOST_LINE_PADDING;
      const top = minY - GHOST_LINE_PADDING;
      const width = (maxX - minX) + GHOST_LINE_PADDING * 2;
      const height = (maxY - minY) + GHOST_LINE_PADDING * 2;
      const path = pts
        .map((p, i) => `${i === 0 ? 'M' : 'L'}${p.x - left} ${p.y - top}`)
        .join(' ');
      return {
        tool: dragState.tool,
        left,
        top,
        width: Math.max(width, 1),
        height: Math.max(height, 1),
        x1: 0, y1: 0, x2: 0, y2: 0,
        path,
      };
    }
    const sx = dragState.startLocal.x;
    const sy = dragState.startLocal.y;
    let cx = dragState.currentLocal.x;
    let cy = dragState.currentLocal.y;
    if (dragState.tool === 'line') {
      if (dragState.shiftKey && dragState.lineShiftAngle !== null) {
        const projected = projectPointToAngle(
          dragState.startLocal,
          dragState.currentLocal,
          dragState.lineShiftAngle,
        );
        cx = projected.x;
        cy = projected.y;
      }
      const left = Math.min(sx, cx) - GHOST_LINE_PADDING;
      const top = Math.min(sy, cy) - GHOST_LINE_PADDING;
      return {
        tool: dragState.tool,
        left,
        top,
        width: Math.max(Math.abs(cx - sx), 1) + GHOST_LINE_PADDING * 2,
        height: Math.max(Math.abs(cy - sy), 1) + GHOST_LINE_PADDING * 2,
        x1: sx - left,
        y1: sy - top,
        x2: cx - left,
        y2: cy - top,
        path: '',
      };
    }
    return {
      tool: dragState.tool,
      left: Math.min(sx, cx),
      top: Math.min(sy, cy),
      width: Math.max(Math.abs(cx - sx), 1),
      height: Math.max(Math.abs(cy - sy), 1),
      x1: 0,
      y1: 0,
      x2: 0,
      y2: 0,
      path: '',
    };
  });

  const pathCreateAnchorPreview = $derived.by(() => {
    if (!isPathTool || hoverFlow === null) return null;
    return pathAnchorCandidateAt(hoverFlow, pathCreateBlockedTargetId());
  });

  const pathCreatePreview = $derived.by(() => {
    const endLocal = pathCreateAnchorPreview?.local ?? hoverScreen;
    if (pathCreateStart === null || endLocal === null) return null;
    const sx = pathCreateStart.local.x;
    const sy = pathCreateStart.local.y;
    const cx = endLocal.x;
    const cy = endLocal.y;
    const left = Math.min(sx, cx) - GHOST_LINE_PADDING;
    const top = Math.min(sy, cy) - GHOST_LINE_PADDING;
    return {
      left,
      top,
      width: Math.max(Math.abs(cx - sx), 1) + GHOST_LINE_PADDING * 2,
      height: Math.max(Math.abs(cy - sy), 1) + GHOST_LINE_PADDING * 2,
      x1: sx - left,
      y1: sy - top,
      x2: cx - left,
      y2: cy - top,
    };
  });

  const lassoPreview = $derived.by(() => {
    if (lassoState === null) return null;
    const sx = lassoState.startLocal.x;
    const sy = lassoState.startLocal.y;
    const cx = lassoState.currentLocal.x;
    const cy = lassoState.currentLocal.y;
    return {
      left: Math.min(sx, cx),
      top: Math.min(sy, cy),
      width: Math.max(Math.abs(cx - sx), 1),
      height: Math.max(Math.abs(cy - sy), 1),
    };
  });

  /** Drag 가 click 으로 취급되는 임계. Lasso/group-drag 판정은 screen px 기준. */
  const DRAG_CLICK_THRESHOLD = 8;

  function canvasLocalFromClient(clientX: number, clientY: number): { x: number; y: number } {
    const rect = canvasRootEl?.getBoundingClientRect();
    if (rect === undefined) return { x: clientX, y: clientY };
    return { x: clientX - rect.left, y: clientY - rect.top };
  }

  function flowToLocal(point: Point): Point {
    const viewport = sessionStore.viewport;
    return {
      x: point.x * viewport.zoom + viewport.x,
      y: point.y * viewport.zoom + viewport.y,
    };
  }

  function pathCreateBlockedTargetId(): string | null {
    const endpoint = pathCreateStart?.endpoint;
    return endpoint?.kind === 'connected' ? endpoint.item_id : null;
  }

  function pathAnchorCandidateAt(
    point: Point,
    blockedId: string | null = null,
  ): PathAnchorCandidate | null {
    debugCount('path.anchorCandidate.scan');
    const topmost = connectableTargetAtPoint(point, sessionStore.items, {
      margin: PATH_CONNECT_PREVIEW_MARGIN,
      excludeId: blockedId,
    });
    if (topmost === null) return null;
    const target = topmost;
    const nearest = nearestAnchor(target, point);
    const nearestPoint = anchorPoint(target, nearest);
    const zoom = sessionStore.viewport.zoom;
    const boxLocal = flowToLocal({ x: target.x, y: target.y });
    return {
      endpoint: {
        kind: 'connected',
        item_id: target.id,
        anchor: nearest,
        fallback_point: nearestPoint,
      },
      point: nearestPoint,
      local: flowToLocal(nearestPoint),
      box: {
        x: boxLocal.x,
        y: boxLocal.y,
        w: target.w * zoom,
        h: target.h * zoom,
      },
      anchors: PATH_PREVIEW_ANCHORS.map((anchor) => {
        const pos = anchorPoint(target, anchor);
        const local = flowToLocal(pos);
        const distance = Math.hypot(pos.x - point.x, pos.y - point.y);
        return {
          anchor,
          x: local.x,
          y: local.y,
          nearest: anchor === nearest,
          hovered: anchor === nearest && distance <= PATH_ANCHOR_HOVER_RADIUS,
        };
      }),
    };
  }

  function pathEndpointAt(point: Point, local: Point): PathCreateStart {
    const candidate = pathAnchorCandidateAt(point, pathCreateBlockedTargetId());
    if (candidate !== null) {
      return {
        endpoint: candidate.endpoint,
        point: candidate.point,
        local: candidate.local,
      };
    }
    return {
      endpoint: { kind: 'free', point },
      point,
      local,
    };
  }

  function pathParentForEndpoints(from: PathEndpoint, to: PathEndpoint): string | null {
    if (from.kind !== 'connected' || to.kind !== 'connected') return null;
    const fromItem = sessionStore.items.get(from.item_id);
    const toItem = sessionStore.items.get(to.item_id);
    if (fromItem === undefined || toItem === undefined) return null;
    return fromItem.parent_id === toItem.parent_id ? fromItem.parent_id : null;
  }

  function screenDistance(
    a: { x: number; y: number },
    b: { x: number; y: number },
  ): number {
    return Math.hypot(b.x - a.x, b.y - a.y);
  }

  function releaseCanvasPointer(pointerId: number): void {
    if (canvasRootEl === null) return;
    try {
      if (canvasRootEl.hasPointerCapture(pointerId)) {
        canvasRootEl.releasePointerCapture(pointerId);
      }
    } catch {
      // Pointer capture may already be released by the browser.
    }
  }

  function removeWindowGestureFallback(): void {
    if (typeof window === 'undefined') return;
    if (windowGesturePointerId === null) return;
    window.removeEventListener('pointermove', onWindowGesturePointerMove, { capture: true });
    window.removeEventListener('pointerup', onWindowGesturePointerUp, { capture: true });
    window.removeEventListener('pointercancel', onWindowGesturePointerCancel, { capture: true });
    windowGesturePointerId = null;
  }

  function installWindowGestureFallback(pointerId: number): void {
    if (typeof window === 'undefined') return;
    if (windowGesturePointerId === pointerId) return;
    removeWindowGestureFallback();
    windowGesturePointerId = pointerId;
    window.addEventListener('pointermove', onWindowGesturePointerMove, { capture: true });
    window.addEventListener('pointerup', onWindowGesturePointerUp, { capture: true });
    window.addEventListener('pointercancel', onWindowGesturePointerCancel, { capture: true });
  }

  function isActiveGesturePointer(e: PointerEvent): boolean {
    return (
      (lassoState !== null && lassoState.pointerId === e.pointerId) ||
      (groupDragState !== null && groupDragState.pointerId === e.pointerId)
    );
  }

  function onWindowGesturePointerMove(e: PointerEvent): void {
    if (!isActiveGesturePointer(e)) return;
    e.preventDefault();
    e.stopPropagation();
    if (lassoState !== null) {
      updateLasso(e);
      return;
    }
    if (groupDragState !== null) updateGroupDrag(e);
  }

  function onWindowGesturePointerUp(e: PointerEvent): void {
    if (!isActiveGesturePointer(e)) return;
    e.preventDefault();
    e.stopPropagation();
    if (lassoState !== null) {
      finishLasso(e);
      return;
    }
    if (groupDragState !== null) finishGroupDrag(e);
  }

  function onWindowGesturePointerCancel(e: PointerEvent): void {
    if (!isActiveGesturePointer(e)) return;
    e.preventDefault();
    e.stopPropagation();
    cancelLassoAndGroupDrag(e.pointerId);
  }

  function cancelLassoAndGroupDrag(pointerId?: number): void {
    groupDragState = null;
    lassoState = null;
    if (pointerId !== undefined) releaseCanvasPointer(pointerId);
    removeWindowGestureFallback();
  }

  function clearSuppressedPaneClick(): void {
    suppressNextPaneClickAfterLasso = false;
    if (suppressPaneClickTimer !== null) {
      clearTimeout(suppressPaneClickTimer);
      suppressPaneClickTimer = null;
    }
  }

  function suppressNextPaneClick(): void {
    clearSuppressedPaneClick();
    suppressNextPaneClickAfterLasso = true;
    suppressPaneClickTimer = setTimeout(() => {
      suppressNextPaneClickAfterLasso = false;
      suppressPaneClickTimer = null;
    }, 0);
  }

  // ADR-0032 D9 — Right-click on selected node 의 M 보존 snapshot.
  // SvelteFlow 의 click-to-select internal logic 이 mousedown(button=2) 시점에
  // M 을 단일 clicked id 로 reset → onnodecontextmenu 가 fire 될 때엔 이미
  // M.size === 1 → ContextMenu 가 항상 single-mode 로 열림 (multi-mode 진입 X).
  //
  // 본 snapshot 은 capture-phase pointerdown 에서 SvelteFlow internal 보다
  // *먼저* 실행되어 pre-reset M 을 보존. onnodecontextmenu 가 clicked node 가
  // snapshot ∈ 면 setM 으로 복원 → multi-mode 진입 정합.
  //
  // Drag-lasso 와 Cmd-click multi-select 양쪽 모두 같은 회귀 (둘 다 right-click
  // 시 SvelteFlow 가 reset). 단 lasso 는 one-shot → 즉시 right-click 패턴이
  // 더 빈번해서 사용자 체감이 큼.
  let rightClickMSnapshot: Set<string> | null = null;
  let hoveredCanvasGroupId: string | null = null;
  const GROUP_OVERLAY_PREFIX = '__group-overlay-';
  const GROUP_HITBOX_PREFIX = '__group-hitbox-';

  function updateCanvasGroupHover(e: PointerEvent): void {
    if (!isSelectMode || lassoState !== null || dragState !== null) {
      clearCanvasGroupHover();
      return;
    }
    const target = e.target as HTMLElement | null;
    const nodeEl = target?.closest('.svelte-flow__node') as HTMLElement | null;
    const nodeId = nodeEl?.dataset.id ?? null;
    let nextGroupId: string | null = null;
    if (nodeId !== null && !nodeId.startsWith(GROUP_OVERLAY_PREFIX)) {
      const hitTarget = canvasTargetFor(nodeId);
      if (sessionStore.groups.has(hitTarget)) {
        nextGroupId = hitTarget;
      }
    }
    if (nextGroupId === hoveredCanvasGroupId) return;
    if (hoveredCanvasGroupId !== null) groupHover.clearIf(hoveredCanvasGroupId);
    hoveredCanvasGroupId = nextGroupId;
    if (nextGroupId !== null) groupHover.set(nextGroupId);
  }

  function clearCanvasGroupHover(): void {
    if (hoveredCanvasGroupId !== null) groupHover.clearIf(hoveredCanvasGroupId);
    hoveredCanvasGroupId = null;
  }

  function isCanvasControlSurface(target: EventTarget | null): boolean {
    if (!(target instanceof Element)) return false;
    return (
      target.closest('.svelte-flow__resize-control') !== null ||
      target.closest('.nodrag') !== null ||
      target.closest('.endpoint') !== null ||
      target.closest('.waypoint') !== null
    );
  }

  function onCanvasPointerDown(e: PointerEvent) {
    if (isCanvasControlSurface(e.target)) return;

    if (e.button === 0 && isSelectMode && !isDragTool && !isSpacePressed && !isHandTool) {
      const isModifierClick = e.metaKey || e.ctrlKey;
      if (isModifierClick) {
        const target = e.target as HTMLElement | null;
        const nodeEl = target?.closest('.svelte-flow__node') as HTMLElement | null;
        const nodeId = nodeEl?.dataset.id ?? nodeIdAtPoint(e.clientX, e.clientY);
        if (nodeId !== null) {
          returnToLayerTabForCanvasSelection();
          const hitTarget = groupIdFromOverlayNode(nodeId) ?? canvasTargetFor(nodeId);
          if (!targetIsInsideDrill(hitTarget)) sessionStore.clearDrill();
          sessionStore.toggleM(hitTarget);
          if (pathEditStore.editingPathId !== null && !sessionStore.M.has(pathEditStore.editingPathId)) {
            pathEditStore.end();
          }
          resetDoubleClickTracker();
          // Keep xyflow from toggling the leaf node internally. In drill scope,
          // multi-select must operate on the current drill-level target, not the
          // lowest descendant DOM node that received pointerdown.
          e.preventDefault();
          e.stopPropagation();
          return;
        }
      }
    }

    if (e.button === 0 && isSelectMode && !isDragTool && !isSpacePressed && !isHandTool) {
      const target = e.target as HTMLElement | null;
      const nodeEl = target?.closest('.svelte-flow__node') as HTMLElement | null;
      const nodeId = nodeEl?.dataset.id ?? null;
      if (nodeId !== null) {
        returnToLayerTabForCanvasSelection();
        const overlayGroupId = groupIdFromOverlayNode(nodeId);
        const hitTarget = overlayGroupId ?? canvasTargetFor(nodeId);
        if (!targetIsInsideDrill(hitTarget)) sessionStore.clearDrill();
        if (sessionStore.groups.has(hitTarget)) {
          if (!(e.metaKey || e.ctrlKey || e.shiftKey)) {
            const now = Date.now();
            const isDblClick =
              lastClickId === nodeId && now - lastClickAt < DOUBLE_CLICK_MS;
            lastClickId = nodeId;
            lastClickAt = now;
            if (isDblClick) {
              resetDoubleClickTracker();
              sessionStore.setDrillRoot(hitTarget);
              const nextSelected =
                overlayGroupId ?? targetAtDrillLevel(nodeId, hitTarget, sessionStore.items, sessionStore.groups);
              sessionStore.setM([nextSelected]);
              if (pathEditStore.editingPathId !== nextSelected) pathEditStore.end();
              if (sessionStore.items.get(nodeId)?.type === 'text') {
                sessionStore.suppressTextEditDblClick(nodeId);
              }
              e.preventDefault();
              e.stopPropagation();
              return;
            }
          }
          if (
            !(e.metaKey || e.ctrlKey || e.shiftKey) &&
            sessionStore.M.size > 1 &&
            sessionStore.M.has(hitTarget)
          ) {
            beginSelectionDrag([...sessionStore.M], e);
            e.preventDefault();
            e.stopPropagation();
            return;
          }
          // Capture-phase preselection runs before SvelteFlow starts native drag.
          // Without this, dragging a nested descendant inside a drill scope can
          // move the leaf item before the click handler projects selection to
          // the current drill-level group.
          if (!(e.metaKey || e.ctrlKey || e.shiftKey)) {
            sessionStore.setM([hitTarget]);
            if (pathEditStore.editingPathId !== hitTarget) pathEditStore.end();
            beginSelectionDrag([hitTarget], e);
            e.preventDefault();
            e.stopPropagation();
            return;
          }
        } else if (
          !(e.metaKey || e.ctrlKey || e.shiftKey) &&
          sessionStore.M.size > 1 &&
          sessionStore.M.has(hitTarget)
        ) {
          beginSelectionDrag([...sessionStore.M], e);
          e.preventDefault();
          e.stopPropagation();
          return;
        } else if (!(e.metaKey || e.ctrlKey || e.shiftKey)) {
          // Make the app selection store authoritative immediately on down.
          // SvelteFlow click/drag events may still follow, but selection UI no
          // longer waits for its internal selected state or mousemove effects.
          sessionStore.setM([hitTarget]);
          if (pathEditStore.editingPathId !== hitTarget) pathEditStore.end();
        }
      }
      if (nodeId === null && !(e.metaKey || e.ctrlKey || e.shiftKey)) {
        beginLasso(e);
        e.preventDefault();
        e.stopPropagation();
        return;
      }
    }

    // Right-button on canvas: snapshot M for context menu restore (button=2).
    if (e.button === 2 && isSelectMode) {
      const target = e.target as HTMLElement | null;
      const nodeEl = target?.closest('.svelte-flow__node') as HTMLElement | null;
      const nodeId = nodeEl?.dataset.id ?? null;
      const targetId = nodeId !== null ? canvasTargetFor(nodeId) : null;
      if (targetId !== null && sessionStore.M.has(targetId) && sessionStore.M.size >= 2) {
        rightClickMSnapshot = new Set(sessionStore.M);
      } else {
        rightClickMSnapshot = null;
      }
    }

    if (!isDragTool) return;
    if (e.button !== 0) return; // left button only
    if (sessionStore.active === null) return;
    // Space-hold takes priority — let SvelteFlow handle the left-button pan.
    if (isSpacePressed) return;

    // Intercept BEFORE SvelteFlow's selection box on .canvas-root. capture
    // phase 등록 (markup attribute capture:true) 으로 SvelteFlow 의 down
    // handler 보다 먼저 호출됨.
    e.preventDefault();
    e.stopPropagation();

    const root = e.currentTarget as HTMLElement;
    const rect = root.getBoundingClientRect();
    const localX = e.clientX - rect.left;
    const localY = e.clientY - rect.top;
    const flow = screenToFlowPosition({ x: e.clientX, y: e.clientY });

    const tool = toolStore.current as DragShape;
    dragState = {
      tool,
      startFlow: flow,
      startLocal: { x: localX, y: localY },
      currentLocal: { x: localX, y: localY },
      shiftKey: e.shiftKey,
      lineShiftAngle: null,
    };
    if (tool === 'free_draw') {
      // 0065 FE-1 — 비반응 buffer init. 직전 stroke 잔여 정리 (cancel/abort
      // 후) + 시작점 push. ghostPreview 의 첫 paint 는 frame bump 로 trigger.
      resetFreeDrawBuffers();
      freeDrawPoints.push({ x: flow.x, y: flow.y });
      freeDrawPointsLocal.push({ x: localX, y: localY });
      scheduleFreeDrawFrame();
    }

    root.setPointerCapture(e.pointerId);
  }

  function translateCanvasItem(item: CanvasItem, dx: number, dy: number): CanvasItem {
    if (item.type === 'line') {
      const nextP1 = { x: item.x + dx, y: item.y + dy };
      const nextP2 = { x: item.x2 + dx, y: item.y2 + dy };
      const nextBox = lineBoxFromEndpoints(nextP1, nextP2);
      return {
        ...item,
        x: nextP1.x,
        y: nextP1.y,
        x2: nextP2.x,
        y2: nextP2.y,
        w: nextBox.w,
        h: nextBox.h,
      };
    }
    if (item.type === 'free_draw') {
      return {
        ...item,
        x: item.x + dx,
        y: item.y + dy,
        points: item.points.map((p) => ({ x: p.x + dx, y: p.y + dy })),
      };
    }
    if (item.type === 'path') {
      return translatePath(item, dx, dy);
    }
    return { ...item, x: item.x + dx, y: item.y + dy };
  }

  function beginSelectionDrag(selectionIds: Iterable<string>, e: PointerEvent): void {
    const groupsArr = [...sessionStore.groups.values()];
    const itemsArr = [...sessionStore.items.values()];
    const originals = new Map<string, CanvasItem>();
    for (const id of selectionIds) {
      const item = sessionStore.items.get(id);
      if (item !== undefined) {
        if (!effectiveLocked(item.locked, item.parent_id, sessionGroupsById)) {
          originals.set(item.id, item);
        }
        continue;
      }
      if (!sessionStore.groups.has(id)) continue;
      for (const descendant of descendantItems(id, groupsArr, itemsArr)) {
        if (effectiveLocked(descendant.locked, descendant.parent_id, sessionGroupsById)) continue;
        originals.set(descendant.id, descendant);
      }
    }
    if (originals.size === 0) return;
    const flow = screenToFlowPosition({ x: e.clientX, y: e.clientY });
    groupDragState = {
      pointerId: e.pointerId,
      startFlow: flow,
      currentFlow: flow,
      startClient: { x: e.clientX, y: e.clientY },
      currentClient: { x: e.clientX, y: e.clientY },
      originals,
      priorSnapshot: sessionStore.layoutSnapshot(),
      moved: false,
    };
    const root = e.currentTarget as HTMLElement;
    canvasRootEl = root;
    root.setPointerCapture(e.pointerId);
    installWindowGestureFallback(e.pointerId);
  }

  function beginLasso(e: PointerEvent): void {
    const root = e.currentTarget as HTMLElement;
    canvasRootEl = root;
    const local = canvasLocalFromClient(e.clientX, e.clientY);
    const flow = screenToFlowPosition({ x: e.clientX, y: e.clientY });
    lassoState = {
      pointerId: e.pointerId,
      startFlow: flow,
      currentFlow: flow,
      startClient: { x: e.clientX, y: e.clientY },
      currentClient: { x: e.clientX, y: e.clientY },
      startLocal: local,
      currentLocal: local,
    };
    root.setPointerCapture(e.pointerId);
    installWindowGestureFallback(e.pointerId);
  }

  function updateLasso(e: PointerEvent): void {
    const state = lassoState;
    if (state === null) return;
    lassoState = {
      ...state,
      currentFlow: screenToFlowPosition({ x: e.clientX, y: e.clientY }),
      currentClient: { x: e.clientX, y: e.clientY },
      currentLocal: canvasLocalFromClient(e.clientX, e.clientY),
    };
  }

  function itemSelectionBox(item: CanvasItem): { x: number; y: number; w: number; h: number } {
    if (item.type === 'line') {
      const box = lineBoxFromEndpoints(
        { x: item.x, y: item.y },
        { x: item.x2, y: item.y2 },
      );
      return { x: box.x, y: box.y, w: box.w, h: box.h };
    }
    if (item.type === 'path') {
      return computePathBBox(item, sessionStore.items);
    }
    return { x: item.x, y: item.y, w: item.w, h: item.h };
  }

  function rectsIntersect(
    a: { x: number; y: number; w: number; h: number },
    b: { x: number; y: number; w: number; h: number },
  ): boolean {
    return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
  }

  function projectSelectionIds(ids: Iterable<string>): string[] {
    const targets = new Set<string>();
    for (const id of ids) {
      targets.add(groupIdFromOverlayNode(id) ?? canvasTargetFor(id));
    }
    const selectedGroups = [...targets].filter((id) => sessionStore.groups.has(id));
    if (selectedGroups.length === 0) return [...targets];
    return [...targets].filter((id) => {
      if (selectedGroups.includes(id)) return true;
      return !selectedGroups.some((groupId) => isDescendantOfGroup(id, groupId));
    });
  }

  function finishLasso(e: PointerEvent): void {
    const state = lassoState;
    if (state === null) return;
    lassoState = null;
    const endFlow = screenToFlowPosition({ x: e.clientX, y: e.clientY });
    const dx = endFlow.x - state.startFlow.x;
    const dy = endFlow.y - state.startFlow.y;
    releaseCanvasPointer(state.pointerId);
    removeWindowGestureFallback();
    const distance = screenDistance(state.startClient, { x: e.clientX, y: e.clientY });
    if (distance < DRAG_CLICK_THRESHOLD) {
      clearCanvasDrillAndSelection();
      return;
    }
    const rect = {
      x: Math.min(state.startFlow.x, endFlow.x),
      y: Math.min(state.startFlow.y, endFlow.y),
      w: Math.abs(dx),
      h: Math.abs(dy),
    };
    const rawIds: string[] = [];
    for (const item of sessionStore.items.values()) {
      if (!effectiveVisibility(item.visibility, item.parent_id, sessionGroupsById)) continue;
      if (rectsIntersect(rect, itemSelectionBox(item))) rawIds.push(item.id);
    }
    const ids = projectSelectionIds(rawIds);
    if (ids.length === 0) {
      clearCanvasDrillAndSelection();
      return;
    }
    sessionStore.setM(ids);
    suppressNextPaneClick();
  }

  function updateGroupDrag(e: PointerEvent): void {
    const state = groupDragState;
    if (state === null) return;
    const flow = screenToFlowPosition({ x: e.clientX, y: e.clientY });
    const dx = flow.x - state.startFlow.x;
    const dy = flow.y - state.startFlow.y;
    const moved =
      state.moved ||
      screenDistance(state.startClient, { x: e.clientX, y: e.clientY }) >= DRAG_CLICK_THRESHOLD;
    groupDragState = {
      ...state,
      currentFlow: flow,
      currentClient: { x: e.clientX, y: e.clientY },
      moved,
    };
    if (!moved) return;
    const movedById = new Map<string, CanvasItem>();
    for (const [id, original] of state.originals) {
      const next = translateCanvasItem(original, dx, dy);
      movedById.set(id, next);
      sessionStore.items.set(id, next);
    }
    refreshLivePathCaches(movedById);
  }

  function finishGroupDrag(e: PointerEvent): void {
    const state = groupDragState;
    if (state === null) return;
    groupDragState = null;
    releaseCanvasPointer(state.pointerId);
    removeWindowGestureFallback();
    const moved =
      state.moved ||
      screenDistance(state.startClient, { x: e.clientX, y: e.clientY }) >= DRAG_CLICK_THRESHOLD;
    if (!moved) return;
    const flow = screenToFlowPosition({ x: e.clientX, y: e.clientY });
    const dx = flow.x - state.startFlow.x;
    const dy = flow.y - state.startFlow.y;
    const movedById = new Map<string, CanvasItem>();
    for (const [id, original] of state.originals) {
      movedById.set(id, translateCanvasItem(original, dx, dy));
    }
    for (const [id, next] of movedById) {
      sessionStore.items.set(id, next);
    }
    refreshLivePathCaches(movedById);
    void sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: mergeMovedItemsWithPathCaches(cur.items, movedById),
      }),
      {
        abortMessage: 'Group drag aborted — session reconnect failed.',
        failMessage: 'Group drag failed — reverted to previous position.',
        priorSnapshot: state.priorSnapshot,
      },
    );
  }

  function onCanvasPointerMove(e: PointerEvent) {
    const needsHoverTracking = isGhostTool !== null || isPathTool || pathCreateStart !== null;
    if (needsHoverTracking) {
      const rootEl = e.currentTarget as HTMLElement;
      const rootRect = rootEl.getBoundingClientRect();
      hoverScreen = {
        x: e.clientX - rootRect.left,
        y: e.clientY - rootRect.top,
      };
      hoverFlow = isPathTool ? screenToFlowPosition({ x: e.clientX, y: e.clientY }) : null;
    } else if (hoverScreen !== null || hoverFlow !== null) {
      hoverScreen = null;
      hoverFlow = null;
    }
    updateCanvasGroupHover(e);
    if (lassoState !== null) {
      if (lassoState.pointerId !== e.pointerId) return;
      e.preventDefault();
      e.stopPropagation();
      updateLasso(e);
      return;
    }
    if (groupDragState !== null) {
      if (groupDragState.pointerId !== e.pointerId) return;
      e.preventDefault();
      e.stopPropagation();
      updateGroupDrag(e);
      return;
    }
    if (dragState === null) return;
    const root = e.currentTarget as HTMLElement;
    const rect = root.getBoundingClientRect();
    const nextLocal = { x: e.clientX - rect.left, y: e.clientY - rect.top };
    if (dragState.tool === 'free_draw') {
      // 0065 FE-1 — 비반응 buffer 직접 append. 최소거리 prune 으로 과도한
      // sample 폐기 + rAF coalesce 로 preview 재계산 ≤ 1회/frame. cap 5000
      // 도달 시 새 점 skip (ADR-0018 D4).
      if (freeDrawPoints.length < FREE_DRAW_MAX_POINTS) {
        const last = freeDrawPointsLocal[freeDrawPointsLocal.length - 1];
        if (last !== undefined) {
          const dx = nextLocal.x - last.x;
          const dy = nextLocal.y - last.y;
          if (dx * dx + dy * dy >= FREE_DRAW_MIN_POINT_DELTA_SQ) {
            const flowPt = screenToFlowPosition({ x: e.clientX, y: e.clientY });
            freeDrawPoints.push({ x: flowPt.x, y: flowPt.y });
            freeDrawPointsLocal.push(nextLocal);
            scheduleFreeDrawFrame();
          }
        }
      }
      // dragState reassign 생략 — free_draw 의 ghostPreview 는 freeDrawFrame
      // 만 trigger 로 사용. currentLocal stale 도 ghostPreview 가 안 읽음.
      return;
    }
    let lineShiftAngle = dragState.lineShiftAngle;
    if (dragState.tool === 'line') {
      if (e.shiftKey) {
        if (lineShiftAngle === null) {
          const dx = nextLocal.x - dragState.startLocal.x;
          const dy = nextLocal.y - dragState.startLocal.y;
          if (Math.hypot(dx, dy) > 0.5) lineShiftAngle = Math.atan2(dy, dx);
        }
      } else {
        lineShiftAngle = null;
      }
    }
    dragState = {
      ...dragState,
      currentLocal: nextLocal,
      shiftKey: e.shiftKey,
      lineShiftAngle,
    };
  }

  function onCanvasPointerUp(e: PointerEvent) {
    if (lassoState !== null) {
      if (lassoState.pointerId !== e.pointerId) return;
      e.preventDefault();
      e.stopPropagation();
      finishLasso(e);
      return;
    }
    if (groupDragState !== null) {
      if (groupDragState.pointerId !== e.pointerId) return;
      e.preventDefault();
      e.stopPropagation();
      finishGroupDrag(e);
      return;
    }
    if (dragState === null) return;
    e.stopPropagation();
    const state = dragState;
    dragState = null;

    let endFlow = screenToFlowPosition({ x: e.clientX, y: e.clientY });
    const dx = endFlow.x - state.startFlow.x;
    const dy = endFlow.y - state.startFlow.y;
    const distance = Math.hypot(dx, dy);

    let item;
    if (state.tool === 'free_draw') {
      // Free draw: 비반응 buffer 에 수집한 points sequence. 점 수 < 2 면
      // 의미없는 stroke (단순 click) — abort.
      const pts = freeDrawPoints.slice();
      resetFreeDrawBuffers();
      if (pts.length < 2) {
        return;
      }
      item = createFreeDrawItem(pts);
    } else if (state.tool === 'line') {
      // Line: endpoint pair 그대로 보존 → 4 방향 (TL→BR, BR→TL, TR→BL, BL→TR).
      // distance < threshold 면 default-size 의 down-right 방향 단일선.
      if (e.shiftKey && state.lineShiftAngle !== null) {
        endFlow = projectPointToAngle(state.startFlow, endFlow, state.lineShiftAngle);
      }
      const p2 =
        distance < DRAG_CLICK_THRESHOLD
          ? { x: state.startFlow.x + 240, y: state.startFlow.y + 80 }
          : endFlow;
      item = createLineItem(state.startFlow, p2);
    } else {
      const constrainedDx = endFlow.x - state.startFlow.x;
      const constrainedDy = endFlow.y - state.startFlow.y;
      // Rect/Ellipse: bounding box 정규화 (drag 방향 무관, w/h ≥ 0).
      const bounds = {
        x: Math.min(state.startFlow.x, endFlow.x),
        y: Math.min(state.startFlow.y, endFlow.y),
        w: distance < DRAG_CLICK_THRESHOLD ? 0 : Math.abs(constrainedDx),
        h: distance < DRAG_CLICK_THRESHOLD ? 0 : Math.abs(constrainedDy),
      };
      item = createShapeItem(state.tool, bounds);
    }

    void commitNewItem(item)
      .then(() => {
        toolStore.consume();
      })
      .catch((err: unknown) => {
        if (err instanceof UnauthorizedError) {
          window.location.href = '/auth';
          return;
        }
        toastStore.show({
          message: `Create failed: ${err instanceof Error ? err.message : String(err)}`,
          tone: 'error',
        });
      });
  }

  function onCanvasPointerCancel(_e: PointerEvent) {
    if (
      (lassoState !== null && lassoState.pointerId === _e.pointerId) ||
      (groupDragState !== null && groupDragState.pointerId === _e.pointerId)
    ) {
      cancelLassoAndGroupDrag(_e.pointerId);
    }
    // OS 가 capture 를 빼앗는 경우 (다른 modal 등) drag state 청소.
    dragState = null;
    // 0065 FE-1 — pending rAF + buffer 정리 (다음 stroke 의 init 에서도
    // resetFreeDrawBuffers 가 idempotent 호출되지만 cancel 시점에 즉시 정리).
    resetFreeDrawBuffers();
  }

  /* ── 0045 P0-A root-cause 후속 fix — literal props 의 매-flush new reference
   * 폭증 차단. `edges={[]}` / `proOptions={{...}}` 같은 inline literal 은 매
   * reactive flush 마다 새 reference 생성 → SvelteFlow 가 prop 변경으로 판단
   * → internal effect → re-derive → effect-depth loop. component-local const 로
   * 추출해 reference 안정화. */
  const EMPTY_EDGES: never[] = [];
  const SVELTE_FLOW_PRO_OPTIONS = { hideAttribution: true };

  // Custom node type lookup table for Svelte Flow.
  // - 'panel'     = gtmux terminal panel (PanelNode). schema v2 의 `type:"terminal"` ↔ 'panel'.
  // - 'text'      = 자유 텍스트 (TextNode)
  // - 'note'      = title + body 메모 (NoteNode)
  // - 'file_path' = path reference (FilePathNode). 실 OS open 은 ADR-0023 후.
  // shape / line / image / document / free_draw 는 Batch 2 / 3 에서 추가.
  const nodeTypes = {
    panel: PanelNode,
    text: TextNode,
    note: NoteNode,
    file_path: FilePathNode,
    rect: ShapeNode,
    ellipse: ShapeNode,
    line: LineNode,
    path: PathNode,
    image: ImageNode,
    document: DocumentNode,
    free_draw: FreeDrawNode,
    // ADR-0038 — snippet collection (badges + inline edit form).
    snippets: SnippetsNode,
    // ADR-0010 D15 — group entity overlay (dotted BBox outline).
    'gtmux-group': GroupOverlay,
  };

  // M cardinality — PanelNode 가 single/multi 분기를 위해 참조.
  const isMultiSelection = $derived(sessionStore.M.size > 1);
  const sessionGroupsById = $derived(new Map(sessionStore.groups));

  /* ── flowNodes id-cache + signature (0045 P0-A) ────────────────────
   *
   * Naïve `items.values().map(itemToNode)` 는 매 reactive pass 마다 새
   * Node array + 새 Node object 를 생성 → SvelteFlow 가 prop identity
   * 변경으로 판단 → 내부 측정/정렬 effect → parent rebuild →
   * `effect_update_depth_exceeded`. (0045 §6 P0-A)
   *
   * Cache 전략:
   *  - 각 item 의 SvelteFlow-relevant payload 를 signature 로 직렬화.
   *  - signature 가 동일하면 이전 Node object reference 재사용.
   *  - signature 는 모든 mutable field 포함:
   *      * common: full JSON.stringify(item) — id/type/parent_id/x/y/w/h/z/
   *        visibility/locked/minimized/label + type payload (line.x2/y2,
   *        text.text/font_size/align, shape.stroke/fill 등) 모두 cover.
   *      * derived: effectiveVisible / effectiveLocked (groups 영향) +
   *        selected (M.has) + m_multi (M.size > 1).
   *  - Map 은 매 pass 재생성 — 삭제된 id 자연 GC. 50 entry 기준 < 1ms.
   *
   * 주의: signature 누락 시 stale render — 본 cache 가 의도와 다른
   * 동작을 발견하면 가장 먼저 makeSignature 의 누락 field 의심.
   */
  /**
   * Node cache — module-local Map, derived 안에서 mutation 안 함 (reactive
   * noise 차단). 매 derived pass 후 별 effect 에서 GC (사라진 id 제거).
   *
   * 본 Map 은 reactive 가 아닌 *plain JS Map*. derived 가 cache.get/set 만
   * 호출 — set 은 reactive trigger 없음. derived 의 read 는 sessionStore.
   * items + sessionStore.M + sessionGroupsById + isMultiSelection 만.
   */
  const nodeCache = new Map<string, { sig: string; node: Node }>();

  /**
   * P0-A signature — 모든 mutation-relevant field 의 *명시 concat*.
   *
   * ⚠️ JSON.stringify(item) 회피 이유 (0045 P0-A 후속 fix):
   * SvelteMap 의 entry 는 reactive proxy 일 수 있음. proxy 의 enumerable
   * property 를 모두 read 하면 derived 가 *모든 field* 의 reactive
   * subscription 등록 → 어떤 field 변경 시 전체 re-derive → 폭발적 loop.
   *
   * 명시 field 만 read = subscription 면적 제한 + 의도 명확.
   * 신규 field 추가 시 본 함수도 명시 update 필요 (누락 = stale render bug).
   */
  /**
   * djb2 string hash — collision 가능성은 사실상 의미 없음 (서로 다른 두 32-bit
   * hash 가 우연히 일치할 확률 ~1/4e9). signature 의 *지문* 만 만들면 충분.
   *
   * 용도: ADR-0038 snippets entry 의 key/body content fingerprint. 같은 length
   * 인데 내용만 바뀐 edit ('ab'→'cd', 'foo'→'bar' 등) 도 cache 가 invalidate
   * 되도록 signature 에 hash 를 포함. 옛 length-only signature 는 same-length
   * edit 에서 cache hit → stale render 회귀.
   *
   * 성능: 일반 사용 (< 50 entries × < 200 byte) 에서 < 50μs. SNIPPETS_ENTRIES_CAP
   * (1000) × SNIPPET_BODY_MAX_BYTES (64 KB) 의 worst case 만 부담 — 그 영역은
   * snippets node 의 사용 패턴 자체가 비전형이라 별 batch 의 entry-level WeakMap
   * cache 도입 후보 (Open).
   */
  function djb2(s: string): number {
    let h = 5381;
    for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) | 0;
    return h;
  }

  function pointSignature(point: Point | null | undefined): string {
    return point == null ? '' : `${point.x},${point.y}`;
  }

  function pathEndpointSignature(endpoint: PathEndpoint): string {
    if (endpoint.kind === 'free') return `free:${endpoint.point.x},${endpoint.point.y}`;
    return [
      'connected',
      endpoint.item_id,
      endpoint.anchor,
      pointSignature(endpoint.offset),
      pointSignature(endpoint.fallback_point),
    ].join(':');
  }

  function makeSignature(
    item: CanvasItem,
    effVisible: boolean,
    effLocked: boolean,
    selected: boolean,
    selectedByGroup: boolean,
    groupHitTarget: boolean,
    mMulti: boolean,
  ): string {
    // Common fields (all CanvasItem)
    const common = `${item.id}|${item.type}|${item.parent_id ?? ''}|${item.x}|${item.y}|${item.w}|${item.h}|${item.z}|${item.visibility}|${item.locked ? 1 : 0}|${item.minimized ? 1 : 0}|${item.label ?? ''}`;
    // Type-specific payload — 명시 field only
    let payload = '';
    switch (item.type) {
      case 'terminal':
        // No type-specific payload
        break;
      case 'text':
        // batch-5 R3 신규: font_weight / italic / underline / strikethrough.
        // ADR-0040/0041 신규: font_family + optional text box fill/stroke.
        // 누락 시 Inspector 변경이 cached Node 로 흡수되어 selection 해제 전엔
        // 반영 안 됨 — 회귀 가드.
        payload = `|${item.text}|${item.font_size}|${item.color}|${item.text_align ?? ''}|${item.text_vertical_align ?? ''}|${item.font_weight ?? ''}|${item.italic ? 1 : 0}|${item.underline ? 1 : 0}|${item.strikethrough ? 1 : 0}|${item.font_family ?? ''}|${item.fill ?? ''}|${item.stroke ?? ''}|${item.stroke_width ?? ''}|${item.fill_enabled === true ? 1 : 0}|${item.stroke_enabled === true ? 1 : 0}|${item.corner_rounded ? 1 : 0}|${item.stroke_dash ?? ''}`;
        break;
      case 'note':
        payload = `|${item.title ?? ''}|${item.body ?? ''}|${item.color ?? ''}`;
        break;
      case 'file_path':
        payload = `|${item.path}|${item.kind ?? ''}`;
        break;
      case 'rect':
        // batch-5 R1+R2 신규: fill_enabled / stroke_enabled / corner_rounded / stroke_dash.
        // ADR-0040/0041 신규: embedded figure text + text style fields.
        payload = `|${item.stroke}|${item.fill}|${item.stroke_width}|${item.fill_enabled === false ? 0 : 1}|${item.stroke_enabled === false ? 0 : 1}|${item.corner_rounded ? 1 : 0}|${item.stroke_dash ?? ''}|${item.text ?? ''}|${item.font_size ?? ''}|${item.color ?? ''}|${item.text_align ?? ''}|${item.text_vertical_align ?? ''}|${item.font_weight ?? ''}|${item.italic ? 1 : 0}|${item.underline ? 1 : 0}|${item.strikethrough ? 1 : 0}|${item.font_family ?? ''}`;
        break;
      case 'ellipse':
        // batch-5 R1+R2 신규: fill_enabled / stroke_enabled / stroke_dash (corner_rounded 없음).
        // ADR-0040/0041 신규: embedded figure text + text style fields.
        payload = `|${item.stroke}|${item.fill}|${item.stroke_width}|${item.fill_enabled === false ? 0 : 1}|${item.stroke_enabled === false ? 0 : 1}|${item.stroke_dash ?? ''}|${item.text ?? ''}|${item.font_size ?? ''}|${item.color ?? ''}|${item.text_align ?? ''}|${item.text_vertical_align ?? ''}|${item.font_weight ?? ''}|${item.italic ? 1 : 0}|${item.underline ? 1 : 0}|${item.strikethrough ? 1 : 0}|${item.font_family ?? ''}`;
        break;
      case 'line':
        // batch-5 R2 신규: stroke_dash. ADR-0043: head markers.
        payload = `|${item.x2}|${item.y2}|${item.stroke}|${item.stroke_width}|${item.stroke_dash ?? ''}|${item.head_from ?? ''}|${item.head_to ?? ''}`;
        break;
      case 'path':
        payload = `|${pathEndpointSignature(item.from)}|${pathEndpointSignature(item.to)}|${item.routing}|${item.head_from}|${item.head_to}|${item.stroke}|${item.stroke_width}|${item.stroke_dash ?? ''}|${(item.waypoints ?? [])
          .map((p) => `${p.id}:${p.x}:${p.y}`)
          .join(',')}`;
        break;
      case 'free_draw':
        // P2 — placeholder until ship
        payload = '|free_draw';
        break;
      case 'image':
        payload = `|${item.asset_id}|${item.mime}|${item.original_w ?? ''}|${item.original_h ?? ''}`;
        break;
      case 'document':
        payload = `|${item.asset_id ?? ''}|${item.mime}|${item.file_name}|${item.size_bytes}|${item.content ?? ''}`;
        break;
      case 'snippets':
        // ADR-0038 — id + content hash. 옛 length-only sig 는 same-length edit
        // ('ab'→'cd' 등) 에서 cache hit → stale render 회귀 (handover §0.5 verify
        // 후 발견 — 2026-05-24). data 는 itemToNode 가 만든 plain object 라
        // reactive proxy 가 아님 → cache 가 hit 되면 OLD data.entries 그대로
        // 반환됨. djb2 32-bit hash 로 content fingerprint 포함 → 같은 length
        // 의 다른 내용도 cache invalidate.
        // 전체 body concat 회피 — entries 1000 × body 64KB 의 OOM 위험.
        payload = `|${item.entries.length}|${item.entries
          .map((e) => `${e.id}:${djb2(e.key)}:${djb2(e.body)}`)
          .join(',')}`;
        break;
    }
    return `${effVisible ? 1 : 0}|${effLocked ? 1 : 0}|${selected ? 1 : 0}|${selectedByGroup ? 1 : 0}|${groupHitTarget ? 1 : 0}|${mMulti ? 1 : 0}|${common}${payload}`;
  }

  /**
   * sessionStore CanvasItem → SvelteFlow Node 어댑터.
   *
   * Stage 5 Batch 1 (terminal / text / note / file_path) 의 4 type 표면.
   * 그 외 (rect/ellipse/line/free_draw/image/document) 는 Batch 2/3 에서 추가 —
   * 현재 unknown type 은 SvelteFlow default 로 fallback (placeholder rendering).
   */
  function itemToNode(item: CanvasItem): Node {
    const visible = effectiveVisibility(item.visibility, item.parent_id, sessionGroupsById);
    const locked = effectiveLocked(item.locked, item.parent_id, sessionGroupsById);
    // 2026-05-20 figure hit-test — fill_enabled=false rect/ellipse 의 interior 는
    // mouse event 제외. ShapeNode 의 .pass-through (자식 wrapper) 만으로는
    // SvelteFlow 의 .svelte-flow__node 가 bbox 전체를 catch — wrapper 자체에
    // 'fill-off' class 를 부여해 CSS rule 로 pointer-events:none. NodeResizer
    // handle 은 SvelteFlow 의 `.svelte-flow__resize-control { pointer-events:all }`
    // 로 그대로 hit 가능 (pointer-events 는 CSS 비상속).
    const classes: string[] = [];
    if (item.minimized) classes.push('is-minimized');
    const selectedByGroup = isSelectedByGroup(item.id);
    if (selectedByGroup) classes.push('group-selected');
    const groupHitTarget = isGroupHitTarget(item.id);
    if (groupHitTarget) classes.push('group-hit-target');
    if (sessionStore.M.has(item.id)) classes.push('m-selected');
    if (
      (item.type === 'rect' || item.type === 'ellipse') &&
      item.fill_enabled === false
    ) {
      classes.push('fill-off');
    }
    if (item.type === 'path') {
      classes.push('path-node-wrapper');
    }
    const common = {
      id: item.id,
      position: { x: item.x, y: item.y },
      draggable:
        !locked &&
        !groupHitTarget &&
        (item.type !== 'path' || !hasConnectedEndpoint(item)),
      selectable: true,
      selected: false,
      zIndex: item.z,
      width: item.w,
      height: item.h,
      class: classes.join(' '),
    };
    if (item.type === 'terminal') {
      return {
        ...common,
        type: 'panel',
        data: {
          id: item.id,
          // schema v2 는 별도 pane_id 없음 — UUID 자체가 BE Terminal id (ADR-0018 D2).
          // PanelNode 다운스트림 (ContextMenu 등) 호환 위해 pane_id 슬롯에도 UUID 노출.
          // Stage 5 multi-xterm subscriber 통합 시 정합 (legacy %N 컨벤션은 폐기).
          pane_id: item.id,
          x: item.x,
          y: item.y,
          w: item.w,
          h: item.h,
          z: item.z,
          visibility: visible,
          minimized: item.minimized,
          locked,
          label: item.label ?? null,
          m_multi: isMultiSelection,
          group_selected: selectedByGroup,
        },
      };
    }
    if (item.type === 'line') {
      // schema: (x,y) = 시작, (x2,y2) = 끝 (canvas 절대 좌표). SvelteFlow Node 는
      // bounding-box top-left 필요 — min(x, x2), min(y, y2) 로 계산하고 endpoint 의
      // box-local 좌표를 data 안에 함께 노출 → LineNode 가 4 방향 모두 정확히 렌더.
      const box = lineBoxFromEndpoints(
        { x: item.x, y: item.y },
        { x: item.x2, y: item.y2 },
      );
      return {
        ...common,
        type: 'line',
        position: { x: box.x, y: box.y },
        width: box.w,
        height: box.h,
        data: {
          ...(item as unknown as Record<string, unknown>),
          visibility: visible,
          locked,
          group_selected: selectedByGroup,
          w: box.w,
          h: box.h,
          _boxX1: item.x - box.x,
          _boxY1: item.y - box.y,
          _boxX2: item.x2 - box.x,
          _boxY2: item.y2 - box.y,
        },
      };
    }
    if (item.type === 'path') {
      const box = computePathBBox(item, sessionStore.items);
      return {
        ...common,
        type: 'path',
        position: { x: box.x, y: box.y },
        width: box.w,
        height: box.h,
        data: {
          ...(item as unknown as Record<string, unknown>),
          visibility: visible,
          locked,
          group_selected: selectedByGroup,
          x: box.x,
          y: box.y,
          w: box.w,
          h: box.h,
        },
      };
    }
    // Generic non-terminal item — type 별 renderer 가 data 의 type-specific
    // payload 를 직접 소비. visibility 는 enum 보존 (renderer 측에서 boolean
    // 변환). spread cast 는 SvelteFlow 의 NodeProps.data 가 unknown 이라 무해.
    return {
      ...common,
      type: item.type,
      data: {
        ...(item as unknown as Record<string, unknown>),
        visibility: visible,
        locked,
        group_selected: selectedByGroup,
      },
    };
  }

  /* ── SvelteFlow nodes — one-way from sessionStore, identity-stable ────
   *
   * SvelteFlow writes measured dimensions back into its local `nodes`
   * prop during `updateNodeInternals()`. Binding that prop to parent state
   * (`bind:nodes`) feeds those internal measurement writes back into this
   * component, where rebuilding nodes from `sessionStore.items` can create
   * a Svelte effect-depth loop on initial hydrate. Keep the source of truth
   * one-way: sessionStore -> flowNodes. Drag/resize commits still write to
   * sessionStore explicitly through event handlers.
   *
   * P0-A (0045): id-cache + signature 로 identity 안정화. 동일 signature
   * 면 이전 Node object reference 재사용 → SvelteFlow 가 prop unchanged 로
   * 판단 → 내부 측정 effect 가 무한 트리거되지 않음.
   */
  const flowNodes = $derived.by<Node[]>(() => {
    debugCount('flowNodes.rebuild');
    const items = sessionStore.items;
    const groupsById = sessionGroupsById;
    const mMulti = isMultiSelection;
    const out: Node[] = [];
    const seen = new Set<string>();
    for (const item of items.values()) {
      const visible = effectiveVisibility(item.visibility, item.parent_id, groupsById);
      const locked = effectiveLocked(item.locked, item.parent_id, groupsById);
      const selected = isFlowSelected(item.id);
      const selectedByGroup = isSelectedByGroup(item.id);
      const groupHitTarget = isGroupHitTarget(item.id);
      const sig = makeSignature(item, visible, locked, selected, selectedByGroup, groupHitTarget, mMulti);
      const cached = nodeCache.get(item.id);
      seen.add(item.id);
      if (cached !== undefined && cached.sig === sig) {
        debugCount('flowNodes.cache.hit');
        out.push(cached.node);
      } else {
        debugCount('flowNodes.cache.miss');
        const node = itemToNode(item);
        // cache 갱신은 derived 안에서 OK — nodeCache 는 plain Map (reactive X).
        // 외부 변수 reassignment 가 아닌 .set() 호출이므로 reactive noise 0.
        nodeCache.set(item.id, { sig, node });
        out.push(node);
      }
    }
    // GC — items 에서 사라진 id 의 cache entry 제거.
    if (nodeCache.size > seen.size) {
      for (const id of nodeCache.keys()) {
        if (!seen.has(id)) nodeCache.delete(id);
      }
    }
    // ADR-0010 D15 + D22.8 — Group entity overlay 노드 합성.
    // 3 mode 우선순위:
    //   selected (M.has)  > outer-dim (drillRoot/ancestor) > hover (groupHover)
    // 빈 BBox (자손 모두 hidden 또는 0개) 는 skip — 시각 noise 회피.
    const groupsArr = [...sessionStore.groups.values()];
    const itemsArr = [...items.values()];
    const PADDING = 8;
    const OVERLAY_Z = 1_000_000; // 항상 자손 panel 위 (자손 z 는 보통 < 1000).

    // Drill scope + selected descendants ancestor chain.
    const outerAncestorIds = new Set<string>();
    if (sessionStore.drillRootId !== null) outerAncestorIds.add(sessionStore.drillRootId);
    if (sessionStore.M.size === 1) {
      const sole = [...sessionStore.M][0];
      if (sole !== undefined) {
        const item = sessionStore.items.get(sole);
        const g = sessionStore.groups.get(sole);
        let parentId = item?.parent_id ?? g?.parent_id ?? null;
        while (parentId !== null) {
          outerAncestorIds.add(parentId);
          const pg = sessionStore.groups.get(parentId);
          parentId = pg?.parent_id ?? null;
        }
      }
    }

    for (const g of groupsArr) {
      const isSelected = sessionStore.M.has(g.id);
      const isOuterDim = !isSelected && outerAncestorIds.has(g.id);
      const isHover = !isSelected && !isOuterDim && groupHover.id === g.id;
      const needsSelectionProxy = g.parent_id === (sessionStore.drillRootId ?? null);
      const needsVisibleOverlay = isSelected || isOuterDim || isHover;
      if (!needsVisibleOverlay && !needsSelectionProxy) continue;
      const visibleDescendants = descendantItems(g.id, groupsArr, itemsArr).filter(
        (it) =>
          effectiveVisibility(it.visibility, it.parent_id, groupsById),
      );
      if (visibleDescendants.length === 0) continue;
      let minX = Number.POSITIVE_INFINITY;
      let minY = Number.POSITIVE_INFINITY;
      let maxX = Number.NEGATIVE_INFINITY;
      let maxY = Number.NEGATIVE_INFINITY;
      for (const it of visibleDescendants) {
        if (it.x < minX) minX = it.x;
        if (it.y < minY) minY = it.y;
        if (it.x + it.w > maxX) maxX = it.x + it.w;
        if (it.y + it.h > maxY) maxY = it.y + it.h;
      }
      if (!Number.isFinite(minX)) continue;
      const overlayBase = {
        type: 'gtmux-group',
        position: { x: minX - PADDING, y: minY - PADDING },
        width: maxX - minX + 2 * PADDING,
        height: maxY - minY + 2 * PADDING,
        draggable: false,
        class: 'gtmux-group',
      } satisfies Partial<Node>;
      if (needsSelectionProxy) {
        out.push({
          ...overlayBase,
          id: `${GROUP_HITBOX_PREFIX}${g.id}`,
          selectable: true,
          selected: false,
          zIndex: OVERLAY_Z - 1,
          data: {
            groupId: g.id,
            mode: 'hitbox',
            color: 'var(--color-accent)',
          },
        });
      }
      if (!needsVisibleOverlay) continue;
      const mode: 'selected' | 'outer-dim' | 'hover' = isSelected
        ? 'selected'
        : isOuterDim
          ? 'outer-dim'
          : 'hover';
      out.push({
        ...overlayBase,
        id: `${GROUP_OVERLAY_PREFIX}${g.id}`,
        selectable: false,
        selected: false,
        // selected = 가장 위, outer-dim = 가운데, hover = 가장 아래 (선택 시각 우선).
        zIndex: OVERLAY_Z + (isSelected ? 2 : isOuterDim ? 1 : 0),
        data: {
          groupId: g.id,
          mode,
          color: 'var(--color-accent)',
        },
      });
    }
    return out;
  });

  function onmove(_event: MouseEvent | TouchEvent | null, viewport: Viewport): void {
    debugCount('canvas.onmove');
    if (applyingStoreViewport) {
      debugCount('canvas.onmove.skip-applying');
      return;
    }
    sessionStore.updateViewport({ x: viewport.x, y: viewport.y, zoom: viewport.zoom });
  }

  /* ── FE-9: sessionStore.viewport → SvelteFlow setViewport ──────────────
   * Session 전환 / 초기 hydrate 시 layout.viewport 가 sessionStore 에 반영되면
   * SvelteFlow 의 내부 viewport 와는 별도. 본 effect 가 두 값이 다를 때만
   * setViewport 호출 → loop 방지.
   *
   * loadLayout 직후 sessionStore.viewport 가 layout 의 viewport 로 갱신되며
   * 본 effect 가 트리거됨. 일반 pan/zoom (SvelteFlow→sessionStore 단방향) 은
   * onmove 가 처리. 본 effect 에서 sessionStore.viewport 자체는 변경하지 않음
   * — 단방향 (sessionStore → SvelteFlow) reactive sync 만. */
  $effect(() => {
    const v = sessionStore.viewport;
    untrack(() => {
      const cur = getViewport();
      const dx = Math.abs(cur.x - v.x);
      const dy = Math.abs(cur.y - v.y);
      const dz = Math.abs(cur.zoom - v.zoom);
      if (dx < 0.5 && dy < 0.5 && dz < 0.001) return;
      debugCount('canvas.setViewport');
      applyingStoreViewport = true;
      void setViewport({ x: v.x, y: v.y, zoom: v.zoom }).finally(() => {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            applyingStoreViewport = false;
          });
        });
      });
    });
  });

  /* ── Focus / zoom-to-selection (ViewportCtrl 의 focus 버튼) ──────────────
   * sessionStore.zoomToIds(ids) → pendingZoomToIds set. 본 effect 가 watch
   * — items 의 union BBox 를 *visible canvas* (left/right panel 또는 rail 이
   * 차지한 영역 제외) 중앙 + 가득 채움 으로 setViewport. 처리 후 1-shot clear.
   *
   * BBox: item.x/y/w/h 의 union. line 은 (x,y)~(x2,y2) 의 BBox 사용.
   * group id 는 visible descendant item BBox union + group overlay padding.
   * padding = 12% (88% 채움). zoom 은 visible 가로/세로 중 더 작은 비율 채택,
   * [0.05, 3] clamp.
   */
  const GROUP_FOCUS_PADDING = 8;

  function computeVisibleCanvas(): { x: number; y: number; w: number; h: number } {
    const root = document.querySelector('.canvas-root') as HTMLElement | null;
    if (root === null) {
      return { x: 0, y: 0, w: window.innerWidth, h: window.innerHeight };
    }
    const rootRect = root.getBoundingClientRect();
    let visibleLeft = 0;
    let visibleRight = rootRect.width;
    const visibleTop = 0;
    const visibleBottom = rootRect.height;
    // LeftPanel (확장 시) 또는 LeftRail (축소 시).
    const left = document.querySelector('.left-panel, .left-rail') as HTMLElement | null;
    if (left !== null) {
      const r = left.getBoundingClientRect();
      const localRight = r.right - rootRect.left;
      if (localRight > visibleLeft) visibleLeft = localRight;
    }
    const right = document.querySelector('.right-panel, .right-rail') as HTMLElement | null;
    if (right !== null) {
      const r = right.getBoundingClientRect();
      const localLeft = r.left - rootRect.left;
      if (localLeft < visibleRight) visibleRight = localLeft;
    }
    return {
      x: visibleLeft,
      y: visibleTop,
      w: Math.max(1, visibleRight - visibleLeft),
      h: Math.max(1, visibleBottom - visibleTop),
    };
  }

  $effect(() => {
    const ids = sessionStore.pendingZoomToIds;
    if (ids === null || ids.length === 0) return;
    untrack(() => {
      let minX = Infinity;
      let minY = Infinity;
      let maxX = -Infinity;
      let maxY = -Infinity;
      let found = 0;
      const includeItemBox = (item: CanvasItem, padding = 0): void => {
        let bx = item.x;
        let by = item.y;
        let bw = item.w;
        let bh = item.h;
        if (item.type === 'line') {
          const x2 = (item as { x2: number }).x2;
          const y2 = (item as { y2: number }).y2;
          bx = Math.min(item.x, x2);
          by = Math.min(item.y, y2);
          bw = Math.abs(x2 - item.x) || 1;
          bh = Math.abs(y2 - item.y) || 1;
        }
        bx -= padding;
        by -= padding;
        bw += padding * 2;
        bh += padding * 2;
        if (bx < minX) minX = bx;
        if (by < minY) minY = by;
        if (bx + bw > maxX) maxX = bx + bw;
        if (by + bh > maxY) maxY = by + bh;
        found += 1;
      };
      for (const id of ids) {
        const item = sessionStore.items.get(id);
        if (item !== undefined) {
          includeItemBox(item);
          continue;
        }
        if (sessionStore.groups.has(id)) {
          const groupsById = sessionGroupsById;
          const descendants = descendantItems(
            id,
            [...sessionStore.groups.values()],
            [...sessionStore.items.values()],
          );
          for (const descendant of descendants) {
            if (!effectiveVisibility(descendant.visibility, descendant.parent_id, groupsById)) continue;
            includeItemBox(descendant, GROUP_FOCUS_PADDING);
          }
        }
      }
      if (found === 0) {
        sessionStore.clearPendingZoom();
        return;
      }
      const bw = Math.max(1, maxX - minX);
      const bh = Math.max(1, maxY - minY);
      const visible = computeVisibleCanvas();
      const padding = 0.88;
      const zoom = Math.min((visible.w / bw) * padding, (visible.h / bh) * padding, 3);
      const zoomClamped = Math.max(0.05, zoom);
      const cx = minX + bw / 2;
      const cy = minY + bh / 2;
      // BBox center 가 visible 영역 의 center 와 일치 — sidebar 가 가린 영역 보정.
      const targetScreenX = visible.x + visible.w / 2;
      const targetScreenY = visible.y + visible.h / 2;
      const next = {
        x: targetScreenX - cx * zoomClamped,
        y: targetScreenY - cy * zoomClamped,
        zoom: zoomClamped,
      };
      sessionStore.updateViewport(next);
      sessionStore.clearPendingZoom();
    });
  });

  // Canvas mount/unmount count — 0045 검증 §8.3 의 "Canvas mount count == refresh당 1회".
  onMount(() => {
    debugCount('canvas.mount');
    return () => debugCount('canvas.unmount');
  });

  onDestroy(() => {
    cancelLassoAndGroupDrag();
    clearSuppressedPaneClick();
  });


  // 노드 클릭 → M 갱신. dual source.
  //   plain          : single (clear + add)
  //   meta or ctrl   : toggle in/out (Mac = Cmd, Windows/Linux = Ctrl)
  //
  // Cross-platform: Mac 의 Ctrl+click 은 native 가 right-click 으로 변환 → 본
  // handler 는 fire 안 됨. Windows/Linux 의 Ctrl+click 은 plain left-click 으로
  // 본 handler 에 도달 → ctrlKey 가 true 일 때 toggle 로 처리. 따라서
  // `metaKey || ctrlKey` 둘 다 허용해도 Mac 의 Ctrl+click=contextmenu 와
  // 충돌하지 않음.
  //
  // R4 (ADR-0017 §Toolbar2 amend, batch-5): point-spawn tool active 인 동안
  // 기존 node 위 click 도 onpaneclick 의 spawn 로직으로 forward — 사용자가
  // *다른 panel 위에 새 item 만들고 싶음* 의도 허용. drag-spawn tool 은 별
  // pointer handler 가 처리해 onnodeclick 까지 안 옴.
  /**
   * Drill-level canvas selection.
   *
   * - Root scope: grouped descendants resolve to the outermost group.
   * - Drill scope: descendants resolve to the direct child under the drill root.
   * - Double click on a group enters that group as a separate drill scope.
   *
   * xyflow `onnodeclick` 만 노출 — dblclick detect 는 click count + timestamp
   * tracking 으로 (xyflow svelte 가 `onnodedoubleclick` 미제공).
   */
  const DOUBLE_CLICK_MS = 350;
  let lastClickId: string | null = null;
  let lastClickAt = 0;

  function resetDoubleClickTracker(): void {
    lastClickId = null;
    lastClickAt = 0;
  }

  function blurActiveCanvasElement(): void {
    if (typeof document === 'undefined') return;
    const active = document.activeElement as HTMLElement | null;
    active?.blur?.();
  }

  function canvasTargetFor(id: string): string {
    return targetAtDrillLevel(
      id,
      sessionStore.drillRootId,
      sessionStore.items,
      sessionStore.groups,
    );
  }

  function targetIsInsideDrill(targetId: string): boolean {
    const root = sessionStore.drillRootId;
    if (root === null) return true;
    if (targetId === root) return true;
    const item = sessionStore.items.get(targetId);
    const group = sessionStore.groups.get(targetId);
    let parentId = item?.parent_id ?? group?.parent_id ?? null;
    while (parentId !== null) {
      if (parentId === root) return true;
      const parent = sessionStore.groups.get(parentId);
      parentId = parent?.parent_id ?? null;
    }
    return false;
  }

  function clearCanvasDrillAndSelection(): void {
    blurActiveCanvasElement();
    pathEditStore.end();
    sessionStore.clearDrill();
    sessionStore.clearM();
  }

  function returnToLayerTabForCanvasSelection(): void {
    if (chromeStore.state.leftPanelTab === 'files') {
      chromeStore.setLeftPanelTab('layers');
    }
  }

  function isSelectedByGroup(itemId: string): boolean {
    const item = sessionStore.items.get(itemId);
    let parentId = item?.parent_id ?? null;
    while (parentId !== null) {
      if (sessionStore.M.has(parentId)) return true;
      const parent = sessionStore.groups.get(parentId);
      parentId = parent?.parent_id ?? null;
    }
    return false;
  }

  function isGroupHitTarget(itemId: string): boolean {
    const target = canvasTargetFor(itemId);
    return target !== itemId && sessionStore.groups.has(target);
  }

  function isFlowSelected(itemId: string): boolean {
    return sessionStore.M.has(itemId);
  }

  function groupIdFromOverlayNode(id: string): string | null {
    const prefix = id.startsWith(GROUP_HITBOX_PREFIX)
      ? GROUP_HITBOX_PREFIX
      : id.startsWith(GROUP_OVERLAY_PREFIX)
        ? GROUP_OVERLAY_PREFIX
        : null;
    if (prefix === null) return null;
    const groupId = id.slice(prefix.length);
    return sessionStore.groups.has(groupId) ? groupId : null;
  }

  function isDescendantOfGroup(targetId: string, groupId: string): boolean {
    let parentId =
      sessionStore.items.get(targetId)?.parent_id ??
      sessionStore.groups.get(targetId)?.parent_id ??
      null;
    while (parentId !== null) {
      if (parentId === groupId) return true;
      parentId = sessionStore.groups.get(parentId)?.parent_id ?? null;
    }
    return false;
  }

  function onnodeclick({ node, event }: { node: Node; event: MouseEvent | TouchEvent }) {
    if (isSelectMode) {
      returnToLayerTabForCanvasSelection();
      const id = node.id;
      const overlayGroupId = groupIdFromOverlayNode(id);
      const target = overlayGroupId ?? canvasTargetFor(id);
      const isMouseEvt = event instanceof MouseEvent;
      const isModifierClick = isMouseEvt && (event.metaKey || event.ctrlKey);
      // D22.5 — Cmd/Ctrl click 도 drill-in 일관.
      if (isModifierClick) {
        // Modifier-click selection is owned by capture-phase pointerdown. Letting
        // nodeclick toggle again reverts M on mouseup/click.
        resetDoubleClickTracker();
        return;
      }
      // Detect double-click (same id within window) — drill-down 한 단계.
      const now = isMouseEvt ? Date.now() : 0;
      const isDblClick =
        isMouseEvt && lastClickId === id && now - lastClickAt < DOUBLE_CLICK_MS;
      lastClickId = id;
      lastClickAt = now;
      if (isDblClick) {
        resetDoubleClickTracker();
        if (sessionStore.groups.has(target)) {
          sessionStore.setDrillRoot(target);
          const nextSelected =
            overlayGroupId ?? targetAtDrillLevel(id, target, sessionStore.items, sessionStore.groups);
          sessionStore.setM([nextSelected]);
          if (pathEditStore.editingPathId !== nextSelected) pathEditStore.end();
          const itemType = sessionStore.items.get(id)?.type;
          if (itemType === 'text' || itemType === 'rect' || itemType === 'ellipse') {
            sessionStore.suppressTextEditDblClick(id);
          }
        } else {
          sessionStore.setM([target]);
          if (pathEditStore.editingPathId !== target) pathEditStore.end();
        }
        blurActiveCanvasElement();
        return;
      }
      if (!targetIsInsideDrill(target)) sessionStore.clearDrill();
      sessionStore.setM([target]);
      if (pathEditStore.editingPathId !== target) pathEditStore.end();
      return;
    }
    if (event instanceof MouseEvent) {
      onpaneclick({ event });
    }
  }

  // ADR-0035 D1 — file_path 도구 의 picker. 전역 `filePicker` store 가 modal
  // visibility + caller callback 을 관리. spawn flow / 수정 flow 둘 다
  // 같은 modal instance 사용 (Canvas mount 의 단일 FilePickerModal).
  //
  // Caller 1 (spawn): file_path 도구 click → canvas click → filePicker.openFor
  //   (workspace='', select → createCanvasItem('file_path', pos) + path 갱신).
  // Caller 2 (rename): FilePathNode 의 onDblClick → filePicker.openFor(parent,
  //   select → applyMutation(path)).

  function onSpawnError(err: unknown): void {
    if (err instanceof UnauthorizedError) {
      window.location.href = '/auth';
      return;
    }
    toastStore.show({
      message: `Create failed: ${err instanceof Error ? err.message : String(err)}`,
      tone: 'error',
    });
  }

  function onAssetUploadError(err: unknown): void {
    if (err instanceof UnauthorizedError) {
      window.location.href = '/auth';
      return;
    }
    toastStore.show({
      message: err instanceof AssetUploadUnavailableError
        ? 'Asset upload API is not available yet. Backend work is required before image/document insertion can complete.'
        : `Asset upload failed: ${err instanceof Error ? err.message : String(err)}`,
      tone: 'error',
      durationMs: 6_000,
    });
  }

  const IMAGE_ACCEPT = 'image/png,image/jpeg,image/gif,image/webp,image/svg+xml';
  const DOCUMENT_ACCEPT = '.md,.txt,.json,.html,.css,.js,.ts,.tsx,.jsx,.pdf,text/*,application/json,application/pdf';

  function onpaneclick({ event }: { event: MouseEvent | TouchEvent }) {
    // Hand tool — exploration only, click no-op (Figma).
    if (isHandTool) return;
    if (suppressNextPaneClickAfterLasso) {
      clearSuppressedPaneClick();
      return;
    }
    if (
      event instanceof MouseEvent &&
      (event.metaKey || event.ctrlKey || event.shiftKey)
    ) {
      return;
    }
    // ── Tool-driven creation ───────────────────────────────────────────
    //
    // 점-spawn 도구 (text/note/file_path/terminal) 가 active 인 동안 빈 캔버스
    // 클릭은 새 item 을 그 위치에 생성. drag-spawn 도구 (rect/ellipse/line) 는
    // 별 pointer handler 가 처리 — onpaneclick 은 *down/up 이 같은 점* 인
    // 경우만 fire. 'select' 는 빈 영역 click 시 M clear (default).
    if (event instanceof MouseEvent) {
      const tool = toolStore.current;
      const flow = screenToFlowPosition({ x: event.clientX, y: event.clientY });

      // cursor=center 보정 — POINT_SPAWN_DEFAULTS 의 5 type 만. text 는 cursor
      // 가 좌상단 그대로 (작은 박스 + 더블 클릭 inline edit 진입 시 자연 정합).
      function centered(t: GhostTool): { x: number; y: number } {
        const s = POINT_SPAWN_DEFAULTS[t];
        return { x: flow.x - s.w / 2, y: flow.y - s.h / 2 };
      }

      if (tool === 'terminal') {
        void spawnMultiSessionTerminal(centered('terminal'));
        return;
      }
      if (tool === 'text') {
        const item = createCanvasItem('text', { x: flow.x, y: flow.y });
        void commitNewItem(item)
          .then(() => toolStore.consume())
          .catch(onSpawnError);
        return;
      }
      if (tool === 'note') {
        const item = createCanvasItem('note', centered('note'));
        void commitNewItem(item)
          .then(() => toolStore.consume())
          .catch(onSpawnError);
        return;
      }
      if (tool === 'path') {
        const local = canvasLocalFromClient(event.clientX, event.clientY);
        hoverScreen = local;
        hoverFlow = flow;
        if (pathCreateStart === null) {
          pathCreateStart = pathEndpointAt(flow, local);
          return;
        }
        const end = pathEndpointAt(flow, local);
        const parentId = pathParentForEndpoints(pathCreateStart.endpoint, end.endpoint);
        const item = autoRoutePath(
          {
            ...createPathItem(pathCreateStart.point, end.point),
            parent_id: parentId,
            from: pathCreateStart.endpoint,
            to: end.endpoint,
          },
          sessionStore.items,
        );
        pathCreateStart = null;
        void commitNewItem(item)
          .then(() => toolStore.consume())
          .catch(onSpawnError);
        return;
      }
      if (tool === 'snippets') {
        // ADR-0038 D6 — point-spawn, empty entries. cursor=center to match
        // the spawn ghost outline (POINT_SPAWN_DEFAULTS.snippets) so what
        // user sees as preview = what gets created at same center point.
        const item = createSnippetsItem(centered('snippets'));
        void commitNewItem(item)
          .then(() => toolStore.consume())
          .catch(onSpawnError);
        return;
      }
      if (tool === 'file_path') {
        const pos = centered('file_path');
        filePicker.openFor('', (path) => {
          const item = createCanvasItem('file_path', pos);
          const withPath = { ...item, path, kind: 'file' as const };
          void commitNewItem(withPath)
            .then(() => {
              toolStore.consume();
              // ★ 2026-05-22 사용자 보고 (#upload-canvas-desync) — async
              // commit-after layout refetch. applyMutation 의 PUT 응답 +
              // loadLayout 흐름이 *드물게* race 로 신규 item 미반영의
              // workaround. silent best-effort (실패해도 functional —
              // root cause 의 별 trace 작업).
              void sessionStore.reloadActiveLayout();
            })
            .catch(onSpawnError);
        });
        return;
      }
      if (tool === 'image') {
        const pos = centered('image');
        void pickLocalFile({ accept: IMAGE_ACCEPT }).then(async (file) => {
          if (file === null) return;
          try {
            const uploaded = await uploadAsset(file, 'image');
            const item = {
              ...createImageItem(pos),
              label: uploaded.file_name,
              asset_id: uploaded.asset_id,
              mime: uploaded.mime,
              original_w: uploaded.original_w,
              original_h: uploaded.original_h,
            };
            await commitNewItem(item);
            toolStore.consume();
            // ★ 2026-05-22 사용자 보고 (#upload-canvas-desync) — async
            // commit-after layout refetch (defensive, silent best-effort).
            void sessionStore.reloadActiveLayout();
          } catch (err) {
            onAssetUploadError(err);
          }
        });
        return;
      }
      if (tool === 'document') {
        const pos = centered('document');
        void pickLocalFile({ accept: DOCUMENT_ACCEPT }).then(async (file) => {
          if (file === null) return;
          try {
            const uploaded = await uploadAsset(file, 'document');
            const item = {
              ...createDocumentItem(pos),
              asset_id: uploaded.asset_id,
              label: uploaded.file_name.replace(/\.[^/.]+$/, ''),
              mime: uploaded.mime,
              file_name: uploaded.file_name,
              size_bytes: uploaded.size_bytes,
              content: undefined,
            };
            await commitNewItem(item);
            toolStore.consume();
            // ★ 2026-05-22 사용자 보고 (#upload-canvas-desync) — async
            // commit-after layout refetch (defensive, silent best-effort).
            void sessionStore.reloadActiveLayout();
          } catch (err) {
            onAssetUploadError(err);
          }
        });
        return;
      }
    }
    clearCanvasDrillAndSelection();
  }

  /**
   * Multi-session terminal spawn emulation (BE P2 endpoint 미 ship 대체).
   *
   * 흐름 (0033 §2.5 의 "manual UUID 생성 + mutateLayout PUT + attach_confirm POST"):
   *  1. createTerminalItem(coords) → fresh UUID
   *  2. mutateLayout(name, append) → BE 가 layout 에 unmatched UUID 보유
   *  3. attachConfirm(name) → BE 가 unmatched UUID 를 spawn_terminal_with_uuid →
   *     publish 0x88 TERMINAL_SPAWNED + 0x87 (other sessions)
   *  4. FE 의 handleTerminalSpawned → terminalPool.bindPaneId →
   *     PanelNode terminalPaneId derived 갱신 → XtermHost mount + PANE_OUT 흐름
   *
   * 실패 시 layout 에 dangling UUID 가 남을 수 있음 — 사용자가 close 또는
   * 다음 attach 에서 재시도. spawn 실패 가 영구라면 manual delete.
   */
  async function spawnMultiSessionTerminal(coords: { x: number; y: number }): Promise<void> {
    const active = sessionStore.active;
    if (active === null) return;
    const name = active.name;
    const fresh = createTerminalItem(coords);
    // 1+2) Append + commit (ADR-0028 D12 entry — history capture).
    const result = await sessionStore.applyMutation(
      (cur) => {
        const maxZ = cur.items.reduce((m, it) => (it.z > m ? it.z : m), 0);
        return {
          ...cur,
          items: [...cur.items, { ...fresh, z: maxZ + 1 }],
        };
      },
      {
        abortMessage: 'Session reconnect failed — terminal spawn aborted.',
        failMessage: 'Terminal create failed',
      },
    );
    if (!result.ok) return;
    sessionStore.setM([fresh.id]);
    // 3) Spawn the unmatched UUID — attachConfirm 은 layout mutation 이 아니므로
    //    history 무관 (spawn 실패해도 layout 의 panel entry 는 그대로).
    try {
      const confirmRes = await attachConfirm(name);
      if (confirmRes.failed.length > 0) {
        const failed = confirmRes.failed.find((f) => f.id === fresh.id);
        if (failed !== undefined) {
          toastStore.show({
            message: `Terminal spawn failed: ${failed.error}`,
            tone: 'error',
          });
        }
      }
      void terminalPool.refresh();
      toolStore.consume();
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Spawn confirm failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  /**
   * 노드 drag stop — 단일 / 다중 선택 모두 동일 path. xyflow 는 두 컴포넌트
   * 에서 본 callback 을 발화:
   *   - 단일 drag → `NodeWrapper` → `{ targetNode, nodes }` (nodes.length≥1)
   *   - 다중 group drag → `NodeSelection` → `{ targetNode: null, nodes }`
   *
   * 따라서 `targetNode === null` 가드를 두면 **multi-drag commit 전체가 skip**
   * 되어 BE PUT 안 됨 → 응답의 `loadLayout` 이 원래 position 으로 store 회귀
   * → 사용자가 선택 해제 후 position 회귀 시각 확인 (회귀 버그).
   *
   * 처리: nodes array 만 확인. line 은 endpoint delta 처리. 일괄
   * mutateLayout(callback) 으로 BE PUT 1회.
   */
  let nodeDragPriorSnapshot: CanvasLayout | null = null;

  function mergeMovedItemsWithPathCaches(
    items: readonly CanvasItem[],
    movedById: ReadonlyMap<string, CanvasItem>,
  ): CanvasItem[] {
    const merged = items.map((it) => movedById.get(it.id) ?? it);
    const itemMap = new Map(merged.map((it) => [it.id, it] as const));
    const movedIds = new Set(movedById.keys());
    return merged.map((it) =>
      it.type === 'path' && (movedById.has(it.id) || isPathConnectedToAny(it, movedIds))
        ? updatePathBBoxCache(it, itemMap)
        : it,
    );
  }

  function refreshLivePathCaches(movedById: ReadonlyMap<string, CanvasItem>): void {
    const movedIds = new Set(movedById.keys());
    for (const item of sessionStore.items.values()) {
      if (item.type !== 'path') continue;
      if (!movedById.has(item.id) && !isPathConnectedToAny(item, movedIds)) continue;
      debugCount('path.liveCache.refresh');
      sessionStore.items.set(item.id, updatePathBBoxCache(item, sessionStore.items));
    }
  }

  function movedItemsFromNodes(nodes: Node[]): Map<string, CanvasItem> {
    const movedById = new Map<string, CanvasItem>();
    for (const n of nodes) {
      const cur = sessionStore.items.get(n.id);
      if (cur === undefined) continue;
      if (effectiveLocked(cur.locked, cur.parent_id, sessionGroupsById)) continue;
      const pos = n.position;
      let next: CanvasItem;
      if (cur.type === 'line') {
        const oldBox = lineBoxFromEndpoints(
          { x: cur.x, y: cur.y },
          { x: cur.x2, y: cur.y2 },
        );
        const dx = pos.x - oldBox.x;
        const dy = pos.y - oldBox.y;
        const nextP1 = { x: cur.x + dx, y: cur.y + dy };
        const nextP2 = { x: cur.x2 + dx, y: cur.y2 + dy };
        const nextBox = lineBoxFromEndpoints(nextP1, nextP2);
        next = {
          ...cur,
          x: nextP1.x,
          y: nextP1.y,
          x2: nextP2.x,
          y2: nextP2.y,
          w: nextBox.w,
          h: nextBox.h,
        };
      } else if (cur.type === 'free_draw') {
        // free_draw 의 points 는 *flow-coord absolute*. wrapper position
        // 만 갱신하면 path 가 화면 따라가지 못함 + drag stop 시 BE PUT
        // 응답으로 옛 좌표 복원 → 사용자에게 "원위치 회귀" 시각. line 의
        // endpoint 평행 이동 패턴 정합 — bbox 의 dx/dy 만큼 모든 점 이동.
        const dx = pos.x - cur.x;
        const dy = pos.y - cur.y;
        next = {
          ...cur,
          x: pos.x,
          y: pos.y,
          points: cur.points.map((p) => ({ x: p.x + dx, y: p.y + dy })),
        };
      } else if (cur.type === 'path') {
        if (hasConnectedEndpoint(cur)) continue;
        const oldBox = computePathBBox(cur, sessionStore.items);
        const dx = pos.x - oldBox.x;
        const dy = pos.y - oldBox.y;
        next = translatePath(cur, dx, dy);
      } else {
        next = { ...cur, x: pos.x, y: pos.y };
      }
      movedById.set(n.id, next);
    }
    return movedById;
  }

  function onnodedrag({ nodes }: { targetNode: Node | null; nodes: Node[] }) {
    if (nodes.length === 0) return;
    if (sessionStore.active === null) return;
    const movedById = movedItemsFromNodes(nodes);
    if (movedById.size === 0) return;
    if (nodeDragPriorSnapshot === null) {
      nodeDragPriorSnapshot = sessionStore.layoutSnapshot();
    }
    // Live store update keeps derived group bboxes in sync while dragging.
    for (const [id, next] of movedById) {
      sessionStore.items.set(id, next);
    }
    refreshLivePathCaches(movedById);
  }

  function onnodedragstop({
    nodes,
  }: { targetNode: Node | null; nodes: Node[] }) {
    if (nodes.length === 0) return;
    if (sessionStore.active === null) return;

    // id → moved item map. 단일 drag 시 nodes.length === 1.
    const movedById = movedItemsFromNodes(nodes);
    if (movedById.size === 0) {
      nodeDragPriorSnapshot = null;
      return;
    }

    // PRE-state snapshot — optimistic update 직전에 잡아 history capture 의
    // 입력으로 명시 (ADR-0028 D7). 그렇지 않으면 layoutSnapshot() 이 이미
    // 새 position 으로 갱신된 후 호출되어 PRE === POST → Cmd+Z 가 no-op.
    const priorSnapshot = nodeDragPriorSnapshot ?? sessionStore.layoutSnapshot();
    nodeDragPriorSnapshot = null;
    // Optimistic store update — bind:nodes 양방향 sync 의 idempotent 결과.
    for (const [id, next] of movedById) {
      sessionStore.items.set(id, next);
    }
    refreshLivePathCaches(movedById);
    // 0065 FE-2 — priorSnapshot 명시 → applyMutation 이 PUT 실패 시 store 를
    // 복원 (drag-stop 의 optimistic update 가 silent 로 회귀 안 되도록).
    void sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: mergeMovedItemsWithPathCaches(cur.items, movedById),
      }),
      {
        abortMessage: 'Drag commit aborted — session reconnect failed.',
        failMessage: 'Drag commit failed — reverted to previous position.',
        priorSnapshot,
      },
    );
  }

  // Right-click handlers — pane area + node. Both prevent the native
  // browser context menu so our styled one wins. paneId / panelId
  // surface for the menu so item actions (Copy / Close) know what
  // they're acting on.
  function onpanecontextmenu({ event }: { event: MouseEvent | TouchEvent }) {
    if (!(event instanceof MouseEvent)) return;
    // ADR-0017 Amend ⑪ — hand 모드는 canvas/component 의 모든 mouse
    // interaction 차단 (pan 만 허용). ContextMenu 열지 않음.
    if (isHandTool) return;
    event.preventDefault();
    // Amend ⑤ — outside-wrapper 우 클릭 = Figma 의 deselect + empty menu.
    rightClickMSnapshot = null;
    clearCanvasDrillAndSelection();
    onContextMenuRequest?.({
      clientX: event.clientX,
      clientY: event.clientY,
      paneId: null,
      panelId: null,
    });
  }

  // ADR-0032 Amend ⑤ — Selection-wrapper 의 hit-test helper. wrapper 의
  // pointer-events:all 이 underlying node 의 click 을 가로채므로, 시각상
  // node 가 있는지 알려면 elementsFromPoint 로 wrapper 를 무시한 hit-test
  // 가 필요. wrapper 안 empty 영역인지 (Figma 의 deselect 트리거) 판정용.
  function nodeIdAtPoint(clientX: number, clientY: number): string | null {
    if (typeof document === 'undefined') return null;
    const elements = document.elementsFromPoint(clientX, clientY);
    for (const el of elements) {
      const e = el as HTMLElement;
      if (e.closest?.('.svelte-flow__selection-wrapper')) continue;
      const nodeEl = e.closest?.('.svelte-flow__node') as HTMLElement | null;
      if (nodeEl !== null) return nodeEl.dataset.id ?? null;
    }
    return null;
  }

  // ADR-0017 ⑪ + ADR-0032 Amend ② D15 — Global capture-phase contextmenu
  // handler. SvelteFlow 의 3 callback (onpane/onnode/onselection)contextmenu
  // 가 정상이라면 이 capture 는 redundant 지만, defense-in-depth 로 유지.
  //
  // 회귀 사후 진단 (Amend ²): drag-lasso 종료 후 SvelteFlow 가 selected node 들의
  // bounding box overlay `.svelte-flow__selection-wrapper` (z:2000, pointer-events:all)
  // 를 깔아 모든 right-click 의 hit target 이 됨. 우리 closest('.svelte-flow__node')
  // 는 null → empty-area menu 가 노출되던 회귀. 본 handler 의 else 분기에서
  // wrapper 를 인지하여 multi mode 진입 (panelId = M 의 임의 멤버).
  function onCanvasContextMenu(e: MouseEvent): void {
    // Hand 모드 — 어떤 menu 도 열지 않음 (ADR-0017 ⑪).
    if (isHandTool) {
      e.preventDefault();
      return;
    }
    if (sessionStore.active === null) return;
    const target = e.target as HTMLElement | null;
    const nodeEl = target?.closest('.svelte-flow__node') as HTMLElement | null;
    const nodeId = nodeEl?.dataset.id ?? null;
    e.preventDefault();
    if (nodeId !== null) {
      // ADR-0010 D22.4 (plan-0013 §3.7 H.3) — drill-aware right-click.
      // grouped item right-click 도 left click 과 동일: M = ancestor +
      // groupEntity ContextMenu open.
      const drillTarget = canvasTargetFor(nodeId);
      if (!targetIsInsideDrill(drillTarget)) sessionStore.clearDrill();
      if (sessionStore.M.size >= 2 && sessionStore.M.has(drillTarget)) {
        rightClickMSnapshot = null;
        onContextMenuRequest?.({
          clientX: e.clientX,
          clientY: e.clientY,
          paneId: null,
          panelId: drillTarget,
        });
        return;
      }
      const isDrillToGroup = drillTarget !== nodeId && sessionStore.groups.has(drillTarget);
      if (isDrillToGroup) {
        sessionStore.setM([drillTarget]);
        rightClickMSnapshot = null;
        onContextMenuRequest?.({
          clientX: e.clientX,
          clientY: e.clientY,
          paneId: null,
          panelId: null,
          groupId: drillTarget,
        });
        return;
      }
      // Root level item — 기존 onnodecontextmenu 와 동일 로직 (D9 snapshot restore).
      if (rightClickMSnapshot !== null && rightClickMSnapshot.has(drillTarget)) {
        sessionStore.setM([...rightClickMSnapshot]);
      } else if (!sessionStore.M.has(drillTarget)) {
        sessionStore.setM([drillTarget]);
      }
      rightClickMSnapshot = null;
      // pane_id (terminal item id = backend pane_id, ADR-0018 D2)
      const item = sessionStore.items.get(nodeId);
      const paneId = item?.type === 'terminal' ? nodeId : null;
      onContextMenuRequest?.({
        clientX: e.clientX,
        clientY: e.clientY,
        paneId,
        panelId: drillTarget,
      });
      return;
    }
    // ADR-0032 Amend ② D15 + Amend ⑤ — selection-wrapper 위 우 클릭.
    // hit-test 로 *node 가 시각상 있는지* 판정 (wrapper 는 z:2000 pointer:all
    // 라 click 가로채지만, 그 아래 node 가 있을 수도 빈 공간일 수도 있음):
    //   - node under wrapper → multi menu (기존 동작 유지)
    //   - 빈 공간 under wrapper → empty menu + clearM (Figma deselect)
    const wrapperEl = target?.closest('.svelte-flow__selection-wrapper');
    if (wrapperEl !== null) {
      rightClickMSnapshot = null;
      const nodeUnder = nodeIdAtPoint(e.clientX, e.clientY);
      if (nodeUnder !== null && sessionStore.M.size >= 2) {
        const anyId = [...sessionStore.M][0];
        if (anyId !== undefined) {
          onContextMenuRequest?.({
            clientX: e.clientX,
            clientY: e.clientY,
            paneId: null,
            panelId: anyId,
          });
        }
        return;
      }
      // Empty under wrapper — Amend ⑤: clearM + empty menu.
      clearCanvasDrillAndSelection();
      onContextMenuRequest?.({
        clientX: e.clientX,
        clientY: e.clientY,
        paneId: null,
        panelId: null,
      });
      return;
    }
    // Pane / 기타 — empty-area menu + clearM (Amend ⑤: outside-wrapper 우
    // 클릭 = Figma 의 deselect + empty menu. Scope-A 의 'M 보존' 정책은 폐기.).
    rightClickMSnapshot = null;
    clearCanvasDrillAndSelection();
    onContextMenuRequest?.({
      clientX: e.clientX,
      clientY: e.clientY,
      paneId: null,
      panelId: null,
    });
  }

  function onselectionchange({ nodes, edges }: { nodes: Node[]; edges: unknown[] }) {
    void nodes;
    void edges;
  }

  // ADR-0032 Amend ② D15 + Amend ⑤ — Selection-wrapper right-click.
  // wrapper 위 우 클릭: hit-test 로 *node 가 시각상 있는지* 판정.
  //   - node under → multi mode menu (panelId = M 의 임의 멤버. ContextMenu 의
  //     isMultiMode 가 panelId ∈ M && M.size >= 2 으로 판정.)
  //   - empty under → empty menu + clearM (Figma deselect).
  function onselectioncontextmenu({
    event,
    nodes,
  }: {
    event: MouseEvent | TouchEvent;
    nodes: Node[];
  }) {
    if (!(event instanceof MouseEvent)) return;
    if (isHandTool) return;
    event.preventDefault();
    rightClickMSnapshot = null;
    const nodeUnder = nodeIdAtPoint(event.clientX, event.clientY);
    if (nodeUnder === null) {
      // Empty under wrapper — Amend ⑤: clearM + empty menu.
      clearCanvasDrillAndSelection();
      onContextMenuRequest?.({
        clientX: event.clientX,
        clientY: event.clientY,
        paneId: null,
        panelId: null,
      });
      return;
    }
    const anyId = nodes[0] !== undefined ? canvasTargetFor(nodes[0].id) : [...sessionStore.M][0];
    if (anyId === undefined) return;
    onContextMenuRequest?.({
      clientX: event.clientX,
      clientY: event.clientY,
      paneId: null,
      panelId: anyId,
    });
  }

  // ADR-0032 Amend ⑤ — Selection-wrapper left click on empty space → clearM
  // (Figma deselect). node under 가 있으면 무시 — 사용자 요구는 *component 가
  // 없는 영역* 만. (node under 위 click 의 동작은 별 결정 — 현 미설정.)
  function onselectionclick({
    event,
  }: {
    event: MouseEvent | TouchEvent;
    nodes: Node[];
  }) {
    if (!(event instanceof MouseEvent)) return;
    if (isHandTool) return;
    if (event.metaKey || event.ctrlKey || event.shiftKey) return;
    const nodeUnder = nodeIdAtPoint(event.clientX, event.clientY);
    if (nodeUnder !== null) return;
    clearCanvasDrillAndSelection();
  }

  function onnodecontextmenu({ event, node }: { event: MouseEvent | TouchEvent; node: Node }) {
    if (!(event instanceof MouseEvent)) return;
    // ADR-0017 Amend ⑪ — hand 모드는 component event 절대 격리.
    if (isHandTool) return;
    event.preventDefault();
    // ADR-0032 D9 — clicked node 가 right-click 직전 M ∈ 이었다면 M 복원
    // (SvelteFlow internal 의 click-to-single reset 회귀 차단). clicked ∉
    // pre-reset M 이면 ADR-0032 D9 의 "M = {clicked} replace" — 명시 setM
    // (SvelteFlow 가 이미 reset 했더라도 idempotent).
    const drillTarget = canvasTargetFor(node.id);
    if (!targetIsInsideDrill(drillTarget)) sessionStore.clearDrill();
    if (rightClickMSnapshot !== null && rightClickMSnapshot.has(drillTarget)) {
      sessionStore.setM([...rightClickMSnapshot]);
    } else if (sessionStore.M.size >= 2 && sessionStore.M.has(drillTarget)) {
      // Preserve existing multi-selection.
    } else if (drillTarget !== node.id && sessionStore.groups.has(drillTarget)) {
      sessionStore.setM([drillTarget]);
      rightClickMSnapshot = null;
      onContextMenuRequest?.({
        clientX: event.clientX,
        clientY: event.clientY,
        paneId: null,
        panelId: null,
        groupId: drillTarget,
      });
      return;
    } else if (!sessionStore.M.has(node.id)) {
      sessionStore.setM([node.id]);
    }
    rightClickMSnapshot = null;
    const data = node.data as Record<string, unknown> | undefined;
    const paneId = typeof data?.['pane_id'] === 'string' ? (data['pane_id'] as string) : null;
    onContextMenuRequest?.({
      clientX: event.clientX,
      clientY: event.clientY,
      paneId,
      panelId: drillTarget,
    });
  }

</script>

<!-- capture-phase pointer handlers: SvelteFlow 의 selection box 보다 먼저 받음.
     onpointerdowncapture / onpointermovecapture / onpointerupcapture 는 Svelte 의
     이벤트 capture 변형 — 본 요소가 root container 라 SvelteFlow 자식 요소 모두
     를 cover. drag tool 비활성 시 down 핸들러가 즉시 early-return 하므로 일반
     이벤트는 SvelteFlow 가 정상 처리. -->
<div
  bind:this={canvasRootEl}
  class="canvas-root"
  role="presentation"
  class:drag-cursor={isDragTool && !isSpacePressed && !isHandTool}
  class:path-input-mode={isPathTool}
  class:path-cursor={isPathTool && !isSpacePressed && !isHandTool}
  class:path-create-pending={pathCreateStart !== null}
  class:text-cursor={isTextTool && !isSpacePressed && !isHandTool}
  class:pan-cursor={isSpacePressed || isHandTool}
  class:hand-mode={isHandTool}
  style:--canvas-zoom={sessionStore.viewport.zoom}
  onpointerdowncapture={onCanvasPointerDown}
  onpointermovecapture={onCanvasPointerMove}
  onpointerupcapture={onCanvasPointerUp}
  onpointercancelcapture={onCanvasPointerCancel}
  onpointerleave={() => {
    hoverScreen = null;
    hoverFlow = null;
    clearCanvasGroupHover();
  }}
  oncontextmenucapture={onCanvasContextMenu}
>
  <!-- NewPanelButton overlay 제거 — 기능은 Toolbar2 의 terminal 도구로 마이그레이션.
       legacy 진입은 `handleTerminalClick(legacy branch)` 가 `requestLegacyNewPane`
       호출. multi-session 은 BE Stage 5-D P2 endpoint 도착 시 wire. -->
  <SvelteFlow
    nodes={flowNodes}
    edges={EMPTY_EDGES}
    {nodeTypes}
    {onnodeclick}
    {onpaneclick}
    {onnodedrag}
    {onnodedragstop}
    {onmove}
    {onpanecontextmenu}
    {onnodecontextmenu}
    {onselectioncontextmenu}
    {onselectionclick}
    {onselectionchange}
    panOnDrag={isMaximizedActive ? [] : panOnDragMask}
    panOnScroll={!isMaximizedActive}
    zoomOnScroll={!isMaximizedActive}
    zoomOnPinch={!isMaximizedActive}
    zoomOnDoubleClick={false}
    selectionOnDrag={false}
    nodesDraggable={isSelectMode && !isMaximizedActive}
    elementsSelectable={false}
    selectionKey={null}
    multiSelectionKey={null}
    minZoom={0.05}
    maxZoom={3}
    fitView={false}
    elevateNodesOnSelect={false}
    onlyRenderVisibleElements={false}
    deleteKey={null}
    proOptions={SVELTE_FLOW_PRO_OPTIONS}
  >
    <!-- patternColor/bgColor 를 prop 으로 넘기면 SVG attribute 로 들어가
         CSS var() 가 풀리지 않음. .svelte-flow 의 --xy-background-*
         CSS var override 만으로 색 적용. -->
    <Background variant={BackgroundVariant.Dots} gap={24} size={1.5} />
  </SvelteFlow>

  {#if dragState !== null && ghostPreview !== null}
    <div
      class="drag-ghost"
      class:ghost-ellipse={ghostPreview.tool === 'ellipse'}
      class:ghost-line={ghostPreview.tool === 'line'}
      class:ghost-free-draw={ghostPreview.tool === 'free_draw'}
      style="left: {ghostPreview.left}px; top: {ghostPreview.top}px; width: {ghostPreview.width}px; height: {ghostPreview.height}px;"
      aria-hidden="true"
    >
      {#if ghostPreview.tool === 'line'}
        <svg
          width={ghostPreview.width}
          height={ghostPreview.height}
          viewBox={`0 0 ${ghostPreview.width} ${ghostPreview.height}`}
          preserveAspectRatio="none"
        >
          <line
            x1={ghostPreview.x1}
            y1={ghostPreview.y1}
            x2={ghostPreview.x2}
            y2={ghostPreview.y2}
            stroke="var(--color-accent)"
            stroke-width={2}
            stroke-linecap="round"
          />
          <circle cx={ghostPreview.x1} cy={ghostPreview.y1} r="3.5" />
          <circle cx={ghostPreview.x2} cy={ghostPreview.y2} r="3.5" />
        </svg>
      {:else if ghostPreview.tool === 'free_draw'}
        <svg
          width={ghostPreview.width}
          height={ghostPreview.height}
          viewBox={`0 0 ${ghostPreview.width} ${ghostPreview.height}`}
          preserveAspectRatio="none"
        >
          <path
            d={ghostPreview.path}
            fill="none"
            stroke="var(--color-accent)"
            stroke-width={2}
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      {/if}
    </div>
  {/if}

  {#if pathCreatePreview !== null}
    <div
      class="path-create-preview"
      style="left: {pathCreatePreview.left}px; top: {pathCreatePreview.top}px; width: {pathCreatePreview.width}px; height: {pathCreatePreview.height}px;"
      aria-hidden="true"
    >
      <svg
        width={pathCreatePreview.width}
        height={pathCreatePreview.height}
        viewBox={`0 0 ${pathCreatePreview.width} ${pathCreatePreview.height}`}
        preserveAspectRatio="none"
      >
        <line
          x1={pathCreatePreview.x1}
          y1={pathCreatePreview.y1}
          x2={pathCreatePreview.x2}
          y2={pathCreatePreview.y2}
          stroke="var(--color-accent)"
          stroke-width={2}
          stroke-linecap="round"
          stroke-dasharray="6 4"
        />
        <circle cx={pathCreatePreview.x1} cy={pathCreatePreview.y1} r="3.5" />
        <circle cx={pathCreatePreview.x2} cy={pathCreatePreview.y2} r="3.5" />
      </svg>
    </div>
  {/if}

  {#if pathCreateAnchorPreview !== null}
    <div
      class="path-anchor-preview-box"
      style="left: {pathCreateAnchorPreview.box.x}px; top: {pathCreateAnchorPreview.box.y}px; width: {pathCreateAnchorPreview.box.w}px; height: {pathCreateAnchorPreview.box.h}px;"
      aria-hidden="true"
    ></div>
    {#each pathCreateAnchorPreview.anchors as previewAnchor (previewAnchor.anchor)}
      <div
        class="path-anchor-preview"
        class:nearest={previewAnchor.nearest}
        class:hovered={previewAnchor.hovered}
        style="left: {previewAnchor.x}px; top: {previewAnchor.y}px;"
        aria-hidden="true"
      ></div>
    {/each}
  {/if}

  {#if lassoPreview !== null}
    <div
      class="selection-marquee"
      style="left: {lassoPreview.left}px; top: {lassoPreview.top}px; width: {lassoPreview.width}px; height: {lassoPreview.height}px;"
      aria-hidden="true"
    ></div>
  {/if}

  {#if pointSpawnGhost !== null && dragState === null}
    <div
      class="point-spawn-ghost"
      data-tool={pointSpawnGhost.tool}
      style="left: {pointSpawnGhost.x}px; top: {pointSpawnGhost.y}px; width: {pointSpawnGhost.w}px; height: {pointSpawnGhost.h}px;"
      aria-hidden="true"
    ></div>
  {/if}
</div>

<!-- ADR-0035 D1 — single picker modal instance. 전역 filePicker store 가
     visibility + caller callback 관리 (spawn / rename 두 caller 공유). -->
<FilePickerModal
  open={filePicker.open}
  initialDir={filePicker.initialDir}
  accept={filePicker.accept}
  onCancel={() => filePicker.cancel()}
  onSelect={(abs) => filePicker.select(abs)}
  onUnauthorized={() => { window.location.href = '/auth'; }}
/>

<style>
  .canvas-root {
    width: 100%;
    height: 100%;
    min-height: 0;
    position: relative;
    background: var(--canvas-bg);
    --canvas-scaler-size: 10px;
    --canvas-scaler-border: 1.5px;
  }

  /* SvelteFlow 의 컨트롤 / 미니맵 / 백그라운드의 default color 를 토큰으로 override.
     xyflow CSS custom properties — @xyflow/svelte/dist/style.css 정의. */
  .canvas-root :global(.svelte-flow) {
    background: var(--canvas-bg);
    --xy-background-color-default: var(--canvas-bg);
    --xy-background-pattern-color-default: var(--canvas-grid);
    --xy-node-border: 0;
    --xy-node-border-selected: 0;
    --xy-node-boxshadow-selected: none;
  }

  /* ref/frontend-design/components.html §05 — Shared rules B/C:
   *   - Selection 시각은 기본적으로 wrapper (.svelte-flow__node) 가 책임.
   *     Minimized node 는 wrapper bbox 대신 node 자체 border 로 표시.
   *   - box-shadow ring 패턴 (border-radius inherit) — outline 과 달리 shape 의
   *     radius 를 따라간다. selection / hover 둘 다 동일 패턴.
   *   - SvelteFlow 의 default `border`/`box-shadow` 는 XY variable 로 비활성화
   *     (위 .svelte-flow 의 --xy-node-* 0/none). 우리 ring 만 표시. */
  .canvas-root :global(.svelte-flow__node) {
    border: 0 !important;
    outline: none !important;
    background: transparent !important;
    box-shadow: none;
    transition: box-shadow 120ms ease;
  }

  .canvas-root :global(.svelte-flow__node:hover) {
    box-shadow: 0 0 0 calc(1px / var(--canvas-zoom, 1)) var(--color-border-strong);
  }

  .canvas-root :global(.svelte-flow__node.m-selected) {
    box-shadow: 0 0 0 calc(1.5px / var(--canvas-zoom, 1)) var(--color-accent);
    /* Elevate selected nodes (and their bbox ring + NodeResizer handles)
       above all other canvas items so the selection chrome is never
       occluded by items with higher data.z. Stays below group overlay
       (OVERLAY_Z = 1_000_000) so group bboxes still render on top.
       Deselecting restores the data.z stacking. */
    z-index: 9999 !important;
  }

  .canvas-root :global(.panel-resize-handle) {
    background: transparent !important;
    border-color: transparent !important;
    width: 7px !important;
    height: 7px !important;
    border-width: 1.5px !important;
    border-style: solid !important;
    border-radius: 1px !important;
    pointer-events: auto !important;
    z-index: 10 !important;
  }

  .canvas-root :global(.panel-resize-handle::after) {
    content: '';
    position: absolute;
    left: 50%;
    top: 50%;
    width: var(--canvas-scaler-size);
    height: var(--canvas-scaler-size);
    box-sizing: border-box;
    background: var(--color-bg);
    border: var(--canvas-scaler-border) solid var(--color-accent);
    border-radius: 1px;
    pointer-events: none;
    transform: translate(-50%, -50%) scale(min(1, calc(1 / var(--canvas-zoom, 1))));
    transform-origin: center;
  }

  .canvas-root :global(.panel-resize-line) {
    border-color: transparent !important;
    pointer-events: auto !important;
  }

  .canvas-root :global(.svelte-flow__node.is-minimized.m-selected) {
    box-shadow: none;
  }

  .canvas-root :global(.svelte-flow__node.group-selected),
  .canvas-root :global(.svelte-flow__node.group-selected:hover),
  .canvas-root :global(.svelte-flow__node.group-selected.m-selected) {
    box-shadow: none;
  }

  .canvas-root :global(.svelte-flow__node.group-hit-target:hover),
  .canvas-root :global(.svelte-flow__node.group-hit-target.m-selected) {
    box-shadow: none;
  }

  .canvas-root :global(.svelte-flow__node.group-selected .svelte-flow__resize-control),
  .canvas-root :global(.svelte-flow__node.group-hit-target .svelte-flow__resize-control),
  .canvas-root :global(.svelte-flow__node.group-hit-target .endpoint),
  .canvas-root :global(.svelte-flow__node.group-selected .endpoint) {
    display: none !important;
    pointer-events: none !important;
  }

  /* line 같은 *대각선 line-art* 의 bounding-box 는 ring 으로 표시하면 회귀
   * (사각형 outline 으로 보임). SvelteFlow 가 type 별 class (`svelte-flow__
   * node-line`) 자동 부여 → 본 selector 로 ring 일괄 비활성. 자체 selection
   * 시각은 LineNode 의 endpoint button 이 담당. */
  .canvas-root :global(.svelte-flow__node-line),
  .canvas-root :global(.svelte-flow__node-line:hover),
  .canvas-root :global(.svelte-flow__node-line.m-selected),
  .canvas-root :global(.svelte-flow__node-path),
  .canvas-root :global(.svelte-flow__node-path:hover),
  .canvas-root :global(.svelte-flow__node-path.m-selected) {
    box-shadow: none;
  }

  /*
   * 2026-05-20 figure hit-test 좁힘 — SvelteFlow 의 `.svelte-flow__node`
   * wrapper 가 bbox 전체 mouse event 를 catch 하던 옛 동작 폐기. 자식 SVG 의
   * pointer-events attribute 가 authoritative hit-test 가 되도록 wrapper
   * 자체를 pass-through.
   *  - `.svelte-flow__node-line` — 모든 line. LineNode 의 invisible hit-target
   *    line (pointer-events="stroke") 만 catch.
   *  - `.svelte-flow__node.fill-off` — fill_enabled=false rect/ellipse.
   *    ShapeNode 의 SVG `<rect>` / `<ellipse>` 의 pointer-events="visibleStroke"
   *    가 stroke ring 만 catch — 내부 클릭은 뒤 layer (canvas / panel) 로 전달.
   *
   * NodeResizer handle/line 은 위 `.panel-resize-*` rule 이 pointer-events 를
   * 명시적으로 되살린다. SvelteFlow 의 기본 `.svelte-flow__resize-control`
   * 에는 pointer-events override 가 없으므로 이 override 가 없으면
   * fill-off wrapper 의 pass-through 와 함께 resize hit-test 도 죽는다.
   */
  .canvas-root :global(.svelte-flow__node-line),
  .canvas-root :global(.svelte-flow__node-path),
  .canvas-root :global(.svelte-flow__node.fill-off),
  .canvas-root :global(.svelte-flow__node.gtmux-group) {
    pointer-events: none;
  }

  /* ADR-0010 D15 — Group overlay wrapper 는 selection ring / hover halo 도 비활성.
     overlay 자체가 dotted outline 의 자체 시각 만 — SvelteFlow 의 default
     selected/hover box-shadow 가 겹치면 시각 혼란. */
  .canvas-root :global(.svelte-flow__node.gtmux-group),
  .canvas-root :global(.svelte-flow__node.gtmux-group:hover),
  .canvas-root :global(.svelte-flow__node.gtmux-group.m-selected) {
    box-shadow: none !important;
    background: transparent !important;
  }

  /* Hand tool is viewport-only. Make every node wrapper transparent to pointer
   * hit-testing so left-drag pans even when the cursor starts over an element,
   * and node-local controls (resize handles, xterm input, double-click editors,
   * image/document action buttons) cannot receive interaction. */
  .canvas-root.hand-mode :global(.svelte-flow__node),
  .canvas-root.hand-mode :global(.svelte-flow__resize-control),
  .canvas-root.hand-mode :global(.svelte-flow__edge),
  .canvas-root.hand-mode :global(.svelte-flow__connection) {
    pointer-events: none !important;
  }

  /* Drag-to-create tool cursor — Batch 2 (rect/ellipse/line). */
  .canvas-root.drag-cursor,
  .canvas-root.drag-cursor :global(.svelte-flow__pane) {
    cursor: crosshair;
  }

  .canvas-root.path-cursor,
  .canvas-root.path-cursor :global(.svelte-flow__pane) {
    cursor: crosshair;
  }

  /* Path tool is an input mode: canvas items are anchor candidates, not direct
   * interaction targets. Keep hit-testing on the pane so clicks over components
   * still create path endpoints; anchor previews remain geometry-driven through
   * hoverFlow/connectableTargetAtPoint. */
  .canvas-root.path-input-mode :global(.svelte-flow__node),
  .canvas-root.path-input-mode :global(.svelte-flow__resize-control),
  .canvas-root.path-input-mode :global(.svelte-flow__edge),
  .canvas-root.path-input-mode :global(.svelte-flow__connection) {
    pointer-events: none !important;
  }

  /* Text tool — I-beam cursor (입력 텍스트 위 cursor 정합). */
  .canvas-root.text-cursor,
  .canvas-root.text-cursor :global(.svelte-flow__pane) {
    cursor: text;
  }

  /* G29: Space-hold pan modifier — grab while Space is held, grabbing
   * during the actual drag. SvelteFlow's panning class is applied to the
   * pane when a pan is in progress. */
  .canvas-root.pan-cursor,
  .canvas-root.pan-cursor :global(.svelte-flow__pane),
  .canvas-root.pan-cursor :global(.svelte-flow__node) {
    cursor: grab;
  }

  .canvas-root.pan-cursor :global(.svelte-flow__pane.dragging),
  .canvas-root.pan-cursor :global(.svelte-flow.dragging),
  .canvas-root.pan-cursor :global(.svelte-flow.dragging .svelte-flow__pane) {
    cursor: grabbing;
  }

  /* Point-spawn tool ghost — cursor hover 시 새 item 의 default 크기로
   * outline 미리보기 (cursor=center 정렬). 5 type: terminal/note/file_path/
   * image/document. Dashed accent, no fill — purely guide, no interactivity. */
  .point-spawn-ghost {
    position: absolute;
    box-sizing: border-box;
    border: 1px dashed var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 6%, transparent);
    pointer-events: none;
    /* canvas-overlay (18) — side-panel (20) 보다 아래라 LeftPanel/RightPanel
       위로 ghost 가 표시되지 않음. */
    z-index: var(--z-canvas-overlay);
    border-radius: var(--radius-sm);
  }

  .selection-marquee {
    position: absolute;
    box-sizing: border-box;
    border: 1px solid var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
    pointer-events: none;
    z-index: var(--z-canvas-overlay);
  }

  .path-create-preview {
    position: absolute;
    pointer-events: none;
    z-index: var(--z-canvas-overlay);
  }

  .path-create-preview svg {
    display: block;
    overflow: visible;
  }

  .path-create-preview circle {
    fill: var(--color-accent);
  }

  .path-anchor-preview-box,
  .path-anchor-preview {
    position: absolute;
    pointer-events: none;
    z-index: var(--z-canvas-overlay);
  }

  .path-anchor-preview-box {
    box-sizing: border-box;
    border: var(--canvas-scaler-border, 1.5px) dashed var(--color-accent);
    border-radius: 4px;
    opacity: 0.55;
  }

  .path-anchor-preview {
    width: var(--canvas-scaler-size, 10px);
    height: var(--canvas-scaler-size, 10px);
    border: var(--canvas-scaler-border, 1.5px) solid var(--color-accent);
    border-radius: 999px;
    background: var(--color-surface);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-bg) 80%, transparent);
    opacity: 0.85;
    transform: translate(-50%, -50%);
  }

  .path-anchor-preview.nearest {
    opacity: 1;
    border-color: var(--color-accent);
  }

  .path-anchor-preview.hovered {
    background: var(--color-accent);
    box-shadow:
      0 0 0 2px color-mix(in srgb, var(--color-bg) 80%, transparent),
      0 0 0 6px color-mix(in srgb, var(--color-accent) 22%, transparent);
  }

  /* Live preview during drag — container-local screen coords. */
  .drag-ghost {
    position: absolute;
    box-sizing: border-box;
    border: 1px solid var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--color-accent) 18%, transparent);
    pointer-events: none;
    z-index: var(--z-canvas-overlay);
  }

  /* Free draw 의 stroke preview — bounding-box 강조 안 함 (path 자체가 시각). */
  .drag-ghost.ghost-free-draw {
    border: none;
    background: transparent;
    box-shadow: none;
  }

  .drag-ghost.ghost-ellipse {
    border-radius: 50%;
  }

  .drag-ghost.ghost-line {
    border: 0;
    background: transparent;
  }

  .drag-ghost.ghost-line svg {
    display: block;
    overflow: visible;
  }

  .drag-ghost.ghost-line circle {
    fill: var(--color-accent);
  }
</style>
