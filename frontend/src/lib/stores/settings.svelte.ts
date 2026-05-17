// SettingsStore — `/api/settings` snapshot 의 in-memory cache.
//
// 정본:
// - BE: `codebase/backend/crates/http-api/src/settings.rs` (GET/PATCH 핸들러)
// - ADR-0021 G25.1.b — `auto_kill_terminal_on_panel_close`
// - ADR-0035 D7   — `picker_show_hidden`
//
// 동작:
//  - default behavior (false/false) 로 시작해 PanelNode 등 consumer 가
//    load 전에도 즉시 read 가능 (fallback = modal 띄움).
//  - `+page.svelte` mount 시 1회 load.
//  - SettingsOverlay 의 toggle 은 PATCH → 응답 snapshot 으로 store 갱신.
//
// 주의: settings 자체는 server-wide + in-memory only (BE 주석). 서버 재시작
// 후 default 로 복귀. FE 측에서 별도 disk 영속 안 함.

import {
  getSettings,
  patchBehavior,
  type BehaviorSettings,
  type SettingsSnapshot,
} from '$lib/http/settings';
import { UnauthorizedError } from '$lib/http/sessions';

const DEFAULT_BEHAVIOR: BehaviorSettings = {
  auto_kill_terminal_on_panel_close: false,
  picker_show_hidden: false,
};

class SettingsStore {
  behavior = $state<BehaviorSettings>({ ...DEFAULT_BEHAVIOR });
  loaded = $state(false);
  errorMessage = $state<string | null>(null);

  /** Server snapshot 으로 store 갱신 (build/server/auth 는 후속 consumer 가 직접 read). */
  applySnapshot(snap: SettingsSnapshot): void {
    this.behavior = { ...snap.behavior };
    this.loaded = true;
    this.errorMessage = null;
  }

  /** App mount 시 1회 호출. 실패는 silent — default false 로 동작. */
  async load(): Promise<void> {
    try {
      const snap = await getSettings();
      this.applySnapshot(snap);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        // auth gate 가 redirect 책임.
        return;
      }
      this.errorMessage = err instanceof Error ? err.message : String(err);
    }
  }

  /** behavior 부분 갱신 — PATCH 후 응답 snapshot 으로 갱신. */
  async setBehavior(partial: Partial<BehaviorSettings>): Promise<void> {
    const snap = await patchBehavior(partial);
    this.applySnapshot(snap);
  }
}

export const settingsStore = new SettingsStore();
