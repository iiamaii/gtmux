<script lang="ts">
  // Svelte Flow custom node — Panel + placeholder 분기 (R8 §F8 zoom-blur 정책 (b)).
  //
  // 책임:
  // - `data` (NodeProps의 data prop) = PanelData (canvas-layout-schema §1 Panel JSON).
  // - 헤더 바 = drag handle. label 표시 + M/lock badge.
  // - 본문 = zoom-blur 분기:
  //     * `|zoom - 1| < 0.02` (R8 §F8 ε) → XtermHost mount
  //     * 그 외 → PanelPlaceholder (xterm DOM 비가시, 데이터 흐름은 unmount로 정지 —
  //       D16 Suspended와 *별개 차원*. zoom 복귀 시 재 mount에서 ring buffer replay
  //       (D15)가 catch-up — 본 R8-O3 측정 대상).
  // - visibility=false → 렌더하지 않음 (Svelte Flow Node.hidden=true도 검토할 수 있으나
  //   본 노드 수준 분기로 단순화).
  // - D6 manipulation/input은 캔버스 측에서 store에 반영. PanelNode는 store 구독만.

  import XtermHost from './XtermHost.svelte';
  import PanelPlaceholder from './PanelPlaceholder.svelte';
  import { ephemeralStore } from '$lib/stores/ephemeral.svelte';

  // Panel JSON shape — Canvas.svelte 와 동일한 잠정 정의 (코드젠 정본 도착 전까지).
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

  // SvelteFlow NodeProps — `data` 는 generic이라 `Record<string, unknown>`으로 들어옴.
  // 본 컴포넌트는 그 안에 PanelData가 들어 있음을 *Canvas.svelte 가 보장*한 입력 계약으로
  // 가정 (`type: 'panel'` 로 매핑된 노드만 본 컴포넌트로 라우팅).
  // SvelteFlow가 전달하는 다른 NodeProps 필드(id, selected, dragging 등)는 본 컴포넌트
  // 입장에서 옵셔널로 받는다.
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

  // R8 §F8 정본: ε = 0.02. `|zoom - 1| < ε` 일 때 xterm DOM 가시.
  // ε 도입 근거: 외부에서 정확히 1.0이 도착하지 않는 부동소수점 조립 (Svelte Flow 내부
  // zoom 보정 + 사용자 wheel step) 보호.
  const ZOOM_UNIT_EPS = 0.02;
  const isAtUnitZoom = $derived(
    Math.abs(ephemeralStore.viewport.zoom - 1) < ZOOM_UNIT_EPS
  );

  // schema 정합: visibility=false면 렌더 X. 단 SvelteFlow는 이미 nodes 배열을 필터링하지
  // 않으므로 본 컴포넌트가 무화면 분기를 직접 처리 (Node.hidden 대신).
  const isVisible = $derived(data.visibility !== false);

  // Streaming State (D16): visibility=hidden 또는 minimized=true → Suspended.
  // xterm 인스턴스를 *마운트조차 하지 않음* — 데이터 흐름은 server 측이 pause로 막음
  // (별도 트랙). 본 컴포넌트는 *렌더만* 차단.
  const isStreaming = $derived(isVisible && data.minimized !== true);

  // 헤더 라벨: label > pane_id > id 폴백.
  const headerLabel = $derived(data.label ?? data.pane_id ?? data.id);

  // M selection 표시 — `selected` (Svelte Flow가 ephemeralStore.m 기반으로 전달, Canvas
  // 측에서 매핑) 이 진실. 단 본 컴포넌트는 추가 검증으로 store 직접 구독도 한다 (M 직접
  // 갱신 시 SvelteFlow 노드 selected가 즉시 반영되지 않을 수 있음).
  const isInM = $derived(selected || ephemeralStore.m.has(data.id));

  // I (Input Target) — 단일. pane_id 매칭. D6 직교.
  const isInI = $derived(
    typeof data.pane_id === 'string' && ephemeralStore.i === data.pane_id
  );

  // panel width/height — schema의 w/h 우선, 미지정 시 디폴트 (R8 F1 60-col × 24-row 추정에
  // 맞춰 480×320). pane_id 미지정 (불완전 hydration) 경우에도 안전.
  const panelW = $derived(data.w ?? 480);
  const panelH = $derived(data.h ?? 320);
</script>

{#if isVisible}
  <div
    class="panel"
    class:m-active={isInM}
    class:i-active={isInI}
    class:locked={data.locked === true}
    style="width: {panelW}px; height: {panelH}px;"
    role="group"
    aria-label={`Panel ${headerLabel}`}
  >
    <header class="panel-header" aria-label={`Drag handle for ${headerLabel}`}>
      <span class="panel-label">{headerLabel}</span>
      <span class="panel-badges">
        {#if data.locked === true}
          <span class="badge badge-lock" aria-label="Locked">L</span>
        {/if}
        {#if data.minimized === true}
          <span class="badge badge-min" aria-label="Minimized">M</span>
        {/if}
        {#if isInI}
          <span class="badge badge-input" aria-label="Input target">I</span>
        {/if}
      </span>
    </header>
    <div class="panel-body">
      {#if isStreaming && isAtUnitZoom && typeof data.pane_id === 'string'}
        <!-- SSoT pane_id is `%N` (string). XtermHost / dispatcher's
             registerPaneOut key both use the integer part as a decimal
             string ("N"), so we strip the leading `%` here at the
             single source of truth. -->
        <XtermHost paneId={data.pane_id.replace(/^%/, '')} />
      {:else}
        <PanelPlaceholder label={headerLabel} reason={isStreaming ? 'zoom' : 'suspended'} />
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

  .panel.m-active {
    outline: 1.5px solid var(--color-accent);
    outline-offset: 0;
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
  }

  .panel-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-md);
  }

  .panel-badges {
    display: inline-flex;
    gap: var(--space-4);
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
</style>
