// zShortcuts — Z-index keyboard shortcuts (ADR-0024 D2).
//
// Matrix (Figma / 일반 디자인 툴 컨벤션):
//   ]          → bringForward     (z 한 칸 위)
//   [          → sendBackward     (z 한 칸 아래)
//   Shift + ]  → bringToFront     (z = max + 1)
//   Shift + [  → sendToBack       (z = min - 1)
//
// 동작 조건:
//   - sessionStore.M.size >= 1 (single + multi 모두 — ADR-0024 D9 atomic block batch).
//   - 입력 중 (input/textarea/contentEditable) 이면 skip (registry default).
//   - Boundary (ADR-0024 D11) 도달 시 silent noop — keyboard path tooltip 없음.
//
// 본 모듈은 `shortcutRegistry` 의 consumer — 직접 keydown listener 등록 X.

import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { zStore } from '$lib/stores/zStore.svelte';
import { shortcutRegistry } from './shortcutRegistry.svelte';

function selectedIds(): string[] {
  return [...sessionStore.M];
}

function withSelected(fn: (ids: string[]) => void): boolean {
  const ids = selectedIds();
  if (ids.length === 0) return false;
  fn(ids);
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

  // ADR-0024 D11 (boundary): noop 시 silent (keyboard path 은 tooltip 없음).
  // canXxx 가 false 면 fire-and-forget 처리하지 않음 — 호출 path 안 zStore 가
  // 내부적으로도 noop 보장.
  unsubs.push(
    shortcutRegistry.register({
      actionId: 'z.bring_forward',
      key: ']',
      customizable: true,
      description: 'Bring forward (z +1)',
      category: 'Z',
      handler: () =>
        withSelected((ids) => {
          if (zStore.canBringForward(ids)) zStore.bringForward(ids);
        }),
    }),
  );
  unsubs.push(
    shortcutRegistry.register({
      actionId: 'z.send_backward',
      key: '[',
      customizable: true,
      description: 'Send backward (z −1)',
      category: 'Z',
      handler: () =>
        withSelected((ids) => {
          if (zStore.canSendBackward(ids)) zStore.sendBackward(ids);
        }),
    }),
  );
  unsubs.push(
    shortcutRegistry.register({
      actionId: 'z.bring_to_front',
      key: '}',
      shift: true,
      customizable: true,
      description: 'Bring to front',
      category: 'Z',
      handler: () =>
        withSelected((ids) => {
          if (zStore.canBringToFront(ids)) zStore.bringToFront(ids);
        }),
    }),
  );
  unsubs.push(
    shortcutRegistry.register({
      actionId: 'z.send_to_back',
      key: '{',
      shift: true,
      customizable: true,
      description: 'Send to back',
      category: 'Z',
      handler: () =>
        withSelected((ids) => {
          if (zStore.canSendToBack(ids)) zStore.sendToBack(ids);
        }),
    }),
  );

  return () => {
    for (const fn of unsubs) fn();
  };
}
