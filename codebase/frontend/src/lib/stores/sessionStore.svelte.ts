// SessionStore — session-scoped layout / viewport / M / I / maximize state.
//
// 정본:
// - ADR-0019 D5 (session-scoped store: 모든 layout/viewport/M/I 는 활성 session 단위)
// - ADR-0021 D5 (session switch 시 store reset)
// - ADR-0018 D1 (CanvasLayout envelope — schema v2)
// - ADR-0018 D7 + ADR-0024 D2 (z mutation 정책 — 4 액션은 별 store/util)
// - frontend-handover §3.1 (architectural invariants — store invariant 1)
// - frontend-handover §6 Stage 1.3
// - CONTEXT.md 의 "Manipulation Selection (M)" / "Input Target (I)" 정의
// - G20 amend: maximized 는 FE-only ephemeral (schema 외) — session 단위 1 item.
//
// Stage 1 skeleton 의 범위:
// - Type / 구조 / 기본 mutation method 만 정의.
// - 실제 HTTP fetch (`switchSession`) / WS frame dispatch 통합 / 기존
//   `panels.svelte.ts` 의 server-wide → session-scoped amend 는 Stage 2~4.
//
// Reactivity:
// - SvelteMap / SvelteSet — entry-level reactivity (`panels.svelte.ts` /
//   `mux.svelte.ts` 패턴 정합).
// - `$state` — primitive / object 의 reactive root.

import { SvelteMap, SvelteSet } from 'svelte/reactivity';

import { debugCount } from '$lib/common/debugCounts';
import { attachConfirm, deleteItem, EtagMismatchError, mutateLayout, UnauthorizedError } from '$lib/http/sessions';
import { danglingTerminals } from '$lib/stores/danglingTerminals.svelte';
import { historyStore } from '$lib/stores/historyStore.svelte';
import { terminalPool } from '$lib/stores/terminalPool.svelte';
import { sessionStorageHint } from '$lib/stores/sessionStorageHint';
import { toastStore } from '$lib/ui/toast-store.svelte';
import { getWebpageId, webpageHeaders } from '$lib/session/webpageId';
import {
  MINIMIZED_TERMINAL_PANEL_HEIGHT,
  type CanvasItem,
  type CanvasLayout,
  type Viewport,
} from '$lib/types/canvas';
import {
  descendantGroups,
  descendantItems,
  effectiveVisibility,
  pruneEmptyGroups,
  type Group,
} from '$lib/types/group';
import { generateUuidV4 } from '$lib/uuid';
import { normalizeLayout } from '$lib/stores/zSpace';
import type { AttachConfirmSummary } from '$lib/types/sessions';

/** Active session 의 식별 정보. attach 성공 시 set, detach 시 null. */
export interface ActiveSession {
  /** Session name (user-facing identifier, ADR-0019). */
  name: string;
}

/**
 * Reattach 결과 — `attemptReattach()` 의 분기 (ADR-0019 D5.1/D5.4 + plan-0008 §4).
 *
 * - `success`: 200 응답 + layout fetch + sessionStore.setActiveSession/loadLayout
 *              완료. 호출자는 본 화면 mount 진입 가능.
 * - `confirm_required`: 200 응답이지만 `unmatched.length > 0` — BE 의 terminal
 *              pool 에 매칭 없는 panel UUID 존재 (= server 재기동 후 모든 terminal
 *              stale 시 가장 흔히 발생). caller (reconnectGate 등) 가 사용자에게
 *              AttachConfirmModal 노출 — silent 흐름이 panel 만 남기고 terminal
 *              respawn 을 건너뛰던 회귀 (2026-05-17 사용자 보고) 의 직접 fix.
 * - `in_use`: 409. 다른 webpage 가 attach 보유.
 * - `not_found`: 404. session 이 BE 에서 사라짐. hint 도 자동 clear 됨.
 * - `unauthorized`: 401. cookie 만료 — caller 가 /auth redirect.
 * - `unreachable`: 5xx / network error / AbortError 외 fetch 실패. message 동봉.
 */
export type ReattachResult =
  | { kind: 'success' }
  | { kind: 'confirm_required'; summary: AttachConfirmSummary }
  | { kind: 'in_use'; holderPid?: number }
  | { kind: 'not_found' }
  | { kind: 'unauthorized' }
  | { kind: 'unreachable'; message: string };

/** Viewport 기본값 — session 없는 상태 / fresh layout 의 fallback. */
const DEFAULT_VIEWPORT: Viewport = { x: 0, y: 0, zoom: 1 };

/** `attemptReattach` 의 WS conn id stub — `WorkspaceSwitcher` 와 동일 패턴. */
function makeWsConnId(): string {
  return getWebpageId();
}

/**
 * Group id 발급 — 표준 UUID v4 (8-4-4-4-12 lowercase hex).
 *
 * BE 정합: `crates/http-api/src/schema.rs:843` 의 `is_uuid_shape()` 가 hex-only
 * 검증 — `'g'` 등 비-hex prefix 사용 시 `BadGroupId` reject. group 과 item id 가
 * 같은 UUID format 을 공유 (구분은 `sessionStore.groups.has(id)` map lookup).
 * 출처: plan-0012 §3.1 A.1 + handover-2026-05-22 §A.1 amend.
 */
function freshGroupId(): string {
  return generateUuidV4();
}

function normalizeLoadedItem(item: CanvasItem): CanvasItem {
  if (
    item.type === 'terminal' &&
    item.minimized === true &&
    item.h < MINIMIZED_TERMINAL_PANEL_HEIGHT
  ) {
    return { ...item, h: MINIMIZED_TERMINAL_PANEL_HEIGHT };
  }
  return item;
}

class SessionStore {
  /** 현 webpage 가 attach 한 session. null = pre-attach / post-detach. */
  active = $state<ActiveSession | null>(null);

  /** `items[]` 의 in-memory representation — id 키 SvelteMap. */
  items = $state(new SvelteMap<string, CanvasItem>());

  /** `groups[]` 의 in-memory representation — id 키 SvelteMap. */
  groups = $state(new SvelteMap<string, Group>());

  /** Viewport (panel/zoom). 양방향 sync 대상 (Stage 7 FE-9). */
  viewport = $state<Viewport>({ ...DEFAULT_VIEWPORT });

  /**
   * Manipulation Selection — 사용자가 제어 대상으로 잡은 Items 의 id 집합.
   * 다중 선택. CONTEXT.md 정의. session-scoped.
   */
  M = $state(new SvelteSet<string>());

  /**
   * Input Target — 키보드 입력이 라우팅되는 terminal id (또는 null).
   * 한 session 안 unique. CONTEXT.md 정의.
   */
  I = $state<string | null>(null);

  /**
   * FE-only drill scope. `null` means canvas root; a group id means canvas hit
   * testing resolves selection to that group's direct children.
   *
   * This is deliberately separate from M: layer tree can select an exact child
   * while the canvas still operates at the containing drill level.
   */
  drillRootId = $state<string | null>(null);

  /**
   * FE-only ephemeral. Maximize 된 item 의 id — MaximizedItemModal 이 본 값
   * watch 해 workspace 전체를 덮는 modal overlay 렌더링. schema 영속 X.
   * in-flow PanelNode / NoteNode 는 그대로 마운트 유지 — modal 의 XtermHost 는
   * dispatcher 의 multi-subscriber (ADR-0021 D1 mirror) 로 동일 paneId fan-out.
   * 한 session 1 item. attach/switch 시 자동 null 로 reset.
   */
  maximizedItemId = $state<string | null>(null);

  /**
   * Focus mode (ADR-0017 §D5). FE-only ephemeral — session 단위.
   * targetPanelId === null 이면 currently-selected M[0] 이 대상.
   * Stage 7 G27 의 시각 효과는 후속.
   */
  focusMode = $state<{ enabled: boolean; targetPanelId: string | null }>({
    enabled: false,
    targetPanelId: null,
  });

  /**
   * R7 (batch-5) — 직전 spawn 된 text item id. `itemFactory.commitNewItem`
   * 의 성공 path 가 text type 이면 set. TextNode 가 mount $effect 에서
   * 본 값과 자신의 id 가 일치하면 즉시 editing=true 진입 + flag clear.
   *
   * FE-only ephemeral — page reload 시 null. session switch / clear 시 reset.
   */
  justSpawnedTextId = $state<string | null>(null);

  /**
   * Minimize / maximize 직전 옛 geometry 의 *in-memory backup*. FE-only — page
   * reload 시 손실 (사용자가 restore 누르면 default size 로 복원). Schema 의
   * item.x/y/w/h 변경 패턴 (PanelNode onMinimize/onMaximize) 에서 옛 값 보존용.
   *
   * Key = item id. Value = { x, y, w, h } 직전 snapshot.
   *
   * - Minimize 진입 시 h 만 백업 (x/y/w 도 함께 저장 — restore 시 일괄 복원)
   *   → h = MINIMIZED_TERMINAL_PANEL_HEIGHT 으로 PUT. restore 시 백업 의 h 복원, entry clear.
   * - Maximize 진입 시 (x, y, w, h) 백업 → viewport visible extent 로 PUT.
   *   restore 시 백업 복원, entry clear.
   *
   * 두 path 가 동시 활성화될 수 없도록 (minimize OR maximize 만), backup entry
   * 는 한 가지 path 의 *원본* 만 가짐.
   */
  restoredItemGeoms = $state<SvelteMap<string, { x: number; y: number; w: number; h: number }>>(
    new SvelteMap(),
  );

  backupItemGeom(id: string, geom: { x: number; y: number; w: number; h: number }): void {
    this.restoredItemGeoms.set(id, { ...geom });
  }

  getRestoredGeom(id: string): { x: number; y: number; w: number; h: number } | null {
    return this.restoredItemGeoms.get(id) ?? null;
  }

  clearRestoredGeom(id: string): void {
    this.restoredItemGeoms.delete(id);
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* Layout lifecycle (loaded ↔ cleared)                                    */
  /* ────────────────────────────────────────────────────────────────────── */

  /**
   * Layout 적용. attach 성공 / LAYOUT_CHANGED rebuild / self-mutate PUT 후 응답
   * 등 *모든* layout refresh path 에서 호출.
   *
   * **M 은 의도적으로 clear 하지 않음** — 사용자 요구 "이동/정렬/크기변경/제거
   * 외 모든 선택 후 동작 은 자동 해제 안 함". layoutMutation 의 응답으로
   * loadLayout 이 호출돼도 사용자의 selection 의도는 carry over.
   *
   * 외부 source (session 진입 / LAYOUT_CHANGED 으로 *전혀 다른* layout) 일 때는
   * caller (setActiveSession / WS dispatcher) 가 명시 clear 책임.
   *
   * `I` (input target) / `maximizedItemId` / focusMode 도 동일 정책 — *유지*.
   * 단 *layout 에서 사라진 id* 는 stale 이지만 next user gesture 가 cleanup.
   */
  loadLayout(layout: CanvasLayout): void {
    debugCount('sessionStore.loadLayout');
    // plan-0012 §6 / ADR-0024 신 D3' — boot 시점 consecutive z normalize.
    // 옛 layout (gap 있는 z) 가 fresh attach / LAYOUT_CHANGED 으로 들어와도 store
    // entry 시점에 *FE 측 invariant* 정합. Idempotent — 이미 정합인 layout 은
    // z 값 변경 0건. 다음 mutation PUT 시 normalized z 가 BE 로 자연 commit.
    const normalized = normalizeLayout(layout);
    this.items.clear();
    for (const it of normalized.items) {
      const item = normalizeLoadedItem(it);
      this.items.set(item.id, item);
    }
    this.groups.clear();
    for (const g of normalized.groups) {
      this.groups.set(g.id, g);
    }
    this.viewport = { ...normalized.viewport };
  }

  /**
   * Active session 의 layout 을 BE 에서 fresh GET 으로 다시 받아 store 동기.
   *
   * 용도 (2026-05-22, 사용자 보고 #upload-canvas-desync) — image/document
   * upload 후 *드물게* canvas 에 신규 item 이 안 보이는 회귀 (browser refresh
   * 시 보임). applyMutation 의 PUT 응답 + loadLayout 흐름이 어떤 race 로
   * store 에 미반영. 본 helper 가 caller 가 명시적으로 호출하면 *narrow
   * equivalent of browser refresh* — WS / 다른 ephemeral state 보존하면서
   * layout 만 fresh sync. workaround layer (root cause trace 는 별 작업).
   *
   * 정책 = *silent best-effort*: 실패 시 toast 안 띄움 (caller 의 본 호출이
   * defensive 라 실패해도 functional). 호출 시점에 active === null 이면 noop.
   * Returns true on success, false otherwise.
   */
  async reloadActiveLayout(): Promise<boolean> {
    const active = this.active;
    if (active === null) return false;
    try {
      const res = await fetch(
        `/api/sessions/${encodeURIComponent(active.name)}/layout`,
        {
          method: 'GET',
          headers: { Accept: 'application/json' },
          credentials: 'include',
        },
      );
      if (!res.ok) return false;
      const layout = (await res.json()) as CanvasLayout;
      this.loadLayout(layout);
      return true;
    } catch {
      return false;
    }
  }

  /**
   * Session attach 진입. Stage 2~3 의 attach handler 가 호출 — 본 skeleton
   * 은 state set 만 (실 HTTP/WS 통합은 후속).
   *
   * sessionStorage hint 도 함께 set — ADR-0019 D5.4 + plan-0008 §4.5.
   * AppPage 다음 reload 시점에 ReconnectModal trigger 의 입력.
   *
   * Session 진입은 *외부 source* — 이전 session 의 ephemeral state (M / I /
   * maximize / focus) 를 carry over 해선 안 되므로 본 메서드가 명시 reset.
   */
  setActiveSession(session: ActiveSession): void {
    this.active = session;
    sessionStorageHint.set(session.name);
    this.M.clear();
    this.drillRootId = null;
    this.I = null;
    this.maximizedItemId = null;
    this.focusMode = { enabled: false, targetPanelId: null };
    this.justSpawnedTextId = null;
    // ADR-0028 D4 — per-session history. 이전 session 의 stack 은 drop.
    historyStore.setActive(session.name);
  }

  /**
   * Detach / pre-attach 상태로 reset. Session switch 흐름의 시작점.
   *
   * sessionStorage hint 도 함께 clear — 명시 detach / logout / [Switch session…]
   * / session [Delete] 흐름 모두 통과. 다음 reload 도 dialog 흐름으로 자연 회귀.
   */
  clear(): void {
    // Pending viewport debounce timer 도 같이 취소. 안 하면 직전 session 의
    // pan/zoom 이 500ms 안에 switch 됐을 때 잘못된 active 로 flush 됨.
    if (this.#viewportTimer !== null) {
      clearTimeout(this.#viewportTimer);
      this.#viewportTimer = null;
    }
    this.active = null;
    this.items.clear();
    this.groups.clear();
    this.viewport = { ...DEFAULT_VIEWPORT };
    this.M.clear();
    this.drillRootId = null;
    this.I = null;
    this.maximizedItemId = null;
    this.focusMode = { enabled: false, targetPanelId: null };
    this.justSpawnedTextId = null;
    sessionStorageHint.clear();
    historyStore.setActive(null);
  }

  /**
   * Session switch — 현재 session detach + 새 session attach.
   *
   * Stage 1 skeleton 에서는 *reset 만* 처리. 실제 HTTP `POST /api/sessions/<name>/attach`
   * + match-or-spawn confirm dialog (ADR-0018 D6) 는 Stage 2~3 에서 추가.
   * Caller 가 attach 응답으로 받은 layout 을 `loadLayout()` 에 넘김.
   */
  switchSession(name: string): void {
    this.clear();
    this.active = { name };
    historyStore.setActive(name);
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* M (Manipulation Selection) — multi-id set                              */
  /* ────────────────────────────────────────────────────────────────────── */

  setM(ids: Iterable<string>): void {
    this.M.clear();
    for (const id of ids) this.M.add(id);
    this.#dedupM();
  }

  addToM(id: string): void {
    this.M.add(id);
    this.#dedupM();
  }

  removeFromM(id: string): void {
    this.M.delete(id);
  }

  toggleM(id: string): void {
    if (this.M.has(id)) this.M.delete(id);
    else this.M.add(id);
    this.#dedupM();
  }

  clearM(): void {
    this.M.clear();
  }

  setDrillRoot(id: string | null): void {
    this.drillRootId = id !== null && this.groups.has(id) ? id : null;
  }

  clearDrill(): void {
    this.drillRootId = null;
  }

  /**
   * Canvas-level Select All target.
   *
   * Root scope selects only root-level visible elements: root items + root
   * groups. Drill scope selects only direct visible children of the active
   * drill root. Descendant items under a child group stay represented by that
   * child group id, matching canvas hit-test priority.
   */
  visibleElementsAtDrillScope(): string[] {
    const parentId = this.drillRootId;
    const groupsById = new Map(this.groups);
    const ids: string[] = [];
    for (const [id, group] of this.groups) {
      if (group.parent_id !== parentId) continue;
      if (!effectiveVisibility(group.visibility, group.parent_id, groupsById)) continue;
      ids.push(id);
    }
    for (const [id, item] of this.items) {
      if (item.parent_id !== parentId) continue;
      if (!effectiveVisibility(item.visibility, item.parent_id, groupsById)) continue;
      ids.push(id);
    }
    return ids;
  }

  selectAllVisibleAtDrillScope(): boolean {
    if (this.active === null) return false;
    const ids = this.visibleElementsAtDrillScope();
    if (ids.length === 0) return false;
    this.setM(ids);
    return true;
  }

  /**
   * ADR-0024 D15 — M 안에 group G + G 의 자손 (item 또는 nested group) 가 동시
   * 포함되면 *자손 제거* (G 만 남김). Group 이 *atomic 단위* 로 작동하도록.
   *
   * ADR-0010 D7 의 "group plain click = 자손 다 M, group 자체는 M 외" 패턴과는
   * 다른 경로 — 본 dedup 은 group 자체가 명시적으로 M 에 들어온 경우 (canvas
   * rail click, Cmd-click group row, createGroup 직후 post-M) 에 활성.
   */
  #dedupM(): void {
    if (this.M.size <= 1) return;
    const groupIds: string[] = [];
    for (const id of this.M) {
      if (this.groups.has(id)) groupIds.push(id);
    }
    if (groupIds.length === 0) return;
    const itemsArr = [...this.items.values()];
    const groupsArr = [...this.groups.values()];
    for (const gid of groupIds) {
      for (const it of descendantItems(gid, groupsArr, itemsArr)) {
        this.M.delete(it.id);
      }
      for (const dg of descendantGroups(gid, groupsArr)) {
        this.M.delete(dg.id);
      }
    }
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* Group lifecycle — ADR-0010 D14 (createGroup) / D12 (ungroup)           */
  /* ────────────────────────────────────────────────────────────────────── */

  /**
   * Group id 식별 — `groups` SvelteMap 의 O(1) lookup.
   *
   * BE 정합: group + item id 모두 표준 UUID 8-4-4-4-12 hex format 공유 →
   * prefix-based 구분 불가능. SvelteMap.has() lookup 이 단일 진실.
   * 출처: plan-0012 §3.1 A.4 + handover-2026-05-22 §A.1 amend.
   */
  isGroupId(id: string): boolean {
    return this.groups.has(id);
  }

  /**
   * ADR-0010 D14 — auto label "Group N", N = 살아있는 라벨의 max + 1.
   * 삭제된 N 는 재사용하지 않음 (사용자 명시 rename 한 group 은 pattern 안 매치).
   */
  nextGroupName(): string {
    let max = 0;
    for (const g of this.groups.values()) {
      const m = /^Group (\d+)$/.exec(g.label ?? '');
      if (m && m[1] !== undefined) {
        const n = parseInt(m[1], 10);
        if (n > max) max = n;
      }
    }
    return `Group ${max + 1}`;
  }

  /**
   * ADR-0010 D14 — Group 생성.
   *
   * 동작:
   *  1. element ids 의 dedup (group + 자손 동시 포함 → 자손 제거).
   *  2. Common ancestor 계산 (모든 element 의 ancestor chain 의 가장 깊은 공통
   *     group, 없으면 null = canvas root).
   *  3. 새 group entity: id = `g<32-hex>`, parent_id = commonAncestor, label =
   *     auto, color = null, visibility = 'visible', locked = false, order =
   *     같은 부모 sibling 의 max order + 1.
   *  4. Atomic mutation (optimistic + PUT):
   *     - groups: 새 group 추가, element 가 group 이면 parent_id 갱신.
   *     - items: element 가 item 이면 parent_id 갱신.
   *     - z: 새 group 이 부모 z-range 의 *top* 에 배치. 자손 z 는 현 z 순서 보존
   *       후 consecutive 정합 (normalizeLayout).
   *  5. Post-M = {newGroupId}. ADR-0010 D14 의 명시.
   *
   * 반환: 성공 → 새 group id. 실패 (active null / 빈 input / PUT fail) → null.
   */
  async createGroup(elementIds: Iterable<string>): Promise<string | null> {
    if (this.active === null) return null;
    // 1. dedup + 존재 검증.
    const inputIds = [...new Set(elementIds)];
    const valid = inputIds.filter(
      (id) => this.items.has(id) || this.groups.has(id),
    );
    const deduped = this.#dedupForGrouping(valid);
    if (deduped.length === 0) return null;

    // 2. Common ancestor (현 store 기준 — transform 안에서 다시 계산하지 않음,
    //    transform 의 input layout 도 같은 active session 의 snapshot 이라 일치).
    const priorSnapshot = this.layoutSnapshot();
    const commonAncestor = this.#commonAncestorOf(priorSnapshot, deduped);

    // 3. 새 group entity 준비.
    const newGroupId = freshGroupId();
    const label = this.nextGroupName();
    const siblingMaxOrder = this.#maxSiblingOrder(priorSnapshot, commonAncestor);
    const newGroup: Group = {
      id: newGroupId,
      parent_id: commonAncestor,
      label,
      color: null,
      visibility: 'visible',
      locked: false,
      order: siblingMaxOrder + 1,
    };

    const dedupedSet = new Set(deduped);
    const transform = (cur: CanvasLayout): CanvasLayout => {
      // 4a. element parent_id 갱신.
      const reparentedItems = cur.items.map((it) =>
        dedupedSet.has(it.id) ? { ...it, parent_id: newGroupId } : it,
      );
      const reparentedGroups = cur.groups.map((g) =>
        dedupedSet.has(g.id) ? { ...g, parent_id: newGroupId } : g,
      );
      // 새 group 이 layout 에 이미 있을 리 없지만 defensive: 중복 X.
      const groupsWithNew = reparentedGroups.some((g) => g.id === newGroupId)
        ? reparentedGroups
        : [...reparentedGroups, newGroup];
      const interim: CanvasLayout = {
        ...cur,
        items: reparentedItems,
        groups: groupsWithNew,
      };
      // 4b. Order overrides — commonAncestor 와 newGroup 두 level.
      const overrides = new Map<string | null, readonly string[]>();
      const caCurrent = this.#blockIdsAtParent(interim, commonAncestor).filter(
        (id) => id !== newGroupId,
      );
      overrides.set(commonAncestor, [...caCurrent, newGroupId]);
      const childOrder = this.#blockIdsAtParent(interim, newGroupId);
      overrides.set(newGroupId, childOrder);
      // 4c. Normalize 으로 consecutive z 정합.
      return normalizeLayout(interim, overrides);
    };

    // Optimistic update (items + groups 모두 surgical).
    const optimistic = transform(priorSnapshot);
    this.#applyLayoutSurgically(optimistic);
    const priorM = [...this.M];
    this.setM([newGroupId]);

    const result = await this.applyMutation(transform, { priorSnapshot });
    if (!result.ok) {
      // applyMutation 의 priorSnapshot 복원이 items + groups 는 처리. M 은 별도.
      this.setM(priorM);
      return null;
    }
    return newGroupId;
  }

  /**
   * ADR-0010 D12 — Group 해체 (비파괴). Group entity 만 제거 + 자손 보존.
   *
   * 동작:
   *  1. groupId 의 직속 자식 (item + nested group) 의 parent_id = group.parent_id.
   *  2. group entity 제거.
   *  3. z 정합 — 자손이 그대로 옛 group 위치에 끼어들도록 commonAncestor level 의
   *     block order 가 [..., before-group, ...descendants, ...after-group] 형태.
   *  4. M.clear() + 자손들로 setM (post-M = direct children).
   *
   * 반환: true = 성공, false = 실패 (active null / group 미존재 / PUT fail).
   */
  async ungroup(groupId: string): Promise<boolean> {
    if (this.active === null) return false;
    const group = this.groups.get(groupId);
    if (!group) return false;

    const priorSnapshot = this.layoutSnapshot();
    const parentOfG = group.parent_id;
    const directChildIds = this.#directChildIdsOf(priorSnapshot, groupId);
    if (directChildIds.length === 0) {
      // empty group — just remove the group entity.
    }

    const transform = (cur: CanvasLayout): CanvasLayout => {
      // 1. 직속 자식 reparent (item + group).
      const directSet = new Set(directChildIds);
      const reparentedItems = cur.items.map((it) =>
        directSet.has(it.id) ? { ...it, parent_id: parentOfG } : it,
      );
      const reparentedGroups = cur.groups.map((g) =>
        directSet.has(g.id) ? { ...g, parent_id: parentOfG } : g,
      );
      // 2. group entity 제거.
      const groupsWithoutG = reparentedGroups.filter((g) => g.id !== groupId);
      const interim: CanvasLayout = {
        ...cur,
        items: reparentedItems,
        groups: groupsWithoutG,
      };
      // 3. parentOfG level 의 order = 옛 [..., G, ...] 에서 G 자리에 자손 (현 z 순서 보존) 삽입.
      const caCurrent = this.#blockIdsAtParent(cur, parentOfG);
      const childOrderInG = this.#blockIdsAtParent(cur, groupId);
      const newOrder: string[] = [];
      for (const id of caCurrent) {
        if (id === groupId) {
          newOrder.push(...childOrderInG);
        } else {
          newOrder.push(id);
        }
      }
      const overrides = new Map<string | null, readonly string[]>();
      overrides.set(parentOfG, newOrder);
      return normalizeLayout(interim, overrides);
    };

    const optimistic = transform(priorSnapshot);
    this.#applyLayoutSurgically(optimistic);
    const priorM = [...this.M];
    const priorDrillRootId = this.drillRootId;
    // Post-M = direct children of G (D12).
    this.setM(directChildIds);
    if (this.drillRootId === groupId) this.clearDrill();

    const result = await this.applyMutation(transform, { priorSnapshot });
    if (!result.ok) {
      this.setM(priorM);
      this.setDrillRoot(priorDrillRootId);
      return false;
    }
    return true;
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* Group helpers (internal)                                                */
  /* ────────────────────────────────────────────────────────────────────── */

  #dedupForGrouping(ids: readonly string[]): string[] {
    if (ids.length <= 1) return [...ids];
    const groupsArr = [...this.groups.values()];
    const itemsArr = [...this.items.values()];
    const groupSet = new Set(ids.filter((id) => this.groups.has(id)));
    const removed = new Set<string>();
    for (const gid of groupSet) {
      for (const it of descendantItems(gid, groupsArr, itemsArr)) {
        removed.add(it.id);
      }
      for (const dg of descendantGroups(gid, groupsArr)) {
        removed.add(dg.id);
      }
    }
    return ids.filter((id) => !removed.has(id));
  }

  #ancestorChainTopDown(layout: CanvasLayout, id: string): (string | null)[] {
    // Returns [null, gRoot, ..., gParent] — id 의 부모 그룹들의 root→deepest path.
    // group + item id 가 같은 UUID format 이라 prefix 구분 불가 — 두 array 모두 lookup.
    let parentId: string | null | undefined;
    const g = layout.groups.find((g) => g.id === id);
    if (g !== undefined) {
      parentId = g.parent_id;
    } else {
      parentId = layout.items.find((it) => it.id === id)?.parent_id;
    }
    if (parentId === undefined) return [];
    const chain: (string | null)[] = [];
    let cur: string | null = parentId;
    while (cur !== null) {
      chain.unshift(cur);
      const g = layout.groups.find((g) => g.id === cur);
      cur = g?.parent_id ?? null;
    }
    chain.unshift(null); // root sentinel
    return chain;
  }

  #commonAncestorOf(layout: CanvasLayout, ids: readonly string[]): string | null {
    if (ids.length === 0) return null;
    const first = ids[0];
    if (first === undefined) return null;
    let common = this.#ancestorChainTopDown(layout, first);
    for (let i = 1; i < ids.length; i++) {
      const id = ids[i];
      if (id === undefined) continue;
      const c = this.#ancestorChainTopDown(layout, id);
      let k = 0;
      const lim = Math.min(common.length, c.length);
      while (k < lim && common[k] === c[k]) k++;
      common = common.slice(0, k);
    }
    if (common.length === 0) return null;
    const last = common[common.length - 1];
    return last === undefined ? null : last;
  }

  #maxSiblingOrder(layout: CanvasLayout, parentId: string | null): number {
    let max = 0;
    for (const g of layout.groups) {
      if (g.parent_id === parentId && g.order > max) max = g.order;
    }
    return max;
  }

  #blockIdsAtParent(layout: CanvasLayout, parentId: string | null): string[] {
    // 현 layout 의 atomic block order — group block 의 min z = 자손 item 의 min z.
    // children-of map 을 만들어 group 의 min z 를 계산.
    const itemsById = new Map(layout.items.map((it) => [it.id, it] as const));
    const childrenOf = new Map<string, { id: string; kind: 'item' | 'group' }[]>();
    for (const g of layout.groups) childrenOf.set(g.id, []);
    for (const it of layout.items) {
      if (it.parent_id !== null) {
        const arr = childrenOf.get(it.parent_id);
        if (arr) arr.push({ id: it.id, kind: 'item' });
      }
    }
    for (const g of layout.groups) {
      if (g.parent_id !== null) {
        const arr = childrenOf.get(g.parent_id);
        if (arr) arr.push({ id: g.id, kind: 'group' });
      }
    }
    function minZ(blockId: string, kind: 'item' | 'group'): number {
      if (kind === 'item') return itemsById.get(blockId)?.z ?? 0;
      let min = Number.POSITIVE_INFINITY;
      const stack = [blockId];
      while (stack.length > 0) {
        const cur = stack.pop() as string;
        const kids = childrenOf.get(cur) ?? [];
        for (const k of kids) {
          if (k.kind === 'item') {
            const z = itemsById.get(k.id)?.z;
            if (z !== undefined && z < min) min = z;
          } else {
            stack.push(k.id);
          }
        }
      }
      return Number.isFinite(min) ? min : 0;
    }
    const blocks: { id: string; kind: 'item' | 'group'; mz: number }[] = [];
    for (const it of layout.items) {
      if (it.parent_id === parentId) {
        blocks.push({ id: it.id, kind: 'item', mz: it.z });
      }
    }
    for (const g of layout.groups) {
      if (g.parent_id === parentId) {
        blocks.push({ id: g.id, kind: 'group', mz: minZ(g.id, 'group') });
      }
    }
    blocks.sort((a, b) => a.mz - b.mz);
    return blocks.map((b) => b.id);
  }

  #directChildIdsOf(layout: CanvasLayout, groupId: string): string[] {
    // Sorted by current min z — ungroup 시 자손 z 순서 보존에 사용.
    return this.#blockIdsAtParent(layout, groupId);
  }

  #applyLayoutSurgically(layout: CanvasLayout): void {
    // optimisticMutation 의 items-only 패턴을 groups 까지 확장. createGroup /
    // ungroup 처럼 groups[] 도 mutate 하는 path 의 optimistic 진입.
    const nextItemIds = new Set<string>();
    for (const it of layout.items) {
      nextItemIds.add(it.id);
      const cur = this.items.get(it.id);
      if (cur !== it) this.items.set(it.id, it);
    }
    for (const id of [...this.items.keys()]) {
      if (!nextItemIds.has(id)) this.items.delete(id);
    }
    const nextGroupIds = new Set<string>();
    for (const g of layout.groups) {
      nextGroupIds.add(g.id);
      const cur = this.groups.get(g.id);
      if (cur !== g) this.groups.set(g.id, g);
    }
    for (const id of [...this.groups.keys()]) {
      if (!nextGroupIds.has(id)) this.groups.delete(id);
    }
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* I (Input Target) — single terminal id                                  */
  /* ────────────────────────────────────────────────────────────────────── */

  setI(terminalId: string | null): void {
    this.I = terminalId;
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* Maximize — FE-only ephemeral, 1-at-a-time (G20)                        */
  /* ────────────────────────────────────────────────────────────────────── */

  maximize(itemId: string): void {
    this.maximizedItemId = itemId;
  }

  unmaximize(): void {
    this.maximizedItemId = null;
  }

  toggleMaximize(itemId: string): void {
    this.maximizedItemId = this.maximizedItemId === itemId ? null : itemId;
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* Focus / zoom-to-item — plan-0010 Task 1                                */
  /* ────────────────────────────────────────────────────────────────────── */

  /**
   * Pending zoom-to-selection signal. ViewportCtrl 의 focus 버튼 클릭 시 set.
   * Canvas 의 $effect 가 본 field 를 watch — SvelteFlow `setViewport` 으로
   * 해당 item/group selection 의 union BBox 를 viewport 중앙에 + 가득 채움. 처리 후 Canvas
   * 가 null 로 복귀 (1-shot signal).
   *
   * 단일 선택: [id]. 다중 선택: 모든 ids — item 과 group 모두 허용.
   */
  pendingZoomToIds = $state<string[] | null>(null);

  zoomToIds(ids: string[]): void {
    if (ids.length === 0) {
      this.pendingZoomToIds = null;
      return;
    }
    this.pendingZoomToIds = [...ids];
  }

  zoomToSelection(): void {
    const ids = Array.from(this.M);
    this.zoomToIds(ids);
  }

  clearPendingZoom(): void {
    this.pendingZoomToIds = null;
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* Viewport                                                               */
  /* ────────────────────────────────────────────────────────────────────── */

  /** Debounce timer for viewport PUT (Stage 7 FE-9). */
  #viewportTimer: ReturnType<typeof setTimeout> | null = null;
  static readonly VIEWPORT_DEBOUNCE_MS = 500;

  /**
   * Update the in-memory viewport and schedule a debounced PUT to
   * persist `viewport` into the session's layout.
   *
   * The debounce coalesces rapid pan/zoom into a single network round
   * trip. Failures are swallowed (logged) — viewport is "close enough"
   * ephemeral state; a missed write only costs the last 500ms of
   * panning on the next reload.
   */
  updateViewport(v: Viewport): void {
    const cur = this.viewport;
    if (
      Math.abs(cur.x - v.x) < 0.5 &&
      Math.abs(cur.y - v.y) < 0.5 &&
      Math.abs(cur.zoom - v.zoom) < 0.001
    ) {
      return;
    }
    this.viewport = { ...v };
    if (this.active === null) return;
    // 예약 시점의 session/viewport 를 closure 로 캡처. 500ms 사이에 session
    // switch 가 일어나도 flush 가 현재 active 에 직전 session 의 viewport 를
    // 쓰는 race 차단 — flush 에서 sessionName 비교로 폐기.
    const sessionName = this.active.name;
    const snapshot: Viewport = { ...v };
    if (this.#viewportTimer !== null) clearTimeout(this.#viewportTimer);
    this.#viewportTimer = setTimeout(() => {
      this.#viewportTimer = null;
      void this.#flushViewport(sessionName, snapshot);
    }, SessionStore.VIEWPORT_DEBOUNCE_MS);
  }

  async #flushViewport(sessionName: string, viewport: Viewport): Promise<void> {
    const active = this.active;
    if (active === null || active.name !== sessionName) return;
    try {
      await mutateLayout(sessionName, (cur) => ({ ...cur, viewport }));
    } catch (err) {
      console.debug('[gtmux] viewport persist failed', err);
    }
  }

  /**
   * Export/download boundary helper — force the pending debounced viewport PUT
   * to complete before code reads the persisted layout back from the server.
   */
  async flushPendingViewport(expectedSessionName?: string): Promise<void> {
    if (this.#viewportTimer !== null) {
      clearTimeout(this.#viewportTimer);
      this.#viewportTimer = null;
    }
    const active = this.active;
    if (active === null) return;
    if (expectedSessionName !== undefined && active.name !== expectedSessionName) return;
    await this.#flushViewport(active.name, { ...this.viewport });
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* Reattach — ADR-0019 D5.1 / D5.4, plan-0008 §4.2                        */
  /* ────────────────────────────────────────────────────────────────────── */

  /**
   * Reattach to `name` — silent / blocking 의 공통 utility.
   *
   * - POST /api/sessions/<name>/attach (cookie + AbortSignal)
   * - 200 → GET /layout → setActiveSession + loadLayout → `success`
   * - 409 → `in_use` (holder.pid 추출)
   * - 404 → `not_found` (hint 도 자동 clear — caller 도 `cancel()` 으로 재clear 가능)
   * - 401 → `unauthorized` (caller 가 /auth redirect)
   * - 5xx / network / fetch throw → `unreachable` (단 AbortError 는 caller 가
   *   signal.aborted 로 자체 detect — 본 method 도 `unreachable` 로 반환하나
   *   caller 가 signal 확인 후 무시)
   *
   * BE 가 200 + `unmatched.length > 0` 응답 → `confirm_required` 반환 (2026-05-17
   * 회귀 fix). caller 가 AttachConfirmModal 노출. silent 진입 후 panel 만 남고
   * terminal respawn 이 누락되던 버그 직접 차단 — WorkspaceSwitcher.tryAttach 의
   * `confirm_required` 분기와 동일 패턴.
   */
  async attemptReattach(
    name: string,
    signal?: AbortSignal,
  ): Promise<ReattachResult> {
    let attachRes: Response;
    try {
      attachRes = await fetch(
        `/api/sessions/${encodeURIComponent(name)}/attach`,
        {
          method: 'POST',
          headers: {
            Accept: 'application/json',
            'Content-Type': 'application/json',
            ...webpageHeaders(),
          },
          credentials: 'include',
          body: JSON.stringify({ ws_conn_id: makeWsConnId() }),
          signal,
        },
      );
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      return { kind: 'unreachable', message };
    }

    if (attachRes.status === 401) {
      sessionStorageHint.clear();
      return { kind: 'unauthorized' };
    }
    if (attachRes.status === 404) {
      sessionStorageHint.clear();
      return { kind: 'not_found' };
    }
    if (attachRes.status === 409) {
      try {
        const body = (await attachRes.json()) as {
          holder?: { pid?: number };
        };
        return { kind: 'in_use', holderPid: body.holder?.pid };
      } catch {
        return { kind: 'in_use' };
      }
    }
    if (attachRes.status >= 500) {
      return {
        kind: 'unreachable',
        message: `server responded ${attachRes.status}`,
      };
    }
    if (!attachRes.ok) {
      return {
        kind: 'unreachable',
        message: `attach returned ${attachRes.status}`,
      };
    }

    // 200 — drain body (lock acquired). matched/unmatched 검사 → unmatched > 0
    // 면 confirm_required 반환 (WorkspaceSwitcher.tryAttach 와 정합). caller 가
    // AttachConfirmModal 노출 책임. 서버 재기동 후 panel 만 남기고 respawn 누락
    // 회귀 (2026-05-17) 직접 차단.
    let attachBody: { matched?: string[]; unmatched?: string[] } = {};
    try {
      attachBody = (await attachRes.json()) as {
        matched?: string[];
        unmatched?: string[];
      };
    } catch {
      /* body 형식 변화 무관 — 아래 layout fetch 가 진실 */
    }
    const matched = attachBody.matched ?? [];
    const unmatched = attachBody.unmatched ?? [];
    if (unmatched.length > 0) {
      return {
        kind: 'confirm_required',
        summary: {
          spawn_count: unmatched.length,
          unmatched_item_ids: unmatched,
          matched_item_ids: matched,
        },
      };
    }

    let layoutRes: Response;
    try {
      layoutRes = await fetch(
        `/api/sessions/${encodeURIComponent(name)}/layout`,
        {
          method: 'GET',
          headers: { Accept: 'application/json' },
          credentials: 'include',
          signal,
        },
      );
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      return { kind: 'unreachable', message };
    }
    if (layoutRes.status === 401) {
      sessionStorageHint.clear();
      return { kind: 'unauthorized' };
    }
    if (!layoutRes.ok) {
      return {
        kind: 'unreachable',
        message: `layout fetch returned ${layoutRes.status}`,
      };
    }
    try {
      const layout = (await layoutRes.json()) as CanvasLayout;
      this.setActiveSession({ name });
      this.loadLayout(layout);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      return { kind: 'unreachable', message: `layout parse: ${message}` };
    }
    return { kind: 'success' };
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* Silent reattach + mutation guard — plan-0008 Phase 2 (Case II)         */
  /* ────────────────────────────────────────────────────────────────────── */

  /**
   * Phase 2 — silent reattach in-flight 여부. visibilitychange / WS reconnect
   * 등의 trigger 가 호출. mutation guard 가 본 promise 를 await.
   */
  reattachInProgress = $state<boolean>(false);

  /** Phase 2 의 마지막 silent reattach 결과 — failure 분기 입력. */
  lastSilentReattachResult = $state<ReattachResult | null>(null);

  /** Internal promise — 동시 호출 dedup. */
  #silentReattachPromise: Promise<ReattachResult> | null = null;

  /**
   * Silent reattach utility — Phase 2 trigger 가 호출.
   *
   * 동시 호출은 동일 promise share (dedup). 결과는 `lastSilentReattachResult`
   * 에 보관. 호출자가 결과에 따라:
   *  - 'success': mutation 계속 진행.
   *  - 'in_use'/'not_found'/'unauthorized'/'unreachable': caller 가 modal/toast.
   *
   * P0-B (0045) — 사용자 변경 보존: silent 의도 상 viewport 는 사용자가
   * silentReattach 발화를 인지하지 못해야 함. attemptReattach 내부의
   * loadLayout 이 viewport 를 layout 의 저장값으로 reset 하므로, wrapper 가
   * 직전 snapshot 을 잡고 200 성공 후 복원. M/I/maximize/focusMode 의 reset 은
   * G20 ephemeral 정책 그대로 (server 가 권위).
   */
  silentReattach(name: string, signal?: AbortSignal): Promise<ReattachResult> {
    if (this.#silentReattachPromise !== null) return this.#silentReattachPromise;
    this.reattachInProgress = true;
    const preReattachViewport = { ...this.viewport };
    const promise = (async (): Promise<ReattachResult> => {
      try {
        const result = await this.attemptReattach(name, signal);
        this.lastSilentReattachResult = result;
        if (result.kind === 'success') {
          this.viewport = preReattachViewport;
        }
        return result;
      } finally {
        this.reattachInProgress = false;
        this.#silentReattachPromise = null;
      }
    })();
    this.#silentReattachPromise = promise;
    return promise;
  }

  /**
   * Mutation guard — outgoing write 진입점 (mutateLayout / deleteItem /
   * attachConfirm 등) 의 *바로 직전* 에 await 하는 helper.
   *
   * 동작:
   *  - reattach in-flight 면 끝날 때까지 await + result === success 면 통과.
   *  - lastSilentReattachResult 가 fail 상태면 그 결과 그대로 반환 (caller
   *    가 modal/toast 분기). caller 는 `await ... ; if (!ok) return;` 패턴.
   *  - 아무 trigger 도 없었으면 ok 반환 (no-op).
   *
   * 호출 예:
   *   const guard = await sessionStore.guardOutgoingMutation();
   *   if (!guard.ok) return;
   *   await mutateLayout(...);
   */
  async guardOutgoingMutation(): Promise<{ ok: boolean; result?: ReattachResult }> {
    if (this.#silentReattachPromise !== null) {
      const result = await this.#silentReattachPromise;
      return { ok: result.kind === 'success', result };
    }
    if (this.lastSilentReattachResult !== null && this.lastSilentReattachResult.kind !== 'success') {
      return { ok: false, result: this.lastSilentReattachResult };
    }
    return { ok: true };
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* Layout mutation entry — ADR-0028 D12                                   */
  /* ────────────────────────────────────────────────────────────────────── */

  /**
   * Current layout 의 deep snapshot — undo/redo 의 history entry 단위 (D7).
   *
   * SvelteMap value 들은 frozen-ish 가 아니므로 spread 로 새 object 추출 — 이후
   * store mutation 이 snapshot 을 mutate 하지 않도록 안전.
   */
  layoutSnapshot(): CanvasLayout {
    return {
      schema_version: 2,
      groups: Array.from(this.groups.values()).map((g) => ({ ...g })),
      items: Array.from(this.items.values()).map((it) => ({ ...it })),
      viewport: { ...this.viewport },
    };
  }

  /**
   * ADR-0028 D12 — 모든 layout mutation 의 단일 entry point.
   *
   * 책임:
   *  1. mutation guard (Phase 2 silentReattach 결과 검사)
   *  2. PRE-state snapshot → historyStore.capture (D3 — 1 PUT = 1 entry)
   *  3. mutateLayout(transform) — etag rebase 1회 자동
   *  4. loadLayout(response) — store 동기
   *  5. error 처리 (Unauthorized redirect, toast)
   *
   * 반환: `{ ok, layout? }` — ok=false 면 caller 가 추가 처리 없이 early return.
   *
   * 기존 직접 `mutateLayout` 호출은 본 helper 로 migration (D11 audit 정합).
   * 단, history capture 가 부적절한 mutation (viewport debounce flush, undo/redo
   * 자체) 은 `captureHistory: false` 로 skip.
   */
  /**
   * ADR-0028 D12 amend (batch-5 후속) — applyMutation 의 optimistic-update
   * 래퍼. 호출 시점에 priorSnapshot 캡처 → transform 을 *로컬 store 에 먼저*
   * 적용 (surgical items.set / delete) → 같은 transform 으로 applyMutation
   * 호출 (server 동기 + priorSnapshot 으로 PUT 실패 시 자동 rollback).
   *
   * 효과: Inspector toggle / dropdown / ColorPicker oncommit 처럼 *commit-
   * based* 1-shot 액션이 round-trip 대기 없이 즉시 UI 반영. server 부하는
   * 그대로 (1 액션 = 1 PUT — Inspector 컨트롤이 이미 commit-based 라 spam
   * 없음). 실패 시 toast + priorSnapshot 으로 store 복원.
   *
   * 사용처: Inspector 의 applyXxx helper. drag stop / NodeResizer onResizeEnd
   * 등 *이미* 수동 optimistic 인 caller 는 기존대로 applyMutation 직접 호출
   * (priorSnapshot 명시) — 본 helper 가 추가 mutation 안 함.
   */
  async optimisticMutation(
    transform: (cur: CanvasLayout) => CanvasLayout,
    options: {
      abortMessage?: string;
      failMessage?: string;
      captureHistory?: boolean;
    } = {},
  ): Promise<{ ok: boolean; layout?: CanvasLayout }> {
    if (this.active === null) return { ok: false };
    const priorSnapshot = this.layoutSnapshot();
    const optimistic = transform(priorSnapshot);
    // Surgical: 사라진 id delete + 변경된 id set. items.clear() + 재추가는
    // O(n) reactive churn 이라 큰 layout 에서 frame drop 위험 — 본 path 는
    // *Inspector 1~N item 변경* 의 hot path 라 surgical 유지.
    const nextIds = new Set<string>();
    for (const it of optimistic.items) {
      nextIds.add(it.id);
      const cur = this.items.get(it.id);
      if (cur !== it) this.items.set(it.id, it);
    }
    for (const id of [...this.items.keys()]) {
      if (!nextIds.has(id)) this.items.delete(id);
    }
    return await this.applyMutation(transform, { ...options, priorSnapshot });
  }

  async applyMutation(
    transform: (cur: CanvasLayout) => CanvasLayout,
    options: {
      abortMessage?: string;
      failMessage?: string;
      captureHistory?: boolean;
      /**
       * PRE-mutation snapshot — caller 가 명시. Drag commit 처럼 store 가
       * optimistic 갱신된 *후* 에 applyMutation 호출하는 path 는 본 옵션으로
       * "optimistic 직전" snapshot 을 전달. 미지정 시 호출 시점의 store snapshot
       * 사용 (Inspector edit 등 optimistic 없는 path 의 기본 동작).
       */
      priorSnapshot?: CanvasLayout;
    } = {},
  ): Promise<{ ok: boolean; layout?: CanvasLayout }> {
    const active = this.active;
    if (active === null) return { ok: false };
    const guard = await this.guardOutgoingMutation();
    if (!guard.ok) {
      toastStore.show({
        message:
          (options.abortMessage ??
            'Session reconnect failed — action aborted.') +
          ' Try refreshing the page.',
        tone: 'error',
        durationMs: 6_000,
      });
      return { ok: false };
    }
    const captureHistory = options.captureHistory !== false;
    const priorSnapshot = options.priorSnapshot ?? null;
    const before = captureHistory
      ? (priorSnapshot ?? this.layoutSnapshot())
      : null;
    try {
      const { layout } = await mutateLayout(active.name, transform);
      this.loadLayout(layout);
      if (before !== null) historyStore.capture(active.name, before);
      return { ok: true, layout };
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return { ok: false };
      }
      // 0065 FE-2 — caller 가 priorSnapshot 을 명시했다는 것은 호출 *전* store
      // 를 optimistic 갱신했다는 신호 (drag stop / NodeResizer / z-order 등).
      // failure 시 priorSnapshot 으로 store 를 복원해 "FE 는 변경된 상태로
      // 보이지만 BE 는 옛 상태" 의 silent 회귀를 차단한다. priorSnapshot 미지정
      // = optimistic update 없는 path (Inspector edit 등) — 별도 복원 무필요.
      if (priorSnapshot !== null) {
        this.loadLayout(priorSnapshot);
      }
      toastStore.show({
        message:
          options.failMessage ??
          `Mutation failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
      return { ok: false };
    }
  }

  /**
   * ADR-0028 D1 — item 제거 (deleteItem) 의 history capture 통합 entry.
   *
   * `deleteItem` 은 별도 BE endpoint (`DELETE /api/sessions/:name/items/:id`) 라
   * `mutateLayout` 우회 — 본 helper 가 PRE-state snapshot + 각 deleteItem
   * 호출 + store 동기 + history capture 책임.
   *
   * Partial 실패 허용: 성공한 id 만 store 에서 제거, history 는 (변경된 항목
   * 있을 때) 1 entry 로 capture (PRE-state). undo 시 mutateLayout 으로 PRE
   * 복원 → BE 가 items[] 에 panel 재추가. terminal 이면 pool 잔존 시 mirror
   * 자연 회복 (ADR-0028 D1.2), 사라진 경우 D9 toast.
   *
   * `killTerminal=true` 호출 시 — terminal 도 죽이므로 undo 시 unmatched 가
   * 더 흔함. caller (PanelNode performClose 의 kill 선택지) 가 명시 책임.
   */
  async applyDeletion(
    ids: readonly string[],
    options: {
      killTerminal?: boolean;
      abortMessage?: string;
    } = {},
  ): Promise<{ ok: number; fail: number }> {
    const active = this.active;
    if (active === null) return { ok: 0, fail: 0 };
    const guard = await this.guardOutgoingMutation();
    if (!guard.ok) {
      toastStore.show({
        message:
          (options.abortMessage ??
            'Session reconnect failed — delete aborted.') +
          ' Try refreshing the page.',
        tone: 'error',
      });
      return { ok: 0, fail: 0 };
    }
    if (ids.length === 0) return { ok: 0, fail: 0 };
    const before = this.layoutSnapshot();
    const kill = options.killTerminal === true;
    if (kill) {
      const beforeItems = new Map(before.items.map((it) => [it.id, it] as const));
      for (const id of ids) {
        this.items.delete(id);
        this.M.delete(id);
      }
      const results = await Promise.allSettled(ids.map((id) => deleteItem(active.name, id, true)));
      let unauthorized = false;
      let ok = 0;
      let fail = 0;
      for (let i = 0; i < results.length; i += 1) {
        const result = results[i]!;
        const id = ids[i]!;
        if (result.status === 'fulfilled') {
          ok += 1;
          continue;
        }
        if (result.reason instanceof UnauthorizedError) {
          unauthorized = true;
          continue;
        }
        const original = beforeItems.get(id);
        if (original !== undefined) this.items.set(id, original);
        console.warn('[gtmux] deleteItem failed', id, result.reason);
        fail += 1;
      }
      if (unauthorized) {
        window.location.href = '/auth';
        return { ok, fail };
      }
      if (ok > 0) {
        const beforePrune = this.layoutSnapshot();
        const afterPrune = pruneEmptyGroups(beforePrune);
        if (afterPrune.groups.length !== this.groups.size) {
          this.#applyLayoutSurgically(afterPrune);
          await this.applyMutation(() => afterPrune, {
            captureHistory: false,
            failMessage: 'Empty group cleanup failed',
            priorSnapshot: beforePrune,
          });
        }
        for (const id of [...this.M]) {
          if (!this.items.has(id) && !this.groups.has(id)) this.M.delete(id);
        }
        if (this.drillRootId !== null && !this.groups.has(this.drillRootId)) {
          this.clearDrill();
        }
        historyStore.capture(active.name, before);
      }
      return { ok, fail };
    }
    let ok = 0;
    let fail = 0;
    let unauthorized = false;
    for (const id of ids) {
      try {
        await deleteItem(active.name, id, kill);
        this.items.delete(id);
        this.M.delete(id);
        ok += 1;
      } catch (err) {
        if (err instanceof UnauthorizedError) {
          unauthorized = true;
          break;
        }
        console.warn('[gtmux] deleteItem failed', id, err);
        fail += 1;
      }
    }
    if (unauthorized) {
      window.location.href = '/auth';
      return { ok, fail };
    }
    if (ok > 0) {
      const beforePrune = this.layoutSnapshot();
      const afterPrune = pruneEmptyGroups(beforePrune);
      if (afterPrune.groups.length !== this.groups.size) {
        this.#applyLayoutSurgically(afterPrune);
        await this.applyMutation(() => afterPrune, {
          captureHistory: false,
          failMessage: 'Empty group cleanup failed',
          priorSnapshot: beforePrune,
        });
      }
      for (const id of [...this.M]) {
        if (!this.items.has(id) && !this.groups.has(id)) this.M.delete(id);
      }
      if (this.drillRootId !== null && !this.groups.has(this.drillRootId)) {
        this.clearDrill();
      }
      historyStore.capture(active.name, before);
    }
    return { ok, fail };
  }

  /**
   * ADR-0028 D8 — undo 1 step. Cmd+Z / Ctrl+Z 키바인드의 진입점.
   *
   * 동작:
   *  1. mutation guard
   *  2. historyStore.popUndo(currentSnapshot) → PRE-state
   *  3. mutateLayout(() => pre) — full snapshot PUT
   *  4. 성공 시 loadLayout, currentSnapshot 은 popUndo 가 redo 에 push
   *  5. 실패 시 D9 — 양 stack reset + toast
   *
   * Undo 자체는 captureHistory:false — 그렇지 않으면 undo→stack push→…→cycle.
   */
  async undo(): Promise<void> {
    const active = this.active;
    if (active === null) return;
    const guard = await this.guardOutgoingMutation();
    if (!guard.ok) return;
    const current = this.layoutSnapshot();
    const pre = historyStore.popUndo(active.name, current);
    if (pre === null) return;
    try {
      const { layout } = await mutateLayout(active.name, () => pre);
      this.loadLayout(layout);
      // ADR-0028 D13 (2026-05-21) — restore 후 unmatched terminal 자동 spawn.
      // Panel+Terminal delete 후 undo 시 killed terminal UUID 가 layout 으로
      // 돌아오지만 BE pool 엔 없음 → empty + Restart 회피 + 새로고침 시 dialog
      // 회피. attachConfirm 의 unmatched-spawn 분기 자연 활용.
      void this.respawnUnmatched(active.name);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      // D9 — etag mismatch (rebase 도 fail) 또는 D1.2 unmatched terminal.
      historyStore.reset(active.name);
      const reason =
        err instanceof EtagMismatchError
          ? 'layout changed by another source'
          : err instanceof Error
            ? err.message
            : String(err);
      toastStore.show({
        message: `Cannot undo — ${reason}. History cleared.`,
        tone: 'warning',
      });
    }
  }

  /**
   * ADR-0028 D13 (2026-05-21) — undo/redo restore 후 layout 에 unmatched
   * terminal (UUID 는 layout 에 있는데 terminalPool 엔 없는 상태) 가 있으면
   * attachConfirm 으로 자동 spawn. paste / spawnMultiSessionTerminal 패턴 정합
   * — dialog 없이 즉시 spawn. spawned UUID 들의 dangling 마킹도 clear.
   */
  async respawnUnmatched(name: string): Promise<void> {
    const needsSpawn = [...this.items.values()].some(
      (it) => it.type === 'terminal' && terminalPool.byId(it.id) === null,
    );
    if (!needsSpawn) return;
    try {
      const res = await attachConfirm(name);
      for (const id of res.spawned) {
        danglingTerminals.clear(id);
      }
      void terminalPool.refresh();
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Terminal restore failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  /** ADR-0028 D8 — redo 1 step. Cmd+Shift+Z / Ctrl+Y 의 진입점. */
  async redo(): Promise<void> {
    const active = this.active;
    if (active === null) return;
    const guard = await this.guardOutgoingMutation();
    if (!guard.ok) return;
    const current = this.layoutSnapshot();
    const next = historyStore.popRedo(active.name, current);
    if (next === null) return;
    try {
      const { layout } = await mutateLayout(active.name, () => next);
      this.loadLayout(layout);
      // ADR-0028 D13 (2026-05-21) — restore 후 unmatched terminal 자동 spawn (undo 와 동일).
      void this.respawnUnmatched(active.name);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      historyStore.reset(active.name);
      const reason =
        err instanceof EtagMismatchError
          ? 'layout changed by another source'
          : err instanceof Error
            ? err.message
            : String(err);
      toastStore.show({
        message: `Cannot redo — ${reason}. History cleared.`,
        tone: 'warning',
      });
    }
  }
}

/** Single session-scoped store instance. */
export const sessionStore = new SessionStore();

/**
 * `ensureMutationOk` — Phase 2 mutation guard 의 사용자-facing wrapper.
 *
 * 모든 outgoing write 의 *진입점* 에서 await 한 후 false 면 early return
 * 패턴. 기본 toast 가 portable 하므로 site 별 reword 필요 없음 — site-specific
 * 메시지가 필요한 경우 `abortMessage` 로 override.
 *
 * 사용:
 *   if (!(await ensureMutationOk('Drag commit aborted.'))) return;
 *   await mutateLayout(...);
 */
export async function ensureMutationOk(abortMessage?: string): Promise<boolean> {
  const guard = await sessionStore.guardOutgoingMutation();
  if (!guard.ok) {
    toastStore.show({
      message:
        (abortMessage ??
          'Session reconnect failed — action aborted.') +
        ' Try refreshing the page or use Switch session… in the menu.',
      tone: 'error',
      durationMs: 6_000,
    });
    return false;
  }
  return true;
}
