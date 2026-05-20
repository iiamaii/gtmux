// zShortcuts — Z-index keyboard shortcuts (ADR-0024 D2).
//
// Matrix (Figma / 일반 디자인 툴 컨벤션):
//   [          → bringForward     (z 한 칸 위)
//   ]          → sendBackward     (z 한 칸 아래)
//   Shift + [  → bringToFront     (z = max + 1)
//   Shift + ]  → sendToBack       (z = min - 1)
//
// 동작 조건:
//   - sessionStore.M.size === 1 (single selection 만)
//   - 입력 중 (input/textarea/contentEditable) 이면 skip (registry default)
//
// 본 모듈은 `shortcutRegistry` 의 consumer — 직접 keydown listener 등록 X.

import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { zStore } from '$lib/stores/zStore.svelte';
import { shortcutRegistry } from './shortcutRegistry.svelte';

function selectedSingleId(): string | null {
  if (sessionStore.M.size !== 1) return null;
  const it = sessionStore.M.values().next();
  return it.done ? null : it.value;
}

function withSelected(fn: (id: string) => void): boolean {
  const id = selectedSingleId();
  if (id === null) return false;
  fn(id);
  return true;
}

/**
 * Register the four z-shortcuts. Returns an unregister callback.
 *
 * `KeyboardEvent.key` already encodes Shift — `[` vs `{` and `]` vs `}`
 * are distinct surface characters on US layouts. We register both
 * spellings so layouts that produce literal `[` while Shift is down
 * (some non-US keyboards) still route correctly via the modifier
 * check.
 */
export function bindZShortcuts(): () => void {
  const unsubs: Array<() => void> = [];

  unsubs.push(
    shortcutRegistry.register({
      key: '[',
      description: 'Bring forward (z +1)',
      category: 'Z',
      handler: () => withSelected((id) => zStore.bringForward(id)),
    }),
  );
  unsubs.push(
    shortcutRegistry.register({
      key: ']',
      description: 'Send backward (z −1)',
      category: 'Z',
      handler: () => withSelected((id) => zStore.sendBackward(id)),
    }),
  );
  unsubs.push(
    shortcutRegistry.register({
      key: '{',
      shift: true,
      description: 'Bring to front (z = max + 1)',
      category: 'Z',
      handler: () => withSelected((id) => zStore.bringToFront(id)),
    }),
  );
  unsubs.push(
    shortcutRegistry.register({
      key: '}',
      shift: true,
      description: 'Send to back (z = min − 1)',
      category: 'Z',
      handler: () => withSelected((id) => zStore.sendToBack(id)),
    }),
  );

  return () => {
    for (const fn of unsubs) fn();
  };
}
