// clipboardShortcuts — Cmd/Ctrl + C / X / V wire (ADR-0030 D5 / D7).
//
// 정본:
// - ADR-0030 D3 — terminal paste = clone (fresh UUID → BE 의 unmatched-spawn 분기 자연 활용)
// - ADR-0030 D4 — paste 좌표: bbox top-left + (24,24)*pasteCount, 상대 위치 보존
// - ADR-0030 D5 — Cut = Copy + applyDeletion(kill=false), locked 제외
// - ADR-0030 D6 — paste 시 새 UUID
// - ADR-0030 D7 — Focus 분기: editable / xterm focus 시 OS default 우선 (registry default `allowInEditable=false` + `allowInXterm=false` 로 명시)
// - ADR-0030 D9 — applyMutation 통과 → historyStore 자동 capture (1 PUT = 1 entry)
// - ADR-0017 D6 amend ⑦ (b) — 본 wire 의 매트릭스 cross-link

import type { CanvasItem } from '$lib/types/canvas';
import { clipboardStore } from '$lib/stores/clipboardStore.svelte';
import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { pasteItems } from '$lib/canvas/clipboardOps.svelte';
import { shortcutRegistry } from './shortcutRegistry.svelte';

function selectionItems(): CanvasItem[] {
  const out: CanvasItem[] = [];
  for (const id of sessionStore.M) {
    const it = sessionStore.items.get(id);
    if (it !== undefined) out.push(it);
  }
  return out;
}

function selectionMutable(): CanvasItem[] {
  // ADR-0030 D5 — locked item 제외 (cut target).
  return selectionItems().filter((it) => !it.locked);
}

function doCopy(): boolean {
  const items = selectionItems();
  if (items.length === 0) return false;
  clipboardStore.copy(items);
  return true;
}

function doCut(): boolean {
  const items = selectionMutable();
  if (items.length === 0) return false;
  clipboardStore.cut(items);
  const ids = items.map((it) => it.id);
  void sessionStore.applyDeletion(ids, { killTerminal: false });
  return true;
}

function doPaste(): boolean {
  if (clipboardStore.entries.length === 0) return false;
  if (sessionStore.active === null) return false;
  const offset = clipboardStore.consumePasteOffset();
  void pasteItems(clipboardStore.entries, { offset, failMessage: 'Paste failed' });
  return true;
}

export function bindClipboardShortcuts(): () => void {
  const unsubs: Array<() => void> = [];

  const register = (
    key: string,
    modifier: 'meta' | 'ctrl',
    description: string,
    run: () => boolean,
  ): void => {
    unsubs.push(
      shortcutRegistry.register({
        key,
        meta: modifier === 'meta',
        ctrl: modifier === 'ctrl',
        description,
        category: 'Edit',
        // ADR-0030 D7 — editable / xterm focus 시 OS default 우선 (registry skip).
        allowInEditable: false,
        allowInXterm: false,
        handler: () => run(),
      }),
    );
  };

  register('c', 'meta', 'Copy', doCopy);
  register('c', 'ctrl', 'Copy (Win/Linux)', doCopy);
  register('x', 'meta', 'Cut', doCut);
  register('x', 'ctrl', 'Cut (Win/Linux)', doCut);
  register('v', 'meta', 'Paste', doPaste);
  register('v', 'ctrl', 'Paste (Win/Linux)', doPaste);

  return () => {
    for (const fn of unsubs) fn();
  };
}
