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
  //     * Cmd / Ctrl + click : M.toggle(id) (multi-select 추가/제거)
  //     * Shift + click      : M.toggle(id) 도 허용 (cross-platform 친화)
  // - 캔버스 dot grid 는 token-driven (--canvas-bg, --canvas-grid).
  // - panOnDrag = [1, 2] — middle/right 마우스 버튼만 pan (left는 selection/drag용).

  import { SvelteFlow, Background, BackgroundVariant, useSvelteFlow } from '@xyflow/svelte';
  import type { Node, Viewport } from '@xyflow/svelte';
  import '@xyflow/svelte/dist/style.css';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { toolStore } from '$lib/stores/toolStore.svelte';
  import { attachConfirm, deleteItem, mutateLayout, UnauthorizedError } from '$lib/http/sessions';
  import type { CanvasItem } from '$lib/types/canvas';
  import { effectiveLocked, effectiveVisibility } from '$lib/types/group';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import PanelNode from './PanelNode.svelte';
  import TextNode from './TextNode.svelte';
  import NoteNode from './NoteNode.svelte';
  import FilePathNode from './FilePathNode.svelte';
  import ShapeNode from './ShapeNode.svelte';
  import LineNode from './LineNode.svelte';
  import {
    commitNewItem,
    createCanvasItem,
    createShapeItem,
    createLineItem,
    createTerminalItem,
    lineBoxFromEndpoints,
  } from './itemFactory';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';

  interface CanvasProps {
    /** ContextMenu trigger — `+page.svelte` 가 호스팅하는 ContextMenu
     *  싱글톤의 `openAt` 으로 wire. `null/undefined` 시 컨텍스트 메뉴
     *  비활성 — Canvas 내부 동작에 영향 없음. */
    onContextMenuRequest?: (args: {
      clientX: number;
      clientY: number;
      paneId?: string | null;
      panelId?: string | null;
    }) => void;
  }

  const { onContextMenuRequest }: CanvasProps = $props();

  // SvelteFlow viewport projection — onpaneclick 의 screen 좌표를 canvas 좌표로 변환.
  // useSvelteFlow 는 SvelteFlowProvider 컨텍스트가 있어야 동작 (+page.svelte 에서 마운트됨).
  const { screenToFlowPosition, setViewport, getViewport } = useSvelteFlow();

  /** Drag-to-create state — Batch 2 의 rect/ellipse/line gesture. */
  type DragShape = 'rect' | 'ellipse' | 'line';
  interface DragState {
    tool: DragShape;
    /** Flow-coord start point (commit 시 기준). */
    startFlow: { x: number; y: number };
    /** Container-local screen coord — ghost overlay 의 left/top 계산용. */
    startLocal: { x: number; y: number };
    currentLocal: { x: number; y: number };
  }
  let dragState = $state<DragState | null>(null);

  const isDragTool = $derived(
    toolStore.current === 'rect' ||
      toolStore.current === 'ellipse' ||
      toolStore.current === 'line',
  );

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
    // ── Delete/Backspace — remove selected items (multi-session only) ────
    // SvelteFlow 의 builtin delete (deleteKey={null}) 는 비활성 — store 와
    // 미동기 상태로 nodes 만 임시 제거되어 "사라졌다 돌아오는" 회귀 야기.
    // 본 핸들러가 단독으로 BE `DELETE /api/sessions/.../items/:id` 호출 +
    // sessionStore 동기. terminal item 은 kill_terminal=false 기본 (G25 —
    // panel 제거만, terminal pool 유지). xterm/editable focus 시 무시.
    if (e.key === 'Delete' || e.key === 'Backspace') {
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
   * Remove every item in `sessionStore.M` via BE `deleteItem` + store sync.
   *
   * - terminal item: `kill_terminal=false` (G25 default) — panel 만 제거,
   *   terminal 은 pool 에 남음 (다른 panel 이 같은 UUID 를 mirror 할 수 있음).
   * - 다른 item type: BE 가 layout 에서만 제거. body 무관.
   * - 부분 실패 허용 — 성공한 것만 store 에서 빼고 토스트로 카운트 표시.
   */
  async function deleteSelected(): Promise<void> {
    const active = sessionStore.active;
    if (active === null) return;
    const guard = await sessionStore.guardOutgoingMutation();
    if (!guard.ok) {
      toastStore.show({
        message: 'Session reconnect failed — delete aborted.',
        tone: 'error',
      });
      return;
    }
    const ids = Array.from(sessionStore.M);
    if (ids.length === 0) return;
    let ok = 0;
    let fail = 0;
    for (const id of ids) {
      try {
        await deleteItem(active.name, id, false);
        sessionStore.items.delete(id);
        sessionStore.M.delete(id);
        ok += 1;
      } catch (err) {
        if (err instanceof UnauthorizedError) {
          window.location.href = '/auth';
          return;
        }
        console.warn('[gtmux] deleteItem failed', id, err);
        fail += 1;
      }
    }
    if (fail === 0) {
      toastStore.show({
        message: `Removed ${ok} item${ok === 1 ? '' : 's'}.`,
        tone: 'success',
      });
    } else {
      toastStore.show({
        message: `Removed ${ok}, failed ${fail}.`,
        tone: 'error',
      });
    }
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
  const panOnDragMask = $derived(
    isSpacePressed || isHandTool ? [0, 1, 2] : [1, 2],
  );

  const GHOST_LINE_PADDING = 8;

  /** Live preview overlay geometry (container-local, screen px). */
  const ghostPreview = $derived.by(() => {
    if (dragState === null) return null;
    const sx = dragState.startLocal.x;
    const sy = dragState.startLocal.y;
    const cx = dragState.currentLocal.x;
    const cy = dragState.currentLocal.y;
    if (dragState.tool === 'line') {
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
    };
  });

  /** Drag 가 click 으로 취급되는 임계 — flow 좌표 기준 8px. */
  const DRAG_CLICK_THRESHOLD = 8;

  function onCanvasPointerDown(e: PointerEvent) {
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

    dragState = {
      tool: toolStore.current as DragShape,
      startFlow: flow,
      startLocal: { x: localX, y: localY },
      currentLocal: { x: localX, y: localY },
    };

    root.setPointerCapture(e.pointerId);
  }

  function onCanvasPointerMove(e: PointerEvent) {
    if (dragState === null) return;
    const root = e.currentTarget as HTMLElement;
    const rect = root.getBoundingClientRect();
    dragState = {
      ...dragState,
      currentLocal: {
        x: e.clientX - rect.left,
        y: e.clientY - rect.top,
      },
    };
  }

  function onCanvasPointerUp(e: PointerEvent) {
    if (dragState === null) return;
    e.stopPropagation();
    const state = dragState;
    dragState = null;

    const endFlow = screenToFlowPosition({ x: e.clientX, y: e.clientY });
    const dx = endFlow.x - state.startFlow.x;
    const dy = endFlow.y - state.startFlow.y;
    const distance = Math.hypot(dx, dy);

    let item;
    if (state.tool === 'line') {
      // Line: endpoint pair 그대로 보존 → 4 방향 (TL→BR, BR→TL, TR→BL, BL→TR).
      // distance < threshold 면 default-size 의 down-right 방향 단일선.
      const p2 =
        distance < DRAG_CLICK_THRESHOLD
          ? { x: state.startFlow.x + 240, y: state.startFlow.y + 80 }
          : endFlow;
      item = createLineItem(state.startFlow, p2);
    } else {
      // Rect/Ellipse: bounding box 정규화 (drag 방향 무관, w/h ≥ 0).
      const bounds = {
        x: Math.min(state.startFlow.x, endFlow.x),
        y: Math.min(state.startFlow.y, endFlow.y),
        w: distance < DRAG_CLICK_THRESHOLD ? 0 : Math.abs(dx),
        h: distance < DRAG_CLICK_THRESHOLD ? 0 : Math.abs(dy),
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
    // OS 가 capture 를 빼앗는 경우 (다른 modal 등) drag state 청소.
    dragState = null;
  }

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
  };

  // M cardinality — PanelNode 가 single/multi 분기를 위해 참조.
  const isMultiSelection = $derived(sessionStore.M.size > 1);
  const sessionGroupsById = $derived(new Map(sessionStore.groups));

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
    const common = {
      id: item.id,
      position: { x: item.x, y: item.y },
      draggable: !locked,
      selected: sessionStore.M.has(item.id),
      zIndex: item.z,
      width: item.w,
      height: item.h,
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
          w: box.w,
          h: box.h,
          _boxX1: item.x - box.x,
          _boxY1: item.y - box.y,
          _boxX2: item.x2 - box.x,
          _boxY2: item.y2 - box.y,
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
      },
    };
  }

  /* ── SvelteFlow nodes — controlled binding via `bind:nodes` ─────────────
   *
   * Root cause of the `effect_update_depth_exceeded` loop:
   *   `<SvelteFlow nodes = $bindable([])>` 가 controlled prop. `bind:nodes`
   *   없이 derived 를 one-way 로 넘기면, xyflow 의 internal `set nodes(new)`
   *   write-back 이 외부 (derived → 불가) 와 sync 실패 → 다음 internal effect
   *   가 자기 store 와 다시 비교하며 자가 trigger → cascade.
   *
   * Fix: 외부에 *명시 mutable $state* (`internalNodes`) 를 두고 `bind:nodes`.
   * sessionStore 변화 시 effect 가 internalNodes 를 *교체* (selected 포함).
   * SvelteFlow internal mutation (drag/click 등) 은 bind 양방향으로
   * internalNodes 에 반영. effect 의 deps 는 sessionStore 만 — SvelteFlow
   * mutation 만 발생한 경우 fire 안 함 → loop 0.
   *
   * selected 의 single source 는 *sessionStore.M*. effect 가 매 rebuild 시
   * `selected: M.has(id)` 를 채움. internal click 으로 selected 가 변하면
   * onnodeclick 의 setM 으로 sessionStore 와 sync → effect → rebuild → ok. */
  let internalNodes: Node[] = $state([]);
  $effect(() => {
    internalNodes = Array.from(sessionStore.items.values()).map(itemToNode);
  });

  function onmove(_event: MouseEvent | TouchEvent | null, viewport: Viewport): void {
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
    const cur = getViewport();
    const dx = Math.abs(cur.x - v.x);
    const dy = Math.abs(cur.y - v.y);
    const dz = Math.abs(cur.zoom - v.zoom);
    if (dx < 0.5 && dy < 0.5 && dz < 0.001) return;
    void setViewport({ x: v.x, y: v.y, zoom: v.zoom });
  });


  // 노드 클릭 → M 갱신. dual source.
  //   plain    : single (clear + add)
  //   meta/ctrl/shift : toggle in/out
  function onnodeclick({ node, event }: { node: Node; event: MouseEvent | TouchEvent }) {
    // Hand tool — exploration mode, clicks do not mutate selection (Figma).
    if (isHandTool) return;
    const id = node.id;
    const isModifierClick =
      event instanceof MouseEvent &&
      (event.metaKey || event.ctrlKey || event.shiftKey);
    if (isModifierClick) {
      sessionStore.toggleM(id);
    } else {
      sessionStore.setM([id]);
    }
  }

  function onpaneclick({ event }: { event: MouseEvent | TouchEvent }) {
    // Hand tool — exploration only, click no-op (Figma).
    if (isHandTool) return;
    // ── Tool-driven creation ───────────────────────────────────────────
    //
    // 점-spawn 도구 (text/note/file_path/terminal) 가 active 인 동안 빈 캔버스
    // 클릭은 새 item 을 그 위치에 생성. drag-spawn 도구 (rect/ellipse/line) 는
    // 별 pointer handler 가 처리 — onpaneclick 은 *down/up 이 같은 점* 인
    // 경우만 fire. 'select' 는 빈 영역 click 시 M clear (default).
    if (event instanceof MouseEvent) {
      const tool = toolStore.current;
      const flow = screenToFlowPosition({ x: event.clientX, y: event.clientY });

      if (tool === 'terminal') {
        void spawnMultiSessionTerminal(flow);
        return;
      }
      if (tool === 'text' || tool === 'note' || tool === 'file_path') {
        const item = createCanvasItem(tool, { x: flow.x, y: flow.y });
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
        return;
      }
    }
    sessionStore.clearM();
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
    const guard = await sessionStore.guardOutgoingMutation();
    if (!guard.ok) {
      toastStore.show({
        message: 'Session reconnect failed — terminal spawn aborted. Use Switch session…',
        tone: 'error',
      });
      return;
    }
    const name = active.name;
    const fresh = createTerminalItem(coords);
    try {
      // 1+2) Append + commit
      const { layout } = await mutateLayout(name, (cur) => {
        const maxZ = cur.items.reduce((m, it) => (it.z > m ? it.z : m), 0);
        return {
          ...cur,
          items: [...cur.items, { ...fresh, z: maxZ + 1 }],
        };
      });
      sessionStore.loadLayout(layout);
      sessionStore.setM([fresh.id]);
      // 3) Spawn the unmatched UUID
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
      // 4) Pool refresh — 0x88 이 즉시 paneId 바인딩, 본 호출은 attach_count 등의
      //    metadata 도 즉시 새로고침해 sidebar 가 정합되도록.
      void terminalPool.refresh();
      toolStore.consume();
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Terminal create failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  function onnodedragstop({ targetNode }: { targetNode: Node | null }) {
    if (!targetNode) return;
    const { id, position } = targetNode;
    const active = sessionStore.active;
    if (active === null) return;
    const sessionName = active.name;
    const item = sessionStore.items.get(id);
    if (item === undefined) return;
    let moved: CanvasItem;
    if (item.type === 'line') {
      // Line 은 (x,y), (x2,y2) 가 절대 좌표 → SvelteFlow Node.position 의
      // 새 값 - 옛 bounding-box-TL = delta 만큼 두 endpoint 모두 이동.
      const oldBox = lineBoxFromEndpoints(
        { x: item.x, y: item.y },
        { x: item.x2, y: item.y2 },
      );
      const dx = position.x - oldBox.x;
      const dy = position.y - oldBox.y;
      const nextP1 = { x: item.x + dx, y: item.y + dy };
      const nextP2 = { x: item.x2 + dx, y: item.y2 + dy };
      const nextBox = lineBoxFromEndpoints(nextP1, nextP2);
      moved = {
        ...item,
        x: nextP1.x,
        y: nextP1.y,
        x2: nextP2.x,
        y2: nextP2.y,
        w: nextBox.w,
        h: nextBox.h,
      };
    } else {
      moved = { ...item, x: position.x, y: position.y };
    }
    sessionStore.items.set(id, moved);
    void mutateLayout(sessionName, (cur) => ({
      ...cur,
      items: cur.items.map((it) => (it.id === id ? moved : it)),
    }))
      .then(({ layout }) => sessionStore.loadLayout(layout))
      .catch((e) => console.warn('[gtmux] drag commit failed:', e));
  }

  // Right-click handlers — pane area + node. Both prevent the native
  // browser context menu so our styled one wins. paneId / panelId
  // surface for the menu so item actions (Copy / Close) know what
  // they're acting on.
  function onpanecontextmenu({ event }: { event: MouseEvent | TouchEvent }) {
    if (!(event instanceof MouseEvent)) return;
    event.preventDefault();
    onContextMenuRequest?.({
      clientX: event.clientX,
      clientY: event.clientY,
      paneId: null,
      panelId: null,
    });
  }

  function onnodecontextmenu({ event, node }: { event: MouseEvent | TouchEvent; node: Node }) {
    if (!(event instanceof MouseEvent)) return;
    event.preventDefault();
    const data = node.data as Record<string, unknown> | undefined;
    const paneId = typeof data?.['pane_id'] === 'string' ? (data['pane_id'] as string) : null;
    onContextMenuRequest?.({
      clientX: event.clientX,
      clientY: event.clientY,
      paneId,
      panelId: node.id,
    });
  }
</script>

<!-- capture-phase pointer handlers: SvelteFlow 의 selection box 보다 먼저 받음.
     onpointerdowncapture / onpointermovecapture / onpointerupcapture 는 Svelte 의
     이벤트 capture 변형 — 본 요소가 root container 라 SvelteFlow 자식 요소 모두
     를 cover. drag tool 비활성 시 down 핸들러가 즉시 early-return 하므로 일반
     이벤트는 SvelteFlow 가 정상 처리. -->
<div
  class="canvas-root"
  class:drag-cursor={isDragTool && !isSpacePressed && !isHandTool}
  class:pan-cursor={isSpacePressed || isHandTool}
  onpointerdowncapture={onCanvasPointerDown}
  onpointermovecapture={onCanvasPointerMove}
  onpointerupcapture={onCanvasPointerUp}
  onpointercancelcapture={onCanvasPointerCancel}
>
  <!-- NewPanelButton overlay 제거 — 기능은 Toolbar2 의 terminal 도구로 마이그레이션.
       legacy 진입은 `handleTerminalClick(legacy branch)` 가 `requestLegacyNewPane`
       호출. multi-session 은 BE Stage 5-D P2 endpoint 도착 시 wire. -->
  <SvelteFlow
    bind:nodes={internalNodes}
    edges={[]}
    {nodeTypes}
    {onnodeclick}
    {onpaneclick}
    {onnodedragstop}
    {onmove}
    {onpanecontextmenu}
    {onnodecontextmenu}
    panOnDrag={panOnDragMask}
    minZoom={0.05}
    maxZoom={3}
    fitView={false}
    elevateNodesOnSelect={true}
    onlyRenderVisibleElements={true}
    deleteKey={null}
    proOptions={{ hideAttribution: true }}
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
      {/if}
    </div>
  {/if}
</div>

<style>
  .canvas-root {
    width: 100%;
    height: 100%;
    min-height: 0;
    position: relative;
    background: var(--canvas-bg);
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

  .canvas-root :global(.svelte-flow__node),
  .canvas-root :global(.svelte-flow__node.selected),
  .canvas-root :global(.svelte-flow__node:focus),
  .canvas-root :global(.svelte-flow__node:focus-visible) {
    border: 0 !important;
    outline: none !important;
    box-shadow: none !important;
    background: transparent !important;
  }

  /* Drag-to-create tool cursor — Batch 2 (rect/ellipse/line). */
  .canvas-root.drag-cursor,
  .canvas-root.drag-cursor :global(.svelte-flow__pane) {
    cursor: crosshair;
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

  /* Live preview during drag — container-local screen coords. */
  .drag-ghost {
    position: absolute;
    box-sizing: border-box;
    border: 2px solid var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--color-accent) 18%, transparent);
    pointer-events: none;
    z-index: 100;
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
