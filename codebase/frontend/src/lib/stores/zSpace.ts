// zSpace — group-scoped consecutive z-space model 의 pure helpers.
//
// 정본:
// - ADR-0024 신 D3' (Group 은 z field 없음 + 자손은 consecutive z-range 점유)
// - ADR-0024 D9~D17 (multi-z atomic block + boundary + normalize + drag-reparent z)
// - plan-0012 §3.1 A.5 (zStore + normalize 알고리즘)
// - SSoT canvas-layout-schema.md §2 (group id pattern `^g...`) + §5.1 (invariants)
//
// 책임:
// - layout 의 모든 atomic block 의 부모-자식 트리 구축 (`buildChildBlocks`)
// - 한 부모 level 의 block order 결정 (`blocksAtParent`)
// - layout 의 z 를 consecutive 정합으로 정규화 (`normalizeLayout`)
// - 4 z 액션의 transform (`applyZOperation` + `canApplyZOp`)
//
// 본 모듈은 *순수 함수* — Svelte runtime / store 의존 0. 모든 mutation 은
// CanvasLayout 입력 → 새 CanvasLayout 반환. caller (zStore / sessionStore) 가
// 결과를 applyMutation / loadLayout 으로 store 에 반영.
//
// **id format 정합** (plan-0012 §3.1 A.1 + handover-2026-05-22 §A.1 amend):
// group + item id 가 모두 표준 UUID 8-4-4-4-12 hex format 공유 — prefix 로 구분
// 불가. group 식별은 `layout.groups.find()` (또는 live store 의 `groups.has()`).
// 출처: BE `crates/http-api/src/schema.rs:843` 의 `is_uuid_shape()` hex-only 검증.

import type { CanvasLayout } from '$lib/types/canvas';

/** Atomic block — z-space 의 단위. */
export type BlockKind = 'item' | 'group';

export interface AtomicBlock {
  /** item id 또는 group id. */
  id: string;
  kind: BlockKind;
}

function addBlock(
  m: Map<string | null, AtomicBlock[]>,
  parentId: string | null,
  block: AtomicBlock,
): void {
  let arr = m.get(parentId);
  if (!arr) {
    arr = [];
    m.set(parentId, arr);
  }
  arr.push(block);
}

/**
 * Layout 의 모든 부모-자식 atomic block 트리를 구축.
 *
 * Key = `parent_id` (null = canvas root). Value = 자손 atomic block 들.
 * 각 list 는 *현재 최소 z 기준 오름차순* 정렬 — group block 의 min z 는 자손
 * item 의 min z (재귀).
 */
export function buildChildBlocks(
  layout: CanvasLayout,
): Map<string | null, AtomicBlock[]> {
  const out = new Map<string | null, AtomicBlock[]>();
  for (const it of layout.items) {
    addBlock(out, it.parent_id, { id: it.id, kind: 'item' });
  }
  for (const g of layout.groups) {
    addBlock(out, g.parent_id, { id: g.id, kind: 'group' });
  }
  const itemsById = new Map(layout.items.map((it) => [it.id, it] as const));
  // childrenOf 는 트리 walk 의 cache (graph traversal 의 O(n) 시간 보장).
  const childrenOf = new Map<string, AtomicBlock[]>();
  for (const g of layout.groups) {
    if (!childrenOf.has(g.id)) childrenOf.set(g.id, []);
  }
  for (const it of layout.items) {
    if (it.parent_id !== null) {
      let arr = childrenOf.get(it.parent_id);
      if (!arr) {
        arr = [];
        childrenOf.set(it.parent_id, arr);
      }
      arr.push({ id: it.id, kind: 'item' });
    }
  }
  for (const g of layout.groups) {
    if (g.parent_id !== null) {
      let arr = childrenOf.get(g.parent_id);
      if (!arr) {
        arr = [];
        childrenOf.set(g.parent_id, arr);
      }
      arr.push({ id: g.id, kind: 'group' });
    }
  }
  const groupMinZ = new Map<string, number>();
  function minZ(block: AtomicBlock): number {
    if (block.kind === 'item') return itemsById.get(block.id)?.z ?? 0;
    const cached = groupMinZ.get(block.id);
    if (cached !== undefined) return cached;
    let min = Number.POSITIVE_INFINITY;
    const stack: string[] = [block.id];
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
    const result = Number.isFinite(min) ? min : 0;
    groupMinZ.set(block.id, result);
    return result;
  }
  for (const arr of out.values()) {
    arr.sort((a, b) => minZ(a) - minZ(b));
  }
  return out;
}

/** 한 parent 의 모든 직속 atomic block (현재 z 기준 오름차순). */
export function blocksAtParent(
  layout: CanvasLayout,
  parentId: string | null,
): AtomicBlock[] {
  return buildChildBlocks(layout).get(parentId) ?? [];
}

/**
 * Layout 의 z 를 consecutive integer 로 정합 (ADR-0024 신 D3' invariant).
 *
 * 알고리즘: root (null) 부터 depth-first 로 atomic block 들을 순회하면서 cursor
 * (0 부터 시작) 를 자손 item 마다 1씩 증가. Group block 은 z 값 자체 없음
 * (재귀 호출만) — group 의 effective z-range = 그 안 자손의 [first, last] cursor.
 *
 * `orderOverrides` 가 주어지면 해당 parent 의 block order 를 그 list 로 강제.
 * 미지정 parent 는 현재 z 기준 정렬 그대로.
 *
 * Idempotent — 이미 invariant 만족인 layout 이면 z 값 변경 0건.
 *
 * 비용: O(items.length + groups.length * avg_descendant_walk). 50 item 기준 ms.
 */
export function normalizeLayout(
  layout: CanvasLayout,
  orderOverrides?: Map<string | null, readonly string[]>,
): CanvasLayout {
  const childBlocks = buildChildBlocks(layout);
  if (orderOverrides) {
    for (const [parentId, newOrder] of orderOverrides) {
      const arr = childBlocks.get(parentId) ?? [];
      const indexMap = new Map(newOrder.map((id, i) => [id, i] as const));
      const sorted = arr.slice().sort((a, b) => {
        const ai = indexMap.get(a.id);
        const bi = indexMap.get(b.id);
        if (ai !== undefined && bi !== undefined) return ai - bi;
        if (ai === undefined && bi === undefined) return 0;
        // unknown id (defensive — caller 의 override 가 일부 block 만 명시한 경우)
        return ai === undefined ? 1 : -1;
      });
      childBlocks.set(parentId, sorted);
    }
  }
  const newItemZ = new Map<string, number>();
  let cursor = 0;
  function recurse(parentId: string | null): void {
    const arr = childBlocks.get(parentId) ?? [];
    for (const block of arr) {
      if (block.kind === 'item') {
        newItemZ.set(block.id, cursor);
        cursor += 1;
      } else {
        recurse(block.id);
      }
    }
  }
  recurse(null);
  let changed = false;
  const newItems = layout.items.map((it) => {
    const z = newItemZ.get(it.id);
    if (z !== undefined && z !== it.z) {
      changed = true;
      return { ...it, z };
    }
    return it;
  });
  return changed ? { ...layout, items: newItems } : layout;
}

/**
 * id (item 또는 group) 의 parent_id. id 가 layout 에 없으면 `undefined`.
 *
 * group + item id 가 같은 UUID format 이라 prefix 구분 불가 — 두 array 모두
 * lookup 후 first match. id 가 disjoint (양쪽에 동시에 존재할 수 없음) 이므로
 * 순서 무관.
 */
export function parentOf(
  layout: CanvasLayout,
  id: string,
): string | null | undefined {
  const g = layout.groups.find((g) => g.id === id);
  if (g !== undefined) return g.parent_id;
  const it = layout.items.find((it) => it.id === id);
  return it?.parent_id;
}

/** id 가 layout 에 존재하는지. */
export function existsIn(layout: CanvasLayout, id: string): boolean {
  return parentOf(layout, id) !== undefined;
}

/* ────────────────────────────────────────────────────────────────────────── */
/* 4 z 액션 (multi atomic block batch)                                        */
/* ────────────────────────────────────────────────────────────────────────── */

export type ZOperation = 'front' | 'back' | 'forward' | 'backward';

/**
 * ADR-0024 D9 + D12 + D14 — multi-select z mutation 의 단일 transform.
 *
 * M 의 atomic block 들 (super-block) 을 부모 z-range 안에서:
 * - `'front'` → top (max z) 으로 일괄 이동.
 * - `'back'` → bottom (min z) 으로 일괄 이동.
 * - `'forward'` → 한 atomic block 위로 swap (super-block 단위).
 * - `'backward'` → 한 atomic block 아래로 swap.
 *
 * **Same-parent 제약** (plan-0012 §3.1 A.5): ids 의 모든 block 의 parent_id 가
 * 같아야 함. mixed parent 면 본 helper 는 `null` 반환 (caller 가 사용자 안내).
 *
 * 반환:
 * - 성공 → 정규화된 새 CanvasLayout.
 * - boundary / 동일 결과 / invalid input → `null` (caller 가 noop 분기).
 */
export function applyZOperation(
  layout: CanvasLayout,
  ids: readonly string[],
  op: ZOperation,
): CanvasLayout | null {
  const validIds = ids.filter((id) => existsIn(layout, id));
  if (validIds.length === 0) return null;
  const parents = new Set<string | null>();
  for (const id of validIds) parents.add(parentOf(layout, id) as string | null);
  if (parents.size > 1) return null;
  const parentId = [...parents][0] as string | null;
  const current = blocksAtParent(layout, parentId).map((b) => b.id);
  const movedSet = new Set(validIds);
  const movedOrdered = current.filter((id) => movedSet.has(id));
  const nonM = current.filter((id) => !movedSet.has(id));
  if (movedOrdered.length === 0) return null;
  if (nonM.length === 0) return null;

  let newOrder: string[];
  switch (op) {
    case 'front':
      newOrder = [...nonM, ...movedOrdered];
      break;
    case 'back':
      newOrder = [...movedOrdered, ...nonM];
      break;
    case 'forward': {
      const slot = countNonMRefSlot(current, movedSet, true);
      if (slot >= nonM.length) return null;
      newOrder = [
        ...nonM.slice(0, slot + 1),
        ...movedOrdered,
        ...nonM.slice(slot + 1),
      ];
      break;
    }
    case 'backward': {
      const slot = countNonMRefSlot(current, movedSet, false);
      if (slot <= 0) return null;
      newOrder = [
        ...nonM.slice(0, slot - 1),
        ...movedOrdered,
        ...nonM.slice(slot - 1),
      ];
      break;
    }
  }
  if (sameOrder(current, newOrder)) return null;
  const overrides = new Map<string | null, readonly string[]>();
  overrides.set(parentId, newOrder);
  return normalizeLayout(layout, overrides);
}

/** Boundary check — 본 op 가 actually noop 인지 여부. */
export function canApplyZOp(
  layout: CanvasLayout,
  ids: readonly string[],
  op: ZOperation,
): boolean {
  return applyZOperation(layout, ids, op) !== null;
}

/**
 * Super-block 의 "현 slot" 계산 — 같은 부모 안 non-M block 중 *기준 M block 보다
 * 아래* (z 작음) 인 개수.
 *
 * - `maxRef=true` (forward) → 기준 = M 중 최상위 (max z).
 * - `maxRef=false` (backward) → 기준 = M 중 최하위 (min z).
 *
 * Slot = K 면 super-block 은 "non-M block K 개 위에 위치". forward = slot+1,
 * backward = slot-1.
 */
function countNonMRefSlot(
  current: readonly string[],
  movedSet: ReadonlySet<string>,
  maxRef: boolean,
): number {
  let refPos = -1;
  if (maxRef) {
    for (let i = current.length - 1; i >= 0; i--) {
      const id = current[i];
      if (id !== undefined && movedSet.has(id)) {
        refPos = i;
        break;
      }
    }
  } else {
    for (let i = 0; i < current.length; i++) {
      const id = current[i];
      if (id !== undefined && movedSet.has(id)) {
        refPos = i;
        break;
      }
    }
  }
  if (refPos === -1) return 0;
  let count = 0;
  for (let i = 0; i < refPos; i++) {
    const id = current[i];
    if (id !== undefined && !movedSet.has(id)) count += 1;
  }
  return count;
}

function sameOrder(a: readonly string[], b: readonly string[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}
