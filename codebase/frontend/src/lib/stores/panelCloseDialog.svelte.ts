// panelCloseDialog — Batch-aware Panel close confirm.
//
// 정본:
// - ADR-0021 D9.3 (Panel/Terminal close 분리)
// - ADR-0010 G25 amend (multi-session 의 confirm 정책)
// - ADR-0032 Amend ④ (multi-select context-menu 의 terminal 포함 batch 제거 시
//   PanelCloseConfirmModal 의 batch-aware dialog 노출)
//
// 동작:
//   - 호출자 (ContextMenu 의 Delete / Cut / Clear all) 가 `show({ items, onConfirm })`.
//   - items 에 terminal 이 1개 이상 있으면 modal 노출. 사용자가 [Panel only] /
//     [Panel + Terminal] 선택 → `onConfirm(killTerminal)`.
//   - items 에 terminal 이 0개면 *즉시* `onConfirm(false)` (modal 건너뜀).
//   - `Settings.behavior.auto_kill_terminal_on_panel_close` (P1+) 면 modal 우회
//     하고 `onConfirm(true)` 즉시 호출 — PanelNode 의 close 분기와 정합.

import { sessionStore } from './sessionStore.svelte';
import { settingsStore } from './settings.svelte';
import { terminalPool } from './terminalPool.svelte';
import type { CanvasItem } from '$lib/types/canvas';

interface ShowArgs {
  /** 제거 대상 item 들 (terminal + non-terminal 혼재 OK). */
  items: readonly CanvasItem[];
  /** 사용자 선택 후 invoke. killTerminal 은 terminal item 들에만 의미 — non-terminal 은 무관. */
  onConfirm: (killTerminal: boolean) => void | Promise<void>;
}

class PanelCloseDialogStore {
  open = $state(false);
  /** Display 용 label — single: panel 이름, batch: "N items". */
  panelLabel = $state('');
  /** Batch count — modal title 분기용. */
  count = $state(1);
  /** Terminal item 들의 attach_count 합. */
  attachCount = $state(0);
  /** 모든 terminal item 의 attached_sessions union (현 session 제외). */
  otherSessions = $state<string[]>([]);

  private pendingOnConfirm: ((killTerminal: boolean) => void | Promise<void>) | null = null;

  show(args: ShowArgs): void {
    const terminals = args.items.filter((it) => it.type === 'terminal');

    // No terminals → modal 생략, kill=false 즉시 적용.
    if (terminals.length === 0) {
      void args.onConfirm(false);
      return;
    }

    // Auto-kill 설정 → modal 우회, kill=true 즉시.
    if (settingsStore.behavior.auto_kill_terminal_on_panel_close) {
      void args.onConfirm(true);
      return;
    }

    const active = sessionStore.active;
    let totalAttach = 0;
    const otherSet = new Set<string>();
    for (const term of terminals) {
      const t = terminalPool.byId(term.id);
      if (t !== null) {
        totalAttach += t.attach_count;
        for (const s of t.attached_sessions) {
          if (active === null || s !== active.name) otherSet.add(s);
        }
      }
    }

    const itemCount = args.items.length;
    if (itemCount === 1) {
      const only = args.items[0]!;
      const label =
        typeof only.label === 'string' && only.label.length > 0
          ? only.label
          : only.id.slice(0, 8);
      this.panelLabel = label;
    } else {
      this.panelLabel = `${itemCount} items`;
    }

    this.count = itemCount;
    this.attachCount = totalAttach;
    this.otherSessions = [...otherSet];
    this.pendingOnConfirm = args.onConfirm;
    this.open = true;
  }

  confirm(killTerminal: boolean): void {
    const cb = this.pendingOnConfirm;
    this.open = false;
    this.pendingOnConfirm = null;
    if (cb !== null) void cb(killTerminal);
  }

  cancel(): void {
    this.open = false;
    this.pendingOnConfirm = null;
  }
}

export const panelCloseDialog = new PanelCloseDialogStore();
