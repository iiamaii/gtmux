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
import { chromeStore } from '$lib/stores/chrome.svelte';
import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { panelCloseDialog } from '$lib/stores/panelCloseDialog.svelte';
import { pasteItems, materializeSelection } from '$lib/canvas/clipboardOps.svelte';
import {
  commitNewItem,
  createTextItemFromClipboardText,
} from '$lib/canvas/itemFactory';
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
  if (chromeStore.state.leftPanelTab === 'files') return false;
  const payload = selectionPayload();
  if (payload.items.length === 0 && payload.groups.length === 0) return false;
  clipboardStore.copy(payload);
  return true;
}

function doCut(): boolean {
  if (chromeStore.state.leftPanelTab === 'files') return false;
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
  if (chromeStore.state.leftPanelTab === 'files') return false;
  if (sessionStore.active === null) return false;
  if (clipboardStore.kind === 'text') {
    void pasteTextAt(canvasViewportCenter(), { centerAtAnchor: true });
    return true;
  }
  if (!clipboardStore.hasItems) return false;
  const offset = clipboardStore.consumePasteOffset();
  void pasteItems(clipboardStore.entries, clipboardStore.groups, {
    offset,
    failMessage: 'Paste failed',
  });
  return true;
}

async function pasteTextAt(
  anchor: { x: number; y: number },
  options: { centerAtAnchor: boolean },
): Promise<boolean> {
  const offset = clipboardStore.consumeTextPasteOffset();
  const item = createTextItemFromClipboardText(clipboardStore.text, {
    x: anchor.x + offset.dx,
    y: anchor.y + offset.dy,
  });
  if (item === null) return false;
  if (options.centerAtAnchor) {
    item.x -= item.w / 2;
    item.y -= item.h / 2;
  }
  const committed = await commitNewItem(item);
  return committed !== null;
}

function canvasViewportCenter(): { x: number; y: number } {
  const viewport = sessionStore.viewport;
  const zoom = viewport.zoom <= 0 ? 1 : viewport.zoom;
  const canvas = document.querySelector('.canvas-root') as HTMLElement | null;
  const local = canvas === null
    ? { x: window.innerWidth / 2, y: window.innerHeight / 2 }
    : { x: canvas.getBoundingClientRect().width / 2, y: canvas.getBoundingClientRect().height / 2 };
  return {
    x: (local.x - viewport.x) / zoom,
    y: (local.y - viewport.y) / zoom,
  };
}

function isEditableFocused(): boolean {
  if (typeof document === 'undefined') return false;
  const el = document.activeElement as HTMLElement | null;
  if (el === null) return false;
  return isEditableElement(el);
}

function isXtermFocused(): boolean {
  if (typeof document === 'undefined') return false;
  const el = document.activeElement as HTMLElement | null;
  return el?.classList.contains('xterm-helper-textarea') === true;
}

function isEditableElement(el: HTMLElement): boolean {
  const tag = el.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable;
}

function selectedTextFromEventTarget(target: EventTarget | null): string {
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    const start = target.selectionStart ?? 0;
    const end = target.selectionEnd ?? start;
    return target.value.slice(start, end);
  }
  return document.getSelection()?.toString() ?? '';
}

function onNativeTextCopyOrCut(event: ClipboardEvent): void {
  if (
    event.target instanceof HTMLTextAreaElement &&
    event.target.readOnly &&
    event.target.getAttribute('aria-hidden') === 'true'
  ) {
    return;
  }
  const text = selectedTextFromEventTarget(event.target);
  if (text.trim().length === 0) return;
  clipboardStore.copyText(text);
}

function onNativePaste(event: ClipboardEvent): void {
  if (chromeStore.state.leftPanelTab === 'files') return;
  if (isEditableFocused() || isXtermFocused()) return;
  if (clipboardStore.kind === 'canvas') return;
  if (sessionStore.active === null) return;
  const text = event.clipboardData?.getData('text/plain') ?? '';
  if (text.trim().length === 0) return;
  clipboardStore.copyText(text);
  event.preventDefault();
  event.stopPropagation();
  void pasteTextAt(canvasViewportCenter(), { centerAtAnchor: true });
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

  window.addEventListener('copy', onNativeTextCopyOrCut, { capture: true });
  window.addEventListener('cut', onNativeTextCopyOrCut, { capture: true });
  window.addEventListener('paste', onNativePaste, { capture: true });

  return () => {
    for (const fn of unsubs) fn();
    window.removeEventListener('copy', onNativeTextCopyOrCut, { capture: true });
    window.removeEventListener('cut', onNativeTextCopyOrCut, { capture: true });
    window.removeEventListener('paste', onNativePaste, { capture: true });
  };
}
