// PanelsStore — Panel 엔티티 (web-only state).
// R8 §F3 store 구조: 분할 class store + SvelteMap entry-level reactivity.
import { SvelteMap } from 'svelte/reactivity';

export interface Panel {
  id: string;
  // 실제 필드는 codegen 산출 CanvasLayout.Panel 사용. 본 스켈레톤은 placeholder.
  // 잠정적으로 잘 사용되는 필드만 명시: x/y/w/h/z + 기타.
  [key: string]: unknown;
}

class PanelsStore {
  panels = $state(new SvelteMap<string, Panel>());

  /**
   * Commit a Panel position delta into the store. The Canvas drag handler
   * calls this on `onnodedragstop` so the store is the source of truth for
   * the next derived re-render (otherwise SvelteFlow's controlled `nodes`
   * prop snaps back to the pre-drag value on the next selection event).
   */
  movePanel(id: string, x: number, y: number): void {
    const current = this.panels.get(id);
    if (!current) return;
    this.panels.set(id, { ...current, x, y });
  }

  /**
   * Commit a Panel resize delta into the store. NodeResizer's `onResizeEnd`
   * calls this; same controlled-snap rationale as `movePanel`. Position is
   * passed alongside size because NodeResizer's top-/left-anchored handles
   * shift the origin.
   */
  resizePanel(id: string, x: number, y: number, w: number, h: number): void {
    const current = this.panels.get(id);
    if (!current) return;
    this.panels.set(id, { ...current, x, y, w, h });
  }
}

export const panelsStore = new PanelsStore();
