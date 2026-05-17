// TerminalPoolStore — Terminal pool 의 in-memory snapshot.
//
// 정본:
// - BE: `GET /api/terminals` (BE-NEW-10)
// - ADR-0021 D7 (server-wide pool)
//
// 단일 polling 채널로 모든 consumer (TerminalsPanel, PaneInfoPanel,
// ActiveSessionDropdown 등) 가 한 snapshot 을 공유. 호출자 1+ 일 때 polling
// 시작, 0 으로 떨어지면 stop.
//
// API:
//   - `subscribe()` : ref-count + lazy start. 반환된 unsubscribe() 로 해제.
//   - `terminals`  : `$state` reactive snapshot.
//   - `byId(id)`   : 조회 헬퍼.
//   - `refresh()`  : 즉시 fetch (외부 트리거 — 예: 사용자 액션 직후).

import { SvelteMap } from 'svelte/reactivity';

import { listTerminals } from '$lib/http/terminals';
import { UnauthorizedError } from '$lib/http/sessions';
import type { TerminalInfo } from '$lib/types/terminals';

const POLL_INTERVAL_MS = 5_000;

class TerminalPoolStore {
  terminals = $state<TerminalInfo[]>([]);
  loading = $state(true);
  errorMessage = $state<string | null>(null);

  /**
   * id ↔ TerminalInfo 의 O(1) lookup map. `terminals` 와 1:1 같이 갱신.
   * PanelNode / LayerTreeView 가 row 마다 호출하는 `byId` 의 linear find 제거.
   */
  terminalsById = $state<SvelteMap<string, TerminalInfo>>(new SvelteMap());

  /**
   * UUID ↔ numeric PaneId 매핑 (0039 §2.3.3, BE 0x88 TERMINAL_SPAWNED 의 단일
   * source-of-truth). XtermHost 의 terminal 모드가 본 map 을 조회 → numeric
   * PaneId 로 PANE_OUT subscriber 등록 + PANE_IN/RESIZE 송신 키.
   *
   * 갱신 경로:
   *  - dispatcher.handleTerminalSpawned (0x88) → bindPaneId
   *  - reconnect 후 stale → 현재 stale 채로 유지 (BE 가 0x88 재발행 시 갱신).
   *    완전 정리는 logout / session detach 같은 lifecycle 에서.
   */
  paneIdByUuid = $state<SvelteMap<string, number>>(new SvelteMap());

  #refs = 0;
  #timer: ReturnType<typeof setInterval> | null = null;

  /** Subscribe — 1+ 활성 호출자 동안 polling 유지. unsubscribe 반환. */
  subscribe(): () => void {
    this.#refs += 1;
    if (this.#refs === 1) {
      void this.refresh();
      this.#timer = setInterval(() => void this.refresh(), POLL_INTERVAL_MS);
    }
    return () => {
      this.#refs = Math.max(0, this.#refs - 1);
      if (this.#refs === 0 && this.#timer !== null) {
        clearInterval(this.#timer);
        this.#timer = null;
      }
    };
  }

  /** 즉시 fetch (사용자 액션 후 latency 단축 용). */
  async refresh(): Promise<void> {
    try {
      const res = await listTerminals();
      this.terminals = res.terminals;
      this.terminalsById.clear();
      for (const t of res.terminals) this.terminalsById.set(t.id, t);
      this.loading = false;
      this.errorMessage = null;
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        // Silent — root +page.svelte 의 auth-gate 가 redirect.
        this.loading = false;
        return;
      }
      this.errorMessage = err instanceof Error ? err.message : String(err);
      this.loading = false;
    }
  }

  /** id 로 1건 조회. O(1) — `terminalsById` map 활용. */
  byId(id: string): TerminalInfo | null {
    return this.terminalsById.get(id) ?? null;
  }

  /** UUID → numeric PaneId 등록 (0x88 TERMINAL_SPAWNED dispatcher 가 호출). */
  bindPaneId(uuid: string, paneId: number): void {
    this.paneIdByUuid.set(uuid, paneId);
  }

  /** UUID → numeric PaneId 조회. 미 binding 시 undefined. */
  paneIdFor(uuid: string): number | undefined {
    return this.paneIdByUuid.get(uuid);
  }

  /** UUID 의 매핑 해제 (terminal kill / forget 시). */
  unbindPaneId(uuid: string): void {
    this.paneIdByUuid.delete(uuid);
  }
}

export const terminalPool = new TerminalPoolStore();
