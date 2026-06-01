<script lang="ts">
  /**
   * SessionListModal — workspace manifest-backed session picker.
   *
   * 보존 불변식: attach/detach/confirm 결정 경로는 부모 WorkspaceSwitcher 가
   * 그대로 소유한다. 본 컴포넌트는 list 표시/조직 mutation 만 담당한다.
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';
  import Input from '$lib/ui/Input.svelte';
  import SessionDeleteConfirmModal from './SessionDeleteConfirmModal.svelte';
  import FileExplorer from './FileExplorer.svelte';
  import {
    changeWorkspace,
    deleteSession,
    duplicateSession,
    renameSession,
    SESSION_NAME_REGEX,
    SessionConflictError,
    UnauthorizedError,
    WorkspaceUpdateUnavailableError,
  } from '$lib/http/sessions';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { workspaceManifest } from '$lib/stores/workspaceManifest.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import type { EnrichedSession, Folder as WorkspaceFolder } from '$lib/types/sessions';

  interface Props {
    open: boolean;
    onClose: () => void;
    /** Session 선택 — 부모가 attach 흐름 진행. 비활성 row 는 호출 X. */
    onSelect: (name: string) => void;
    /** Folder context 에서 새 session 생성. 부모가 NewSessionModal 로 전환. */
    onCreateInFolder?: (folderId: string | null) => void;
    /** 401 시 부모가 redirect 처리 — 호출되면 `/auth` 로 이동. */
    onUnauthorized?: () => void;
    /** Polling 주기 (ms). 기본 1000 (G18). 테스트 시 override. */
    pollIntervalMs?: number;
  }

  type FolderRow = {
    kind: 'folder';
    id: string;
    folder: WorkspaceFolder;
    depth: number;
    childCount: number;
  };
  type SessionRow = {
    kind: 'session';
    id: string;
    session: EnrichedSession;
    depth: number;
  };
  type TreeRow = FolderRow | SessionRow;

  const {
    open,
    onClose,
    onSelect,
    onCreateInFolder,
    onUnauthorized,
    pollIntervalMs = 1000,
  }: Props = $props();

  let loading = $state(true);
  let errorMessage = $state<string | null>(null);
  let pollTimer: ReturnType<typeof setInterval> | null = null;
  let search = $state('');
  let pendingDeleteName = $state<string | null>(null);
  let pendingDeleteFolder = $state<WorkspaceFolder | null>(null);
  let pendingRenameSession = $state<EnrichedSession | null>(null);
  let renameName = $state('');
  let renameSubmitting = $state(false);
  let renameError = $state<string | null>(null);
  let pendingDuplicateSession = $state<EnrichedSession | null>(null);
  let duplicateName = $state('');
  let duplicateFolderId = $state<string | null>(null);
  let duplicateSubmitting = $state(false);
  let duplicateError = $state<string | null>(null);
  let pendingWorkspaceSession = $state<EnrichedSession | null>(null);

  let sessions = $derived(workspaceManifest.sessions);
  let folders = $derived(workspaceManifest.folders);
  let rows = $derived.by(() => buildRows());
  let renameInputError = $derived(
    pendingRenameSession === null
      ? null
      : sessionNameInputError(renameName, pendingRenameSession.name, true),
  );
  let duplicateInputError = $derived(
    pendingDuplicateSession === null
      ? null
      : sessionNameInputError(duplicateName, null, false),
  );
  let moveMenuFolders = $derived.by(() =>
    folders
      .slice()
      .sort((a, b) => workspaceManifest.folderPath(a.id).localeCompare(workspaceManifest.folderPath(b.id))),
  );

  async function refresh(): Promise<void> {
    try {
      await workspaceManifest.load();
      loading = false;
      errorMessage = null;
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      errorMessage = err instanceof Error ? err.message : String(err);
      loading = false;
    }
  }

  $effect(() => {
    if (open) {
      loading = true;
      errorMessage = null;
      void refresh();
      pollTimer = setInterval(() => {
        void refresh();
      }, pollIntervalMs);
      return () => {
        if (pollTimer !== null) {
          clearInterval(pollTimer);
          pollTimer = null;
        }
      };
    }
  });

  function canDelete(name: string): boolean {
    return sessionStore.active?.name !== name;
  }

  function canRenameOrDuplicate(session: EnrichedSession): boolean {
    return !session.active && canDelete(session.name);
  }

  function normalizeSessionName(value: string): string {
    return value.trim();
  }

  function nameExists(value: string, except: string | null): boolean {
    const name = normalizeSessionName(value);
    return sessions.some((session) => session.name === name && session.name !== except);
  }

  function sessionNameInputError(
    value: string,
    except: string | null,
    requireChanged: boolean,
  ): string | null {
    const name = normalizeSessionName(value);
    if (!SESSION_NAME_REGEX.test(name)) {
      return 'Use 1-64 letters, numbers, underscores, or hyphens.';
    }
    if (requireChanged && except !== null && name === except) {
      return 'Choose a different name.';
    }
    if (nameExists(name, except)) {
      return 'A session with that name already exists.';
    }
    return null;
  }

  function uniqueDuplicateName(sourceName: string): string {
    for (let index = 0; index < 1000; index += 1) {
      const suffix = index === 0 ? '_copy' : `_copy${index}`;
      const prefix = sourceName.slice(0, Math.max(1, 64 - suffix.length));
      const candidate = `${prefix}${suffix}`;
      if (SESSION_NAME_REGEX.test(candidate) && !nameExists(candidate, null)) return candidate;
    }
    return '';
  }

  function matchesSearch(session: EnrichedSession): boolean {
    const q = search.trim().toLowerCase();
    if (q.length === 0) return true;
    return (
      session.name.toLowerCase().includes(q) ||
      session.tags.some((tag) => tag.toLowerCase().includes(q)) ||
      session.workspace_root.toLowerCase().includes(q) ||
      workspaceManifest.folderPath(session.folder_id).toLowerCase().includes(q)
    );
  }

  function folderMatches(folder: WorkspaceFolder): boolean {
    const q = search.trim().toLowerCase();
    return q.length > 0 && folder.name.toLowerCase().includes(q);
  }

  function visibleSessions(): EnrichedSession[] {
    return sessions.filter((session) => matchesSearch(session));
  }

  function folderHasVisibleContent(folder: WorkspaceFolder, visible: EnrichedSession[]): boolean {
    if (folderMatches(folder)) return true;
    if (visible.some((session) => session.folder_id === folder.id)) return true;
    return folders
      .filter((candidate) => candidate.parent_id === folder.id)
      .some((child) => folderHasVisibleContent(child, visible));
  }

  function folderChildCount(folderId: string): number {
    const directSessions = sessions.filter((session) => session.folder_id === folderId).length;
    const directFolders = folders.filter((folder) => folder.parent_id === folderId).length;
    return directSessions + directFolders;
  }

  function sortedFolders(parentId: string | null, visible: EnrichedSession[]): WorkspaceFolder[] {
    return folders
      .filter((folder) => folder.parent_id === parentId)
      .filter((folder) => search.trim().length === 0 || folderHasVisibleContent(folder, visible))
      .sort((a, b) => a.order - b.order || a.name.localeCompare(b.name));
  }

  function sortedSessions(folderId: string | null, visible: EnrichedSession[]): EnrichedSession[] {
    return visible
      .filter((session) => session.folder_id === folderId)
      .sort((a, b) => a.order - b.order || a.name.localeCompare(b.name));
  }

  function buildRows(): TreeRow[] {
    const visible = visibleSessions();
    const out: TreeRow[] = [];

    const appendFolder = (folder: WorkspaceFolder, depth: number): void => {
      out.push({
        kind: 'folder',
        id: `folder:${folder.id}`,
        folder,
        depth,
        childCount: folderChildCount(folder.id),
      });
      if (folder.collapsed) return;
      for (const child of sortedFolders(folder.id, visible)) appendFolder(child, depth + 1);
      for (const session of sortedSessions(folder.id, visible)) {
        out.push({
          kind: 'session',
          id: `session:${session.name}`,
          session,
          depth: depth + 1,
        });
      }
    };

    for (const folder of sortedFolders(null, visible)) appendFolder(folder, 0);
    for (const session of sortedSessions(null, visible)) {
      out.push({
        kind: 'session',
        id: `session:${session.name}`,
        session,
        depth: 0,
      });
    }
    return out;
  }

  async function toggleFolder(folder: WorkspaceFolder): Promise<void> {
    try {
      await workspaceManifest.setCollapsed(folder.id, !folder.collapsed);
    } catch (err) {
      showMutationError('Folder update failed', err);
    }
  }

  async function confirmDeleteFolder(): Promise<void> {
    const folder = pendingDeleteFolder;
    if (folder === null) return;
    pendingDeleteFolder = null;
    try {
      await workspaceManifest.deleteFolder(folder.id);
    } catch (err) {
      showMutationError('Delete folder failed', err);
    }
  }

  function beginRename(session: EnrichedSession): void {
    pendingRenameSession = session;
    renameName = session.name;
    renameError = null;
  }

  function closeRenameDialog(): void {
    if (renameSubmitting) return;
    pendingRenameSession = null;
    renameName = '';
    renameError = null;
  }

  function beginDuplicate(session: EnrichedSession): void {
    pendingDuplicateSession = session;
    duplicateName = uniqueDuplicateName(session.name);
    duplicateFolderId = session.folder_id;
    duplicateError = null;
  }

  function closeDuplicateDialog(): void {
    if (duplicateSubmitting) return;
    pendingDuplicateSession = null;
    duplicateName = '';
    duplicateFolderId = null;
    duplicateError = null;
  }

  function sessionActionConflictMessage(err: SessionConflictError, action: string): string {
    if (err.code === 'name_conflict' || err.code === 'session_already_exists') {
      return 'A session with that name already exists.';
    }
    if (err.code === 'session_active') {
      return `${action} is unavailable while the session is attached.`;
    }
    return err.message;
  }

  async function submitRename(): Promise<void> {
    const session = pendingRenameSession;
    if (session === null) return;
    if (renameInputError !== null) {
      renameError = renameInputError;
      return;
    }
    const nextName = normalizeSessionName(renameName);
    renameSubmitting = true;
    renameError = null;
    try {
      const result = await renameSession(session.name, nextName);
      await refresh();
      toastStore.show({
        message: `Renamed session "${session.name}" to "${result.name}".`,
        tone: 'success',
      });
      pendingRenameSession = null;
      renameName = '';
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      renameError =
        err instanceof SessionConflictError
          ? sessionActionConflictMessage(err, 'Rename')
          : err instanceof Error
            ? err.message
            : String(err);
    } finally {
      renameSubmitting = false;
    }
  }

  async function submitDuplicate(): Promise<void> {
    const session = pendingDuplicateSession;
    if (session === null) return;
    if (duplicateInputError !== null) {
      duplicateError = duplicateInputError;
      return;
    }
    const newName = normalizeSessionName(duplicateName);
    duplicateSubmitting = true;
    duplicateError = null;
    try {
      const result = await duplicateSession(session.name, {
        new_name: newName,
        folder_id: duplicateFolderId,
      });
      await refresh();
      toastStore.show({
        message: `Duplicated session "${session.name}" as "${result.name}".`,
        tone: 'success',
      });
      pendingDuplicateSession = null;
      duplicateName = '';
      duplicateFolderId = null;
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      duplicateError =
        err instanceof SessionConflictError
          ? sessionActionConflictMessage(err, 'Duplicate')
          : err instanceof Error
            ? err.message
            : String(err);
    } finally {
      duplicateSubmitting = false;
    }
  }

  async function onConfirmDelete(): Promise<void> {
    const name = pendingDeleteName;
    if (name === null) return;
    pendingDeleteName = null;
    try {
      await deleteSession(name);
      await refresh();
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      showMutationError('Delete failed', err);
    }
  }

  async function submitWorkspaceChange(path: string): Promise<void> {
    const session = pendingWorkspaceSession;
    if (session === null) return;
    pendingWorkspaceSession = null;
    try {
      const result = await changeWorkspace(session.name, path);
      await refresh();
      toastStore.show({
        message: `Workspace changed to ${result.workspace_root}.`,
        tone: 'success',
      });
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      const message = err instanceof WorkspaceUpdateUnavailableError
        ? 'This server does not support session workspace changes yet.'
        : err instanceof Error
          ? err.message
          : String(err);
      toastStore.show({ message, tone: 'error', durationMs: 6_000 });
    }
  }

  function showMutationError(prefix: string, err: unknown): void {
    toastStore.show({
      message: `${prefix}: ${err instanceof Error ? err.message : String(err)}`,
      tone: 'error',
    });
  }

  function formatModified(unix: number): string | null {
    if (!Number.isFinite(unix) || unix <= 0) return null;
    const diffMs = Math.max(0, Date.now() - unix * 1000);
    const sec = Math.round(diffMs / 1000);
    if (sec < 60) return 'now';
    const min = Math.round(sec / 60);
    if (min < 60) return `${min}m`;
    const hr = Math.round(min / 60);
    if (hr < 24) return `${hr}h`;
    const day = Math.round(hr / 24);
    return `${day}d`;
  }

  function sessionMeta(s: EnrichedSession): string {
    const parts = [];
    parts.push(`${s.item_count} items`, `${s.terminal_count} terms`);
    const modified = formatModified(s.modified_at);
    if (modified !== null) parts.push(`modified ${modified}`);
    if (s.tags.length > 0) parts.push(s.tags.map((tag) => `#${tag}`).join(' '));
    return parts.join(' · ');
  }

  function workspacePath(s: EnrichedSession): string {
    const path = s.workspace_root.trim();
    return path.length > 0 ? path : 'Workspace unavailable';
  }
</script>

<Modal
  {open}
  onclose={onClose}
  title="Open existing session"
  dismissOnBackdrop={false}
>
  {#snippet body()}
    <div class="toolbar">
      <label class="search-field">
        <svg class="search-icon" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="11" cy="11" r="7" />
          <line x1="16.5" y1="16.5" x2="21" y2="21" />
        </svg>
        <input
          bind:value={search}
          type="search"
          aria-label="Search sessions"
          placeholder="Search sessions, tags, folders, workspace"
          class="search-input"
        />
      </label>
    </div>

    {#if loading}
      <p class="state">Loading sessions…</p>
    {:else if errorMessage !== null}
      <p class="state error" role="alert">{errorMessage}</p>
    {:else if sessions.length === 0}
      <p class="state">No sessions yet.</p>
    {:else if rows.length === 0}
      <p class="state">No matching sessions.</p>
    {:else}
      <ul class="tree" role="listbox" aria-label="Workspace sessions">
        {#each rows as row (row.id)}
          {#if row.kind === 'folder'}
            <li class="folder-row" style={`padding-left: ${row.depth * 18}px`}>
              <button type="button" class="folder-main" onclick={() => void toggleFolder(row.folder)}>
                <span class="glyph" aria-hidden="true">{row.folder.collapsed ? '›' : '⌄'}</span>
                <span class="glyph" aria-hidden="true">□</span>
                <span class="folder-name">{row.folder.name}</span>
                <span class="count">{row.childCount}</span>
              </button>
              <Dropdown>
                {#snippet trigger({ toggle })}
                  <IconButton aria-label="Folder actions" title="Folder actions" size="sm" onclick={toggle}>
                    <span class="glyph" aria-hidden="true">...</span>
                  </IconButton>
                {/snippet}
                {#snippet menu({ close })}
                  <button
                    type="button"
                    onclick={() => {
                      onCreateInFolder?.(row.folder.id);
                      close();
                    }}
                  >
                    <span class="glyph" aria-hidden="true">+</span>
                    <span>New session here</span>
                  </button>
                  <button
                    type="button"
                    class="danger"
                    onclick={() => {
                      pendingDeleteFolder = row.folder;
                      close();
                    }}
                  >
                    <span class="glyph" aria-hidden="true">×</span>
                    <span>Delete folder</span>
                  </button>
                {/snippet}
              </Dropdown>
            </li>
          {:else}
            <li class="session-row-wrap" style={`padding-left: ${row.depth * 18}px`}>
              <button
                type="button"
                class:disabled={row.session.active}
                class="session-row"
                disabled={row.session.active}
                title={row.session.active ? 'In use by another webpage' : undefined}
                onclick={() => onSelect(row.session.name)}
              >
                <span class="session-main">
                  <span class="session-name">
                    {row.session.name}
                  </span>
                  <span class="session-workspace" title={workspacePath(row.session)}>
                    <span class="workspace-label">Workspace</span>
                    <span class="workspace-path mono">{workspacePath(row.session)}</span>
                  </span>
                  <span class="session-meta">{sessionMeta(row.session)}</span>
                </span>
                {#if row.session.active}
                  <span class="badge">in use</span>
                {/if}
              </button>
              <Dropdown>
                {#snippet trigger({ toggle })}
                  <IconButton aria-label="Session actions" title="Session actions" size="sm" onclick={toggle}>
                    <span class="glyph" aria-hidden="true">...</span>
                  </IconButton>
                {/snippet}
                {#snippet menu({ close })}
                  <button
                    type="button"
                    disabled={!canRenameOrDuplicate(row.session)}
                    onclick={() => {
                      beginRename(row.session);
                      close();
                    }}
                  >
                    <span class="glyph" aria-hidden="true">✎</span>
                    <span>Rename</span>
                  </button>
                  <button
                    type="button"
                    disabled={!canRenameOrDuplicate(row.session)}
                    onclick={() => {
                      beginDuplicate(row.session);
                      close();
                    }}
                  >
                    <span class="glyph" aria-hidden="true">⧉</span>
                    <span>Duplicate</span>
                  </button>
                  <button
                    type="button"
                    onclick={() => {
                      pendingWorkspaceSession = row.session;
                      close();
                    }}
                  >
                    <span class="glyph" aria-hidden="true">□</span>
                    <span>Change workspace</span>
                  </button>
                  <button
                    type="button"
                    class="danger"
                    disabled={row.session.active || !canDelete(row.session.name)}
                    onclick={() => {
                      pendingDeleteName = row.session.name;
                      close();
                    }}
                  >
                    <span class="glyph" aria-hidden="true">×</span>
                    <span>Remove</span>
                  </button>
                {/snippet}
              </Dropdown>
            </li>
          {/if}
        {/each}
      </ul>
    {/if}
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={onClose}>Cancel</Button>
  {/snippet}
</Modal>

<SessionDeleteConfirmModal
  open={pendingDeleteName !== null}
  sessionName={pendingDeleteName ?? ''}
  onCancel={() => (pendingDeleteName = null)}
  onConfirm={() => void onConfirmDelete()}
/>

<FileExplorer
  open={pendingWorkspaceSession !== null}
  mode="dir"
  title="Change workspace"
  initialDir={pendingWorkspaceSession?.workspace_root ?? ''}
  onCancel={() => (pendingWorkspaceSession = null)}
  onPick={(path) => void submitWorkspaceChange(path)}
/>

<Modal
  open={pendingRenameSession !== null}
  onclose={closeRenameDialog}
  title="Rename session"
  dismissOnBackdrop={!renameSubmitting}
  dismissOnEsc={!renameSubmitting}
>
  {#snippet body()}
    <div class="form-stack">
      <p class="modal-copy">
        Current name: <span class="mono">{pendingRenameSession?.name}</span>
      </p>
      <Input
        bind:value={renameName}
        label="New session name"
        placeholder="session_name"
        autofocus
        disabled={renameSubmitting}
        error={renameError ?? renameInputError}
        oninput={() => (renameError = null)}
        onkeydown={(e) => {
          if (e.key === 'Enter') void submitRename();
        }}
      />
    </div>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" disabled={renameSubmitting} onclick={closeRenameDialog}>Cancel</Button>
    <Button
      variant="primary"
      disabled={renameSubmitting || renameInputError !== null}
      onclick={() => void submitRename()}
    >
      {renameSubmitting ? 'Renaming…' : 'Rename'}
    </Button>
  {/snippet}
</Modal>

<Modal
  open={pendingDuplicateSession !== null}
  onclose={closeDuplicateDialog}
  title="Duplicate session"
  dismissOnBackdrop={!duplicateSubmitting}
  dismissOnEsc={!duplicateSubmitting}
>
  {#snippet body()}
    <div class="form-stack">
      <p class="modal-copy">
        Source: <span class="mono">{pendingDuplicateSession?.name}</span>
      </p>
      <Input
        bind:value={duplicateName}
        label="New session name"
        placeholder="session_copy"
        autofocus
        disabled={duplicateSubmitting}
        error={duplicateError ?? duplicateInputError}
        oninput={() => (duplicateError = null)}
        onkeydown={(e) => {
          if (e.key === 'Enter') void submitDuplicate();
        }}
      />
      <label class="select-field">
        <span class="select-label">Folder</span>
        <select class="select-control" bind:value={duplicateFolderId} disabled={duplicateSubmitting}>
          <option value={null}>Workspace root</option>
          {#each moveMenuFolders as folder (folder.id)}
            <option value={folder.id}>{workspaceManifest.folderPath(folder.id)}</option>
          {/each}
        </select>
      </label>
    </div>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" disabled={duplicateSubmitting} onclick={closeDuplicateDialog}>Cancel</Button>
    <Button
      variant="primary"
      disabled={duplicateSubmitting || duplicateInputError !== null}
      onclick={() => void submitDuplicate()}
    >
      {duplicateSubmitting ? 'Duplicating…' : 'Duplicate'}
    </Button>
  {/snippet}
</Modal>

<Modal
  open={pendingDeleteFolder !== null}
  onclose={() => (pendingDeleteFolder = null)}
  title="Delete folder"
  dismissOnBackdrop={false}
>
  {#snippet body()}
    <p class="state">
      Delete folder "{pendingDeleteFolder?.name}"? Sessions move to root.
    </p>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (pendingDeleteFolder = null)}>Cancel</Button>
    <Button variant="danger" onclick={() => void confirmDeleteFolder()}>Delete</Button>
  {/snippet}
</Modal>

<style>
  .toolbar {
    margin-bottom: var(--space-12);
  }

  .search-field {
    position: relative;
    display: block;
  }

  .search-icon {
    position: absolute;
    left: var(--space-10);
    top: 50%;
    transform: translateY(-50%);
    color: var(--color-fg-subtle);
    pointer-events: none;
  }

  .search-input {
    width: 100%;
    height: 32px;
    box-sizing: border-box;
    padding: 0 var(--space-12) 0 34px;
    background: var(--color-surface);
    color: var(--color-fg);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    font-family: inherit;
    font-size: var(--text-base);
    transition: border-color var(--motion-fast) var(--motion-easing);
  }

  .search-input:hover {
    border-color: var(--color-fg-subtle);
  }

  .search-input:focus-visible {
    outline: 2px solid var(--color-info);
    outline-offset: 1px;
    border-color: var(--color-info);
  }

  .state {
    margin: 0;
    padding: var(--space-24) 0;
    text-align: center;
    color: var(--color-fg-muted);
    font-size: var(--text-md);
  }

  .state.error {
    color: var(--color-danger);
  }

  .form-stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-12);
  }

  .modal-copy {
    margin: 0;
    color: var(--color-fg-muted);
    font-size: var(--text-base);
  }

  .mono {
    font-family: var(--font-mono);
    color: var(--color-fg);
  }

  .select-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-16);
  }

  .select-label {
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
    font-weight: var(--weight-medium);
  }

  .select-control {
    height: 32px;
    padding: 0 var(--space-12);
    background: var(--color-surface);
    color: var(--color-fg);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    font-family: inherit;
    font-size: var(--text-base);
  }

  .select-control:focus-visible {
    outline: 2px solid var(--color-info);
    outline-offset: 1px;
    border-color: var(--color-info);
  }

  .select-control:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .tree {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    max-height: min(58vh, 560px);
    overflow: auto;
  }

  .folder-row,
  .session-row-wrap {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--space-4);
    align-items: center;
  }

  .folder-main,
  .session-row {
    width: 100%;
    min-height: 42px;
    display: flex;
    align-items: center;
    gap: var(--space-8);
    padding: var(--space-8) var(--space-10);
    background: var(--color-surface-2);
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    color: var(--color-fg);
    text-align: left;
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing);
  }

  .folder-main:hover,
  .session-row:hover:not(.disabled) {
    background: var(--color-glass-1);
    border-color: var(--color-border-strong);
  }

  .folder-name,
  .session-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder-name {
    font-weight: var(--weight-medium);
  }

  .count,
  .badge {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    border-radius: var(--radius-pill);
    border: 1px solid var(--color-border);
    color: var(--color-fg-muted);
    background: var(--color-surface);
    padding: 1px 6px;
    flex-shrink: 0;
  }

  .session-main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .session-name {
    display: flex;
    align-items: center;
    gap: var(--space-6);
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }

  .session-workspace {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: var(--space-6);
    min-width: 0;
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
  }

  .workspace-label {
    font-family: var(--font-mono);
    color: var(--color-fg-subtle);
    text-transform: uppercase;
    font-size: 10px;
    letter-spacing: 0.4px;
  }

  .workspace-path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg-muted);
  }

  .session-meta {
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .session-row.disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .glyph {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 14px;
    font-size: var(--text-base);
    line-height: 1;
  }
</style>
