// Group type + propagation helpers (ADR-0010 + ADR-0024).
//
// 정본:
// - ADR-0010 D1~D13 (G-hybrid: pure organization, no frame storage)
// - ADR-0010 D6 (propagation: visibility AND / locked OR)
// - ADR-0010 D12 (Ungroup 비파괴 — Group 만 삭제, 자손 보존)
// - ADR-0010 D13 (multi-session session-local 적용)
// - ADR-0024 D3 (Group 은 z field 없음 — flat global z space)
//
// 어휘:
// - Group = 자식들 묶음 (트리). 자체 상태 = label/color/visibility/locked/order.
// - effective state = self + ancestor 전파 결과.

import type { CanvasItem, CanvasLayout, Visibility } from './canvas';

/* ────────────────────────────────────────────────────────────────────────── */
/* Group entity (ADR-0010 SSoT)                                                */
/* ────────────────────────────────────────────────────────────────────────── */

/**
 * Group 자체 상태 — frame (x/y/w/h) 영속 X (D1: G-hybrid).
 *
 * ⚠️ ADR-0024 D3: Group 은 z field 없음 — 모든 items 의 z 는 group 과 무관하게
 * *flat global z space* 공유.
 *
 * Visibility/locked 의 *effective* 값은 ancestor 전파 결과 — `effectiveVisibility`
 * / `effectiveLocked` 헬퍼 참조.
 */
export interface Group {
  /** Stable id. ADR-0010 SSoT pattern `^g[0-9a-zA-Z]{1,32}$`. */
  id: string;
  /** 부모 Group id (트리) 또는 null (Canvas 루트). */
  parent_id: string | null;
  /** 사용자 자유 라벨. null 이면 가장 가까운 ancestor 의 label inherit (D6). */
  label: string | null;
  /** Hex color `#rrggbb`. null 이면 ancestor inherit. */
  color: string | null;
  /** Self visibility — schema v2 string enum 정합 (ADR-0018 D1). */
  visibility: Visibility;
  /** Self locked. effective 는 ancestor 중 하나라도 locked 면 true (D6 OR). */
  locked: boolean;
  /** 형제 노드 내 정렬 키 (Sidebar Layer list). */
  order: number;
}

/* ────────────────────────────────────────────────────────────────────────── */
/* Propagation helpers (ADR-0010 D6)                                          */
/* ────────────────────────────────────────────────────────────────────────── */

/** `Visibility` 를 boolean 으로 narrow — propagation 의 boolean 연산 정합. */
const isVisible = (v: Visibility): boolean => v === 'visible';

/**
 * 한 노드(item 또는 group)의 ancestor chain 을 부모 → 루트 순으로 반환.
 * 사이클은 ADR-0010 D2 + R5 검증으로 차단되어 있다고 가정.
 */
export function getAncestors(parentId: string | null, groupsById: Map<string, Group>): Group[] {
  const out: Group[] = [];
  let cur = parentId;
  while (cur !== null) {
    const g = groupsById.get(cur);
    if (!g) break;
    out.push(g);
    cur = g.parent_id;
  }
  return out;
}

/**
 * Effective visibility — AND 전파 (ADR-0010 D6).
 *
 * self 가 hidden 이거나, 임의 ancestor 가 hidden 이면 effective hidden.
 * 한 ancestor 라도 hidden 이면 전체 자손이 effective hidden.
 */
export function effectiveVisibility(
  selfVisibility: Visibility,
  parentId: string | null,
  groupsById: Map<string, Group>
): boolean {
  if (!isVisible(selfVisibility)) return false;
  for (const a of getAncestors(parentId, groupsById)) {
    if (!isVisible(a.visibility)) return false;
  }
  return true;
}

/**
 * Effective locked — OR 전파 (ADR-0010 D6).
 *
 * self 가 locked 이거나, 임의 ancestor 가 locked 면 effective locked.
 * 잠긴 group 안에 있는 panel 은 자신이 unlocked 여도 effective locked.
 */
export function effectiveLocked(
  selfLocked: boolean,
  parentId: string | null,
  groupsById: Map<string, Group>
): boolean {
  if (selfLocked) return true;
  for (const a of getAncestors(parentId, groupsById)) {
    if (a.locked) return true;
  }
  return false;
}

/**
 * label inherit — ADR-0010 D6.
 *
 * self.label 이 있으면 그 값, 없으면 가장 가까운 ancestor 의 non-null label.
 * 영속화는 self 값만, render 시 inherit.
 */
export function inheritedLabel(
  selfLabel: string | null | undefined,
  parentId: string | null,
  groupsById: Map<string, Group>
): string | null {
  if (selfLabel != null) return selfLabel;
  for (const a of getAncestors(parentId, groupsById)) {
    if (a.label != null) return a.label;
  }
  return null;
}

/** Same rule as `inheritedLabel`, applied to Group.color. */
export function inheritedColor(
  selfColor: string | null | undefined,
  parentId: string | null,
  groupsById: Map<string, Group>
): string | null {
  if (selfColor != null) return selfColor;
  for (const a of getAncestors(parentId, groupsById)) {
    if (a.color != null) return a.color;
  }
  return null;
}

/* ────────────────────────────────────────────────────────────────────────── */
/* Tree walk helpers                                                          */
/* ────────────────────────────────────────────────────────────────────────── */

/**
 * 한 group 의 직속 자식 group 들 (재귀 X).
 * Sidebar Layer list 의 tree render 시 사용.
 */
export function directChildGroups(
  groupId: string | null,
  groups: readonly Group[]
): Group[] {
  return groups
    .filter((g) => g.parent_id === groupId)
    .slice()
    .sort((a, b) => a.order - b.order);
}

/**
 * 한 group 의 직속 자식 item 들 (재귀 X).
 * `groupId === null` 면 Canvas 루트 직속 item.
 */
export function directChildItems(
  groupId: string | null,
  items: readonly CanvasItem[]
): CanvasItem[] {
  return items.filter((it) => it.parent_id === groupId);
}

/**
 * 한 group 의 모든 자손 item (재귀). Group close (D10) / multi-select (D7) /
 * GroupCloseConfirmModal (G25) 의 대상 list 계산에 사용.
 */
export function descendantItems(
  groupId: string,
  groups: readonly Group[],
  items: readonly CanvasItem[]
): CanvasItem[] {
  const result: CanvasItem[] = [];
  const stack: string[] = [groupId];
  while (stack.length > 0) {
    const cur = stack.pop()!;
    for (const it of items) {
      if (it.parent_id === cur) result.push(it);
    }
    for (const g of groups) {
      if (g.parent_id === cur) stack.push(g.id);
    }
  }
  return result;
}

/**
 * 한 group 의 모든 자손 group (재귀, 자기 자신 제외). Ungroup 시 자손 group
 * 의 parent_id 재책정에 사용.
 */
export function descendantGroups(
  groupId: string,
  groups: readonly Group[]
): Group[] {
  const result: Group[] = [];
  const stack: string[] = [groupId];
  while (stack.length > 0) {
    const cur = stack.pop()!;
    for (const g of groups) {
      if (g.parent_id === cur) {
        result.push(g);
        stack.push(g.id);
      }
    }
  }
  return result;
}

/**
 * Remove groups that no longer contain any descendant item.
 *
 * Semantics: a group is kept only if at least one item remains somewhere under
 * its subtree. This recursively removes empty nested groups and ancestors that
 * became empty after item deletion.
 */
export function pruneEmptyGroups(layout: CanvasLayout): CanvasLayout {
  if (layout.groups.length === 0) return layout;
  const keep = new Set<string>();
  const groupsById = new Map(layout.groups.map((g) => [g.id, g] as const));
  for (const it of layout.items) {
    let parentId = it.parent_id;
    const seen = new Set<string>();
    while (parentId !== null && !seen.has(parentId)) {
      seen.add(parentId);
      keep.add(parentId);
      const g = groupsById.get(parentId);
      parentId = g?.parent_id ?? null;
    }
  }
  if (keep.size === layout.groups.length) return layout;
  return {
    ...layout,
    groups: layout.groups.filter((g) => keep.has(g.id)),
  };
}

/* ────────────────────────────────────────────────────────────────────────── */
/* Drill-in helpers (ADR-0010 D21 + plan-0013 §3.1)                          */
/* ────────────────────────────────────────────────────────────────────────── */

/**
 * Item / group 의 *가장 root 쪽* ancestor group id.
 *
 * Walk: id.parent_id → parent.parent_id → ... 중 마지막 non-null group id.
 * - id 가 root 직속 (parent_id === null) → `null` 반환 (drill-in 의미 없음, root level).
 * - id 가 layout 에 없으면 → `null`.
 *
 * Legacy helper for root-scope grouping decisions. Canvas hit testing should use
 * `targetAtDrillLevel` so drill scope and selection stay separate.
 */
export function outermostGroupAncestor(
  id: string,
  items: ReadonlyMap<string, CanvasItem>,
  groups: ReadonlyMap<string, Group>,
): string | null {
  const item = items.get(id);
  let parentId: string | null;
  if (item !== undefined) {
    parentId = item.parent_id;
  } else {
    const g = groups.get(id);
    if (g === undefined) return null;
    parentId = g.parent_id;
  }
  let cur = parentId;
  let outermost: string | null = null;
  while (cur !== null) {
    outermost = cur;
    const g = groups.get(cur);
    if (g === undefined) break;
    cur = g.parent_id;
  }
  return outermost;
}

/**
 * Drill-in 한 단계 inner — 현 M (= `currentSelectionId`) 의 자손 chain 중 *clicked
 * target* 의 ancestor chain 위 한 단계 inner 를 반환.
 *
 * 예 (P ∈ B ∈ A):
 * - currentSelection = A, clicked = P → B (한 단계 inner).
 * - currentSelection = B, clicked = P → P (leaf).
 * - currentSelection = P (leaf) → null (이미 가장 inner).
 * - clicked 가 currentSelection 안 아닌 chain → null.
 */
export function innerGroupOrSelf(
  clickedTargetId: string,
  currentSelectionId: string,
  items: ReadonlyMap<string, CanvasItem>,
  groups: ReadonlyMap<string, Group>,
): string | null {
  // Walk clickedTargetId 의 ancestor chain. cur 이 currentSelectionId 에 도달 직전
  // 의 ancestor 가 정답.
  let cur: string | null = clickedTargetId;
  let prev: string | null = null;
  while (cur !== null && cur !== currentSelectionId) {
    prev = cur;
    const item = items.get(cur);
    const g = groups.get(cur);
    cur = item?.parent_id ?? g?.parent_id ?? null;
  }
  if (cur !== currentSelectionId) return null;
  return prev;
}

function parentIdOf(
  id: string,
  items: ReadonlyMap<string, CanvasItem>,
  groups: ReadonlyMap<string, Group>,
): string | null | undefined {
  const item = items.get(id);
  if (item !== undefined) return item.parent_id;
  const group = groups.get(id);
  if (group !== undefined) return group.parent_id;
  return undefined;
}

/**
 * Canvas hit target at the current drill level.
 *
 * Root scope (`drillRootId === null`) treats the outermost containing group as
 * atomic. Inside a drill root, the hit resolves to the direct child of that
 * group on the clicked element's ancestor chain. This keeps canvas click, drag,
 * and right-click priority aligned with the visible drill level while preserving
 * leaf selection for non-canvas surfaces such as the layer tree.
 */
export function targetAtDrillLevel(
  clickedTargetId: string,
  drillRootId: string | null,
  items: ReadonlyMap<string, CanvasItem>,
  groups: ReadonlyMap<string, Group>,
): string {
  if (drillRootId === null) {
    if (groups.has(clickedTargetId)) return clickedTargetId;
    return outermostGroupAncestor(clickedTargetId, items, groups) ?? clickedTargetId;
  }

  if (clickedTargetId === drillRootId) return drillRootId;

  let cur: string | null = clickedTargetId;
  while (cur !== null) {
    const parentId = parentIdOf(cur, items, groups);
    if (parentId === undefined) break;
    if (parentId === drillRootId) return cur;
    cur = parentId;
  }

  // The click is outside the active drill root. Resolve it as a root-level hit;
  // callers may clear/switch the drill root before applying the selection.
  return outermostGroupAncestor(clickedTargetId, items, groups) ?? clickedTargetId;
}

/**
 * Nearest parent group id for an item or group. Used by non-canvas tree
 * selection to enter the containing drill scope while selecting the exact row.
 */
export function directParentGroupId(
  id: string,
  items: ReadonlyMap<string, CanvasItem>,
  groups: ReadonlyMap<string, Group>,
): string | null {
  return parentIdOf(id, items, groups) ?? null;
}

/**
 * id 의 ancestor group chain — root 가까운 ancestor 부터 leaf parent 까지.
 *
 * 예 (P ∈ B ∈ A, A.parent=null):
 * - id=P → [A, B]
 * - id=B → [A]
 * - id=A → []
 *
 * Inspector breadcrumb (D22.7) + Esc drill-out 의 계산 baseline.
 */
export function ancestorChain(
  id: string,
  items: ReadonlyMap<string, CanvasItem>,
  groups: ReadonlyMap<string, Group>,
): Group[] {
  const item = items.get(id);
  let parentId: string | null;
  if (item !== undefined) {
    parentId = item.parent_id;
  } else {
    const g = groups.get(id);
    if (g === undefined) return [];
    parentId = g.parent_id;
  }
  const chain: Group[] = [];
  let cur = parentId;
  while (cur !== null) {
    const g = groups.get(cur);
    if (g === undefined) break;
    chain.unshift(g); // root 가까운 쪽 first.
    cur = g.parent_id;
  }
  return chain;
}
