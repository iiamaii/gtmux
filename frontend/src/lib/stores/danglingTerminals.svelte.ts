// DanglingTerminalsStore — terminal UUID → reason map for BE 0x85 events.
//
// 정본:
// - BE Stage 5-B (0034 §3): UUID-carrying `terminal-died` 0x85 frame
// - ADR-0021 D10: dangling recovery via `respawnTerminal`
//
// dispatcher 가 0x85 수신 시 `mark(uuid, reason)` 호출. PanelDanglingOverlay
// 가 자신의 panel terminal_id 가 set 안에 있으면 표시. 사용자가 click 하면
// respawn 후 `clear(uuid)` 로 mark 해제.
//
// 서버 truth 는 `GET /api/terminals` (alive flag) — terminalPool.refresh() 가
// 따라잡으면 alive 가 true 가 되지만, 본 store 는 즉시 UI 갱신을 위한 hint.

import { SvelteMap } from 'svelte/reactivity';

import type { TerminalDiedReason } from '$lib/ws/decode';

class DanglingTerminalsStore {
  byId = $state<SvelteMap<string, TerminalDiedReason>>(new SvelteMap());

  mark(terminalId: string, reason: TerminalDiedReason): void {
    this.byId.set(terminalId, reason);
  }

  clear(terminalId: string): void {
    this.byId.delete(terminalId);
  }

  has(terminalId: string): boolean {
    return this.byId.has(terminalId);
  }

  reasonFor(terminalId: string): TerminalDiedReason | null {
    return this.byId.get(terminalId) ?? null;
  }
}

export const danglingTerminals = new DanglingTerminalsStore();
