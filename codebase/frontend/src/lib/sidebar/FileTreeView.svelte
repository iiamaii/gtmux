<script lang="ts">
  /**
   * FileTreeView — Project Workspace file tree.
   *
   * Separate from LayerTreeView by design: same visual language, different data
   * model (filesystem vs canvas layout).
   */

  import { chromeStore } from '$lib/stores/chrome.svelte';
  import { filePreviewStore } from '$lib/stores/filePreview.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { workspaceManifest } from '$lib/stores/workspaceManifest.svelte';
  import {
    DirNotAllowedError,
    DirNotFoundError,
    listDir,
    type FsEntry,
  } from '$lib/http/fs';
  import {
    changeWorkspace,
    UnauthorizedError,
    WorkspaceUpdateUnavailableError,
  } from '$lib/http/sessions';
  import FileExplorer from '$lib/chrome/FileExplorer.svelte';

  type Row = {
    path: string;
    entry: FsEntry;
    depth: number;
    expanded: boolean;
    loading: boolean;
  };
  type FileIconKind = 'directory' | 'image' | 'document' | 'code' | 'text' | 'archive' | 'file';

  let rootPath = $state('');
  let rootError = $state<string | null>(null);
  let rootLoading = $state(false);
  let changeOpen = $state(false);
  let loadedKey = $state('');
  let childrenByDir = $state(new Map<string, FsEntry[]>());
  let expandedDirs = $state(new Set<string>());
  let loadingDirs = $state(new Set<string>());
  let errorByDir = $state(new Map<string, string>());

  const activeName = $derived(sessionStore.active?.name ?? null);
  const activeSession = $derived(
    activeName === null
      ? null
      : workspaceManifest.sessions.find((session) => session.name === activeName) ?? null,
  );
  const targetRoot = $derived(activeSession?.workspace_root ?? '');
  const rows = $derived.by(() => flattenRows());

  function joinPath(dir: string, name: string): string {
    if (dir.length === 0 || dir.endsWith('/')) return `${dir}${name}`;
    return `${dir}/${name}`;
  }

  function displayPath(path: string): string {
    return path || 'Server workspace';
  }

  function extension(path: string): string {
    const name = path.split('/').filter(Boolean).pop()?.toLowerCase() ?? path.toLowerCase();
    const dot = name.lastIndexOf('.');
    return dot < 0 ? '' : name.slice(dot + 1);
  }

  function fileIconKind(row: Row): FileIconKind {
    if (row.entry.kind === 'directory') return 'directory';
    const ext = extension(row.path);
    if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'avif'].includes(ext)) return 'image';
    if (['md', 'markdown', 'pdf', 'doc', 'docx', 'rtf'].includes(ext)) return 'document';
    if (['ts', 'tsx', 'js', 'jsx', 'svelte', 'rs', 'css', 'html', 'json', 'toml', 'yaml', 'yml'].includes(ext)) return 'code';
    if (['txt', 'log', 'csv', 'tsv'].includes(ext)) return 'text';
    if (['zip', 'tar', 'gz', 'tgz', 'bz2', 'xz', '7z'].includes(ext)) return 'archive';
    return 'file';
  }

  async function loadWorkspaceManifest(): Promise<void> {
    try {
      await workspaceManifest.load();
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      console.debug('[gtmux] file tree manifest load failed', err);
    }
  }

  $effect(() => {
    if (activeName !== null) void loadWorkspaceManifest();
  });

  $effect(() => {
    const key = `${activeName ?? ''}:${targetRoot}`;
    if (activeName === null || key === loadedKey) return;
    loadedKey = key;
    void loadRoot(targetRoot);
  });

  async function loadRoot(dir: string): Promise<void> {
    rootLoading = true;
    rootError = null;
    childrenByDir = new Map();
    expandedDirs = new Set();
    errorByDir = new Map();
    try {
      const res = await listDir(dir);
      rootPath = res.dir;
      childrenByDir = new Map([[res.dir, res.entries]]);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      rootPath = dir;
      rootError = fsErrorMessage(err);
    } finally {
      rootLoading = false;
    }
  }

  async function loadDir(path: string): Promise<void> {
    loadingDirs = new Set(loadingDirs).add(path);
    errorByDir = new Map([...errorByDir].filter(([key]) => key !== path));
    try {
      const res = await listDir(path);
      const nextChildren = new Map(childrenByDir);
      nextChildren.set(path, res.entries);
      childrenByDir = nextChildren;
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      errorByDir = new Map(errorByDir).set(path, fsErrorMessage(err));
    } finally {
      const nextLoading = new Set(loadingDirs);
      nextLoading.delete(path);
      loadingDirs = nextLoading;
    }
  }

  function fsErrorMessage(err: unknown): string {
    if (err instanceof DirNotAllowedError) return 'Directory is outside the server workspace.';
    if (err instanceof DirNotFoundError) return 'Directory not found.';
    return err instanceof Error ? err.message : String(err);
  }

  function flattenRows(): Row[] {
    const out: Row[] = [];
    const walk = (dir: string, depth: number): void => {
      const children = childrenByDir.get(dir) ?? [];
      for (const entry of children) {
        const path = joinPath(dir, entry.name);
        const isDir = entry.kind === 'directory';
        out.push({
          path,
          entry,
          depth,
          expanded: isDir && expandedDirs.has(path),
          loading: isDir && loadingDirs.has(path),
        });
        if (isDir && expandedDirs.has(path)) walk(path, depth + 1);
      }
    };
    if (rootPath.length > 0) walk(rootPath, 0);
    return out;
  }

  function toggleDirectory(path: string): void {
    const next = new Set(expandedDirs);
    if (next.has(path)) {
      next.delete(path);
      expandedDirs = next;
      return;
    }
    next.add(path);
    expandedDirs = next;
    if (!childrenByDir.has(path)) void loadDir(path);
  }

  function onRowClick(row: Row): void {
    if (row.entry.kind === 'directory') {
      toggleDirectory(row.path);
      return;
    }
    filePreviewStore.select(row.path, row.entry);
    chromeStore.setRightPanelTab('preview');
  }

  async function onWorkspacePicked(path: string): Promise<void> {
    const name = activeName;
    if (name === null) return;
    changeOpen = false;
    try {
      await changeWorkspace(name, path);
      await workspaceManifest.load();
      await loadRoot(path);
      toastStore.show({
        message: `Workspace changed to ${path}.`,
        tone: 'success',
      });
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
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
</script>

<!-- Keep inline SVGs here until lucide-svelte's runes-mode build issue is resolved. -->
{#snippet fileIconSvg(row: Row)}
  {@const kind = fileIconKind(row)}
  <span class="type-icon" class:folder-type-icon={kind === 'directory'} aria-hidden="true">
    {#if kind === 'directory'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>
      </svg>
    {:else if kind === 'image'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <rect x="3" y="4" width="18" height="16" rx="2"/>
        <circle cx="9" cy="10" r="1.5"/>
        <path d="M3 17l5-4 4 3 5-5 4 4"/>
      </svg>
    {:else if kind === 'document'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <path d="M6 3h8l4 4v14a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z"/>
        <path d="M14 3v4h4"/>
        <path d="M8 13h8M8 17h5"/>
      </svg>
    {:else if kind === 'code'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <path d="m8 9-4 3 4 3"/>
        <path d="m16 9 4 3-4 3"/>
        <path d="m14 5-4 14"/>
      </svg>
    {:else if kind === 'text'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <path d="M6 4h12"/>
        <path d="M6 8h12"/>
        <path d="M6 12h9"/>
        <path d="M6 16h11"/>
        <path d="M6 20h7"/>
      </svg>
    {:else if kind === 'archive'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <path d="M5 7h14v12a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2z"/>
        <path d="M7 3h10l2 4H5z"/>
        <path d="M10 11h4"/>
      </svg>
    {:else}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <path d="M6 3h8l4 4v14a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z"/>
        <path d="M14 3v4h4"/>
      </svg>
    {/if}
  </span>
{/snippet}

<div class="file-tree">
  <header class="files-head">
    <div class="workspace-meta">
      <span class="eyebrow">Workspace</span>
      <span class="workspace-path mono" title={displayPath(rootPath || targetRoot)}>
        {displayPath(rootPath || targetRoot)}
      </span>
    </div>
    <button
      type="button"
      class="change-btn"
      disabled={activeName === null}
      onclick={() => (changeOpen = true)}
    >
      Change
    </button>
  </header>

  {#if activeName === null}
    <p class="state">Connect a session to browse files.</p>
  {:else if rootLoading}
    <p class="state">Loading files…</p>
  {:else if rootError !== null}
    <p class="state error">{rootError}</p>
  {:else if rows.length === 0}
    <p class="state">Empty workspace.</p>
  {:else}
    <ul class="tree" role="tree" aria-label="Workspace file tree">
      {#each rows as row (row.path)}
        {@const selected = filePreviewStore.selection?.path === row.path}
        <li
          class="row"
          class:selected
          role="treeitem"
          aria-selected={selected}
          aria-expanded={row.entry.kind === 'directory' ? row.expanded : undefined}
        >
          <div
            class="row-inner"
            style:padding-left={`${row.depth * 16 + 4}px`}
          >
            <span
              class="caret"
              class:caret-disabled={row.entry.kind !== 'directory'}
              role="presentation"
              onclick={(e: MouseEvent) => {
                e.stopPropagation();
                if (row.entry.kind === 'directory') toggleDirectory(row.path);
              }}
              onkeydown={() => {}}
            >
              {#if row.entry.kind === 'directory'}
                {row.loading ? '…' : row.expanded ? '▾' : '▸'}
              {/if}
            </span>
            <button
              type="button"
              class="row-button"
              title={row.path}
              onclick={() => onRowClick(row)}
            >
              {@render fileIconSvg(row)}
              <span class="label">{row.entry.name}</span>
            </button>
          </div>
          {#if errorByDir.has(row.path)}
            <p class="row-error">{errorByDir.get(row.path)}</p>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<FileExplorer
  open={changeOpen}
  mode="dir"
  title="Change workspace"
  initialDir={rootPath || targetRoot}
  onCancel={() => (changeOpen = false)}
  onPick={(path) => void onWorkspacePicked(path)}
/>

<style>
  .file-tree {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
    font-size: var(--text-md);
    line-height: var(--leading-normal);
    user-select: none;
  }

  .files-head {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--space-8);
    padding: var(--space-10) var(--space-10) var(--space-8);
    border-bottom: 1px solid var(--color-border);
    flex: 0 0 auto;
  }

  .workspace-meta {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .eyebrow {
    font-family: var(--font-mono);
    font-size: 10px;
    text-transform: uppercase;
    color: var(--color-fg-subtle);
    letter-spacing: 0.6px;
  }

  .mono {
    font-family: var(--font-mono);
  }

  .workspace-path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
  }

  .change-btn {
    align-self: end;
    height: 24px;
    padding: 0 var(--space-8);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
    color: var(--color-fg);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .change-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .tree {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    list-style: none;
    margin: 0;
    padding: var(--space-4) 0;
  }

  .row {
    display: block;
    position: relative;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .row + .row {
    margin-top: 2px;
  }

  .row-inner {
    display: flex;
    align-items: center;
    gap: 0;
    width: 100%;
    min-width: 0;
    transition: box-shadow var(--motion-fast) var(--motion-easing);
  }

  .row:hover {
    background: var(--color-glass-1);
  }

  .row.selected {
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    color: var(--color-accent);
  }

  .row.selected .row-inner {
    box-shadow: inset 2px 0 0 var(--color-accent);
  }

  .row-button {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex: 1 1 auto;
    min-width: 0;
    padding: var(--space-4) var(--space-8) var(--space-4) 0;
    background: transparent;
    border: 0;
    color: inherit;
    text-align: left;
    cursor: pointer;
    font: inherit;
  }

  .caret {
    width: 16px;
    flex: 0 0 16px;
    display: inline-block;
    text-align: left;
    color: var(--color-fg-muted);
    cursor: pointer;
    user-select: none;
  }

  .caret-disabled {
    cursor: default;
    color: transparent;
  }

  .label {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .type-icon {
    flex: 0 0 16px;
    width: 16px;
    height: 16px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-family: var(--font-mono);
    font-size: var(--text-base);
    color: var(--color-fg-muted);
  }

  .folder-type-icon {
    color: var(--color-accent);
  }

  .row.selected .type-icon {
    color: var(--color-accent);
  }

  .state {
    margin: 0;
    padding: var(--space-16) var(--space-12);
    color: var(--color-fg-muted);
    text-align: center;
    font-size: var(--text-md);
  }

  .state.error,
  .row-error {
    color: var(--color-danger);
  }

  .row-error {
    margin: 2px 0 4px 28px;
    font-size: var(--text-sm);
  }
</style>
