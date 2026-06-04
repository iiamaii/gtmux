// editingShortcuts — 기본 편집 단축키 wire (ADR-0017 D6 amend ⑤/⑥/⑦/⑧).
//
// 정본:
// - ADR-0017 D6 amend ⑦ (a) — Cmd/Ctrl+A: canvas + LayerTreeView focus 모두
//   sessionStore.M 에 현재 drill scope 의 visible direct elements 전체 set.
//   Root scope 는 top-level item/group 만, drill-in scope 는 해당 group 의 direct
//   child item/group 만 선택. xterm/editable focus 는 OS default 로 routing.
// - ADR-0010 D14 / D21 / D22.5 + plan-0013 — sessionStore.M 은 **group id 도
//   포함 가능** (Cmd+G 직후 / drill-in plain click / sidebar grouped item click).
//   본 wire 의 batch handler 는 item + group 모두 분기 — Lock / Hide toggle 은
//   group 의 self.locked / self.visibility 도 동일 패턴으로 갱신.
//   Nudge 는 group id 를 descendant item 으로 materialize 하며, group 자체는
//   frame 이 없으므로 좌표를 저장하지 않는다.
// - ADR-0017 D6 amend ⑧ (Arrow nudge) — `↑↓←→` 1px / `Shift+↑↓←→` 8px /
//   `Cmd|Ctrl+↑↓←→` 64px. M.size ≥ 1, locked 제외, 250ms idle debounce
//   (nudgeBuffer).
// - ADR-0030 D11 — Cmd/Ctrl+D Duplicate. Clipboard 미오염, paste 와 동일 (24, 24)
//   offset.

import { chromeStore } from '$lib/stores/chrome.svelte';
import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { pasteItems, materializeSelection } from '$lib/canvas/clipboardOps.svelte';
import { effectiveLocked } from '$lib/types/group';
import type { ClipboardPayload } from '$lib/stores/clipboardStore.svelte';
import { nudgeBuffer } from './nudgeBuffer.svelte';
import { shortcutRegistry } from './shortcutRegistry.svelte';

function selectAllVisible(): boolean {
  if (chromeStore.state.leftPanelTab === 'files') return false;
  return sessionStore.selectAllVisibleAtDrillScope();
}

function filterDuplicatePayload(payload: ClipboardPayload): ClipboardPayload {
  const items = payload.items.filter(
    (it) => !effectiveLocked(it.locked, it.parent_id, sessionStore.groups),
  );
  if (payload.groups.length === 0) return { items, groups: [] };

  const payloadGroupIds = new Set(payload.groups.map((g) => g.id));
  const keepGroupIds = new Set<string>();
  for (const item of items) {
    let parentId = item.parent_id;
    while (parentId !== null && payloadGroupIds.has(parentId)) {
      keepGroupIds.add(parentId);
      parentId = sessionStore.groups.get(parentId)?.parent_id ?? null;
    }
  }

  return {
    items,
    groups: payload.groups.filter((g) => keepGroupIds.has(g.id)),
  };
}

function doDuplicate(): boolean {
  // ADR-0030 D11 + D12.6 — clipboard state 변경 0, paste 와 동일 (24, 24) offset.
  // Group entity 가 M 에 있으면 자손 sub-tree 까지 materializeSelection 으로
  // 확장 (D12.1). locked item 은 source 에서 제외 (D11 + D12.6).
  // 2026-06-03 정정: group ancestor locked 도 source 제외로 전파하고, 남은
  // movable item 을 포함하는 group shell 만 유지한다.
  if (sessionStore.active === null) return false;
  const payload = filterDuplicatePayload(
    materializeSelection(
      sessionStore.M,
      sessionStore.items,
      sessionStore.groups,
    ),
  );
  if (payload.items.length === 0 && payload.groups.length === 0) return false;
  void pasteItems(
    payload.items,
    payload.groups,
    { offset: { dx: 24, dy: 24 }, failMessage: 'Duplicate failed' },
  );
  return true;
}

type ArrowKey = 'ArrowUp' | 'ArrowDown' | 'ArrowLeft' | 'ArrowRight';
type ArrowDirection = 'up' | 'down' | 'left' | 'right';

function dirDelta(key: ArrowKey, step: number): { dx: number; dy: number } {
  switch (key) {
    case 'ArrowUp':
      return { dx: 0, dy: -step };
    case 'ArrowDown':
      return { dx: 0, dy: step };
    case 'ArrowLeft':
      return { dx: -step, dy: 0 };
    case 'ArrowRight':
      return { dx: step, dy: 0 };
  }
}

function doNudge(key: ArrowKey, step: number): boolean {
  const ids = selectionIds();
  if (ids.length === 0) return false;
  const { dx, dy } = dirDelta(key, step);
  nudgeBuffer.tick(ids, dx, dy);
  return true;
}

function arrowDirection(key: ArrowKey): ArrowDirection {
  switch (key) {
    case 'ArrowUp': return 'up';
    case 'ArrowDown': return 'down';
    case 'ArrowLeft': return 'left';
    case 'ArrowRight': return 'right';
  }
}

/** Selected items + groups 의 batch toggle 동작 (ADR-0017 D6 amend ⑨ + plan-0013).
 *  M 에 group + item mixed 시 각자 분기 — items.set 으로 갱신, groups.set 으로 갱신. */
function selectionIds(): string[] {
  const out: string[] = [];
  for (const id of sessionStore.M) {
    if (sessionStore.items.has(id) || sessionStore.groups.has(id)) out.push(id);
  }
  return out;
}

interface LockVisibilityState {
  locked: boolean;
  visibility: 'visible' | 'hidden';
}

function lookupSelfState(id: string): LockVisibilityState | null {
  const it = sessionStore.items.get(id);
  if (it !== undefined) return { locked: it.locked, visibility: it.visibility };
  const g = sessionStore.groups.get(id);
  if (g !== undefined) return { locked: g.locked, visibility: g.visibility };
  return null;
}

/**
 * Lock toggle (batch) — ids 미지정 시 sessionStore.M 의 모든 element (item + group).
 * Figma 패턴: all locked → 모두 unlock, 그 외 (mixed / all unlocked) → 모두 lock.
 * Group 의 self.locked 갱신은 ADR-0010 D6 의 effective locked 전파 (OR) 자연 적용.
 */
function doToggleLock(ids?: readonly string[]): boolean {
  if (sessionStore.active === null) return false;
  const effective = ids === undefined ? selectionIds() : ids;
  if (effective.length === 0) return false;
  const states = effective
    .map((id) => lookupSelfState(id))
    .filter((s): s is LockVisibilityState => s !== null);
  if (states.length === 0) return false;
  const allLocked = states.every((s) => s.locked === true);
  const nextLocked = !allLocked;
  const idSet = new Set(effective);
  void sessionStore.applyMutation(
    (cur) => ({
      ...cur,
      items: cur.items.map((it) => (idSet.has(it.id) ? { ...it, locked: nextLocked } : it)),
      groups: cur.groups.map((g) => (idSet.has(g.id) ? { ...g, locked: nextLocked } : g)),
    }),
    { failMessage: 'Lock toggle failed' },
  );
  return true;
}

/**
 * Visibility toggle (batch) — ids 미지정 시 sessionStore.M 의 모든 element.
 * Figma 패턴: all hidden → 모두 visible, 그 외 → 모두 hide.
 * Group 의 self.visibility 갱신은 ADR-0010 D6 의 effective visibility 전파 (AND) 자연 적용.
 */
function doToggleVisibility(ids?: readonly string[]): boolean {
  if (sessionStore.active === null) return false;
  const effective = ids === undefined ? selectionIds() : ids;
  if (effective.length === 0) return false;
  const states = effective
    .map((id) => lookupSelfState(id))
    .filter((s): s is LockVisibilityState => s !== null);
  if (states.length === 0) return false;
  const allHidden = states.every((s) => s.visibility === 'hidden');
  const nextVisibility: 'visible' | 'hidden' = allHidden ? 'visible' : 'hidden';
  const idSet = new Set(effective);
  void sessionStore.applyMutation(
    (cur) => ({
      ...cur,
      items: cur.items.map((it) =>
        idSet.has(it.id) ? { ...it, visibility: nextVisibility } : it,
      ),
      groups: cur.groups.map((g) =>
        idSet.has(g.id) ? { ...g, visibility: nextVisibility } : g,
      ),
    }),
    { failMessage: 'Visibility toggle failed' },
  );
  return true;
}

export { doToggleLock, doToggleVisibility };

const NUDGE_PX = 1;
const NUDGE_SHIFT_PX = 8;
const NUDGE_LARGE_PX = 64;

export function bindEditingShortcuts(): () => void {
  const unsubs: Array<() => void> = [];

  const reg = (
    descriptor: Parameters<typeof shortcutRegistry.register>[0],
  ): void => {
    unsubs.push(
      shortcutRegistry.register({
        category: 'Edit',
        allowInEditable: false,
        allowInXterm: false,
        ...descriptor,
      }),
    );
  };

  // Cmd/Ctrl+A — Select visible elements at the current canvas drill level.
  reg({ actionId: 'selection.select_all', key: 'a', meta: true, customizable: true, description: 'Select all visible elements at current level', handler: () => selectAllVisible() });
  reg({ actionId: 'selection.select_all', key: 'a', ctrl: true, customizable: true, description: 'Select all visible elements at current level (Win/Linux)', handler: () => selectAllVisible() });

  // Cmd/Ctrl+D — Duplicate (ADR-0030 D11).
  reg({ actionId: 'selection.duplicate', key: 'd', meta: true, customizable: true, description: 'Duplicate', handler: () => doDuplicate() });
  reg({ actionId: 'selection.duplicate', key: 'd', ctrl: true, customizable: true, description: 'Duplicate (Win/Linux)', handler: () => doDuplicate() });

  // Cmd/Ctrl+L — Lock toggle (D6 amend ⑦).
  reg({ actionId: 'selection.toggle_lock', key: 'l', meta: true, customizable: true, description: 'Lock toggle (selection)', handler: () => doToggleLock() });
  reg({ actionId: 'selection.toggle_lock', key: 'l', ctrl: true, customizable: true, description: 'Lock toggle (Win/Linux)', handler: () => doToggleLock() });

  // Cmd/Ctrl+Shift+H — Hide toggle (D6 amend ⑦).
  reg({ actionId: 'selection.toggle_visibility', key: 'h', meta: true, shift: true, customizable: true, description: 'Hide toggle (selection)', handler: () => doToggleVisibility() });
  reg({ actionId: 'selection.toggle_visibility', key: 'h', ctrl: true, shift: true, customizable: true, description: 'Hide toggle (Win/Linux)', handler: () => doToggleVisibility() });

  // Arrow nudge — 4 direction × 3 modifier (D6 amend ⑥).
  const dirs: ArrowKey[] = ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'];
  for (const key of dirs) {
    const label = arrowDirection(key);
    const protectedReason = 'Directional nudge has a dense repeated binding matrix; custom support is deferred.';
    reg({ actionId: `selection.nudge.${label}.small`, key, customizable: false, protectedReason, description: `Nudge ${label} 1px`, handler: () => doNudge(key, NUDGE_PX) });
    reg({ actionId: `selection.nudge.${label}.medium`, key, shift: true, customizable: false, protectedReason, description: `Nudge ${label} 8px`, handler: () => doNudge(key, NUDGE_SHIFT_PX) });
    reg({ actionId: `selection.nudge.${label}.large`, key, meta: true, customizable: false, protectedReason, description: `Nudge ${label} 64px`, handler: () => doNudge(key, NUDGE_LARGE_PX) });
    reg({ actionId: `selection.nudge.${label}.large`, key, ctrl: true, customizable: false, protectedReason, description: `Nudge ${label} 64px (Win/Linux)`, handler: () => doNudge(key, NUDGE_LARGE_PX) });
  }

  return () => {
    for (const fn of unsubs) fn();
  };
}
