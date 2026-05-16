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

import type { CanvasItem, Visibility } from './canvas';

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
