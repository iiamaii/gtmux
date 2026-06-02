import type { FsEntry } from '$lib/http/fs';

export interface FilePreviewSelection {
  path: string;
  entry: FsEntry;
}

class FilePreviewStore {
  selection = $state<FilePreviewSelection | null>(null);
  selectedEntries = $state<FilePreviewSelection[]>([]);
  selectedPaths = $state(new Set<string>());
  anchorPath = $state<string | null>(null);

  select(path: string, entry: FsEntry): void {
    this.selection = { path, entry };
    this.selectedEntries = [{ path, entry }];
    this.selectedPaths = new Set([path]);
    this.anchorPath = path;
  }

  setSelection(
    entries: readonly FilePreviewSelection[],
    primaryPath?: string | null,
    anchorPath?: string | null,
  ): void {
    this.selectedEntries = entries.map((entry) => ({
      path: entry.path,
      entry: entry.entry,
    }));
    this.selectedPaths = new Set(entries.map((entry) => entry.path));
    const primary =
      entries.find((entry) => entry.path === primaryPath) ??
      entries[0] ??
      null;
    this.selection = primary === null ? null : { path: primary.path, entry: primary.entry };
    this.anchorPath = anchorPath ?? primary?.path ?? null;
  }

  setAnchor(path: string | null): void {
    this.anchorPath = path;
  }

  rebasePath(oldPath: string, newPath: string, nextEntry: FsEntry): void {
    this.selectedEntries = this.selectedEntries.map((selected) => {
      if (!isSameOrChild(selected.path, oldPath)) return selected;
      const wasTarget = selected.path === oldPath;
      return {
        path: replacePathPrefix(selected.path, oldPath, newPath),
        entry: wasTarget ? nextEntry : selected.entry,
      };
    });
    this.selectedPaths = new Set(
      [...this.selectedPaths].map((path) =>
        isSameOrChild(path, oldPath) ? replacePathPrefix(path, oldPath, newPath) : path,
      ),
    );
    if (this.selection !== null && isSameOrChild(this.selection.path, oldPath)) {
      const wasTarget = this.selection.path === oldPath;
      this.selection = {
        path: replacePathPrefix(this.selection.path, oldPath, newPath),
        entry: wasTarget ? nextEntry : this.selection.entry,
      };
    }
    if (this.anchorPath !== null && isSameOrChild(this.anchorPath, oldPath)) {
      this.anchorPath = replacePathPrefix(this.anchorPath, oldPath, newPath);
    }
  }

  removePath(pathToRemove: string): void {
    this.selectedEntries = this.selectedEntries.filter(
      (selected) => !isSameOrChild(selected.path, pathToRemove),
    );
    this.selectedPaths = new Set(
      [...this.selectedPaths].filter((path) => !isSameOrChild(path, pathToRemove)),
    );
    if (this.selection !== null && isSameOrChild(this.selection.path, pathToRemove)) {
      const next = this.selectedEntries[0] ?? null;
      this.selection = next === null ? null : { path: next.path, entry: next.entry };
    }
    if (this.anchorPath !== null && isSameOrChild(this.anchorPath, pathToRemove)) {
      this.anchorPath = this.selection?.path ?? null;
    }
  }

  clear(): void {
    this.selection = null;
    this.selectedEntries = [];
    this.selectedPaths = new Set();
    this.anchorPath = null;
  }
}

function isSameOrChild(path: string, parent: string): boolean {
  const prefix = parent.endsWith('/') ? parent : `${parent}/`;
  return path === parent || path.startsWith(prefix);
}

function replacePathPrefix(path: string, oldPrefix: string, newPrefix: string): string {
  if (path === oldPrefix) return newPrefix;
  return `${newPrefix}${path.slice(oldPrefix.length)}`;
}

export const filePreviewStore = new FilePreviewStore();
