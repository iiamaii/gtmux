// clipboardOps — paste 본체 op (ADR-0030 D4/D6/D9 + D12 amend ③).
//
// 정본:
// - ADR-0030 D3 — terminal paste = clone (fresh UUID → BE unmatched-spawn)
// - ADR-0030 D4 — bbox top-left + (dx, dy), 상대 위치 보존
// - ADR-0030 D6 — 모든 item 의 새 UUID
// - ADR-0030 D9 — applyMutation 통과 → historyStore 자동 capture
// - ADR-0030 D12.1 — Group entity + 자손 sub-tree deep clone
// - ADR-0030 D12.2 — nested group 내부 트리 그대로 재현 (parent_id 동형 재매핑)
// - ADR-0030 D12.3 — mixed selection 동일 level paste
// - ADR-0030 D12.4 — paste 후 M = 새 top-level entry 만 (Figma 패턴)
// - ADR-0030 D12.5 — terminal 자손 sequencing = layout PUT atomic + attachConfirm 순차
// - ADR-0030 D12.8 — undo = layout snapshot 1 entry (ADR-0028 D1.1 정합)
//
// clipboardShortcuts / ContextMenu / (Phase B) editingShortcuts 의 공통 helper.

import type { CanvasItem, CanvasLayout, LineItem } from '$lib/types/canvas';
import type { Group } from '$lib/types/group';
import type { ClipboardPayload } from '$lib/stores/clipboardStore.svelte';
import { descendantItems, descendantGroups } from '$lib/types/group';
import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { terminalPool } from '$lib/stores/terminalPool.svelte';
import { attachConfirm } from '$lib/http/sessions';
import { toastStore } from '$lib/ui/toast-store.svelte';
import { UnauthorizedError } from '$lib/http/sessions';
import { normalizeLayout } from '$lib/stores/zSpace';
import { generateUuidV4 } from '$lib/uuid';

export interface PasteOptions {
  /** 누적 offset (dx, dy). clipboardStore.consumePasteOffset() 결과 또는 Duplicate 의 고정 offset. */
  offset: { dx: number; dy: number };
  /** Paste 후 selection 을 새 item 으로 교체 (default true, Figma 패턴). */
  setSelection?: boolean;
  failMessage?: string;
  /** D12.2 — paste 의 anchor parent group (null = canvas root). 미지정 시 root. */
  anchorParentId?: string | null;
}

/**
 * Materialize a clipboard payload from a selection (M).
 *
 * ADR-0030 D12.1 + ADR-0010 D7 + ADR-0024 D15 의 합성:
 *  - group id 를 만나면 자손 group + 자손 item 까지 sub-tree 전부 포함.
 *  - 자손 panel 이 이미 ids 에 있어도 dedup (group 의 sub-tree 안으로 흡수).
 *  - top-level (selection 의 source pool 안에서 *부모 group 이 pool 에 없는 것*)
 *    은 paste 시 anchor parent 아래 sibling 으로 배치 (D12.3 mixed semantics).
 */
export function materializeSelection(
  ids: Iterable<string>,
  itemsMap: ReadonlyMap<string, CanvasItem>,
  groupsMap: ReadonlyMap<string, Group>,
): ClipboardPayload {
  const idSet = new Set(ids);
  if (idSet.size === 0) return { items: [], groups: [] };

  const groupsArr = [...groupsMap.values()];
  const itemsArr = [...itemsMap.values()];

  const groupIds = new Set<string>();
  const itemIds = new Set<string>();

  for (const id of idSet) {
    if (groupsMap.has(id)) {
      groupIds.add(id);
      for (const dg of descendantGroups(id, groupsArr)) groupIds.add(dg.id);
      for (const di of descendantItems(id, groupsArr, itemsArr)) itemIds.add(di.id);
    } else if (itemsMap.has(id)) {
      itemIds.add(id);
    }
  }

  const items: CanvasItem[] = [];
  for (const id of itemIds) {
    const it = itemsMap.get(id);
    if (it !== undefined) items.push(it);
  }
  const groups: Group[] = [];
  for (const id of groupIds) {
    const g = groupsMap.get(id);
    if (g !== undefined) groups.push(g);
  }
  return { items, groups };
}

/**
 * Paste a clipboard payload (items + group sub-tree).
 *
 * Sequencing (ADR-0030 D12.5):
 *  1. Clone whole sub-tree with fresh UUIDs + parent_id 동형 재매핑.
 *  2. Single applyMutation PUT — atomic layout snapshot (D9 / D12.8).
 *  3. normalizeLayout 으로 group-scoped consecutive z 정합 (ADR-0024).
 *  4. M = 새 top-level entry id (D12.4).
 *  5. Terminal 자손 있으면 attachConfirm 순차 호출 (D3 amend ② / D12.5).
 */
export async function pasteItems(
  sources: readonly CanvasItem[],
  sourceGroups: readonly Group[],
  options: PasteOptions,
): Promise<boolean> {
  if (sources.length === 0 && sourceGroups.length === 0) return false;
  if (sessionStore.active === null) return false;

  const anchorParentId = options.anchorParentId ?? null;
  const cloned = cloneSubtree(sources, sourceGroups, anchorParentId);
  const { dx, dy } = options.offset;

  // offset 의 의미 = 모든 item 의 평행 이동량 (dx, dy). caller 가 bbox 기반
  // anchor 를 사용하려면 (right-click paste) 미리 (flow - bboxX, flow - bboxY)
  // 를 계산해서 넘김. 단일 Cmd+V 의 (24,24) * pasteCount 도 동일 의미.
  const offsetItems = cloned.items.map((it) => {
    const out = { ...it, x: it.x + dx, y: it.y + dy } as CanvasItem;
    if (out.type === 'line') {
      const lineOut = out as LineItem;
      lineOut.x2 = (it as LineItem).x2 + dx;
      lineOut.y2 = (it as LineItem).y2 + dy;
    }
    return out;
  });

  const newItemIds = cloned.items.map((it) => it.id);

  const res = await sessionStore.applyMutation(
    (cur: CanvasLayout) => {
      const interim: CanvasLayout = {
        ...cur,
        items: [...cur.items, ...offsetItems],
        groups: [...cur.groups, ...cloned.groups],
      };
      return normalizeLayout(interim);
    },
    { failMessage: options.failMessage ?? 'Paste failed' },
  );

  if (res.ok && options.setSelection !== false) {
    // D12.4 — M = top-level entry 만. 새 group 이 있으면 자손 panel 은 제외.
    if (cloned.topLevelIds.length > 0) {
      sessionStore.setM(cloned.topLevelIds);
    } else {
      sessionStore.setM(newItemIds);
    }
  }

  // ADR-0030 D3 amend ② / D12.5 — terminal item 포함 시 즉시 BE 의
  // unmatched-spawn 발동. group 자손이어도 동일 path (자손 items 의 type 검사).
  if (res.ok && offsetItems.some((it) => it.type === 'terminal')) {
    const active = sessionStore.active;
    if (active !== null) {
      try {
        const confirmRes = await attachConfirm(active.name);
        if (confirmRes.failed.length > 0) {
          const firstFailed = confirmRes.failed[0];
          if (firstFailed !== undefined) {
            toastStore.show({
              message: `Terminal spawn failed: ${firstFailed.error}`,
              tone: 'error',
            });
          }
        }
        void terminalPool.refresh();
      } catch (err) {
        if (err instanceof UnauthorizedError) {
          window.location.href = '/auth';
          return res.ok;
        }
        toastStore.show({
          message: `Paste spawn failed: ${err instanceof Error ? err.message : String(err)}`,
          tone: 'error',
        });
      }
    }
  }

  return res.ok;
}

interface ClonedSubtree {
  items: CanvasItem[];
  groups: Group[];
  /** D12.4 — paste 후 M 대상 (source pool 의 top-level entry 들의 새 id). */
  topLevelIds: string[];
}

/**
 * Deep clone a (items + groups) sub-tree with fresh UUIDs.
 *
 * ADR-0030 D12.1 + D12.2:
 *  - 모든 group/item id → 새 UUID (Map 으로 매핑 보존).
 *  - parent_id 재매핑: source pool 안 다른 group 이 부모면 그 group 의 새 id,
 *    아니면 anchorParentId (= source pool 의 *top-level* 항목).
 *  - 좌표는 *원본 그대로* 반환 (caller 가 bbox + offset 적용).
 *
 * Invariant (ADR-0030 D3 amend ①): 모든 type 의 모든 field 보존 — id 만 새 UUID.
 * Line 의 x2/y2 는 caller 가 dx/dy 적용.
 */
function cloneSubtree(
  sources: readonly CanvasItem[],
  sourceGroups: readonly Group[],
  anchorParentId: string | null,
): ClonedSubtree {
  // 1. 새 id 매핑 (group + item 둘 다).
  const idMap = new Map<string, string>();
  for (const g of sourceGroups) idMap.set(g.id, generateUuidV4());
  for (const it of sources) idMap.set(it.id, generateUuidV4());

  // 2. source pool 안에 부모가 있는지 판단 (top-level vs nested).
  const sourceGroupIds = new Set(sourceGroups.map((g) => g.id));
  const topLevelIds: string[] = [];

  // 3. Groups clone — parent_id 재매핑.
  const newGroups: Group[] = sourceGroups.map((g) => {
    const cloned = structuredClone($state.snapshot(g)) as Group;
    const newId = idMap.get(g.id)!;
    const parentInPool = g.parent_id !== null && sourceGroupIds.has(g.parent_id);
    const newParentId = parentInPool ? idMap.get(g.parent_id!)! : anchorParentId;
    if (!parentInPool) topLevelIds.push(newId);
    return { ...cloned, id: newId, parent_id: newParentId };
  });

  // 4. Items clone — parent_id 재매핑, line x2/y2 는 caller offset.
  const newItems: CanvasItem[] = sources.map((src) => {
    const cloned = structuredClone($state.snapshot(src)) as CanvasItem;
    const newId = idMap.get(src.id)!;
    const parentInPool = src.parent_id !== null && sourceGroupIds.has(src.parent_id);
    const newParentId = parentInPool ? idMap.get(src.parent_id!)! : anchorParentId;
    if (!parentInPool) topLevelIds.push(newId);
    const base = { ...cloned, id: newId, parent_id: newParentId };
    // ADR-0040 D9: label_auto intentionally differs from the id-only clone rule
    // so pasted text-capable items derive their label on the next text edit.
    return src.type === 'text' || src.type === 'rect' || src.type === 'ellipse'
      ? ({ ...base, label_auto: true } as CanvasItem)
      : (base as CanvasItem);
  });

  return { items: newItems, groups: newGroups, topLevelIds };
}
