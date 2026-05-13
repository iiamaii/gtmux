// GroupsStore — Group 엔티티 (G-hybrid, ADR-0010).
import { SvelteMap } from 'svelte/reactivity';

export interface Group {
  id: string;
  // 실제 필드는 codegen 산출 CanvasLayout.Group 사용.
}

class GroupsStore {
  groups = $state(new SvelteMap<string, Group>());
}

export const groupsStore = new GroupsStore();
