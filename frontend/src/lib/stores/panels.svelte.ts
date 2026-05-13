// PanelsStore — Panel 엔티티 (web-only state).
// R8 §F3 store 구조: 분할 class store + SvelteMap entry-level reactivity.
import { SvelteMap } from 'svelte/reactivity';

export interface Panel {
  id: string;
  // 실제 필드는 codegen 산출 CanvasLayout.Panel 사용. 본 스켈레톤은 placeholder.
}

class PanelsStore {
  panels = $state(new SvelteMap<string, Panel>());
}

export const panelsStore = new PanelsStore();
