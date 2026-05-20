// groupCloseDialog — open/close state for GroupCloseConfirmModal.
//
// 정본:
// - ADR-0021 D9.3 (Group close bulk 1 dialog)
// - frontend-handover-v2 §3.2 (GroupCloseConfirmModal)
//
// 동작:
//   - Sidebar GroupRow (또는 ContextMenu) 의 [Close group] 진입 →
//     `groupCloseDialog.show(groupId)` 호출.
//   - +page.svelte 가 `GroupCloseConfirmModal` 을 mount.
//   - Modal 의 [Cancel] / commit 후 `close()`.

class GroupCloseDialogStore {
  open = $state(false);
  groupId = $state<string | null>(null);

  show(groupId: string): void {
    this.groupId = groupId;
    this.open = true;
  }

  close(): void {
    this.open = false;
    this.groupId = null;
  }
}

export const groupCloseDialog = new GroupCloseDialogStore();
