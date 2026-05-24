// clipboardShortcuts — Cmd/Ctrl + C / X / V wire (ADR-0030 D5 / D7 + D12 amend ③).
//
// 정본:
// - ADR-0030 D3 — terminal paste = clone (fresh UUID → BE 의 unmatched-spawn 분기 자연 활용)
// - ADR-0030 D4 — paste 좌표: bbox top-left + (24,24)*pasteCount, 상대 위치 보존
// - ADR-0030 D5 — Cut = Copy + applyDeletion(kill=false), locked 제외
// - ADR-0030 D6 — paste 시 새 UUID
// - ADR-0030 D7 — Focus 분기: editable / xterm focus 시 OS default 우선
// - ADR-0030 D9 — applyMutation 통과 → historyStore 자동 capture (1 PUT = 1 entry)
// - ADR-0030 D12 amend ③ (2026-05-25) — Group entity 가 M 에 있으면 자손 sub-tree
//   까지 materialize (D12.1) + cut 의 destructive 는 자손 items 만 deleteItem,
//   group entity 는 pruneEmptyGroups 자동 정리.
// - ADR-0017 D6 amend ⑦ (b) — 본 wire 의 매트릭스 cross-link

import type { CanvasItem } from '$lib/types/canvas';
import { clipboardStore, type ClipboardPayload } from '$lib/stores/clipboardStore.svelte';
import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { panelCloseDialog } from '$lib/stores/panelCloseDialog.svelte';
import { pasteItems, materializeSelection } from '$lib/canvas/clipboardOps.svelte';
import { effectiveLocked } from '$lib/types/group';
import { shortcutRegistry } from './shortcutRegistry.svelte';

function selectionPayload(): ClipboardPayload {
  return materializeSelection(sessionStore.M, sessionStore.items, sessionStore.groups);
}

function filterMutablePayload(payload: ClipboardPayload): ClipboardPayload {
  // ADR-0030 D5 + D12.6 — locked 제외. group 의 effective locked 면 자손도 제외.
  // 단순화: 자손 item 의 effective locked 만 체크 (group entity 는 자손이 모두
  // 제거되면 pruneEmptyGroups 가 자동 cleanup, 따로 검사 불필요).
  const mutableItems = payload.items.filter(
    (it) => !effectiveLocked(it.locked, it.parent_id, sessionStore.groups),
  );
  return { items: mutableItems, groups: payload.groups };
}

function doCopy(): boolean {
  const payload = selectionPayload();
  if (payload.items.length === 0 && payload.groups.length === 0) return false;
  clipboardStore.copy(payload);
  return true;
}

function doCut(): boolean {
  const payload = filterMutablePayload(selectionPayload());
  if (payload.items.length === 0 && payload.groups.length === 0) return false;
  clipboardStore.cut(payload);

  // ADR-0030 D12.7 — group cut destructive 는 자손 items 만 deleteItem API 로
  // 보내고, group entity 는 sessionStore.applyDeletion 의 pruneEmptyGroups path
  // (sessionStore.svelte.ts:1350/1393) 가 자동 cleanup.
  const itemTargets: CanvasItem[] = [...payload.items];
  if (itemTargets.length === 0) {
    // group only selection — 자손 item 이 없으면 destructive 도 없음 (빈 group 는
    // ADR-0010 D4 에 의해 존재하지 않으므로 실질 도달 불가).
    return true;
  }

  // ADR-0032 Amend ⑥ — terminal 포함 시 PanelCloseConfirmModal 경유.
  panelCloseDialog.show({
    items: itemTargets,
    onConfirm: async (killTerminal) => {
      const ids = itemTargets.map((it) => it.id);
      await sessionStore.applyDeletion(ids, { killTerminal });
    },
  });
  return true;
}

function doPaste(): boolean {
  if (!clipboardStore.hasItems) return false;
  if (sessionStore.active === null) return false;
  const offset = clipboardStore.consumePasteOffset();
  void pasteItems(clipboardStore.entries, clipboardStore.groups, {
    offset,
    failMessage: 'Paste failed',
  });
  return true;
}

export function bindClipboardShortcuts(): () => void {
  const unsubs: Array<() => void> = [];

  const register = (
    actionId: string,
    key: string,
    modifier: 'meta' | 'ctrl',
    description: string,
    run: () => boolean,
  ): void => {
    unsubs.push(
      shortcutRegistry.register({
        actionId,
        key,
        meta: modifier === 'meta',
        ctrl: modifier === 'ctrl',
        description,
        category: 'Edit',
        customizable: true,
        // ADR-0030 D7 — editable / xterm focus 시 OS default 우선 (registry skip).
        allowInEditable: false,
        allowInXterm: false,
        handler: () => run(),
      }),
    );
  };

  register('selection.copy', 'c', 'meta', 'Copy', doCopy);
  register('selection.copy', 'c', 'ctrl', 'Copy (Win/Linux)', doCopy);
  register('selection.cut', 'x', 'meta', 'Cut', doCut);
  register('selection.cut', 'x', 'ctrl', 'Cut (Win/Linux)', doCut);
  register('selection.paste', 'v', 'meta', 'Paste', doPaste);
  register('selection.paste', 'v', 'ctrl', 'Paste (Win/Linux)', doPaste);

  return () => {
    for (const fn of unsubs) fn();
  };
}
