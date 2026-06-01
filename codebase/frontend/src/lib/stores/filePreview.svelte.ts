import type { FsEntry } from '$lib/http/fs';

export interface FilePreviewSelection {
  path: string;
  entry: FsEntry;
}

class FilePreviewStore {
  selection = $state<FilePreviewSelection | null>(null);

  select(path: string, entry: FsEntry): void {
    this.selection = { path, entry };
  }

  clear(): void {
    this.selection = null;
  }
}

export const filePreviewStore = new FilePreviewStore();
