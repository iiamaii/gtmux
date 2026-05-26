// DocumentViewModeStore — per-itemId Document viewer mode (rendered/source).
//
// 정본:
// - ADR-0018 D10 amend ⑥ (2026-05-22) — viewMode persist store 도입.
// - ADR-0037 D7 close (2026-05-22) — normal↔maximize 전환 시 reset 회귀 해소.
//
// 동기:
// - DocumentNode (normal) + MaximizedItemModal (maximize) 가 *같은* document 의
//   viewMode 를 각자 component-local `$state` 로 갖고 있었음 → normal 에서
//   source 로 토글 후 maximize 진입 시 modal 의 viewMode 가 default 'rendered'
//   라 *reset* 된 것처럼 보임. 같은 itemId 의 viewMode 는 단일.
// - 사용자가 모드 토글 후 unmount/remount (panel scroll, virtualization, item
//   swap 등) 시에도 reset 회피.
//
// 정책:
// - default 'rendered' 는 *storage 안 함* — Map 의 absent entry 가 'rendered'
//   을 의미. memory 절약 + Map size 가 *non-default* item 수와 일치.
// - Session-local ephemeral — item delete 시 cleanup 은 caller 책임 (optional —
//   dead entry 가 남아도 다른 id 와 충돌 없음, 다음 같은 id 안 쓰면 stale).

import { SvelteMap } from 'svelte/reactivity';

import type { DocumentViewMode } from '$lib/canvas/documentRender';

class DocumentViewModeStore {
  /** itemId → non-default viewMode. default 'rendered' 는 entry 부재로 표현. */
  byId = $state<SvelteMap<string, DocumentViewMode>>(new SvelteMap());

  /** itemId 의 viewMode (없으면 default 'rendered'). */
  get(itemId: string): DocumentViewMode {
    return this.byId.get(itemId) ?? 'rendered';
  }

  /**
   * itemId 의 viewMode 설정. 'rendered' 는 entry 제거 (default — memory 절약).
   * SvelteMap 의 mutation 이 reactive 라 subscriber (양 컴포넌트) 자동 갱신.
   */
  set(itemId: string, mode: DocumentViewMode): void {
    if (mode === 'rendered') {
      this.byId.delete(itemId);
    } else {
      this.byId.set(itemId, mode);
    }
  }

  /** Item delete 시 cleanup. caller (sessionStore.applyDeletion 등) 책임. */
  clear(itemId: string): void {
    this.byId.delete(itemId);
  }
}

export const documentViewModeStore = new DocumentViewModeStore();
