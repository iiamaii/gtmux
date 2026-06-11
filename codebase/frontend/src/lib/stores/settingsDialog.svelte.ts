// settingsDialog — open/close + active section state for SettingsOverlay.
//
// 정본:
// - frontend-handover-v2 FE-8 + G19 (Settings full-screen overlay)
// - ADR-0017 amend ④ (Settings overlay shape — 2026-05-16)
//
// Sections (Stage 7 — partial wire this round):
//   - 'theme'      (G27) — system/light/dark + xterm preview
//   - 'shortcuts'  (G26) — shortcut registry + overrides
//   - 'storage'    (FE-NEW-8) — layout import/export + file picker visibility
//   - 'auth'       — sign out / token rotate / auth status
//   - 'behavior'   — auto_kill_terminal_on_panel_close toggle (G25)
//   - 'components' — browser-local component presentation defaults
//   - 'about'      — build/runtime info + system actions

export type SettingsSection =
  | 'theme'
  | 'shortcuts'
  | 'storage'
  | 'auth'
  | 'behavior'
  | 'components'
  | 'about';

class SettingsDialogStore {
  open = $state(false);
  section = $state<SettingsSection>('theme');

  show(section?: SettingsSection): void {
    if (section !== undefined) this.section = section;
    this.open = true;
  }

  close(): void {
    this.open = false;
  }

  toggle(): void {
    this.open = !this.open;
  }

  setSection(section: SettingsSection): void {
    this.section = section;
  }
}

export const settingsDialog = new SettingsDialogStore();
