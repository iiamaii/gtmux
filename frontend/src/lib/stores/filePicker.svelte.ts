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
  #onSelect: ((absolutePath: string) => void) | null = null;

  /**
   * Caller 의 select callback 을 등록하고 modal open. caller 는 cancel /
   * select 의 처리 (item spawn / path 갱신) 를 책임.
   */
  openFor(initialDir: string, onSelect: (absolutePath: string) => void): void {
    this.initialDir = initialDir;
    this.#onSelect = onSelect;
    this.open = true;
  }

  cancel(): void {
    this.open = false;
    this.#onSelect = null;
  }

  select(absolutePath: string): void {
    const cb = this.#onSelect;
    this.open = false;
    this.#onSelect = null;
    if (cb !== null) cb(absolutePath);
  }
}

export const filePicker = new FilePickerStore();
