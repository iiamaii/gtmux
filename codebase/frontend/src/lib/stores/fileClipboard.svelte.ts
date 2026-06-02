import type { FsEntry } from '$lib/http/fs';

export interface FileClipboardEntry {
  path: string;
  rootPath: string;
  name: string;
  kind: FsEntry['kind'];
  sizeBytes: number | null;
}

class FileClipboardStore {
  entries = $state<FileClipboardEntry[]>([]);
  copiedAt = $state(0);

  get hasEntries(): boolean {
    return this.entries.length > 0;
  }

  copy(entries: readonly FileClipboardEntry[]): void {
    this.entries = entries.map((entry) => ({ ...entry }));
    this.copiedAt = Date.now();
  }

  clear(): void {
    this.entries = [];
    this.copiedAt = 0;
  }
}

export const fileClipboardStore = new FileClipboardStore();
