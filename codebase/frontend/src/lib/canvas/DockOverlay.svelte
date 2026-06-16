<script lang="ts">
  /**
   * DockOverlay — edge-dock affordance (ADR-0051 D4, amend ②).
   *
   * Rendered as a SvelteFlow custom node — Canvas 의 flowNodes derived 에서
   * activeDock 의 landing box (size-match + flush) 로 position / size 를 set 한다.
   * 본 컴포넌트는 비파괴 *미리보기* 만 책임: drop 전 실제 item 은 변경하지 않는다.
   *
   * 비주얼 사양 (amend ②): 신규-item 배치 가이드 `.point-spawn-ghost`(Canvas.svelte)
   * 와 동일하게 보이도록 맞춘다 —
   *   - ghost: 4 변 모두 `1px dashed accent` + `accent 6%` 채움 + `radius-sm`.
   *     SvelteFlow node 라 canvas zoom 으로 확대되므로 border 두께를 `1px / zoom`
   *     으로 보정해 on-screen 1px 를 유지 (point-spawn-ghost 와 동일 시각).
   *   - interfacing side (`data.side`): target 과 맞닿는 한 변만 **실선(solid
   *     accent)**. 같은 on-screen 두께(`1px / zoom`)의 가는 선을 그 변 위에 덧대어
   *     해당 변의 dashed 를 덮는다.
   *   - overlay 전체: 약간 투명(opacity) + 최상단 z (z 는 Canvas 의 SvelteFlow
   *     zIndex = OVERLAY_Z + 3 가 실제 제어 — 본 CSS 는 pointer-events 무력화만).
   *
   * Pointer events: none — drag 중 underlying node 의 hit-test 를 가리지 않도록.
   * GroupOverlay 와 동일 패턴. (--color-accent / --radius-sm / --canvas-zoom 는
   * canvas root 에서 상속됨 — point-spawn-ghost 와 동일 토큰.)
   */

  import type { DockSide } from './edgeDock';

  interface OverlayData {
    /** Which side of the ghost touches the target (the flush edge). */
    side: DockSide;
  }

  let { data }: { data: OverlayData } = $props();
  const side = $derived(data.side);
</script>

<div class="dock-overlay" data-side={side}>
  <div class="dock-ghost"></div>
  <div class="dock-side" data-side={side}></div>
</div>

<style>
  /* Wrapper 가 100% — SvelteFlow node 의 width/height = landing box.
     약간 투명 + 최상단 z (실제 z 제어는 Canvas 의 SvelteFlow zIndex). */
  .dock-overlay {
    position: relative;
    width: 100%;
    height: 100%;
    pointer-events: none;
    box-sizing: border-box;
    opacity: 0.85;
  }

  /* Ghost — landing 윤곽선. `.point-spawn-ghost` 와 동일 스타일: dashed accent
     border + accent 6% 채움 + radius-sm. node 가 zoom 으로 확대되므로 border
     두께를 1px/zoom 으로 보정해 on-screen 1px 유지 (point-spawn-ghost 정합). */
  .dock-ghost {
    position: absolute;
    inset: 0;
    border: calc(1px / var(--canvas-zoom, 1)) dashed var(--color-accent);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--color-accent) 6%, transparent);
    box-sizing: border-box;
  }

  /* Interfacing side — target 과 맞닿는 변만 실선(solid accent). dashed border
     와 동일한 on-screen 두께(1px/zoom)로 그 변 위에 덧대 dashed 를 덮는다. */
  .dock-side {
    position: absolute;
    background: var(--color-accent);
    --line: calc(1px / var(--canvas-zoom, 1));
  }
  .dock-side[data-side='L'] {
    left: 0;
    top: 0;
    bottom: 0;
    width: var(--line);
  }
  .dock-side[data-side='R'] {
    right: 0;
    top: 0;
    bottom: 0;
    width: var(--line);
  }
  .dock-side[data-side='T'] {
    top: 0;
    left: 0;
    right: 0;
    height: var(--line);
  }
  .dock-side[data-side='B'] {
    bottom: 0;
    left: 0;
    right: 0;
    height: var(--line);
  }
</style>
