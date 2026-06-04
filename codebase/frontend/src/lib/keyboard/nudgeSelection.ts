import type { CanvasItem } from '$lib/types/canvas';
import type { Group } from '$lib/types/group';
import { descendantItems, effectiveLocked } from '$lib/types/group';

/**
 * Resolve a manipulation selection into the concrete items that keyboard nudge
 * can move. Group entities have no frame, so they materialize to movable
 * descendant items. Locked items and items under a locked ancestor are skipped.
 */
export function materializeNudgeItemIds(
  selectedIds: Iterable<string>,
  itemsMap: ReadonlyMap<string, CanvasItem>,
  groupsMap: Map<string, Group>,
): string[] {
  const out = new Set<string>();
  const groupsArr = [...groupsMap.values()];
  const itemsArr = [...itemsMap.values()];

  for (const id of selectedIds) {
    if (itemsMap.has(id)) {
      out.add(id);
      continue;
    }
    if (!groupsMap.has(id)) continue;
    for (const item of descendantItems(id, groupsArr, itemsArr)) {
      out.add(item.id);
    }
  }

  return [...out].filter((id) => {
    const item = itemsMap.get(id);
    if (item === undefined) return false;
    return !effectiveLocked(item.locked, item.parent_id, groupsMap);
  });
}
