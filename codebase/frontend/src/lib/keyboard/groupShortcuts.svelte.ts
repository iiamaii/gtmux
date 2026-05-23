// groupShortcuts — Group lifecycle keyboard shortcuts (ADR-0010 D17).
//
// 정본:
// - ADR-0010 D17 (Cmd+G / Cmd+Shift+G — Group / Ungroup, MVP 포함)
// - plan-0012 §3.5 E.1
//
// Matrix (Figma / Sketch 컨벤션):
//   Cmd+G / Ctrl+G              → Group     (M.size >= 1 일 때 createGroup)
//   Cmd+Shift+G / Ctrl+Shift+G  → Ungroup   (M 의 단일 element 가 group type 일 때 ungroup)
//
// 동작 조건:
//   - sessionStore.active !== null (no-session 시 발동 skip)
//   - 입력 중 (input/textarea/contentEditable) 이면 skip (registry default).
//     Modifier 가 있으니 default `allowInEditable` 는 true — caller 가 직접 차단.
//
// 본 모듈은 `shortcutRegistry` 의 consumer — 직접 keydown listener 등록 X.

import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { shortcutRegistry, type ShortcutDescriptor } from './shortcutRegistry.svelte';

function onGroup(): boolean {
  if (sessionStore.active === null) return true;
  if (sessionStore.M.size < 1) return true;
  void sessionStore.createGroup([...sessionStore.M]);
  return true;
}

function onUngroup(): boolean {
  if (sessionStore.active === null) return true;
  if (sessionStore.M.size !== 1) return true;
  const sole = [...sessionStore.M][0];
  if (sole === undefined || !sessionStore.isGroupId(sole)) return true;
  void sessionStore.ungroup(sole);
  return true;
}

export function bindGroupShortcuts(): () => void {
  const unsubs: Array<() => void> = [];
  const reg = (d: ShortcutDescriptor) => unsubs.push(shortcutRegistry.register(d));

  reg({ key: 'g', meta: true, category: 'Canvas', description: 'Group selection', handler: onGroup });
  reg({ key: 'g', ctrl: true, category: 'Canvas', description: 'Group selection (Win/Linux)', handler: onGroup });
  reg({ key: 'G', meta: true, shift: true, category: 'Canvas', description: 'Ungroup selection', handler: onUngroup });
  reg({ key: 'G', ctrl: true, shift: true, category: 'Canvas', description: 'Ungroup selection (Win/Linux)', handler: onUngroup });

  return () => {
    for (const fn of unsubs) fn();
  };
}
