<script lang="ts">
  // Svelte Flow 캔버스 host — R3 보고서 채택, ADR-0012 D2/D5/D6.
  //
  // 책임:
  // - `panelsStore.panels` (SvelteMap<string, Panel>) → Svelte Flow `nodes` 매핑.
  //   `$derived`로 entry-level fine-grain reactivity (R8 §F3).
  // - viewport (`ephemeralStore.viewport`) bind — pan/zoom (D14 0x83 VIEWPORT_CHANGED
  //   broadcast는 dispatcher가 store 갱신 → 본 컴포넌트는 store 구독만).
  // - 노드 드래그 → 위치 갱신 (현재 store는 read-only placeholder이므로 onnodedragstop의
  //   target node position만 console.debug에 기록 — store mutation API 미배선까지의
  //   임시 동작. 실제 D11 G-hybrid drag-delta 처리는 panels.svelte.ts에 movePanel
  //   action이 노출되는 시점에 본 핸들러에서 연동).
  // - 노드 클릭 → M selection 토글 (D6 manipulation, D23 elevate z-on-select).
  //   `ephemeralStore.m` 직접 mutation은 SvelteSet add/delete 사용.
  // - zoom-blur 처리 (ADR-0012 O1, R8 §F8 정책 (b)): zoom != 1 (|zoom - 1| >= 0.02) 시
  //   PanelNode 내부 xterm을 placeholder로 교체. 본 Canvas는 store 갱신만 담당하고
  //   placeholder 분기는 PanelNode 내부에서 isAtUnitZoom 으로 판단.
  // - panOnDrag = [1, 2] — middle/right 마우스 버튼만 pan (left는 selection/drag용).

  import { SvelteFlow, Background } from '@xyflow/svelte';
  import type { Node, Viewport } from '@xyflow/svelte';
  import '@xyflow/svelte/dist/style.css';
  import { panelsStore } from '$lib/stores/panels.svelte';
  import { ephemeralStore } from '$lib/stores/ephemeral.svelte';
  import PanelNode from './PanelNode.svelte';

  // Panel JSON shape — `docs/ssot/canvas-layout-schema.md` §1 `$defs/Panel`의 부분 view.
  // 코드젠된 `$lib/types/canvas-layout.d.ts`가 정본이 되기 전까지의 잠정 정의 — 정본
  // 산출 후 본 타입은 `import type { Panel } from '$lib/types/canvas-layout'`로 교체.
  // 본 모듈만의 사용이므로 inline 유지 (4개 canvas 컴포넌트 각각 동일한 잠정 정의).
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

  // Panel store → SvelteFlow nodes 매핑.
  // SvelteMap entry-level fine-grain (R8 §F3): 단일 entry 변경 시 영향받는 derived만
  // 재실행. 본 derived는 *전체 array 재생성*이라 50 Panel 전체가 한 번에 다시
  // 만들어질 수 있다 — 위반 시 (R8-O1) keyed each 패턴으로 전환.
  const nodes = $derived<Node[]>(
    Array.from(panelsStore.panels.values() as Iterable<PanelData>).map((p) => ({
      id: p.id,
      type: 'panel',
      position: { x: p.x ?? 0, y: p.y ?? 0 },
      data: p as unknown as Record<string, unknown>,
      // Panel.locked = self만 — effective lock은 ancestor OR 누적이지만 본 노드 수준에서는
      // self만 draggable 차단. Group 트리 OR cascade는 PanelNode 안에서 effective 계산.
      draggable: p.locked !== true,
      // D23: M selection에 들어가면 elevateNodesOnSelect=true가 z-index 들어올림.
      selected: ephemeralStore.m.has(p.id),
      // schema의 z 필드는 ZIndexMode prop와 별개로 Node-level zIndex로 매핑.
      zIndex: p.z ?? 0,
      width: p.w,
      height: p.h
    }))
  );

  // viewport는 ephemeral broadcast (0x83) 대상이지만 본 task 범위(FE-2)는 캔버스 mount까지.
  // WS 송신은 dispatcher 트랙. 본 컴포넌트는 viewport 변경 시 store에 commit만 한다
  // (PanelNode의 `isAtUnitZoom` derived가 placeholder 토글을 위해 매 frame 봐야 하므로
  // debounce 없이 즉시 갱신).
  function onmove(_event: MouseEvent | TouchEvent | null, viewport: Viewport): void {
    ephemeralStore.viewport = { x: viewport.x, y: viewport.y, zoom: viewport.zoom };
  }

  // 노드 클릭 → M selection 토글. 단일 클릭은 *추가/제거* (Set semantics).
  // 빈 캔버스 클릭 시 M 비우는 동작은 `onpaneclick`으로 별도 처리.
  function onnodeclick({ node }: { node: Node }) {
    const id = node.id;
    if (ephemeralStore.m.has(id)) {
      ephemeralStore.m.delete(id);
    } else {
      ephemeralStore.m.add(id);
    }
  }

  // 캔버스 빈 영역 클릭 → M 클리어 (Figma 컨벤션).
  function onpaneclick() {
    if (ephemeralStore.m.size > 0) {
      ephemeralStore.m.clear();
    }
  }

  // 노드 드래그 종료 → 위치 commit. 본 task 범위에서는 store mutation API가 placeholder
  // 단계라 console.debug만. 실제 D11 G-hybrid drag-delta는 panels.svelte.ts의 movePanel
  // 액션이 노출되는 시점에 본 핸들러에서 호출.
  function onnodedragstop({ targetNode }: { targetNode: Node | null }) {
    if (!targetNode) return;
    console.debug('node drag stop', targetNode.id, targetNode.position);
  }
</script>

<div class="canvas-root">
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
    <Background />
  </SvelteFlow>
</div>

<style>
  .canvas-root {
    width: 100%;
    height: 100%;
    min-height: 0;
  }
</style>
