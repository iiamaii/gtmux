<script lang="ts">
  /**
   * FilePickerModal — file system picker for the `file_path` tool.
   *
   * 정본: ADR-0035 D5 (UI form), 0061 BE work-package §3 (FE wire).
   *
   * MVP: workspace root only (BE Stage 1 ADR-0035 D10). External roots
   * (D2.1 picker.roots) land in Stage 3 — `[+ Add browse root]` is hidden
   * until that ship.
   *
   * UX:
   *   - Open at workspace root (empty dir query).
   *   - Breadcrumb segments → navigate up.
   *   - Double-click directory → enter.
   *   - Double-click file → select + close (modal commit with absolute path).
   *   - Single-click → "Selected" footer + Select button enabled.
   *   - Filter input (client-side fuzzy contains-match on entries).
   *   - Cancel / Esc → close (no commit, item not spawned by caller).
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { listDir, DirNotAllowedError, DirNotFoundError, type FsEntry } from '$lib/http/fs';
  import { UnauthorizedError } from '$lib/http/sessions';

  interface Props {
    open: boolean;
    onCancel: () => void;
    onSelect: (absolutePath: string) => void;
    onUnauthorized?: () => void;
    /** Optional initial path to open at. Empty = workspace root. */
    initialDir?: string;
  }

  const { open, onCancel, onSelect, onUnauthorized, initialDir = '' }: Props = $props();

  let currentDir = $state<string>('');
  let parentDir = $state<string | null>(null);
  let entries = $state<FsEntry[]>([]);
  let total = $state(0);
  let truncated = $state(false);
  let loading = $state(false);
  let errorMessage = $state<string | null>(null);
  let selectedName = $state<string | null>(null);
  let filter = $state('');
  /** Per-session override of Settings.picker_show_hidden (ADR-0035 D7).
   * undefined → BE Settings 값 사용 (default false). true/false → override.
   * checkbox toggle 으로 변경. */
  let showHidden = $state<boolean | undefined>(undefined);

  const filteredEntries = $derived.by((): FsEntry[] => {
    if (filter.length === 0) return entries;
    const needle = filter.toLowerCase();
    return entries.filter((e) => e.name.toLowerCase().includes(needle));
  });

  const selectedAbsolute = $derived.by((): string | null => {
    if (selectedName === null) return null;
    if (currentDir.endsWith('/')) return `${currentDir}${selectedName}`;
    return `${currentDir}/${selectedName}`;
  });

  const breadcrumbSegments = $derived.by((): { name: string; abs: string }[] => {
    if (currentDir.length === 0) return [];
    const parts = currentDir.split('/').filter((p) => p.length > 0);
    const segs: { name: string; abs: string }[] = [];
    let acc = '';
    for (const part of parts) {
      acc = `${acc}/${part}`;
      segs.push({ name: part, abs: acc });
    }
    return segs;
  });

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
        errorMessage = 'Directory is outside the workspace.';
      } else if (err instanceof DirNotFoundError) {
        errorMessage = 'Directory not found.';
      } else {
        errorMessage = err instanceof Error ? err.message : String(err);
      }
    } finally {
      loading = false;
    }
  }

  // Open lifecycle — fetch on every open.
  $effect(() => {
    if (open) {
      void navigate(initialDir);
    } else {
      // reset on close so a re-open starts fresh
      currentDir = '';
      parentDir = null;
      entries = [];
      total = 0;
      truncated = false;
      errorMessage = null;
      selectedName = null;
      filter = '';
    }
  });

  function onEntryClick(entry: FsEntry): void {
    selectedName = entry.name;
  }

  function onEntryDblClick(entry: FsEntry): void {
    if (entry.kind === 'directory') {
      const next = currentDir.endsWith('/') ? `${currentDir}${entry.name}` : `${currentDir}/${entry.name}`;
      void navigate(next);
    } else {
      // file → select + close
      const abs = currentDir.endsWith('/') ? `${currentDir}${entry.name}` : `${currentDir}/${entry.name}`;
      onSelect(abs);
    }
  }

  function onSelectClick(): void {
    if (selectedAbsolute === null) return;
    onSelect(selectedAbsolute);
  }

  function onSegmentClick(abs: string): void {
    void navigate(abs);
  }

  function onUpClick(): void {
    if (parentDir === null) return;
    void navigate(parentDir);
  }

  function onToggleHidden(e: Event): void {
    const checked = (e.currentTarget as HTMLInputElement).checked;
    showHidden = checked;
    void navigate(currentDir);
  }

  function fmtSize(bytes: number | null): string {
    if (bytes === null) return '';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
</script>

<Modal
  {open}
  onclose={onCancel}
  title="Pick a file"
  dismissOnBackdrop={true}
  dismissOnEsc={true}
>
  {#snippet body()}
    <!-- Breadcrumb -->
    <div class="picker-crumbs mono" aria-label="Path">
      <button
        type="button"
        class="crumb crumb-root"
        disabled={parentDir === null}
        onclick={onUpClick}
        aria-label="Up to parent"
        title="Up"
      >
        ↑
      </button>
      <span class="crumb crumb-cur" title={currentDir}>{currentDir}</span>
    </div>

    <!-- Filter + Show-hidden toggle (ADR-0035 D7) -->
    <div class="picker-filter">
      <input
        type="text"
        class="text-input mono"
        placeholder="Filter…"
        value={filter}
        oninput={(e) => (filter = (e.currentTarget as HTMLInputElement).value)}
        autocomplete="off"
      />
      <label class="hidden-toggle mono" title="Show files / dirs starting with a dot (e.g. .git, .env). Server default = off.">
        <input
          type="checkbox"
          checked={showHidden === true}
          onchange={onToggleHidden}
        />
        <span>Show hidden</span>
      </label>
    </div>

    <!-- Entries -->
    <div class="picker-list-wrap">
      {#if loading}
        <p class="state">Loading…</p>
      {:else if errorMessage !== null}
        <p class="state error" role="alert">{errorMessage}</p>
      {:else if filteredEntries.length === 0}
        <p class="state">
          {filter.length > 0 ? 'No matches.' : 'Empty directory.'}
        </p>
      {:else}
        <ul class="picker-list" role="listbox" aria-label="Entries">
          {#each filteredEntries as e (e.name)}
            <li>
              <button
                type="button"
                class="picker-row"
                class:selected={selectedName === e.name}
                ondblclick={() => onEntryDblClick(e)}
                onclick={() => onEntryClick(e)}
              >
                {#if e.kind === 'directory'}
                  <svg class="icon" width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
                    <path d="M1.5 3.5a1 1 0 0 1 1-1h3l1.2 1.5h5.3a1 1 0 0 1 1 1V11a1 1 0 0 1-1 1H2.5a1 1 0 0 1-1-1V3.5z" />
                  </svg>
                {:else}
                  <svg class="icon" width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
                    <path d="M3.5 1.5h4.5L11 4.5V12.5H3.5V1.5z" />
                    <path d="M8 1.5v3h3" />
                  </svg>
                {/if}
                <span class="name" title={e.name}>{e.name}</span>
                <span class="size">{fmtSize(e.size_bytes)}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      {#if truncated}
        <p class="state truncated">
          Showing first {entries.length} of {total}. Use filter to narrow.
        </p>
      {/if}
    </div>

    <!-- Selected footer -->
    <div class="picker-selected mono" title={selectedAbsolute ?? ''}>
      {selectedAbsolute ?? '— Select a file —'}
    </div>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={onCancel}>Cancel</Button>
    <Button
      variant="primary"
      onclick={onSelectClick}
      disabled={selectedAbsolute === null}
    >Select</Button>
  {/snippet}
</Modal>

<style>
  .picker-crumbs {
    display: flex;
    align-items: center;
    gap: var(--space-6);
    padding: var(--space-6) var(--space-8);
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    margin-bottom: var(--space-8);
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
    min-width: 0;
  }

  .crumb-root {
    width: 20px;
    height: 20px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    cursor: pointer;
    flex-shrink: 0;
  }

  .crumb-root:hover:not(:disabled) {
    color: var(--color-fg);
    border-color: var(--color-fg);
  }

  .crumb-root:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .crumb-cur {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg);
    flex: 1 1 auto;
    min-width: 0;
  }

  .picker-filter {
    display: flex;
    align-items: center;
    gap: var(--space-8);
    margin-bottom: var(--space-8);
  }

  .picker-filter .text-input {
    flex: 1 1 auto;
    min-width: 0;
  }

  .hidden-toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
    cursor: pointer;
    user-select: none;
  }

  .hidden-toggle input[type='checkbox'] {
    accent-color: var(--color-accent);
    cursor: pointer;
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

  .picker-list-wrap {
    max-height: 360px;
    min-height: 220px;
    overflow-y: auto;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface);
  }

  .picker-list {
    list-style: none;
    margin: 0;
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .picker-row {
    width: 100%;
    display: grid;
    grid-template-columns: 18px 1fr auto;
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

  .picker-row:hover {
    background: var(--color-glass-1);
  }

  .picker-row.selected {
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    border-color: var(--color-accent);
  }

  .picker-row .icon {
    color: var(--color-fg-muted);
    flex-shrink: 0;
  }

  .picker-row .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .picker-row .size {
    color: var(--color-fg-subtle);
    font-size: 11px;
    flex-shrink: 0;
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

  .picker-selected {
    margin-top: var(--space-8);
    padding: 6px 10px;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
