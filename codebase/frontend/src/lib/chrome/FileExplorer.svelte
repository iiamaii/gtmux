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
    }
  });

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

  function onToggleHidden(e: Event): void {
    showHidden = (e.currentTarget as HTMLInputElement).checked;
    void navigate(currentDir);
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
      await navigate(currentDir);
      selectedName = name;
    } catch (err) {
      errorMessage = mutationErrorMessage(err, 'Create directory failed');
    } finally {
      mutating = false;
    }
  }

  async function removeSelectedDirectory(): Promise<void> {
    if (selectedAbsolute === null || selectedEntry?.kind !== 'directory') return;
    mutating = true;
    errorMessage = null;
    try {
      await rmdirFs(selectedAbsolute);
      await navigate(currentDir);
    } catch (err) {
      errorMessage = mutationErrorMessage(err, 'Remove directory failed');
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
</script>

<Modal {open} onclose={onCancel} title={modalTitle} dismissOnBackdrop={true} dismissOnEsc={true}>
  {#snippet body()}
    <div class="explorer-path mono">
      <button
        type="button"
        class="path-up"
        disabled={parentDir === null}
        onclick={onUpClick}
        aria-label="Up to parent"
        title="Up"
      >↑</button>
      <span class="path-current" title={currentDir}>{currentDir || 'Server workspace'}</span>
    </div>

    <div class="explorer-toolbar">
      <input
        type="text"
        class="text-input mono"
        placeholder={mode === 'file' && normalizedFilter.length > 0 ? `Filter ${filterDescription}…` : 'Filter…'}
        bind:value={search}
        autocomplete="off"
      />
      <label class="hidden-toggle mono" title="Show dot-prefixed files and directories.">
        <input type="checkbox" checked={showHidden === true} onchange={onToggleHidden} />
        <span>Hidden</span>
      </label>
    </div>

    <div class="mkdir-row">
      <input
        type="text"
        class="text-input mono"
        placeholder="New folder"
        bind:value={newDirName}
        disabled={mutating || currentDir.length === 0}
        onkeydown={(e) => {
          if (e.key === 'Enter') void createDirectory();
        }}
      />
      <Button size="sm" onclick={() => void createDirectory()} disabled={mutating || newDirName.trim().length === 0}>
        Mkdir
      </Button>
      <Button
        size="sm"
        variant="ghost"
        onclick={() => void removeSelectedDirectory()}
        disabled={!canRemoveSelectedDir}
      >
        Rmdir
      </Button>
    </div>

    <div class="explorer-list-wrap">
      {#if loading}
        <p class="state">Loading…</p>
      {:else if errorMessage !== null}
        <p class="state error" role="alert">{errorMessage}</p>
      {:else if filteredEntries.length === 0}
        <p class="state">{search.length > 0 ? 'No matches.' : 'Empty directory.'}</p>
      {:else}
        <ul class="explorer-list" role="listbox" aria-label="Workspace files">
          {#each filteredEntries as entry (entry.name)}
            <li>
              <button
                type="button"
                class="explorer-row"
                class:selected={selectedName === entry.name}
                class:dimmed={!entryMatchesFilter(entry)}
                onclick={() => onEntryClick(entry)}
                ondblclick={() => onEntryDblClick(entry)}
              >
                <span class="glyph" aria-hidden="true">{entry.kind === 'directory' ? '□' : '·'}</span>
                <span class="name" title={entry.name}>{entry.name}</span>
                <span class="size">{fmtSize(entry.size_bytes)}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      {#if truncated}
        <p class="state truncated">Showing first {entries.length} of {total}. Use filter to narrow.</p>
      {/if}
    </div>

    <div class="selected mono" title={pickTarget ?? ''}>
      {#if pickTarget !== null}
        {mode === 'dir' ? 'Directory' : 'File'}: {pickTarget}
      {:else}
        {mode === 'dir' ? 'Select current or child directory.' : 'Select a file.'}
      {/if}
    </div>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={onCancel} disabled={mutating}>Cancel</Button>
    <Button variant="primary" onclick={onPickClick} disabled={!canPick}>
      {mode === 'dir' ? 'Choose' : 'Select'}
    </Button>
  {/snippet}
</Modal>

<style>
  .mono {
    font-family: var(--font-mono);
  }

  .explorer-path {
    display: flex;
    align-items: center;
    gap: var(--space-6);
    padding: var(--space-6) var(--space-8);
    margin-bottom: var(--space-8);
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    min-width: 0;
    color: var(--color-fg-muted);
  }

  .path-up {
    width: 20px;
    height: 20px;
    padding: 0;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-fg-muted);
    cursor: pointer;
    flex-shrink: 0;
  }

  .path-up:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .path-current {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg);
  }

  .explorer-toolbar,
  .mkdir-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--space-8);
    align-items: center;
    margin-bottom: var(--space-8);
  }

  .mkdir-row {
    grid-template-columns: minmax(0, 1fr) auto auto;
  }

  .text-input {
    height: 28px;
    padding: 0 8px;
    width: 100%;
    box-sizing: border-box;
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    color: var(--color-fg);
  }

  .text-input:focus {
    outline: 2px solid var(--color-accent);
    outline-offset: 0;
    border-color: var(--color-accent);
  }

  .hidden-toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
  }

  .explorer-list-wrap {
    max-height: 360px;
    min-height: 220px;
    overflow-y: auto;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface);
  }

  .explorer-list {
    list-style: none;
    margin: 0;
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .explorer-row {
    width: 100%;
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-8);
    padding: 4px 8px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--color-fg);
    text-align: left;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-base);
  }

  .explorer-row:hover {
    background: var(--color-glass-1);
  }

  .explorer-row.selected {
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    border-color: var(--color-accent);
  }

  .explorer-row.dimmed {
    opacity: 0.45;
  }

  .glyph {
    color: var(--color-fg-muted);
  }

  .name,
  .path-current,
  .selected {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .size {
    color: var(--color-fg-subtle);
    font-size: 11px;
  }

  .state {
    margin: 0;
    padding: var(--space-12);
    text-align: center;
    color: var(--color-fg-muted);
    font-size: var(--text-md);
  }

  .state.error {
    color: var(--color-danger);
  }

  .state.truncated {
    border-top: 1px solid var(--color-border);
    background: var(--color-surface-2);
    padding: 6px 8px;
    font-size: var(--text-sm);
  }

  .selected {
    margin-top: var(--space-8);
    padding: 6px 10px;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
  }
</style>
