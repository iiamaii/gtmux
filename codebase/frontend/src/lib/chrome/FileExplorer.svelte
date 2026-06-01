<script lang="ts">
  /**
   * FileExplorer — unified directory/file picker for Project Workspace flows.
   *
   * Modes:
   * - `dir`: pick current directory or a selected child directory.
   * - `file`: pick a selected file, optionally filtered by extension.
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import {
    DirAlreadyExistsError,
    DirNotAllowedError,
    DirNotEmptyError,
    DirNotFoundError,
    FsApiUnavailableError,
    listDir,
    mkdirFs,
    rmdirFs,
    type FsEntry,
  } from '$lib/http/fs';
  import { UnauthorizedError } from '$lib/http/sessions';

  type Mode = 'dir' | 'file';

  interface Props {
    open: boolean;
    mode?: Mode;
    title?: string;
    onCancel: () => void;
    onPick: (absolutePath: string) => void;
    onUnauthorized?: () => void;
    initialDir?: string;
    filter?: string[];
    filterDescription?: string;
  }

  const {
    open,
    mode = 'file',
    title,
    onCancel,
    onPick,
    onUnauthorized,
    initialDir = '',
    filter = [],
    filterDescription = 'files',
  }: Props = $props();

  let currentDir = $state<string>('');
  let parentDir = $state<string | null>(null);
  let entries = $state<FsEntry[]>([]);
  let total = $state(0);
  let truncated = $state(false);
  let loading = $state(false);
  let mutating = $state(false);
  let errorMessage = $state<string | null>(null);
  let selectedName = $state<string | null>(null);
  let search = $state('');
  let showHidden = $state<boolean | undefined>(undefined);
  let newDirName = $state('');
  let mkdirOpen = $state(false);
  let pendingRemoveDir = $state<string | null>(null);

  const normalizedFilter = $derived(filter.map((ext) => ext.toLowerCase()));
  const modalTitle = $derived(title ?? (mode === 'dir' ? 'Choose workspace root' : 'Pick a file'));

  function joinPath(dir: string, name: string): string {
    if (dir.length === 0 || dir.endsWith('/')) return `${dir}${name}`;
    return `${dir}/${name}`;
  }

  function entryMatchesFilter(entry: FsEntry): boolean {
    if (entry.kind === 'directory') return true;
    if (normalizedFilter.length === 0) return true;
    const lower = entry.name.toLowerCase();
    return normalizedFilter.some((ext) => lower.endsWith(ext));
  }

  const filteredEntries = $derived.by((): FsEntry[] => {
    const accepted = entries.filter(entryMatchesFilter);
    if (search.trim().length === 0) return accepted;
    const needle = search.trim().toLowerCase();
    return accepted.filter((entry) => entry.name.toLowerCase().includes(needle));
  });

  const selectedEntry = $derived.by((): FsEntry | null => {
    if (selectedName === null) return null;
    return entries.find((entry) => entry.name === selectedName) ?? null;
  });

  const selectedAbsolute = $derived(
    selectedName === null ? null : joinPath(currentDir, selectedName),
  );

  const pickTarget = $derived.by((): string | null => {
    if (mode === 'dir') {
      if (selectedEntry?.kind === 'directory' && selectedAbsolute !== null) return selectedAbsolute;
      return currentDir.length > 0 ? currentDir : null;
    }
    if (selectedEntry?.kind !== 'file' || selectedAbsolute === null) return null;
    if (!entryMatchesFilter(selectedEntry)) return null;
    return selectedAbsolute;
  });

  const canPick = $derived(!loading && !mutating && pickTarget !== null);
  const canRemoveSelectedDir = $derived(
    !loading && !mutating && selectedEntry?.kind === 'directory' && selectedAbsolute !== null,
  );
  const breadcrumbSegments = $derived.by(() => buildBreadcrumbSegments(currentDir));

  async function navigate(dir: string): Promise<void> {
    loading = true;
    errorMessage = null;
    selectedName = null;
    try {
      const res = await listDir(dir, { showHidden });
      currentDir = res.dir;
      parentDir = res.parent;
      entries = res.entries;
      total = res.total;
      truncated = res.truncated;
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      if (err instanceof DirNotAllowedError) {
        errorMessage = 'Directory is outside the server workspace.';
      } else if (err instanceof DirNotFoundError) {
        errorMessage = 'Directory not found.';
      } else {
        errorMessage = err instanceof Error ? err.message : String(err);
      }
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (open) {
      void navigate(initialDir);
    } else {
      currentDir = '';
      parentDir = null;
      entries = [];
      total = 0;
      truncated = false;
      loading = false;
      mutating = false;
      errorMessage = null;
      selectedName = null;
      search = '';
      newDirName = '';
      mkdirOpen = false;
      pendingRemoveDir = null;
    }
  });

  type BreadcrumbSegment =
    | { kind: 'segment'; label: string; path: string; current: boolean }
    | { kind: 'ellipsis' };

  function buildBreadcrumbSegments(path: string): BreadcrumbSegment[] {
    if (path.length === 0) {
      return [{ kind: 'segment', label: 'Server workspace', path: '', current: true }];
    }
    const absolute = path.startsWith('/');
    const parts = path.split('/').filter(Boolean);
    if (parts.length === 0) {
      return [{ kind: 'segment', label: '/', path: '/', current: true }];
    }
    const segments: BreadcrumbSegment[] = [];
    let cursor = absolute ? '' : '';
    for (let i = 0; i < parts.length; i += 1) {
      const part = parts[i] ?? '';
      cursor = absolute ? `${cursor}/${part}` : joinPath(cursor, part);
      segments.push({
        kind: 'segment',
        label: part,
        path: cursor,
        current: i === parts.length - 1,
      });
    }
    if (segments.length <= 4) return segments;
    return [
      segments[0] ?? { kind: 'segment', label: '/', path: '/', current: false },
      { kind: 'ellipsis' },
      ...segments.slice(-2),
    ];
  }

  function onEntryClick(entry: FsEntry): void {
    selectedName = entry.name;
  }

  function onEntryDblClick(entry: FsEntry): void {
    const abs = joinPath(currentDir, entry.name);
    if (entry.kind === 'directory') {
      void navigate(abs);
      return;
    }
    if (mode === 'file' && entryMatchesFilter(entry)) onPick(abs);
  }

  function onPickClick(): void {
    if (pickTarget === null) return;
    onPick(pickTarget);
  }

  function onUpClick(): void {
    if (parentDir === null) return;
    void navigate(parentDir);
  }

  async function createDirectory(): Promise<void> {
    const name = newDirName.trim();
    if (name.length === 0 || name.includes('/')) {
      errorMessage = 'Directory name must not be empty or contain slashes.';
      return;
    }
    const path = joinPath(currentDir, name);
    mutating = true;
    errorMessage = null;
    try {
      await mkdirFs(path);
      newDirName = '';
      mkdirOpen = false;
      await navigate(currentDir);
      selectedName = name;
    } catch (err) {
      errorMessage = mutationErrorMessage(err, 'Create directory failed');
    } finally {
      mutating = false;
    }
  }

  function requestRemoveSelectedDirectory(): void {
    if (selectedAbsolute === null || selectedEntry?.kind !== 'directory') return;
    pendingRemoveDir = selectedAbsolute;
  }

  async function confirmRemoveDirectory(): Promise<void> {
    if (pendingRemoveDir === null) return;
    const path = pendingRemoveDir;
    pendingRemoveDir = null;
    mutating = true;
    errorMessage = null;
    try {
      await rmdirFs(path);
      selectedName = null;
      await navigate(currentDir);
    } catch (err) {
      errorMessage = mutationErrorMessage(err, 'Delete folder failed');
    } finally {
      mutating = false;
    }
  }

  function mutationErrorMessage(err: unknown, prefix: string): string {
    if (err instanceof FsApiUnavailableError) {
      return `${prefix}: server does not support workspace directory mutation yet.`;
    }
    if (err instanceof DirNotAllowedError) return `${prefix}: directory is outside the server workspace.`;
    if (err instanceof DirAlreadyExistsError) return `${prefix}: directory already exists.`;
    if (err instanceof DirNotEmptyError) return `${prefix}: directory is not empty.`;
    if (err instanceof UnauthorizedError) {
      onUnauthorized?.();
      return `${prefix}: unauthorized.`;
    }
    return `${prefix}: ${err instanceof Error ? err.message : String(err)}`;
  }

  function fmtSize(bytes: number | null): string {
    if (bytes === null) return '';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function fmtMtime(unix: number | null): string {
    if (unix === null || !Number.isFinite(unix) || unix <= 0) return '';
    const diffMs = Math.max(0, Date.now() - unix * 1000);
    const min = Math.round(diffMs / 60_000);
    if (min < 1) return 'now';
    if (min < 60) return `${min}m ago`;
    const hr = Math.round(min / 60);
    if (hr < 24) return `${hr}h ago`;
    const day = Math.round(hr / 24);
    return `${day}d ago`;
  }

  function isRowDimmed(entry: FsEntry): boolean {
    if (mode === 'dir' && entry.kind !== 'directory') return true;
    return !entryMatchesFilter(entry);
  }

  function stateTitle(): string {
    if (errorMessage === null) return search.length > 0 ? 'No matches' : 'Empty folder';
    if (errorMessage.toLowerCase().includes('outside')) return 'Outside workspace';
    if (errorMessage.toLowerCase().includes('support')) return 'Server update required';
    return 'Unable to read directory';
  }
</script>

{#snippet folderIcon()}
  <svg class="ic" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.2 1.5h5.2A1.5 1.5 0 0 1 14 6v5.5A1.5 1.5 0 0 1 12.5 13h-9A1.5 1.5 0 0 1 2 11.5z"/>
  </svg>
{/snippet}

{#snippet fileIcon()}
  <svg class="ic" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M4 2h5l3 3v9H4z"/>
    <path d="M9 2v3h3"/>
  </svg>
{/snippet}

{#snippet infoIcon()}
  <svg class="ic sm" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
    <circle cx="8" cy="8" r="6"/>
    <path d="M8 5v3.5M8 11h.01"/>
  </svg>
{/snippet}

<Modal
  {open}
  onclose={onCancel}
  title={modalTitle}
  dismissOnBackdrop={true}
  dismissOnEsc={true}
  size="wide"
  flushBody
>
  {#snippet body()}
    <div class="fx-bar">
      <button
        type="button"
        class="icon-btn"
        disabled={parentDir === null}
        onclick={onUpClick}
        aria-label="Up to parent"
        title="Up one level"
      >
        <svg class="ic" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
          <path d="M8 13V4M4 7l4-3 4 3"/>
        </svg>
      </button>
      <div class="crumbs" title={currentDir || 'Server workspace'}>
        {#each breadcrumbSegments as crumb, index (crumb.kind === 'segment' ? crumb.path : 'ellipsis')}
          {#if crumb.kind === 'ellipsis'}
            <span class="seg ellip">...</span>
          {:else}
            {#if index > 0}<span class="sep">/</span>{/if}
            <button
              type="button"
              class="seg"
              class:cur={crumb.current}
              onclick={() => void navigate(crumb.path)}
            >
              {crumb.label}
            </button>
          {/if}
        {/each}
      </div>
      <div class="fx-tools">
        <span class="fx-mode">{mode}</span>
        <button
          type="button"
          class="icon-btn"
          title="New folder"
          aria-label="New folder"
          disabled={mutating || currentDir.length === 0}
          onclick={() => (mkdirOpen = !mkdirOpen)}
        >
          <svg class="ic" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
            <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.2 1.5h5.2A1.5 1.5 0 0 1 14 6v5.5A1.5 1.5 0 0 1 12.5 13h-9A1.5 1.5 0 0 1 2 11.5z"/>
            <path d="M8 7.5v3M6.5 9h3"/>
          </svg>
        </button>
        <button
          type="button"
          class="icon-btn"
          title="Upload here is not available yet"
          aria-label="Upload here"
          disabled
        >
          <svg class="ic" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
            <path d="M8 11V3M5 6l3-3 3 3M3 13h10"/>
          </svg>
        </button>
        <button
          type="button"
          class="icon-btn"
          class:on={showHidden === true}
          title="Show hidden"
          aria-label="Show hidden"
          onclick={() => {
            showHidden = showHidden === true ? false : true;
            void navigate(currentDir);
          }}
        >
          <svg class="ic" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
            <path d="M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z"/>
            <circle cx="8" cy="8" r="2"/>
          </svg>
        </button>
        <button
          type="button"
          class="icon-btn"
          title="Refresh"
          aria-label="Refresh"
          disabled={loading}
          onclick={() => void navigate(currentDir)}
        >
          <svg class="ic" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
            <path d="M13 8a5 5 0 1 1-1.5-3.5M13 3v2.5h-2.5"/>
          </svg>
        </button>
      </div>
    </div>

    <div class="filter-row">
      <input
        type="text"
        class="text-input mono"
        placeholder={mode === 'file' && normalizedFilter.length > 0 ? `Filter ${filterDescription}...` : 'Filter...'}
        bind:value={search}
        autocomplete="off"
      />
      <button
        type="button"
        class="icon-btn danger"
        title="Delete selected empty folder"
        aria-label="Delete selected empty folder"
        onclick={requestRemoveSelectedDirectory}
        disabled={!canRemoveSelectedDir}
      >
        <svg class="ic" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M3 4.5h10M6 4.5V3h4v1.5M5 4.5l.6 8.5h4.8L11 4.5"/>
        </svg>
      </button>
    </div>

    {#if mkdirOpen}
      <div class="fx-mkdir">
        <span class="ico folder">{@render folderIcon()}</span>
        <input
          type="text"
          class="text-input mono compact"
          placeholder="new-folder-name"
          bind:value={newDirName}
          disabled={mutating || currentDir.length === 0}
          onkeydown={(e) => {
            if (e.key === 'Enter') void createDirectory();
            if (e.key === 'Escape') {
              mkdirOpen = false;
              newDirName = '';
            }
          }}
        />
        <Button size="sm" variant="primary" onclick={() => void createDirectory()} disabled={mutating || newDirName.trim().length === 0}>
          Create
        </Button>
      </div>
    {/if}

    <div class="explorer-list-wrap">
      {#if loading}
        <div class="list-state">
          <span class="spin" aria-hidden="true"></span>
          <span class="state-desc">Reading directory...</span>
        </div>
      {:else if errorMessage !== null}
        <div class="list-state danger" role="alert">
          <span class="state-disc">{@render infoIcon()}</span>
          <span class="state-lead">{stateTitle()}</span>
          <span class="state-desc">{errorMessage}</span>
        </div>
      {:else if filteredEntries.length === 0}
        <div class="list-state muted">
          <span class="state-disc">{@render folderIcon()}</span>
          <span class="state-lead">{search.length > 0 ? 'No matches' : 'Empty folder'}</span>
          <span class="state-desc">{search.length > 0 ? 'Refine the filter.' : 'Nothing here yet.'}</span>
        </div>
      {:else}
        <ul class="explorer-list" role="listbox" aria-label="Workspace files">
          {#each filteredEntries as entry (entry.name)}
            <li>
              <button
                type="button"
                class="explorer-row"
                class:dir={entry.kind === 'directory'}
                class:selected={selectedName === entry.name}
                class:dimmed={isRowDimmed(entry)}
                onclick={() => onEntryClick(entry)}
                ondblclick={() => onEntryDblClick(entry)}
              >
                <span class="glyph" aria-hidden="true">
                  {#if entry.kind === 'directory'}
                    {@render folderIcon()}
                  {:else}
                    {@render fileIcon()}
                  {/if}
                </span>
                <span class="name" title={entry.name}>{entry.name}</span>
                <span class="size">{fmtSize(entry.size_bytes)}</span>
                <span class="mtime">{fmtMtime(entry.mtime_unix)}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      {#if truncated}
        <p class="truncated">Showing first {entries.length} of {total}. Refine the filter.</p>
      {/if}
    </div>

    <div class="fx-foot">
      <span class="selected" title={pickTarget ?? ''}>
      {#if pickTarget !== null}
        Selected · <b>{pickTarget}</b>
      {:else}
        {mode === 'dir' ? 'Select current or child directory.' : 'Select a file.'}
      {/if}
      </span>
      <Button variant="ghost" size="sm" onclick={onCancel} disabled={mutating}>Cancel</Button>
      <Button variant="primary" size="sm" onclick={onPickClick} disabled={!canPick}>
        {mode === 'dir' ? 'Select this folder' : 'Open file'}
      </Button>
    </div>
  {/snippet}
</Modal>

<Modal
  open={pendingRemoveDir !== null}
  onclose={() => (pendingRemoveDir = null)}
  title="Delete folder"
  dismissOnBackdrop={!mutating}
  dismissOnEsc={!mutating}
  size="sm"
>
  {#snippet body()}
    <div class="confirm-copy">
      <span class="confirm-disc" aria-hidden="true">
        <svg class="ic" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M3 4.5h10M6 4.5V3h4v1.5M5 4.5l.6 8.5h4.8L11 4.5"/>
        </svg>
      </span>
      <span>
        <strong>Delete <span class="mono">{pendingRemoveDir?.split('/').filter(Boolean).pop() ?? 'folder'}</span>?</strong>
        <small>Only empty directories can be removed. This cannot be undone.</small>
      </span>
    </div>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" size="sm" onclick={() => (pendingRemoveDir = null)} disabled={mutating}>Cancel</Button>
    <Button variant="danger" size="sm" onclick={() => void confirmRemoveDirectory()} disabled={mutating}>Delete</Button>
  {/snippet}
</Modal>

<style>
  .mono {
    font-family: var(--font-mono);
  }

  .ic {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }

  .ic.sm {
    width: 12px;
    height: 12px;
  }

  .fx-bar {
    display: flex;
    align-items: center;
    gap: var(--space-6);
    height: 32px;
    padding: 0 var(--space-6);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-2);
    min-width: 0;
  }

  .crumbs {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    overflow: hidden;
    flex: 1 1 auto;
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-base);
  }

  .seg {
    min-width: 0;
    max-width: 140px;
    padding: 2px 5px;
    border-radius: var(--radius-sm);
    color: inherit;
    background: transparent;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: inherit;
  }

  .seg:hover {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .seg.cur {
    color: var(--color-fg);
    font-weight: var(--weight-semibold);
  }

  .seg.ellip {
    flex: 0 0 auto;
    pointer-events: none;
  }

  .sep {
    opacity: 0.5;
    flex: 0 0 auto;
  }

  .fx-tools {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    flex: 0 0 auto;
  }

  .fx-mode {
    margin-right: var(--space-4);
    padding: 1px 7px;
    border: 1px solid color-mix(in srgb, var(--color-accent) 40%, transparent);
    border-radius: var(--radius-pill);
    color: var(--color-accent);
    font-family: var(--font-mono);
    font-size: 9px;
    letter-spacing: 0.5px;
    line-height: var(--leading-normal);
    text-transform: uppercase;
  }

  .icon-btn {
    width: 24px;
    height: 24px;
    display: inline-grid;
    place-items: center;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-fg-muted);
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .icon-btn:hover:not(:disabled),
  .icon-btn.on {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .icon-btn.on {
    color: var(--color-accent);
  }

  .icon-btn.danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
    color: var(--color-danger);
  }

  .icon-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .filter-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--space-6);
    align-items: center;
    padding: var(--space-8) var(--space-10);
    border-bottom: 1px solid var(--color-border);
  }

  .text-input {
    height: 28px;
    padding: 0 var(--space-10);
    width: 100%;
    min-width: 0;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-fg);
    font-size: var(--text-base);
    letter-spacing: -0.1px;
  }

  .text-input:focus {
    outline: none;
    border-color: var(--color-accent);
    background: var(--color-surface);
  }

  .text-input.compact {
    height: 24px;
    font-size: var(--text-base);
  }

  .fx-mkdir {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr) auto;
    gap: var(--space-10);
    align-items: center;
    min-height: 30px;
    padding: 0 var(--space-12);
    border-bottom: 1px solid color-mix(in srgb, var(--color-accent) 18%, transparent);
    background: color-mix(in srgb, var(--color-accent) 6%, transparent);
  }

  .explorer-list-wrap {
    max-height: 320px;
    min-height: 218px;
    overflow-y: auto;
    background: var(--color-surface);
  }

  .explorer-list {
    list-style: none;
    margin: 0;
    padding: var(--space-4) 0;
  }

  .explorer-row {
    width: 100%;
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr) auto auto;
    align-items: center;
    gap: var(--space-10);
    height: 28px;
    padding: 0 var(--space-12);
    background: transparent;
    border: 0;
    border-radius: 0;
    color: var(--color-fg);
    text-align: left;
    cursor: pointer;
    font-family: var(--font-sans);
    font-size: var(--text-md);
    letter-spacing: -0.1px;
  }

  .explorer-row:hover {
    background: var(--color-glass-1);
  }

  .explorer-row.selected {
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
  }

  .explorer-row.dimmed {
    opacity: 0.44;
  }

  .glyph {
    width: 16px;
    height: 16px;
    display: grid;
    place-items: center;
    color: var(--color-fg-muted);
  }

  .explorer-row.dir .glyph,
  .explorer-row.selected .glyph {
    color: var(--color-accent);
  }

  .name,
  .selected {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .size {
    color: var(--color-fg-subtle);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    white-space: nowrap;
  }

  .mtime {
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    white-space: nowrap;
  }

  .list-state {
    min-height: 218px;
    display: grid;
    place-items: center;
    align-content: center;
    gap: var(--space-8);
    padding: var(--space-24) var(--space-16);
    text-align: center;
    color: var(--color-fg-muted);
  }

  .state-disc {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--color-glass-1);
    color: var(--color-fg-muted);
  }

  .list-state.danger .state-disc {
    background: color-mix(in srgb, var(--color-danger) 14%, transparent);
    color: var(--color-danger);
  }

  .state-lead {
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
    color: var(--color-fg);
  }

  .state-desc {
    max-width: 280px;
    font-size: var(--text-base);
    letter-spacing: -0.1px;
    color: var(--color-fg-muted);
  }

  .spin {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid var(--color-border-strong);
    border-top-color: var(--color-accent);
    animation: spin 900ms linear infinite;
  }

  .truncated {
    margin: 0;
    border-top: 1px solid var(--color-border);
    background: var(--color-surface-2);
    padding: var(--space-6) var(--space-10);
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
  }

  .fx-foot {
    display: flex;
    align-items: center;
    gap: var(--space-10);
    padding: var(--space-10) var(--space-14);
    border-top: 1px solid var(--color-border);
    background: var(--color-surface);
  }

  .selected {
    flex: 1 1 auto;
    margin-right: auto;
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
    letter-spacing: -0.1px;
  }

  .selected b {
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-weight: var(--weight-medium);
  }

  .confirm-copy {
    display: flex;
    gap: var(--space-10);
    align-items: flex-start;
    color: var(--color-fg);
  }

  .confirm-disc {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    flex: 0 0 auto;
    background: color-mix(in srgb, var(--color-danger) 14%, transparent);
    color: var(--color-danger);
  }

  .confirm-copy strong,
  .confirm-copy small {
    display: block;
  }

  .confirm-copy strong {
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
  }

  .confirm-copy small {
    margin-top: var(--space-4);
    color: var(--color-fg-muted);
    font-size: var(--text-base);
    line-height: var(--leading-normal);
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
