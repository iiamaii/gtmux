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

  import { onMount, untrack } from 'svelte';
  import { SvelteFlow, Background, BackgroundVariant, useSvelteFlow } from '@xyflow/svelte';
  import type { Node, Viewport } from '@xyflow/svelte';
  import '@xyflow/svelte/dist/style.css';
  import { debugCount } from '$lib/common/debugCounts';
  import { ensureMutationOk, sessionStore } from '$lib/stores/sessionStore.svelte';
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
  import MaximizedPanelModal from './MaximizedPanelModal.svelte';
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
  let applyingStoreViewport = false;

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

  /**
   * Cursor hover preview — terminal 도구가 active 일 때 cursor 위치에 새
   * panel 의 *default 크기 (480×320 flow units)* 윤곽선을 미리보기. zoom 에
   * 따라 screen px 크기가 비례 (`size_screen = size_flow * zoom`).
   * cursor 가 .canvas-root 밖이거나 노드 위면 null 로 hide.
   */
  let hoverScreen = $state<{ x: number; y: number } | null>(null);

  const isPanelTool = $derived(toolStore.current === 'terminal');
  const terminalGhost = $derived.by(() => {
    if (!isPanelTool || hoverScreen === null) return null;
    return {
      x: hoverScreen.x,
      y: hoverScreen.y,
      w: 480 * sessionStore.viewport.zoom,
      h: 320 * sessionStore.viewport.zoom,
    };
  });

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
    // Always track hover screen position — terminal ghost preview 의 입력.
    const rootEl = e.currentTarget as HTMLElement;
    const rootRect = rootEl.getBoundingClientRect();
    hoverScreen = {
      x: e.clientX - rootRect.left,
      y: e.clientY - rootRect.top,
    };
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
  function makeSignature(
    item: CanvasItem,
    effVisible: boolean,
    effLocked: boolean,
    selected: boolean,
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
        payload = `|${item.text}|${item.font_size}|${item.color}|${item.text_align ?? ''}|${item.text_vertical_align ?? ''}`;
        break;
      case 'note':
        payload = `|${item.title ?? ''}|${item.body ?? ''}|${item.color ?? ''}`;
        break;
      case 'file_path':
        payload = `|${item.path}|${item.kind ?? ''}`;
        break;
      case 'rect':
      case 'ellipse':
        payload = `|${item.stroke}|${item.fill}|${item.stroke_width}`;
        break;
      case 'line':
        payload = `|${item.x2}|${item.y2}|${item.stroke}|${item.stroke_width}`;
        break;
      case 'free_draw':
        // P2 — placeholder until ship
        payload = '|free_draw';
        break;
      case 'image':
      case 'document':
        // P2 — placeholder until ship
        payload = `|${item.type}`;
        break;
    }
    return `${effVisible ? 1 : 0}|${effLocked ? 1 : 0}|${selected ? 1 : 0}|${mMulti ? 1 : 0}|${common}${payload}`;
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
      const selected = sessionStore.M.has(item.id);
      const sig = makeSignature(item, visible, locked, selected, mMulti);
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

  /* ── plan-0010 Task 1 — Focus / zoom-to-item ───────────────────────────
   * Layer 패널 의 focus 버튼 클릭 시 sessionStore.zoomToItem(id) →
   * pendingZoomToItemId set. 본 effect 가 watch — item BBox 를 viewport
   * 중앙 + 가득 채움 으로 setViewport. 처리 후 1-shot clear.
   *
   * BBox: item.x/y/w/h. line 은 (x,y)~(x2,y2) 의 BBox 사용. fit 의 padding
   * = 12% (Figma "Zoom to selection" 비율). zoom 은 viewport 가로/세로
   * 중 더 작은 비율 채택.
   */
  $effect(() => {
    const targetId = sessionStore.pendingZoomToItemId;
    if (targetId === null) return;
    untrack(() => {
      const item = sessionStore.items.get(targetId);
      if (item === undefined) {
        sessionStore.clearPendingZoom();
        return;
      }
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
      const root = (document.querySelector('.canvas-root') as HTMLElement) ?? null;
      const vw = root?.clientWidth ?? window.innerWidth;
      const vh = root?.clientHeight ?? window.innerHeight;
      const padding = 0.88;
      const zoom = Math.min((vw / bw) * padding, (vh / bh) * padding, 3);
      const zoomClamped = Math.max(0.05, zoom);
      const cx = bx + bw / 2;
      const cy = by + bh / 2;
      const next = {
        x: vw / 2 - cx * zoomClamped,
        y: vh / 2 - cy * zoomClamped,
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


  // 노드 클릭 → M 갱신. dual source.
  //   plain    : single (clear + add)
  //   meta/ctrl/shift : toggle in/out
  function onnodeclick({ node, event }: { node: Node; event: MouseEvent | TouchEvent }) {
    // Select mode 만 selection 허용 — hand / 도구 mode 는 click no-op (Figma).
    if (!isSelectMode) return;
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
  function onnodedragstop({
    nodes,
  }: { targetNode: Node | null; nodes: Node[] }) {
    if (nodes.length === 0) return;
    const active = sessionStore.active;
    if (active === null) return;
    const sessionName = active.name;

    // id → moved item map. 단일 drag 시 nodes.length === 1.
    const movedById = new Map<string, CanvasItem>();
    for (const n of nodes) {
      const cur = sessionStore.items.get(n.id);
      if (cur === undefined) continue;
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
      } else {
        next = { ...cur, x: pos.x, y: pos.y };
      }
      movedById.set(n.id, next);
    }
    if (movedById.size === 0) return;

    // Optimistic store update — bind:nodes 양방향 sync 의 idempotent 결과.
    for (const [id, next] of movedById) {
      sessionStore.items.set(id, next);
    }
    void (async () => {
      if (!(await ensureMutationOk('Drag commit aborted — session reconnect failed.'))) return;
      try {
        const { layout } = await mutateLayout(sessionName, (cur) => ({
          ...cur,
          items: cur.items.map((it) => movedById.get(it.id) ?? it),
        }));
        sessionStore.loadLayout(layout);
      } catch (e) {
        console.warn('[gtmux] drag commit failed:', e);
      }
    })();
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

  /**
   * Selection box (lasso) 변화 sync — Cmd/Ctrl click 의 toggle 과 동등 취급.
   * Layer panel 등 sessionStore.M 의 모든 consumer 가 자동 갱신.
   *
   * `selectionOnDrag={true}` 로 left-drag 이 selection box. 사용자가 한 번
   * 드래그하면 SvelteFlow internal 이 영역 안 node 들의 `selected` 를 set 후
   * 본 callback fire — 우리는 그 결과를 store M 으로 sync.
   *
   * 단일 click (onnodeclick) 흐름과 충돌 우려: onnodeclick 이 먼저 fire 후
   * onselectionchange 가 *같은 단일 id* set — 동일 결과 (no-op). modifier
   * click 의 toggleM 도 그 후 store 와 internal 이 동기적.
   */
  function onselectionchange({ nodes }: { nodes: Node[]; edges: unknown[] }) {
    // Select mode 외에는 selection sync 안 함 — elementsSelectable={false} 가
    // SvelteFlow internal 의 selection 자체를 막지만 defensive guard 유지.
    if (!isSelectMode) return;
    const ids = nodes.map((n) => n.id);
    // Fast no-op — 동일 set 이면 skip (callback frequency 높음, drag 중 매 frame).
    if (ids.length === sessionStore.M.size) {
      let same = true;
      for (const id of ids) {
        if (!sessionStore.M.has(id)) {
          same = false;
          break;
        }
      }
      if (same) return;
    }
    sessionStore.setM(ids);
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
  role="presentation"
  class:drag-cursor={isDragTool && !isSpacePressed && !isHandTool}
  class:pan-cursor={isSpacePressed || isHandTool}
  onpointerdowncapture={onCanvasPointerDown}
  onpointermovecapture={onCanvasPointerMove}
  onpointerupcapture={onCanvasPointerUp}
  onpointercancelcapture={onCanvasPointerCancel}
  onpointerleave={() => (hoverScreen = null)}
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
    {onnodedragstop}
    {onmove}
    {onpanecontextmenu}
    {onnodecontextmenu}
    {onselectionchange}
    panOnDrag={isMaximizedActive ? [] : panOnDragMask}
    panOnScroll={!isMaximizedActive}
    zoomOnScroll={!isMaximizedActive}
    zoomOnPinch={!isMaximizedActive}
    zoomOnDoubleClick={!isMaximizedActive}
    selectionOnDrag={isSelectMode && !isSpacePressed && !isMaximizedActive}
    nodesDraggable={isSelectMode && !isMaximizedActive}
    elementsSelectable={isSelectMode}
    minZoom={0.05}
    maxZoom={3}
    fitView={false}
    elevateNodesOnSelect={true}
    onlyRenderVisibleElements={true}
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

  {#if terminalGhost !== null && dragState === null}
    <div
      class="terminal-ghost"
      style="left: {terminalGhost.x}px; top: {terminalGhost.y}px; width: {terminalGhost.w}px; height: {terminalGhost.h}px;"
      aria-hidden="true"
    ></div>
  {/if}

  {#if sessionStore.maximizedItemId !== null}
    <MaximizedPanelModal itemId={sessionStore.maximizedItemId} />
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

  /* ref/frontend-design/components.html §05 — Shared rules B/C:
   *   - Selection 시각은 wrapper (.svelte-flow__node) 가 책임. 각 shape 컴포넌트
   *     는 자체 outline 갖지 않음 (모두 outline: none) — 단일 source.
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
    box-shadow: 0 0 0 1px var(--color-border-strong);
  }

  .canvas-root :global(.svelte-flow__node.selected),
  .canvas-root :global(.svelte-flow__node:focus),
  .canvas-root :global(.svelte-flow__node:focus-visible) {
    box-shadow: 0 0 0 1.5px var(--color-accent);
  }

  /* line 같은 *대각선 line-art* 의 bounding-box 는 ring 으로 표시하면 회귀
   * (사각형 outline 으로 보임). SvelteFlow 가 type 별 class (`svelte-flow__
   * node-line`) 자동 부여 → 본 selector 로 ring 일괄 비활성. 자체 selection
   * 시각은 LineNode 의 endpoint button 이 담당. */
  .canvas-root :global(.svelte-flow__node-line),
  .canvas-root :global(.svelte-flow__node-line:hover),
  .canvas-root :global(.svelte-flow__node-line.selected),
  .canvas-root :global(.svelte-flow__node-line:focus),
  .canvas-root :global(.svelte-flow__node-line:focus-visible) {
    box-shadow: none;
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

  /* Terminal tool — cursor hover preview of the panel-to-be-spawned.
     Dashed accent outline, no fill — purely guide, no interactivity. */
  .terminal-ghost {
    position: absolute;
    box-sizing: border-box;
    border: 1.5px dashed var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 6%, transparent);
    pointer-events: none;
    z-index: 99;
    border-radius: var(--radius-sm);
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
