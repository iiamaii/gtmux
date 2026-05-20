// changeTerminalDialog — global open/close state for ChangeTerminalModal.
//
// 정본:
// - frontend-handover-v2 FE-NEW-4 (ChangeTerminalModal)
// - ADR-0021 D8 (Terminal binding UI — picker → rebind)
// - ADR-0017 §D2 amend (panel header more menu 가 [Change terminal...] 진입)
//
// 동작:
//   - ContextMenu (또는 header more menu) 의 [Change terminal...] 항목이
//     `changeTerminalDialog.open(panelId)` 호출.
//   - +page.svelte 가 `ChangeTerminalModal` 를 mount 하고 본 store 의 state 를 prop 으로 전달.
//   - Modal 의 [Cancel] / commit 후 `close()`.
//
// Single-instance singleton — 동시에 한 modal 만. 다른 panel 의 더 [Change terminal...]
// 클릭 시 `panelId` 만 교체 (modal 은 그대로 열려있음).

class ChangeTerminalDialogStore {
  open = $state(false);
  panelId = $state<string | null>(null);

  show(panelId: string): void {
    this.panelId = panelId;
    this.open = true;
  }

  close(): void {
    this.open = false;
    this.panelId = null;
  }
}

export const changeTerminalDialog = new ChangeTerminalDialogStore();
