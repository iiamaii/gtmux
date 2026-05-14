<script lang="ts">
  // Svelte Flow 캔버스 host — R3 보고서 채택, ADR-0012 D2/D5/D6.
  //
  // 책임:
  // - `panelsStore.panels` (SvelteMap<string, Panel>) → Svelte Flow `nodes` 매핑.
  //   `$derived`로 entry-level fine-grain reactivity (R8 §F3).
  // - viewport (`ephemeralStore.viewport`) bind — pan/zoom (D14 0x83 VIEWPORT_CHANGED
  //   broadcast는 dispatcher가 store 갱신 → 본 컴포넌트는 store 구독만).
  // - 노드 드래그 → 위치 갱신 (panelsStore.movePanel + PUT /api/layout).
  // - 노드 클릭 → M selection 갱신.
  //     * plain click       : M = [id] (single — Figma 컨벤션)
  //     * Cmd / Ctrl + click : M.toggle(id) (multi-select 추가/제거)
  //     * Shift + click      : M.toggle(id) 도 허용 (cross-platform 친화)
  // - 캔버스 dot grid 는 token-driven (--canvas-bg, --canvas-grid).
  // - panOnDrag = [1, 2] — middle/right 마우스 버튼만 pan (left는 selection/drag용).

  import { SvelteFlow, Background, BackgroundVariant } from '@xyflow/svelte';
  import type { Node, Viewport } from '@xyflow/svelte';
  import '@xyflow/svelte/dist/style.css';
  import { panelsStore } from '$lib/stores/panels.svelte';
  import { ephemeralStore } from '$lib/stores/ephemeral.svelte';
  import { putLayoutCommitCurrent } from '$lib/http/layout';
  import PanelNode from './PanelNode.svelte';
  import NewPanelButton from './NewPanelButton.svelte';

  const TOKEN_STORAGE_KEY = 'gtmux_token';
  function readToken(): string | null {
    try {
      return sessionStorage.getItem(TOKEN_STORAGE_KEY);
    } catch {
      return null;
    }
  }

  // Panel JSON shape — `docs/ssot/canvas-layout-schema.md` §1 `$defs/Panel`의 부분 view.
  // 코드젠된 `$lib/types/canvas-layout.d.ts`가 정본이 되기 전까지의 잠정 정의 — 정본
  // 산출 후 본 타입은 `import type { Panel } from '$lib/types/canvas-layout'`로 교체.
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
  }

  // Custom node type lookup table for Svelte Flow.
  // 'panel' = gtmux Panel custom node. 다른 type은 사용하지 않음 (single node type, MVP).
  const nodeTypes = { panel: PanelNode };

  // M cardinality — PanelNode 가 single/multi 분기를 위해 참조.
  const isMultiSelection = $derived(ephemeralStore.m.size > 1);

  // Panel store → SvelteFlow nodes 매핑.
  const nodes = $derived<Node[]>(
    Array.from(panelsStore.panels.values() as Iterable<PanelData>).map((p) => ({
      id: p.id,
      type: 'panel',
      position: { x: p.x ?? 0, y: p.y ?? 0 },
      data: {
        ...(p as unknown as Record<string, unknown>),
        // PanelNode 가 자기 자신만으로는 *전체 M 의 크기* 를 알 수 없으므로
        // Canvas 에서 미리 계산해 data 에 주입. `selected` 와 함께 사용.
        m_multi: isMultiSelection,
      },
      draggable: p.locked !== true,
      selected: ephemeralStore.m.has(p.id),
      zIndex: p.z ?? 0,
      width: p.w,
      height: p.h
    }))
  );

  function onmove(_event: MouseEvent | TouchEvent | null, viewport: Viewport): void {
    ephemeralStore.viewport = { x: viewport.x, y: viewport.y, zoom: viewport.zoom };
  }

  // 노드 클릭 → M 갱신.
  //   plain    : single (clear + add)
  //   meta/ctrl/shift : toggle in/out
  function onnodeclick({ node, event }: { node: Node; event: MouseEvent | TouchEvent }) {
    const id = node.id;
    const isModifierClick =
      event instanceof MouseEvent &&
      (event.metaKey || event.ctrlKey || event.shiftKey);
    if (isModifierClick) {
      if (ephemeralStore.m.has(id)) {
        ephemeralStore.m.delete(id);
      } else {
        ephemeralStore.m.add(id);
      }
    } else {
      ephemeralStore.m.clear();
      ephemeralStore.m.add(id);
    }
  }

  function onpaneclick() {
    if (ephemeralStore.m.size > 0) {
      ephemeralStore.m.clear();
    }
  }

  function onnodedragstop({ targetNode }: { targetNode: Node | null }) {
    if (!targetNode) return;
    const { id, position } = targetNode;
    panelsStore.movePanel(id, position.x, position.y);
    const token = readToken();
    if (token === null) {
      console.warn('[gtmux] drag commit skipped: no auth token');
      return;
    }
    void putLayoutCommitCurrent(token).catch((e) => {
      console.warn('[gtmux] drag commit failed:', e);
    });
  }
</script>

<div class="canvas-root">
  <div class="canvas-toolbar">
    <NewPanelButton />
  </div>
  <SvelteFlow
    {nodes}
    edges={[]}
    {nodeTypes}
    {onnodeclick}
    {onpaneclick}
    {onnodedragstop}
    {onmove}
    panOnDrag={[1, 2]}
    minZoom={0.05}
    maxZoom={3}
    fitView={false}
    elevateNodesOnSelect={true}
    onlyRenderVisibleElements={true}
    proOptions={{ hideAttribution: true }}
  >
    <Background
      variant={BackgroundVariant.Dots}
      gap={24}
      size={1}
      bgColor="var(--canvas-bg)"
      patternColor="var(--canvas-grid)"
    />
  </SvelteFlow>
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
  }

  /* 캔버스 좌상단에 overlay. */
  .canvas-toolbar {
    position: absolute;
    top: var(--space-8);
    left: var(--space-8);
    z-index: 5;
    display: flex;
    align-items: center;
    gap: var(--space-8);
    pointer-events: none;
  }

  .canvas-toolbar :global(.new-panel-btn),
  .canvas-toolbar :global(.new-panel-error) {
    pointer-events: auto;
  }
</style>
