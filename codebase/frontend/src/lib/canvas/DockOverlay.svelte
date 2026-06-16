<script lang="ts">
  /**
   * DockOverlay — edge-dock affordance (ADR-0051 D4).
   *
   * Rendered as a SvelteFlow custom node — Canvas 의 flowNodes derived 에서
   * activeDock 의 landing box (size-match + flush) 로 position / size 를 set 한다.
   * 본 컴포넌트는 비파괴 *미리보기* 만 책임: drop 전 실제 item 은 변경하지 않는다.
   *
   * 두 시각 요소:
   *   - ghost: landing box 전체의 dashed accent outline (착지 윤곽선).
   *   - side bar: target 과 맞닿는 변(`side`)을 강조하는 solid accent band.
   *
   * Pointer events: none — drag 중 underlying node 의 hit-test 를 가리지 않도록.
   * GroupOverlay 와 동일 패턴.
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
  /* Wrapper 가 100% — SvelteFlow node 의 width/height = landing box. */
  .dock-overlay {
    position: relative;
    width: 100%;
    height: 100%;
    pointer-events: none;
    box-sizing: border-box;
  }

  /* Ghost — landing 윤곽선. zoom 보정으로 일정 두께 유지 (GroupOverlay 정합). */
  .dock-ghost {
    position: absolute;
    inset: 0;
    border: calc(2px / var(--canvas-zoom, 1)) dashed var(--color-accent);
    border-radius: 2px;
    background: color-mix(in srgb, var(--color-accent) 8%, transparent);
    box-sizing: border-box;
  }

  /* Side band — target 과 맞닿는 변 강조. 두께는 zoom 보정. */
  .dock-side {
    position: absolute;
    background: var(--color-accent);
    --band: calc(3px / var(--canvas-zoom, 1));
  }
  .dock-side[data-side='L'] {
    left: 0;
    top: 0;
    bottom: 0;
    width: var(--band);
  }
  .dock-side[data-side='R'] {
    right: 0;
    top: 0;
    bottom: 0;
    width: var(--band);
  }
  .dock-side[data-side='T'] {
    top: 0;
    left: 0;
    right: 0;
    height: var(--band);
  }
  .dock-side[data-side='B'] {
    bottom: 0;
    left: 0;
    right: 0;
    height: var(--band);
  }
</style>
