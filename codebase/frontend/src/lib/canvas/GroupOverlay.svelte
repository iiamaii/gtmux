<script lang="ts">
  /**
   * GroupOverlay — Group entity 의 canvas 시각 (ADR-0010 D15).
   *
   * 정본:
   * - ADR-0010 D15 amend (BBox dotted outline, no rail/header chip)
   * - ADR-0010 D20 (selected vs hover mode 의 alpha 차이)
   * - plan-0012 §3.3 C.1
   * - ADR-0028 D12 (applyMutation priorSnapshot rollback)
   *
   * Rendered as a SvelteFlow custom node — position / size 는 Canvas 의 flowNodes
   * derived 에서 자손 union BBox + padding 으로 set. 본 컴포넌트는 *시각 + 상호작용*
   * (click / contextmenu / drag) 책임.
   *
   * Pointer events: overlay 는 pointer-events:none. group selection / context menu /
   * drag 는 underlying descendant node hit-test 와 Canvas 의 drill-level targeting 이
   * 담당한다. 이렇게 해야 bbox 가 자손 element 의 hover/edit input 을 가리지 않는다.
   */

  interface OverlayData {
    groupId: string;
    /**
     * - `'selected'` (alpha 1.0) — M.has(groupId).
     * - `'hover'` (alpha 0.4) — groupHover.id === groupId, M 외.
     * - `'outer-dim'` (alpha 0.3) — ADR-0010 D22.8 + design handover §8.2.1.
     *   drill-in 상태의 outer ancestor 표시 (M 의 element 의 ancestor group).
     *   Figma isolation mode 의 *테두리 dim* hint. Rail 미렌더.
     * - `'hitbox'` (alpha 0) — lasso selection proxy. Canvas current drill-level
     *   group selection must be native-selectable without showing extra chrome.
     */
    mode: 'selected' | 'hover' | 'outer-dim' | 'hitbox';
    /** inheritedColor() 결과 — null 이면 fallback (theme neutral accent). */
    color: string | null;
  }

  let { data }: { data: OverlayData } = $props();

  // SvelteFlow 의 NodeProps.data 가 unknown 이라 runtime narrow.
  const groupId = $derived(data.groupId);
  const mode = $derived(data.mode);
  // Fallback = ADR-0010 D18 (root level group 의 inherit chain 종료점).
  const color = $derived(data.color ?? 'var(--color-accent)');
</script>

<div class="group-overlay" data-mode={mode} data-group-id={groupId} style="--group-color: {color};"></div>

<style>
  /* Wrapper 가 100% — SvelteFlow node 의 width/height = padded BBox. */
  .group-overlay {
    position: relative;
    width: 100%;
    height: 100%;
    border: calc(2px / var(--canvas-zoom, 1)) dashed var(--group-color);
    border-radius: 2px;
    pointer-events: none;
    box-sizing: border-box;
  }

  /* Mode alpha:
   *   selected   = 진한 outline (ADR-0010 D20)
   *   hover      = 약한 outline (ADR-0010 D20)
   *   outer-dim  = 더 약한 outline (ADR-0010 D22.8 — drill-in 의 outer ancestor) */
  .group-overlay[data-mode='selected'] {
    opacity: 0.9;
  }
  .group-overlay[data-mode='hover'] {
    opacity: 0.45;
  }
  .group-overlay[data-mode='outer-dim'] {
    opacity: 0.3;
  }
  .group-overlay[data-mode='hitbox'] {
    opacity: 0;
  }

</style>
