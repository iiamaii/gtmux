// FilePicker — 전역 picker store (ADR-0035 D5).
//
// 두 caller 가 같은 modal instance 를 사용:
//   1. Canvas.svelte 의 file_path 도구 spawn flow (Toolbar click → canvas
//      click → modal). select 시 새 item spawn.
//   2. FilePathNode.svelte 의 더블 클릭 (수정 의도). select 시 그 item 의
//      path 갱신.
//
// FilePickerModal 한 instance 가 Canvas.svelte 안에 mount, store 의
// state 를 props 로 binding.

class FilePickerStore {
  open = $state(false);
  initialDir = $state<string>('');
  accept = $state<{ extensions: string[]; description: string } | null>(null);
  rootKind = $state<'server' | 'workspace'>('server');
  rootPath = $state<string>('');
  // ADR-0035 — file_path 는 파일/디렉터리 둔 다 참조 가능. 이 opt-in 이
  // true 면 picker 가 directory 도 선택 대상으로 노출 (ADR-0047 의 files-tab
  // drag-add 와 동일 surface). image/document 는 false 로 file-only 유지.
  allowDirectories = $state(false);
  #onSelect: ((absolutePath: string, kind: 'directory' | 'file') => void) | null = null;

  /**
   * Caller 의 select callback 을 등록하고 modal open. caller 는 cancel /
   * select 의 처리 (item spawn / path 갱신) 를 책임.
   */
  openFor(
    initialDir: string,
    onSelect: (absolutePath: string, kind: 'directory' | 'file') => void,
    options?: {
      accept?: { extensions: string[]; description: string };
      rootKind?: 'server' | 'workspace';
      rootPath?: string;
      allowDirectories?: boolean;
    },
  ): void {
    this.initialDir = initialDir;
    this.accept = options?.accept ?? null;
    this.rootKind = options?.rootKind ?? 'server';
    this.rootPath = options?.rootPath ?? '';
    this.allowDirectories = options?.allowDirectories ?? false;
    this.#onSelect = onSelect;
    this.open = true;
  }

  cancel(): void {
    this.open = false;
    this.accept = null;
    this.rootKind = 'server';
    this.rootPath = '';
    this.allowDirectories = false;
    this.#onSelect = null;
  }

  select(absolutePath: string, kind: 'directory' | 'file'): void {
    const cb = this.#onSelect;
    this.open = false;
    this.accept = null;
    this.rootKind = 'server';
    this.rootPath = '';
    this.allowDirectories = false;
    this.#onSelect = null;
    if (cb !== null) cb(absolutePath, kind);
  }
}

export const filePicker = new FilePickerStore();
