// SessionIODialog — Import / Export modal open state.
//
// 정본:
// - ADR-0029 (Session import/export 파일 포맷 및 API 정책)
// - SessionMenu 의 Import… / Export… entry 가 호출
//
// `mode` 가 `null` 이면 닫힘. `'export'` / `'import'` 가 각각 ExportSessionModal /
// ImportSessionModal 의 trigger.

export type SessionIOMode = 'export' | 'import';

class SessionIODialogStore {
  mode = $state<SessionIOMode | null>(null);

  openExport(): void {
    this.mode = 'export';
  }

  openImport(): void {
    this.mode = 'import';
  }

  close(): void {
    this.mode = null;
  }
}

export const sessionIODialog = new SessionIODialogStore();
