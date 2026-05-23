// editingShortcuts — 기본 편집 단축키 wire (ADR-0017 D6 amend ⑤/⑥/⑦/⑧).
//
// 정본:
// - ADR-0017 D6 amend ⑦ (a) — Cmd/Ctrl+A: canvas + LayerTreeView focus 모두
//   sessionStore.M 에 active session 의 visible items 전체 set. xterm/editable
//   focus 는 OS default 로 routing (registry skip).
// - ADR-0010 D14 / D21 / D22.5 + plan-0013 — sessionStore.M 은 **group id 도
//   포함 가능** (Cmd+G 직후 / drill-in plain click / sidebar grouped item click).
//   본 wire 의 batch handler 는 item + group 모두 분기 — Lock / Hide toggle 은
//   group 의 self.locked / self.visibility 도 동일 패턴으로 갱신.
//   Nudge / Duplicate / Cmd+A 같은 *geometry / item-only payload* 동작은 item
//   ids 만 처리 (group 은 frame 없음 — descendant 동작은 별 batch).
// - ADR-0017 D6 amend ⑧ (Arrow nudge) — `↑↓←→` 1px / `Shift+↑↓←→` 8px /
//   `Cmd|Ctrl+↑↓←→` 64px. M.size ≥ 1, locked 제외, 250ms idle debounce
//   (nudgeBuffer).
// - ADR-0030 D11 — Cmd/Ctrl+D Duplicate. Clipboard 미오염, paste 와 동일 (24, 24)
//   offset.

import type { CanvasItem } from '$lib/types/canvas';
import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { pasteItems } from '$lib/canvas/clipboardOps.svelte';
import { nudgeBuffer } from './nudgeBuffer.svelte';
import { shortcutRegistry } from './shortcutRegistry.svelte';

function selectAllVisible(): boolean {
  if (sessionStore.active === null) return false;
  const ids: string[] = [];
  for (const [id, it] of sessionStore.items) {
    if (it.visibility === 'visible') ids.push(id);
  }
  if (ids.length === 0) return false;
  sessionStore.setM(ids);
  return true;
}

function selectionMutableItems(): CanvasItem[] {
  const out: CanvasItem[] = [];
  for (const id of sessionStore.M) {
    const it = sessionStore.items.get(id);
    if (it !== undefined && !it.locked) out.push(it);
  }
  return out;
}

function selectionMutableIds(): string[] {
  return selectionMutableItems().map((it) => it.id);
}

function doDuplicate(): boolean {
  // ADR-0030 D11 — clipboard state 변경 0, paste 와 동일 (24, 24) offset.
  const sources = selectionMutableItems();
  if (sources.length === 0) return false;
  if (sessionStore.active === null) return false;
  void pasteItems(sources, { offset: { dx: 24, dy: 24 }, failMessage: 'Duplicate failed' });
  return true;
}

type ArrowKey = 'ArrowUp' | 'ArrowDown' | 'ArrowLeft' | 'ArrowRight';

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
  const ids = selectionMutableIds();
  if (ids.length === 0) return false;
  const { dx, dy } = dirDelta(key, step);
  nudgeBuffer.tick(ids, dx, dy);
  return true;
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

  // Cmd/Ctrl+A — Select all visible items (D6 amend ⑤ (a)).
  reg({ key: 'a', meta: true, description: 'Select all visible items', handler: () => selectAllVisible() });
  reg({ key: 'a', ctrl: true, description: 'Select all visible items (Win/Linux)', handler: () => selectAllVisible() });

  // Cmd/Ctrl+D — Duplicate (ADR-0030 D11).
  reg({ key: 'd', meta: true, description: 'Duplicate', handler: () => doDuplicate() });
  reg({ key: 'd', ctrl: true, description: 'Duplicate (Win/Linux)', handler: () => doDuplicate() });

  // Cmd/Ctrl+L — Lock toggle (D6 amend ⑦).
  reg({ key: 'l', meta: true, description: 'Lock toggle (selection)', handler: () => doToggleLock() });
  reg({ key: 'l', ctrl: true, description: 'Lock toggle (Win/Linux)', handler: () => doToggleLock() });

  // Cmd/Ctrl+Shift+H — Hide toggle (D6 amend ⑦).
  reg({ key: 'h', meta: true, shift: true, description: 'Hide toggle (selection)', handler: () => doToggleVisibility() });
  reg({ key: 'h', ctrl: true, shift: true, description: 'Hide toggle (Win/Linux)', handler: () => doToggleVisibility() });

  // Arrow nudge — 4 direction × 3 modifier (D6 amend ⑥).
  const dirs: ArrowKey[] = ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'];
  for (const key of dirs) {
    const label = key.slice(5).toLowerCase();
    reg({ key, description: `Nudge ${label} 1px`, handler: () => doNudge(key, NUDGE_PX) });
    reg({ key, shift: true, description: `Nudge ${label} 8px`, handler: () => doNudge(key, NUDGE_SHIFT_PX) });
    reg({ key, meta: true, description: `Nudge ${label} 64px`, handler: () => doNudge(key, NUDGE_LARGE_PX) });
    reg({ key, ctrl: true, description: `Nudge ${label} 64px (Win/Linux)`, handler: () => doNudge(key, NUDGE_LARGE_PX) });
  }

  return () => {
    for (const fn of unsubs) fn();
  };
}
