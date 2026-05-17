// DanglingTerminalsStore — terminal UUID → reason map for BE 0x85 events.
//
// 정본:
// - BE Stage 5-B (0034 §3): UUID-carrying `terminal-died` 0x85 frame
// - ADR-0021 D10: dangling recovery via `respawnTerminal`
//
// dispatcher 가 0x85 수신 시 `mark(uuid, reason)` 호출. PanelDanglingOverlay
// 가 자신의 panel terminal_id 가 set 안에 있으면 visual 표시. mount 시 자동
// `startRespawn(uuid)` lock 확보 후 respawn 호출 — multi-webpage / multi-panel
// 동시 trigger 의 *client-side single-flight* 보장 (BE 의 idempotent 와 무관).
//
// 다른 webpage / 다른 panel 이 이미 respawn 진행 중 (`startRespawn` false) 이면
// 우리는 spinner 만 보여주고 0x88 도착 후 `handleTerminalSpawned` 가 호출하는
// `clear` 로 자연 해제. webpage 간 race window 는 ms 수준이고, BE 가 같은
// UUID 의 동시 respawn 요청을 어떻게 처리하든 (idempotent / conflict) FE 의
// loop 은 차단 — 0x88 한 번이면 양쪽의 overlay 가 모두 사라짐.
//
// 서버 truth 는 `GET /api/terminals` (alive flag) — terminalPool.refresh() 가
// 따라잡으면 alive 가 true 가 되지만, 본 store 는 즉시 UI 갱신을 위한 hint.

import { SvelteMap, SvelteSet } from 'svelte/reactivity';

import type { TerminalDiedReason } from '$lib/ws/decode';

class DanglingTerminalsStore {
  byId = $state<SvelteMap<string, TerminalDiedReason>>(new SvelteMap());

  /**
   * 현재 respawn 호출이 in-flight 인 UUID set. multi-panel / multi-webpage 의
   * 동시 trigger 차단. 0x88 도착 (handleTerminalSpawned → clear) 이나 호출
   * 실패 (releaseRespawn) 로 해제.
   */
  inFlight = $state<SvelteSet<string>>(new SvelteSet());

  mark(terminalId: string, reason: TerminalDiedReason): void {
    this.byId.set(terminalId, reason);
  }

  clear(terminalId: string): void {
    this.byId.delete(terminalId);
    this.inFlight.delete(terminalId);
  }

  has(terminalId: string): boolean {
    return this.byId.has(terminalId);
  }

  reasonFor(terminalId: string): TerminalDiedReason | null {
    return this.byId.get(terminalId) ?? null;
  }

  /**
   * Respawn 호출 lock 확보 시도. 이미 in-flight 면 false (skip) 반환.
   * 성공 시 true — caller 가 respawn POST 호출. 0x88 도착 시 `clear` 가
   * 자연 해제, 실패 시 `releaseRespawn` 으로 해제.
   */
  startRespawn(terminalId: string): boolean {
    if (this.inFlight.has(terminalId)) return false;
    this.inFlight.add(terminalId);
    return true;
  }

  /** Respawn 실패 시 lock 해제 (mark 는 유지 — 사용자가 재시도). */
  releaseRespawn(terminalId: string): void {
    this.inFlight.delete(terminalId);
  }

  isRespawning(terminalId: string): boolean {
    return this.inFlight.has(terminalId);
  }
}

export const danglingTerminals = new DanglingTerminalsStore();
