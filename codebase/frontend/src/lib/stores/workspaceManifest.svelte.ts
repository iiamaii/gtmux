// WorkspaceManifestStore — session organization state.
//
// 정본:
// - ADR-0044 D-B2~B8
// - docs/reports/2026-06-01-fe-handover-instance-and-session-org.md §3
//
// Counts / active / modified_at are BE-derived and never written through the
// manifest PUT body. This store only mutates folder/session organization.

import { getWorkspace, ManifestStaleError, putManifest } from '$lib/http/workspace';
import type {
  EnrichedSession,
  Folder,
  ManifestPutBody,
  SessionOrg,
  WorkspaceListResponse,
} from '$lib/types/sessions';
import { generateUuidV4 } from '$lib/uuid';

type ManifestDraft = {
  folders: Folder[];
  sessions: EnrichedSession[];
};

function sessionOrg(session: EnrichedSession): SessionOrg {
  return {
    folder_id: session.folder_id,
    order: session.order,
    tags: [...session.tags],
    favorite: session.favorite,
  };
}

function compareByOrderThenName<T extends { order: number; name: string }>(a: T, b: T): number {
  return a.order - b.order || a.name.localeCompare(b.name);
}

function nextOrder(values: Array<{ order: number }>): number {
  return values.reduce((max, value) => Math.max(max, value.order), -1) + 1;
}

function normalizeFolderId(folderId: string | null): string | null {
  return folderId === '' ? null : folderId;
}

class WorkspaceManifestStore {
  folders = $state<Folder[]>([]);
  sessions = $state<EnrichedSession[]>([]);
  etag = $state('');
  loading = $state(false);
  errorMessage = $state<string | null>(null);

  applySnapshot(snapshot: WorkspaceListResponse): void {
    this.folders = snapshot.folders.map((folder) => ({ ...folder }));
    this.sessions = snapshot.sessions.map((session) => ({
      ...session,
      tags: [...session.tags],
    }));
    this.etag = snapshot.manifest_etag;
    this.errorMessage = null;
  }

  async load(): Promise<void> {
    this.loading = true;
    try {
      this.applySnapshot(await getWorkspace());
    } catch (err) {
      this.errorMessage = err instanceof Error ? err.message : String(err);
      throw err;
    } finally {
      this.loading = false;
    }
  }

  toPutBody(folders = this.folders, sessions = this.sessions): ManifestPutBody {
    const sessionMap: Record<string, SessionOrg> = {};
    for (const session of sessions) {
      sessionMap[session.name] = sessionOrg(session);
    }
    return {
      manifest_version: 1,
      folders: folders.map((folder) => ({ ...folder })),
      sessions: sessionMap,
    };
  }

  async applyManifest(mutator: (draft: ManifestDraft) => void): Promise<void> {
    const run = async (): Promise<void> => {
      const draft: ManifestDraft = {
        folders: this.folders.map((folder) => ({ ...folder })),
        sessions: this.sessions.map((session) => ({
          ...session,
          tags: [...session.tags],
        })),
      };
      mutator(draft);
      const { manifest_etag } = await putManifest(
        this.toPutBody(draft.folders, draft.sessions),
        this.etag,
      );
      this.folders = draft.folders;
      this.sessions = draft.sessions;
      this.etag = manifest_etag;
      this.errorMessage = null;
    };

    try {
      await run();
    } catch (err) {
      if (err instanceof ManifestStaleError) {
        await this.load();
        await run();
        return;
      }
      this.errorMessage = err instanceof Error ? err.message : String(err);
      throw err;
    }
  }

  async createFolder(name: string, parentId: string | null = null): Promise<string> {
    const id = generateUuidV4();
    await this.applyManifest((draft) => {
      const siblings = draft.folders.filter((folder) => folder.parent_id === parentId);
      draft.folders.push({
        id,
        parent_id: parentId,
        name: name.trim(),
        order: nextOrder(siblings),
        color: null,
        collapsed: false,
      });
    });
    return id;
  }

  async renameFolder(id: string, name: string): Promise<void> {
    await this.applyManifest((draft) => {
      const folder = draft.folders.find((f) => f.id === id);
      if (folder !== undefined) folder.name = name.trim();
    });
  }

  async deleteFolder(id: string): Promise<void> {
    await this.applyManifest((draft) => {
      const deleted = draft.folders.find((folder) => folder.id === id);
      if (deleted === undefined) return;
      const parentId = deleted.parent_id;
      draft.folders = draft.folders
        .filter((folder) => folder.id !== id)
        .map((folder) =>
          folder.parent_id === id
            ? {
                ...folder,
                parent_id: parentId,
              }
            : folder,
        );
      draft.sessions = draft.sessions.map((session) =>
        session.folder_id === id
          ? {
              ...session,
              folder_id: null,
              order: nextOrder(draft.sessions.filter((s) => s.folder_id === null)),
            }
          : session,
      );
    });
  }

  async moveSession(name: string, folderId: string | null, order?: number): Promise<void> {
    const nextFolderId = normalizeFolderId(folderId);
    await this.applyManifest((draft) => {
      const session = draft.sessions.find((s) => s.name === name);
      if (session === undefined) return;
      const siblings = draft.sessions.filter((s) => s.name !== name && s.folder_id === nextFolderId);
      session.folder_id = nextFolderId;
      session.order = order ?? nextOrder(siblings);
    });
  }

  async toggleFavorite(name: string): Promise<void> {
    await this.applyManifest((draft) => {
      const session = draft.sessions.find((s) => s.name === name);
      if (session !== undefined) session.favorite = !session.favorite;
    });
  }

  async setTags(name: string, tags: string[]): Promise<void> {
    await this.applyManifest((draft) => {
      const session = draft.sessions.find((s) => s.name === name);
      if (session !== undefined) session.tags = [...tags];
    });
  }

  async setCollapsed(id: string, collapsed: boolean): Promise<void> {
    await this.applyManifest((draft) => {
      const folder = draft.folders.find((f) => f.id === id);
      if (folder !== undefined) folder.collapsed = collapsed;
    });
  }

  async moveSessionToFolder(name: string, folderId: string | null): Promise<void> {
    await this.moveSession(name, folderId);
  }

  folderPath(folderId: string | null): string {
    if (folderId === null) return '';
    const byId = new Map(this.folders.map((folder) => [folder.id, folder] as const));
    const names: string[] = [];
    const seen = new Set<string>();
    let cursor: string | null = folderId;
    while (cursor !== null) {
      if (seen.has(cursor)) break;
      seen.add(cursor);
      const folder = byId.get(cursor);
      if (folder === undefined) break;
      names.unshift(folder.name);
      cursor = folder.parent_id;
    }
    return names.join(' / ');
  }

  sortedFolders(parentId: string | null): Folder[] {
    return this.folders
      .filter((folder) => folder.parent_id === parentId)
      .slice()
      .sort(compareByOrderThenName);
  }

  sortedSessions(folderId: string | null): EnrichedSession[] {
    return this.sessions
      .filter((session) => session.folder_id === folderId)
      .slice()
      .sort(compareByOrderThenName);
  }
}

export const workspaceManifest = new WorkspaceManifestStore();
