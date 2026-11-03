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
// Behavior는 서버 전역 설정이며 호스트가 파일 편집을 제공하면 TOML에 저장됨.
// FE는 PATCH 성공 후에만 반영하고 별도 파일을 쓰지 않음.

import {
  getSettings,
  type AuthInfo,
  patchBehavior,
  type BehaviorSettings,
  type BuildInfo,
  type ServerInfo,
  type SettingsSnapshot,
} from '$lib/http/settings';
import { UnauthorizedError } from '$lib/http/sessions';

const DEFAULT_BEHAVIOR: BehaviorSettings = {
  auto_kill_terminal_on_panel_close: false,
  picker_show_hidden: false,
  reload_on_session_switch: true,
  // ADR-0049 D3-(a) — default false (off). BE 미배선 시 snapshot 에 없을 수 있어
  // applySnapshot 가 spread 해도 누락되면 이 default 가 남아 false 로 게이트.
  osc52_clipboard_write_enabled: false,
};

class SettingsStore {
  behavior = $state<BehaviorSettings>({ ...DEFAULT_BEHAVIOR });
  build = $state<BuildInfo | null>(null);
  server = $state<ServerInfo | null>(null);
  auth = $state<AuthInfo | null>(null);
  loaded = $state(false);
  errorMessage = $state<string | null>(null);

  /** Server snapshot 으로 store 갱신. */
  applySnapshot(snap: SettingsSnapshot): void {
    // Merge over DEFAULT_BEHAVIOR so a field the BE has not wired yet (e.g.
    // ADR-0049 osc52_clipboard_write_enabled before the BE handover lands)
    // keeps its safe default instead of becoming undefined.
    this.behavior = { ...DEFAULT_BEHAVIOR, ...snap.behavior };
    this.build = snap.build;
    this.server = snap.server;
    this.auth = snap.auth;
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
