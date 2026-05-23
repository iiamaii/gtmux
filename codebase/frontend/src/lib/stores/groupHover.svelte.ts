// groupHover — sidebar/canvas 의 양방향 hover signal (ADR-0010 D20).
//
// 정본:
// - ADR-0010 D20 (sidebar group row + canvas color rail 의 hover → BBox preview)
// - plan-0012 §3.4 C.2
//
// 동작:
// - Sidebar group row 의 mouseenter → `set(id)`, mouseleave → `clear()`.
// - Canvas GroupOverlay 의 rail hover → 동일 store 갱신.
// - 두 side 가 같은 store 의 `id` 를 watch — 양방향 시각 동기.
//
// 단일 hover (한 시점에 한 group 만 hover) — 다중 hover 없음.

class GroupHoverStore {
  id = $state<string | null>(null);

  set(id: string): void {
    this.id = id;
  }

  clear(): void {
    this.id = null;
  }

  /** id 가 현 hover 와 일치하는 경우만 clear — race 방어. */
  clearIf(id: string): void {
    if (this.id === id) this.id = null;
  }
}

export const groupHover = new GroupHoverStore();
