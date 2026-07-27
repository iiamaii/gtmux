// documentScrollStore — per-itemId live document scroll anchor.
//
// 정본: ADR-0056 D1 (in-memory 실시간 공유 계층).
//
// documentViewMode.svelte.ts 와 같은 패턴: itemId 키 reactive SvelteMap 을
// DocumentNode (canvas) 와 MaximizedItemModal (maximize) 양쪽이 구독해 스크롤
// 위치를 공유한다. 스크롤 시 store 갱신, 반대편 표면이 mount/전환될 때 store
// 값으로 복원 → maximize 열기/닫기 전환에서 위치가 연속된다.
//
// 정책:
// - viewMode store 와 달리 default(문서 처음)는 entry 부재로 표현. 부재 =
//   "이 표면에 대한 live 위치 없음" → 복원 시 item.view_state.anchor 로 fallback.
// - restore-in-progress 가드: 프로그램적 복원 스크롤이 저장 트리거를 다시
//   치지 않도록(ADR-0056 D4 루프 가드) itemId 별 flag 를 둔다. 가드 Set 은
//   UI 반응 불요 → $state 아님 (비교 전용).
// - Session-local ephemeral. item delete 시 sessionStore.applyDeletion 이
//   삭제된 document id 마다 clear(id) 호출, session teardown(clear()) 은
//   clearAll() 로 전체 prune → dead entry 누수 없음.

import { SvelteMap } from 'svelte/reactivity';

import type { DocViewAnchor } from '$lib/canvas/documentAnchor';

class DocumentScrollStore {
  /** itemId → live scroll anchor. 부재 = live 위치 없음. */
  byId = $state<SvelteMap<string, DocViewAnchor>>(new SvelteMap());

  /** Restore-in-progress guard set (non-reactive — 저장 트리거 억제 전용). */
  #restoring = new Set<string>();

  /** itemId 의 live anchor (없으면 null). */
  get(itemId: string): DocViewAnchor | null {
    return this.byId.get(itemId) ?? null;
  }

  /** 스크롤 시 live anchor 갱신 (매 이벤트 — cheap). */
  set(itemId: string, anchor: DocViewAnchor): void {
    this.byId.set(itemId, anchor);
  }

  /** Item delete 시 cleanup. caller (sessionStore.applyDeletion 등) 책임. */
  clear(itemId: string): void {
    this.byId.delete(itemId);
  }

  /** Session teardown 시 전체 prune (sessionStore.clear 책임). */
  clearAll(): void {
    this.byId.clear();
    this.#restoring.clear();
  }

  /** 프로그램적 복원 스크롤 전후로 감싼다 — 그 사이의 scroll 이벤트는 저장 skip. */
  beginRestore(itemId: string): void {
    this.#restoring.add(itemId);
  }

  endRestore(itemId: string): void {
    this.#restoring.delete(itemId);
  }

  isRestoring(itemId: string): boolean {
    return this.#restoring.has(itemId);
  }
}

export const documentScrollStore = new DocumentScrollStore();
