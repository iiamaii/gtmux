<script module lang="ts">
  // Module-scoped (persists across FileTreeView remounts — LeftPanel
  // {#if}-unmounts the Files tab). Tracks the workspace/session key the Files
  // selection was last cleared for, so we clear it ONLY when that key actually
  // changes — not on every remount (ADR-0046 D6 amend ⑪: the Files selection
  // persists across tab transitions and is re-displayed on return).
  let lastClearedWorkspaceKey = '';

  // Module-scoped scroll memory (survives remounts, in-memory only — no
  // localStorage write per scroll). One {key, top} for the active workspace, so
  // returning to the Files tab restores the previous scroll offset cheaply.
  let savedTreeScroll: { key: string; top: number } = { key: '', top: 0 };
</script>

<script lang="ts">
  /**
   * FileTreeView — Project Workspace file tree.
   *
   * Separate from LayerTreeView by design: same visual language, different data
   * model (filesystem vs canvas layout).
   */

  import { onMount } from 'svelte';
  import { chromeStore } from '$lib/stores/chrome.svelte';
  import { fileClipboardStore } from '$lib/stores/fileClipboard.svelte';
  import { filePreviewStore } from '$lib/stores/filePreview.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { workspaceManifest } from '$lib/stores/workspaceManifest.svelte';
  import {
    DirNotAllowedError,
    DirNotFoundError,
    DirNotEmptyError,
    FsAlreadyExistsError,
    FsInvalidRequestError,
    FsApiUnavailableError,
    FsInvalidNameError,
    FsMoveCycleError,
    FsNameConflictError,
    FsNotFoundError,
    FsPayloadTooLargeError,
    FsUnsupportedMimeError,
    copyFs,
    listDir,
    mkdirFs,
    moveFs,
    removeFs,
    renameFs,
    searchFs,
    uploadFs,
    type FsEntry,
    type FsSearchEntry,
    type MoveFsEntry,
  } from '$lib/http/fs';
  import {
    changeWorkspace,
    UnauthorizedError,
    WorkspaceUpdateUnavailableError,
  } from '$lib/http/sessions';
  import FileExplorer from '$lib/chrome/FileExplorer.svelte';
  import PanelEmptyState from '$lib/chrome/PanelEmptyState.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
  import { pickLocalFiles } from '$lib/files/localFilePicker';
  import {
    WORKSPACE_FILE_DRAG_MIME,
    encodeWorkspaceFileDragPayload,
    isDocumentPath,
    isImagePath,
  } from '$lib/files/workspaceAssets';
  import { rebindCanvasLayoutPathsForMove } from '$lib/files/workspaceMoveRebind';
  import { commitNewItem, createCanvasItemFromWorkspaceFile } from '$lib/canvas/itemFactory';
  import { shortcutRegistry, type ShortcutDescriptor } from '$lib/keyboard/shortcutRegistry.svelte';
  import { readExpandedTreeState, writeExpandedTreeState } from './treeExpansionState';
  import { matchNamePath } from './treeMatch';
  import { ancestorIndices } from './stickyAncestors';
  import { debounce } from '$lib/common/debounce';

  // ADR-0052 D2 — the search input now lives in the LeftPanel footer (one
  // unified search bar shared across tabs). This component no longer owns the
  // input/clear-button/local query state; it receives the active query text as
  // a prop and reacts to it (Phase 1 client filter + Phase 2 server search, D4).
  let { query = '' }: { query?: string } = $props();

  type Row = {
    path: string;
    entry: FsEntry;
    depth: number;
    expanded: boolean;
    loading: boolean;
  };
  type FileIconKind = 'directory' | 'image' | 'document' | 'code' | 'text' | 'archive' | 'file';
  type FileDragState = {
    sourceRows: Row[];
    sourcePaths: string[];
  };
  // ADR-0052 D4 — a single flat search hit (Phase 1 client row or Phase 2 server
  // result), keyed by absolute `path` for dedupe and ranked for the flat list.
  type SearchResult = {
    path: string;
    entry: FsEntry;
    relpath: string;
    ranges: [number, number][];
  };

  const FILE_TREE_EXPANSION_STORAGE_KEY = 'gtmux:file-tree-expanded:v1';
  const MAX_FILE_TREE_EXPANSIONS = 200;
  // ADR-0052 D7 — sticky parent header stack depth cap.
  const MAX_STICKY = 6;
  // Fallback row height when the live `.row` offsetHeight cannot be measured yet
  // (≈ icon row + 2px row gap; matches the CSS in this file). Replaced by the
  // measured value on first render — ADR-0052 D7.
  const STICKY_ROW_HEIGHT_FALLBACK = 26;
  // ADR-0052 D4 — cap the flat result list so a broad query stays cheap to render.
  const MAX_SEARCH_RESULTS = 500;

  let rootPath = $state('');
  let rootError = $state<string | null>(null);
  let rootLoading = $state(false);
  let changeOpen = $state(false);
  let loadedKey = $state('');
  let childrenByDir = $state(new Map<string, FsEntry[]>());
  let expandedDirs = $state(new Set<string>());
  let loadingDirs = $state(new Set<string>());
  let errorByDir = $state(new Map<string, string>());
  let contextMenu = $state<{
    row: Row | null;
    x: number;
    y: number;
  } | null>(null);
  let contextMenuEl: HTMLDivElement | undefined = $state();
  let mkdirOpen = $state(false);
  let mkdirParentDir = $state('');
  let mkdirName = $state('');
  let mkdirSubmitting = $state(false);
  let mkdirError = $state<string | null>(null);
  let renameOpen = $state(false);
  let renameTarget = $state<Row | null>(null);
  let renameName = $state('');
  let renameSubmitting = $state(false);
  let renameError = $state<string | null>(null);
  let removeTargets = $state<Row[]>([]);
  let removeSubmitting = $state(false);
  let removeError = $state<string | null>(null);
  let uploadConflict = $state<{ dir: string; files: File[] } | null>(null);
  let uploadConflictSubmitting = $state(false);
  let uploadConflictError = $state<string | null>(null);
  let fileDragState = $state<FileDragState | null>(null);
  let dropTargetDir = $state<string | null>(null);
  let moveSubmitting = $state(false);

  // ── Search (ADR-0052 D2/D4) — driven by the `query` PROP (footer-owned). ──
  // Phase 2 (server) state. `serverResults` holds the latest server response for
  // the *current* query; `searchLoading` reflects an in-flight request and
  // `serverTruncated` surfaces the BE `truncated` flag.
  let serverResults = $state<FsSearchEntry[]>([]);
  let serverTruncated = $state(false);
  let searchLoading = $state(false);
  // AbortController for the in-flight Phase 2 request, and a monotonically
  // increasing request id so a slow/stale response is ignored (ADR-0052 D4).
  let searchAbort: AbortController | null = null;
  let searchSeq = 0;

  // ── Sticky parent headers (ADR-0052 D7) ──
  let stickyIndices = $state<number[]>([]);
  let measuredRowHeight = $state(0);
  const searching = $derived(query.trim().length > 0);

  const activeName = $derived(sessionStore.active?.name ?? null);
  const activeSession = $derived(
    activeName === null
      ? null
      : workspaceManifest.sessions.find((session) => session.name === activeName) ?? null,
  );
  const targetRoot = $derived(activeSession?.workspace_root ?? '');
  const rows = $derived.by(() => flattenRows());
  const selectedCount = $derived(filePreviewStore.selectedPaths.size);
  const mkdirValidationError = $derived(validateFolderName(mkdirName));
  const canSubmitMkdir = $derived(
    mkdirOpen && !mkdirSubmitting && mkdirValidationError === null,
  );
  const renameValidationError = $derived(validateFsEntryName(renameName, 'name'));
  const canSubmitRename = $derived(
    renameOpen && !renameSubmitting && renameValidationError === null,
  );
  const uploadTargetDir = $derived(resolveUploadTargetDir());
  const rootDropTargetDir = $derived(rootPath || targetRoot);

  // ── Search results (ADR-0052 D4) ──
  // Phase 1 (client, instant): filter the already-loaded tree rows. Reacts
  // synchronously to `query`/`rows` — no debounce, no network.
  const phase1Results = $derived.by((): SearchResult[] => {
    if (!searching) return [];
    const out: SearchResult[] = [];
    for (const row of rows) {
      const relpath = relpathOf(row.path);
      const match = matchNamePath(query, row.entry.name, relpath);
      if (!match.matched) continue;
      out.push({ path: row.path, entry: row.entry, relpath, ranges: match.ranges });
    }
    return out;
  });

  // Phase 1 + Phase 2 merged, deduped by absolute path (Phase 1 wins — it carries
  // the full FsEntry incl. size/mtime), then ranked. ADR-0052 D4.
  const searchResults = $derived.by((): SearchResult[] => {
    if (!searching) return [];
    const byPath = new Map<string, SearchResult>();
    for (const result of phase1Results) byPath.set(result.path, result);
    for (const hit of serverResults) {
      if (byPath.has(hit.path)) continue; // dedupe — Phase-1 row already present.
      const relpath = relpathOf(hit.path);
      const match = matchNamePath(query, hit.name, relpath);
      // The server already applied the same matcher (D3 shared semantics); recompute
      // only to obtain the name highlight ranges for the flat-list render.
      byPath.set(hit.path, {
        path: hit.path,
        entry: { name: hit.name, kind: hit.kind, size_bytes: null, mtime_unix: null },
        relpath,
        ranges: match.ranges,
      });
    }
    return rankSearchResults([...byPath.values()], query).slice(0, MAX_SEARCH_RESULTS);
  });

  function joinPath(dir: string, name: string): string {
    if (dir.length === 0 || dir.endsWith('/')) return `${dir}${name}`;
    return `${dir}/${name}`;
  }

  function displayPath(path: string): string {
    return path || 'Server workspace';
  }

  function basename(path: string): string {
    const leaf = path.split('/').filter(Boolean).pop();
    return leaf ?? displayPath(path);
  }

  function parentDir(path: string): string {
    const trimmed = path.replace(/\/+$/, '');
    const slash = trimmed.lastIndexOf('/');
    if (slash <= 0) return rootPath || targetRoot;
    return trimmed.slice(0, slash);
  }

  // ADR-0052 D4 — workspace-relative path (strip `rootPath` + leading slash) used
  // as the second match key and the dim dir-context in the flat result list.
  function relpathOf(absPath: string): string {
    const root = rootPath || targetRoot;
    if (root.length === 0) return absPath;
    if (absPath === root) return '';
    const prefix = root.endsWith('/') ? root : `${root}/`;
    if (absPath.startsWith(prefix)) return absPath.slice(prefix.length);
    return absPath;
  }

  // ADR-0052 D4 — dim relative-dir context shown after the name in a result row.
  function resultContextDir(result: SearchResult): string {
    const slash = result.relpath.lastIndexOf('/');
    return slash < 0 ? '' : result.relpath.slice(0, slash);
  }

  // Adapt a flat search result to the `Row` shape so the shared `fileIconSvg`
  // snippet (reads only `entry.kind` + `path`) can render its icon.
  function resultIconRow(result: SearchResult): Row {
    return { path: result.path, entry: result.entry, depth: 0, expanded: false, loading: false };
  }

  function isSameOrChild(path: string, parent: string): boolean {
    const prefix = parent.endsWith('/') ? parent : `${parent}/`;
    return path === parent || path.startsWith(prefix);
  }

  function replacePathPrefix(path: string, oldPrefix: string, newPrefix: string): string {
    if (path === oldPrefix) return newPrefix;
    return `${newPrefix}${path.slice(oldPrefix.length)}`;
  }

  function compactPath(path: string): string {
    const display = displayPath(path);
    const parts = display.split('/').filter(Boolean);
    if (parts.length <= 2) return display;
    return `.../${parts.slice(-2).join('/')}`;
  }

  function resolveUploadTargetDir(): string {
    const selection = filePreviewStore.selection;
    if (selection !== null) {
      if (selection.entry.kind === 'directory') return selection.path;
      return parentDir(selection.path);
    }
    return rootPath || targetRoot;
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
    if (activeName === null) return;
    // ADR-0046 D6 amend ⑪ — clear the Files selection only when the
    // workspace/session actually changed (guard persists across remounts), so
    // the selection survives Files↔Layers/Terminals tab switches and is
    // re-displayed on return. A bare remount (same key) keeps the selection.
    if (key !== lastClearedWorkspaceKey) {
      lastClearedWorkspaceKey = key;
      filePreviewStore.clear();
    }
    // Reload the tree when this instance hasn't loaded this key yet (loadedKey
    // is component-local, so a remount repopulates the local tree state).
    if (key === loadedKey) return;
    loadedKey = key;
    void loadRoot(targetRoot);
  });

  // ── Scroll restore (ADR-0046 D6 amend ⑪ follow-up) ───────────────────
  // Save the scroll offset to module scope on scroll (cheap, in-memory); restore
  // it once the tree has rendered after a remount. Keyed by workspace so an
  // unrelated workspace never inherits a stale offset.
  let treeScrollEl = $state<HTMLElement | undefined>(undefined);
  let scrollRestored = false; // per-mount; resets on remount (new instance).

  function currentTreeScrollKey(): string {
    return `${activeName ?? ''}:${rootPath || targetRoot}`;
  }

  function onTreeScroll(): void {
    closeContextMenu();
    const el = treeScrollEl;
    if (el === undefined) return;
    savedTreeScroll = { key: currentTreeScrollKey(), top: el.scrollTop };
    recomputeSticky(); // ADR-0052 D7 — keep the sticky stack in sync with scroll.
  }

  // ── Sticky parent headers (ADR-0052 D7) ──
  // Uniform-row arithmetic: measure the true per-row SCROLL PITCH, derive the
  // topmost visible row index, then collect its ancestor row indices from the
  // flat `rows` + `depth` via `ancestorIndices`.
  function effectiveRowHeight(): number {
    return measuredRowHeight > 0 ? measuredRowHeight : STICKY_ROW_HEIGHT_FALLBACK;
  }

  // ADR-0052 D7 (bug fix) — the divisor for `topIndex` must be the real per-row
  // PITCH, not a single row's box height. Tree rows carry inter-row spacing
  // (`.row + .row { margin-top }`), so the on-screen step between consecutive
  // rows = offsetHeight + margin. Measuring `offsetHeight` alone yields a divisor
  // a couple px too small, which inflates `topIndex = floor(scrollTop / pitch)`;
  // the error accumulates with scroll distance and points `ancestorIndices` at
  // the wrong row, collapsing the sticky chain to only its shallow head. Measure
  // the pitch directly as the vertical offset between the first two `.row`
  // elements so depth/scroll stay accurate. Only `.row` <li>s exist in the tree
  // <ul> (the taller `.sticky-row`s live in a sibling overlay), so this never
  // measures a sticky/wrapper element.
  function measureRowHeight(): void {
    const el = treeScrollEl;
    if (el === undefined) return;
    const rowEls = el.querySelectorAll('.row');
    const first = rowEls[0] as HTMLElement | undefined;
    if (first === undefined) return;
    const second = rowEls[1] as HTMLElement | undefined;
    if (second !== undefined) {
      // True pitch = vertical step between consecutive rows (includes the
      // inter-row margin). This is the value `topIndex` must divide by.
      const pitch = second.offsetTop - first.offsetTop;
      if (pitch > 0) {
        measuredRowHeight = pitch;
        return;
      }
    }
    // Single-row fallback: box height (margin only appears between rows, so a
    // lone row has none to add). Still better than the static constant.
    if (first.offsetHeight > 0) measuredRowHeight = first.offsetHeight;
  }

  function recomputeSticky(): void {
    // Sticky is hierarchy-only; the flat search list has no ancestors (D7).
    if (searching) {
      if (stickyIndices.length > 0) stickyIndices = [];
      return;
    }
    const el = treeScrollEl;
    if (el === undefined || rows.length === 0) {
      if (stickyIndices.length > 0) stickyIndices = [];
      return;
    }
    if (measuredRowHeight === 0) measureRowHeight();
    const rowHeight = effectiveRowHeight();
    const topIndex = Math.floor(el.scrollTop / rowHeight);
    const next = ancestorIndices(rows, topIndex, MAX_STICKY);
    if (!sameIndices(next, stickyIndices)) stickyIndices = next;
  }

  function sameIndices(a: number[], b: number[]): boolean {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i += 1) if (a[i] !== b[i]) return false;
    return true;
  }

  function onStickyClick(index: number): void {
    const el = treeScrollEl;
    if (el === undefined) return;
    // Scroll the clicked ancestor row to the top of the viewport (D7).
    el.scrollTop = index * effectiveRowHeight();
    recomputeSticky();
  }

  // Recompute the sticky stack whenever the flattened rows change (expand/collapse,
  // lazy load, prune), search toggles, or the live scroll element (re)binds —
  // uniform-row arithmetic depends on `rows` and on `treeScrollEl` existing.
  // Reading `treeScrollEl` here makes this run on mount (once the <ul> binds) and
  // again after the search→tree branch swap re-creates it, not only on scroll.
  // ADR-0052 D7. `measureRowHeight()` runs unconditionally so the pitch is
  // re-measured once a second row exists (upgrading the single-row fallback).
  $effect(() => {
    void rows;
    void searching;
    void treeScrollEl;
    measureRowHeight();
    recomputeSticky();
  });

  $effect(() => {
    // Re-runs as rows hydrate (childrenByDir grows). Restore once the content is
    // tall enough to honor the saved offset, so deep offsets survive async
    // expanded-dir hydration. O(1) work; a few runs during load, then idle.
    void childrenByDir;
    const el = treeScrollEl;
    if (el === undefined || scrollRestored || rootLoading) return;
    if (savedTreeScroll.key !== currentTreeScrollKey() || savedTreeScroll.top <= 0) {
      scrollRestored = true;
      return;
    }
    if (el.scrollHeight - el.clientHeight < savedTreeScroll.top) return; // more rows incoming
    el.scrollTop = savedTreeScroll.top;
    scrollRestored = true;
  });

  $effect(() => {
    if (chromeStore.state.leftPanelTab !== 'files') {
      contextMenu = null;
    }
  });

  // When the flat search list is dismissed (query cleared), the tree `<ul>` is
  // re-created by the `{#if}` branch swap and loses its scrollTop. Re-arm the
  // restore effect so the pre-search offset (still in `savedTreeScroll`, since
  // `onTreeScroll` only fires on the tree) is re-applied. ADR-0052 D4 + amend ⑪.
  let wasSearching = false;
  $effect(() => {
    const now = searching;
    if (wasSearching && !now) scrollRestored = false;
    wasSearching = now;
  });

  // ── Search Phase 2 (ADR-0052 D4) — debounced recursive server search ──
  // `runServerSearch` issues the request; the debounced wrapper coalesces rapid
  // keystrokes (~150ms). Each call cancels the prior in-flight request and bumps
  // `searchSeq`, so a late response from a stale query is dropped.
  const debouncedServerSearch = debounce((q: string, root: string) => {
    void runServerSearch(q, root);
  }, 150);

  async function runServerSearch(q: string, root: string): Promise<void> {
    if (searchAbort !== null) searchAbort.abort();
    const controller = new AbortController();
    searchAbort = controller;
    const seq = ++searchSeq;
    searchLoading = true;
    try {
      const res = await searchFs(root, q, { limit: MAX_SEARCH_RESULTS, signal: controller.signal });
      if (seq !== searchSeq) return; // a newer query superseded this one — ignore.
      serverResults = res.results;
      serverTruncated = res.truncated;
    } catch (err) {
      if (seq !== searchSeq) return;
      // Abort is expected on supersession; ignore it. Any other failure degrades
      // gracefully to Phase-1-only results (D4) — no destructive surface.
      if (err instanceof DOMException && err.name === 'AbortError') return;
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      serverResults = [];
      serverTruncated = false;
      console.debug('[gtmux] fs search failed', err);
    } finally {
      if (seq === searchSeq) searchLoading = false;
    }
  }

  // Drive Phase 2 from `query`/`rootPath`. Empty query cancels any in-flight
  // request and clears server state; a non-empty query schedules a debounced call.
  // Only `query` + root are read here — server-result writes must NOT re-trigger
  // this effect (that would re-issue the search on every response, ADR-0052 D4).
  $effect(() => {
    const q = query.trim();
    const root = rootPath || targetRoot;
    if (q.length === 0 || root.length === 0) {
      cancelServerSearch();
      return;
    }
    debouncedServerSearch(q, root);
  });

  // Cancel the debounce + abort the in-flight request and reset server state.
  // Reads/writes server state imperatively (outside the reactive read graph).
  function cancelServerSearch(): void {
    debouncedServerSearch.cancel();
    if (searchAbort !== null) {
      searchAbort.abort();
      searchAbort = null;
    }
    searchSeq += 1; // invalidate any pending response.
    searchLoading = false;
    serverResults = [];
    serverTruncated = false;
  }

  function onSearchResultClick(result: SearchResult): void {
    // Single-select into the preview store — same contract as a tree row click
    // (ADR-0046 amend ④), so the Preview updates. Does not disturb the tree.
    filePreviewStore.select(result.path, result.entry);
    chromeStore.setRightPanelTab('preview');
  }

  async function loadRoot(dir: string): Promise<void> {
    const sessionName = activeName;
    rootLoading = true;
    rootError = null;
    childrenByDir = new Map();
    expandedDirs = new Set();
    errorByDir = new Map();
    try {
      const res = await listDir(dir);
      rootPath = res.dir;
      sessionStore.setActiveWorkspaceRoot(res.dir);
      expandedDirs = readRestoredExpandedDirs(sessionName, res.dir);
      childrenByDir = new Map([[res.dir, res.entries]]);
      void hydrateExpandedDirs(expandedDirs, res.dir);
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

  async function hydrateExpandedDirs(restored: ReadonlySet<string>, root: string): Promise<void> {
    const dirs = [...restored]
      .filter((path) => path !== root && isPathWithinRoot(path, root))
      .sort((a, b) => a.split('/').length - b.split('/').length)
      .slice(0, MAX_FILE_TREE_EXPANSIONS);
    for (const path of dirs) {
      if (!expandedDirs.has(path) || childrenByDir.has(path)) continue;
      await loadDir(path);
    }
  }

  async function refreshRoot(): Promise<void> {
    if (activeName === null) return;
    await loadRoot(rootPath || targetRoot);
  }

  async function uploadToDir(dir: string): Promise<void> {
    contextMenu = null;
    const files = await pickLocalFiles({ multiple: true });
    if (files.length === 0) return;
    await attemptUpload(dir, files, 'reject');
  }

  async function attemptUpload(
    dir: string,
    files: readonly File[],
    onConflict: 'reject' | 'rename' | 'overwrite',
  ): Promise<void> {
    try {
      await uploadFs(dir, files, onConflict);
      await refreshRoot();
      toastStore.show({
        message: onConflict === 'rename'
          ? 'Uploaded with renamed filename.'
          : onConflict === 'overwrite'
            ? 'Uploaded and overwritten.'
            : `Uploaded ${files.length} file${files.length === 1 ? '' : 's'}.`,
        tone: 'success',
      });
    } catch (err) {
      if (err instanceof FsNameConflictError) {
        uploadConflict = { dir, files: [...files] };
        uploadConflictError = null;
        return;
      }
      toastStore.show({
        message: uploadErrorMessage(err),
        tone: 'error',
        durationMs: 6_000,
      });
    }
  }

  async function submitUploadConflict(policy: 'rename' | 'overwrite'): Promise<void> {
    const pending = uploadConflict;
    if (pending === null) return;
    uploadConflictSubmitting = true;
    uploadConflictError = null;
    try {
      await attemptUpload(pending.dir, pending.files, policy);
      uploadConflict = null;
    } catch (err) {
      uploadConflictError = uploadErrorMessage(err);
    } finally {
      uploadConflictSubmitting = false;
    }
  }

  function closeUploadConflict(): void {
    if (uploadConflictSubmitting) return;
    uploadConflict = null;
    uploadConflictError = null;
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
      // A directory that no longer exists (deleted on disk) must not stay in the
      // persisted expanded set — otherwise it is re-listed and 404s on every
      // Files-tab entry. Prune it (self-healing) instead of surfacing an error.
      if (err instanceof DirNotFoundError) {
        if (expandedDirs.has(path)) {
          const nextExpanded = new Set(expandedDirs);
          nextExpanded.delete(path);
          expandedDirs = nextExpanded;
          persistExpandedDirs();
        }
        const prunedChildren = new Map(childrenByDir);
        prunedChildren.delete(path);
        childrenByDir = prunedChildren;
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

  // ── Search ranking (ADR-0052 D4) ──
  // Tiers: 0 = exact name (case-insensitive), 1 = name-substring (earlier match
  // index first), 2 = path-only match. Stable within a tier by `path` so the
  // order is deterministic across Phase-1/Phase-2 merges and re-renders.
  function rankSearchResults(results: SearchResult[], q: string): SearchResult[] {
    const tokens = q
      .trim()
      .toLowerCase()
      .split(/[\s/]+/)
      .filter((token) => token.length > 0);
    const scored = results.map((result) => ({
      result,
      ...searchTier(result, tokens),
    }));
    scored.sort((a, b) => {
      if (a.tier !== b.tier) return a.tier - b.tier;
      if (a.nameIndex !== b.nameIndex) return a.nameIndex - b.nameIndex;
      return a.result.path < b.result.path ? -1 : a.result.path > b.result.path ? 1 : 0;
    });
    return scored.map((entry) => entry.result);
  }

  // Classify a result into a ranking tier plus the earliest name-substring index.
  function searchTier(
    result: SearchResult,
    tokens: string[],
  ): { tier: number; nameIndex: number } {
    const lowerName = result.entry.name.toLowerCase();
    // Exact whole-name match against the joined query (case-insensitive).
    if (tokens.length > 0 && lowerName === tokens.join('')) {
      return { tier: 0, nameIndex: 0 };
    }
    // Earliest position at which any token appears inside the name; if the name
    // has a highlight range we already know the name matched (use its start).
    let nameIndex = Number.POSITIVE_INFINITY;
    for (const token of tokens) {
      const index = lowerName.indexOf(token);
      if (index >= 0 && index < nameIndex) nameIndex = index;
    }
    if (Number.isFinite(nameIndex)) return { tier: 1, nameIndex };
    // No token in the name → matched via path only.
    return { tier: 2, nameIndex: Number.MAX_SAFE_INTEGER };
  }

  // ADR-0052 D4 — split a name into text-safe segments around the highlight
  // ranges. NEVER use innerHTML; the template renders plain text + <mark> spans.
  function highlightSegments(
    name: string,
    ranges: [number, number][],
  ): { text: string; mark: boolean }[] {
    if (ranges.length === 0) return [{ text: name, mark: false }];
    const segments: { text: string; mark: boolean }[] = [];
    let cursor = 0;
    for (const [start, end] of ranges) {
      const clampedStart = Math.max(cursor, Math.min(start, name.length));
      const clampedEnd = Math.max(clampedStart, Math.min(end, name.length));
      if (clampedStart > cursor) {
        segments.push({ text: name.slice(cursor, clampedStart), mark: false });
      }
      if (clampedEnd > clampedStart) {
        segments.push({ text: name.slice(clampedStart, clampedEnd), mark: true });
      }
      cursor = clampedEnd;
    }
    if (cursor < name.length) segments.push({ text: name.slice(cursor), mark: false });
    return segments;
  }

  function toggleDirectory(path: string): void {
    const next = new Set(expandedDirs);
    if (next.has(path)) {
      next.delete(path);
      expandedDirs = next;
      persistExpandedDirs();
      return;
    }
    next.add(path);
    expandedDirs = next;
    persistExpandedDirs();
    if (!childrenByDir.has(path)) void loadDir(path);
  }

  function rowSelectionEntry(row: Row): { path: string; entry: FsEntry } {
    return { path: row.path, entry: row.entry };
  }

  function rowsForPaths(paths: Iterable<string>): Row[] {
    const wanted = new Set(paths);
    return rows.filter((row) => wanted.has(row.path));
  }

  function selectedRowsForAction(): Row[] {
    return rowsForPaths(filePreviewStore.selectedPaths);
  }

  function contextRowsForAction(): Row[] {
    const menu = contextMenu;
    if (menu?.row === null || menu === null) return [];
    if (selectedCount >= 2 && filePreviewStore.selectedPaths.has(menu.row.path)) {
      return selectedRowsForAction();
    }
    return [menu.row];
  }

  function visibleRangeRows(a: string, b: string): Row[] {
    const ia = rows.findIndex((row) => row.path === a);
    const ib = rows.findIndex((row) => row.path === b);
    if (ia < 0 || ib < 0) return [];
    const [lo, hi] = ia <= ib ? [ia, ib] : [ib, ia];
    return rows.slice(lo, hi + 1);
  }

  function applyRowSelection(
    selectedRows: readonly Row[],
    primaryPath: string | null,
    anchorPath: string | null = primaryPath,
  ): void {
    filePreviewStore.setSelection(
      selectedRows.map(rowSelectionEntry),
      primaryPath,
      anchorPath,
    );
    chromeStore.setRightPanelTab('preview');
  }

  function onRowClick(row: Row, e: MouseEvent): void {
    contextMenu = null;
    if (e.shiftKey) {
      const anchor = filePreviewStore.anchorPath;
      if (anchor !== null && anchor !== row.path) {
        const range = visibleRangeRows(anchor, row.path);
        if (range.length > 0) {
          if (e.metaKey || e.ctrlKey) {
            const paths = new Set(filePreviewStore.selectedPaths);
            for (const rangeRow of range) paths.add(rangeRow.path);
            applyRowSelection(rowsForPaths(paths), row.path, anchor);
          } else {
            applyRowSelection(range, row.path, anchor);
          }
          return;
        }
      }
    }
    if (e.metaKey || e.ctrlKey || e.shiftKey) {
      const paths = new Set(filePreviewStore.selectedPaths);
      if (paths.has(row.path)) paths.delete(row.path);
      else paths.add(row.path);
      const nextRows = rowsForPaths(paths);
      const primaryPath = paths.has(row.path) ? row.path : (nextRows[0]?.path ?? null);
      applyRowSelection(nextRows, primaryPath, row.path);
      return;
    }
    applyRowSelection([row], row.path, row.path);
  }

  function onRowContextMenu(row: Row, e: MouseEvent): void {
    e.preventDefault();
    e.stopPropagation();
    if (!(selectedCount >= 2 && filePreviewStore.selectedPaths.has(row.path))) {
      applyRowSelection([row], row.path, row.path);
    } else {
      filePreviewStore.setAnchor(row.path);
    }
    openContextMenu(row, e.clientX, e.clientY);
  }

  function onEmptyContextMenu(e: MouseEvent): void {
    e.preventDefault();
    e.stopPropagation();
    openContextMenu(null, e.clientX, e.clientY);
  }

  function onFileDragStart(row: Row, e: DragEvent): void {
    if (e.dataTransfer === null || rootPath.length === 0) return;
    const dragRows =
      filePreviewStore.selectedPaths.has(row.path) && selectedCount >= 2
        ? selectedRowsForAction()
        : [row];
    const moveRows = dedupeAncestorRows(dragRows);
    fileDragState = {
      sourceRows: moveRows,
      sourcePaths: dragRows.map((dragRow) => dragRow.path),
    };
    e.dataTransfer.effectAllowed = 'copyMove';
    e.dataTransfer.setData(
      WORKSPACE_FILE_DRAG_MIME,
      encodeWorkspaceFileDragPayload(dragRows.map((dragRow) => ({
        path: dragRow.path,
        rootPath,
        name: dragRow.entry.name,
        kind: dragRow.entry.kind,
        sizeBytes: dragRow.entry.size_bytes,
      }))),
    );
    e.dataTransfer.setData('text/plain', dragRows.map((dragRow) => dragRow.path).join('\n'));
  }

  function dedupeAncestorRows(rowsToDedupe: readonly Row[]): Row[] {
    return rowsToDedupe.filter((row) =>
      !rowsToDedupe.some((candidate) =>
        candidate.path !== row.path &&
        candidate.entry.kind === 'directory' &&
        isSameOrChild(row.path, candidate.path),
      ),
    );
  }

  function clearFileDragState(): void {
    fileDragState = null;
    dropTargetDir = null;
  }

  function hasWorkspaceFileDrag(e: DragEvent): boolean {
    return (
      !moveSubmitting &&
      fileDragState !== null &&
      e.dataTransfer?.types.includes(WORKSPACE_FILE_DRAG_MIME) === true
    );
  }

  function isInvalidMoveTarget(rowsToMove: readonly Row[], targetDir: string): boolean {
    if (targetDir.length === 0) return true;
    return rowsToMove.some((row) =>
      row.entry.kind === 'directory' && isSameOrChild(targetDir, row.path),
    );
  }

  function movableRowsForTarget(rowsToMove: readonly Row[], targetDir: string): Row[] {
    if (isInvalidMoveTarget(rowsToMove, targetDir)) return [];
    return rowsToMove.filter((row) => parentDir(row.path) !== targetDir);
  }

  function canMoveToTarget(targetDir: string): boolean {
    const state = fileDragState;
    if (state === null) return false;
    return movableRowsForTarget(state.sourceRows, targetDir).length > 0;
  }

  function onFileRowDragOver(row: Row, e: DragEvent): void {
    if (row.entry.kind !== 'directory' || !hasWorkspaceFileDrag(e)) return;
    if (!canMoveToTarget(row.path)) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer !== null) e.dataTransfer.dropEffect = 'move';
    dropTargetDir = row.path;
  }

  function onFileRowDragLeave(row: Row, _e: DragEvent): void {
    if (dropTargetDir !== row.path) return;
    queueMicrotask(() => {
      if (dropTargetDir === row.path) dropTargetDir = null;
    });
  }

  function onFileRowDrop(row: Row, e: DragEvent): void {
    if (row.entry.kind !== 'directory') return;
    void commitFileMove(row.path, e);
  }

  function onRootDragOver(e: DragEvent): void {
    if (!hasWorkspaceFileDrag(e)) return;
    const target = e.target as HTMLElement | null;
    if (target?.closest('.row') !== null || target?.closest('.files-head') !== null) return;
    if (!canMoveToTarget(rootDropTargetDir)) return;
    e.preventDefault();
    if (e.dataTransfer !== null) e.dataTransfer.dropEffect = 'move';
    dropTargetDir = rootDropTargetDir;
  }

  function onRootDragLeave(e: DragEvent): void {
    const related = e.relatedTarget as Node | null;
    if (related !== null && (e.currentTarget as HTMLElement).contains(related)) return;
    if (dropTargetDir === rootDropTargetDir) dropTargetDir = null;
  }

  function onRootDrop(e: DragEvent): void {
    const target = e.target as HTMLElement | null;
    if (target?.closest('.row') !== null || target?.closest('.files-head') !== null) return;
    void commitFileMove(rootDropTargetDir, e);
  }

  async function commitFileMove(destDir: string, e: DragEvent): Promise<void> {
    const state = fileDragState;
    if (state === null) return;
    e.preventDefault();
    e.stopPropagation();
    const sourceRows = movableRowsForTarget(state.sourceRows, destDir);
    clearFileDragState();
    if (sourceRows.length === 0) return;
    contextMenu = null;
    moveSubmitting = true;
    try {
      const result = await moveFs(sourceRows.map((row) => row.path), destDir, 'reject');
      if (result.entries.length === 0) return;
      syncMoveResult(result.entries, sourceRows);
      const rebind = await rebindCanvasPathsAfterMove(result.entries);
      await refreshRoot();
      if (!rebind.ok) return;
      const canvasSuffix = rebind.changedItemCount > 0 ? ' Canvas links updated.' : '';
      toastStore.show({
        message: result.entries.length === 1
          ? `Moved "${result.entries[0]?.name ?? 'item'}".${canvasSuffix}`
          : `Moved ${result.entries.length} workspace items.${canvasSuffix}`,
        tone: 'success',
      });
    } catch (err) {
      toastStore.show({
        message: uploadErrorMessage(err),
        tone: 'error',
        durationMs: 6_000,
      });
    } finally {
      moveSubmitting = false;
    }
  }

  function syncMoveResult(entries: readonly MoveFsEntry[], sourceRows: readonly Row[]): void {
    const rowByPath = new Map(sourceRows.map((row) => [row.path, row] as const));
    for (const entry of entries) {
      const sourceRow = rowByPath.get(entry.source);
      const nextEntry: FsEntry = {
        ...(sourceRow?.entry ?? { size_bytes: null, mtime_unix: null }),
        name: entry.name,
        kind: entry.kind,
      };
      filePreviewStore.rebasePath(entry.source, entry.path, nextEntry);
      if (entry.kind === 'directory') rebaseExpandedDirs(entry.source, entry.path);
    }
  }

  async function rebindCanvasPathsAfterMove(
    entries: readonly MoveFsEntry[],
  ): Promise<{ ok: boolean; changedItemCount: number }> {
    const workspaceRoot = sessionStore.effectiveWorkspaceRoot;
    const preview = rebindCanvasLayoutPathsForMove(
      sessionStore.layoutSnapshot(),
      entries,
      workspaceRoot,
    );
    if (preview.changedItemCount === 0) return { ok: true, changedItemCount: 0 };
    const result = await sessionStore.applyMutation(
      (cur) => rebindCanvasLayoutPathsForMove(cur, entries, workspaceRoot).layout,
      {
        captureHistory: false,
        failMessage: 'Workspace files moved, but canvas path rebinding failed.',
      },
    );
    return { ok: result.ok, changedItemCount: preview.changedItemCount };
  }

  function closeContextMenu(): void {
    contextMenu = null;
  }

  function openContextMenu(row: Row | null, x: number, y: number): void {
    contextMenu = { row, x, y };
    queueMicrotask(clampContextMenu);
  }

  function clampContextMenu(): void {
    if (contextMenu === null || contextMenuEl === undefined) return;
    const rect = contextMenuEl.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let nx = contextMenu.x;
    let ny = contextMenu.y;
    if (nx + rect.width > vw) nx = Math.max(0, vw - rect.width - 4);
    if (ny + rect.height > vh) ny = Math.max(0, vh - rect.height - 4);
    if (nx !== contextMenu.x || ny !== contextMenu.y) {
      contextMenu = { ...contextMenu, x: nx, y: ny };
    }
  }

  function onWindowPointerDown(e: PointerEvent): void {
    if (contextMenu === null || contextMenuEl === undefined) return;
    if (contextMenuEl.contains(e.target as Node)) return;
    closeContextMenu();
  }

  function onWindowKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      closeContextMenu();
      clearFileDragState();
    }
  }

  function onWindowBlur(): void {
    closeContextMenu();
    clearFileDragState();
  }

  function onFileTreeClick(e: MouseEvent): void {
    closeContextMenu();
    const target = e.target as HTMLElement | null;
    if (target?.closest('.row') !== null) return;
    if (target?.closest('.files-head') !== null) return;
    filePreviewStore.clear();
  }

  function selectEntry(row: Row): void {
    applyRowSelection([row], row.path, row.path);
    contextMenu = null;
  }

  async function copyPath(path: string): Promise<void> {
    contextMenu = null;
    const result = await copyTextToSystemClipboard(path);
    toastStore.show({
      message: result.ok ? 'Copied file path.' : (result.reason ?? 'Copy failed.'),
      tone: result.ok ? 'success' : 'error',
    });
  }

  async function copyPaths(rowsToCopy: readonly Row[]): Promise<void> {
    contextMenu = null;
    if (rowsToCopy.length === 0) return;
    const result = await copyTextToSystemClipboard(
      rowsToCopy.map((row) => row.path).join('\n'),
    );
    toastStore.show({
      message: result.ok
        ? `Copied ${rowsToCopy.length} path${rowsToCopy.length === 1 ? '' : 's'}.`
        : (result.reason ?? 'Copy failed.'),
      tone: result.ok ? 'success' : 'error',
    });
  }

  function copyRowsToWorkspaceClipboard(rowsToCopy: readonly Row[]): boolean {
    contextMenu = null;
    if (rowsToCopy.length === 0 || rootPath.length === 0) return false;
    fileClipboardStore.copy(rowsToCopy.map((row) => ({
      path: row.path,
      rootPath,
      name: row.entry.name,
      kind: row.entry.kind,
      sizeBytes: row.entry.size_bytes,
    })));
    toastStore.show({
      message: `Copied ${rowsToCopy.length} workspace item${rowsToCopy.length === 1 ? '' : 's'}.`,
      tone: 'success',
    });
    return true;
  }

  function copyCurrentSelection(): boolean {
    return copyRowsToWorkspaceClipboard(selectedRowsForAction());
  }

  function resolvePasteTargetDir(row: Row | null = null): string {
    if (row !== null) {
      if (row.entry.kind === 'directory') return row.path;
      return parentDir(row.path);
    }
    return uploadTargetDir;
  }

  async function pasteWorkspaceClipboardTo(dir: string): Promise<void> {
    contextMenu = null;
    const entries = fileClipboardStore.entries;
    if (entries.length === 0) return;
    try {
      await copyFs(entries.map((entry) => entry.path), dir, 'rename');
      await refreshRoot();
      toastStore.show({
        message: `Pasted ${entries.length} workspace item${entries.length === 1 ? '' : 's'}.`,
        tone: 'success',
      });
    } catch (err) {
      toastStore.show({
        message: uploadErrorMessage(err),
        tone: 'error',
        durationMs: 6_000,
      });
    }
  }

  function pasteCurrentClipboard(): boolean {
    if (!fileClipboardStore.hasEntries) return false;
    void pasteWorkspaceClipboardTo(resolvePasteTargetDir());
    return true;
  }

  function selectAllRows(): boolean {
    if (rows.length === 0) return false;
    applyRowSelection(rows, rows[0]?.path ?? null, rows[0]?.path ?? null);
    return true;
  }

  function registerFileShortcut(
    actionId: string,
    key: string,
    modifier: 'meta' | 'ctrl',
    description: string,
    run: () => boolean,
  ): () => void {
    const descriptor: ShortcutDescriptor = {
      actionId,
      key,
      meta: modifier === 'meta',
      ctrl: modifier === 'ctrl',
      description,
      category: 'Files',
      customizable: false,
      protectedReason: 'Files shortcuts are scoped to the Files tab and share OS-standard edit keys.',
      allowInEditable: false,
      allowInXterm: false,
      handler: () => {
        if (chromeStore.state.leftPanelTab !== 'files') return false;
        return run();
      },
    };
    return shortcutRegistry.register(descriptor);
  }

  onMount(() => {
    const unsubs = [
      registerFileShortcut('files.select_all', 'a', 'meta', 'Select all files', selectAllRows),
      registerFileShortcut('files.select_all', 'a', 'ctrl', 'Select all files (Win/Linux)', selectAllRows),
      registerFileShortcut('files.copy', 'c', 'meta', 'Copy selected files', copyCurrentSelection),
      registerFileShortcut('files.copy', 'c', 'ctrl', 'Copy selected files (Win/Linux)', copyCurrentSelection),
      registerFileShortcut('files.paste', 'v', 'meta', 'Paste files here', pasteCurrentClipboard),
      registerFileShortcut('files.paste', 'v', 'ctrl', 'Paste files here (Win/Linux)', pasteCurrentClipboard),
    ];
    return () => {
      for (const unsub of unsubs) unsub();
      // ADR-0052 D4 — tear down any pending/in-flight search on unmount.
      debouncedServerSearch.cancel();
      if (searchAbort !== null) searchAbort.abort();
    };
  });

  function openCreateFolder(parentDir: string): void {
    contextMenu = null;
    mkdirParentDir = parentDir;
    mkdirName = '';
    mkdirError = null;
    mkdirSubmitting = false;
    mkdirOpen = true;
  }

  function closeCreateFolder(): void {
    if (mkdirSubmitting) return;
    mkdirOpen = false;
    mkdirParentDir = '';
    mkdirName = '';
    mkdirError = null;
  }

  function validateFolderName(name: string): string | null {
    return validateFsEntryName(name, 'folder');
  }

  function validateFsEntryName(name: string, label: 'folder' | 'name'): string | null {
    const trimmed = name.trim();
    const noun = label === 'folder' ? 'Folder name' : 'Name';
    if (trimmed.length === 0) return `Enter a ${label}.`;
    if (trimmed === '.' || trimmed === '..') return `${noun} cannot be "." or "..".`;
    if (trimmed.includes('/') || trimmed.includes('\\')) return `${noun} cannot contain path separators.`;
    if (trimmed.includes('\0')) return `${noun} contains an invalid character.`;
    return null;
  }

  async function submitCreateFolder(): Promise<void> {
    const validation = validateFolderName(mkdirName);
    if (validation !== null) {
      mkdirError = validation;
      return;
    }
    const trimmed = mkdirName.trim();
    mkdirSubmitting = true;
    mkdirError = null;
    try {
      await mkdirFs(joinPath(mkdirParentDir, trimmed));
      mkdirOpen = false;
      mkdirParentDir = '';
      mkdirName = '';
      await refreshRoot();
      toastStore.show({ message: `Created folder "${trimmed}".`, tone: 'success' });
    } catch (err) {
      mkdirError = uploadErrorMessage(err);
    } finally {
      mkdirSubmitting = false;
    }
  }

  function onMkdirKeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && canSubmitMkdir) {
      e.preventDefault();
      void submitCreateFolder();
    }
  }

  function openRename(row: Row): void {
    contextMenu = null;
    renameTarget = row;
    renameName = row.entry.name;
    renameError = null;
    renameSubmitting = false;
    renameOpen = true;
  }

  function closeRename(): void {
    if (renameSubmitting) return;
    renameOpen = false;
    renameTarget = null;
    renameName = '';
    renameError = null;
  }

  function onRenameKeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && canSubmitRename) {
      e.preventDefault();
      void submitRename();
    }
  }

  function syncSelectionAfterRename(oldPath: string, newPath: string, nextEntry: FsEntry): void {
    filePreviewStore.rebasePath(oldPath, newPath, nextEntry);
  }

  function rebaseExpandedDirs(oldPath: string, newPath: string): void {
    const next = new Set<string>();
    for (const path of expandedDirs) {
      next.add(isSameOrChild(path, oldPath) ? replacePathPrefix(path, oldPath, newPath) : path);
    }
    expandedDirs = next;
    persistExpandedDirs();
  }

  function pruneExpandedDirs(pathToRemove: string): void {
    expandedDirs = new Set([...expandedDirs].filter((path) => !isSameOrChild(path, pathToRemove)));
    persistExpandedDirs();
  }

  async function submitRename(): Promise<void> {
    const target = renameTarget;
    if (target === null) return;
    const validation = validateFsEntryName(renameName, 'name');
    if (validation !== null) {
      renameError = validation;
      return;
    }
    const nextName = renameName.trim();
    renameSubmitting = true;
    renameError = null;
    try {
      const result = await renameFs(target.path, nextName);
      const nextEntry: FsEntry = {
        ...target.entry,
        name: result.name,
        kind: result.kind,
      };
      if (target.entry.kind === 'directory') rebaseExpandedDirs(target.path, result.path);
      syncSelectionAfterRename(target.path, result.path, nextEntry);
      renameOpen = false;
      renameTarget = null;
      await refreshRoot();
      toastStore.show({ message: `Renamed to "${result.name}".`, tone: 'success' });
    } catch (err) {
      renameError = uploadErrorMessage(err);
    } finally {
      renameSubmitting = false;
    }
  }

  function openRemove(row: Row): void {
    openRemoveRows([row]);
  }

  function openRemoveRows(rowsToRemove: readonly Row[]): void {
    contextMenu = null;
    removeTargets = [...rowsToRemove];
    removeError = null;
    removeSubmitting = false;
  }

  function closeRemove(): void {
    if (removeSubmitting) return;
    removeTargets = [];
    removeError = null;
  }

  async function submitRemove(): Promise<void> {
    const targets = removeTargets;
    if (targets.length === 0) return;
    removeSubmitting = true;
    removeError = null;
    try {
      for (const target of targets) {
        await removeFs(target.path);
        filePreviewStore.removePath(target.path);
        if (target.entry.kind === 'directory') pruneExpandedDirs(target.path);
      }
      removeTargets = [];
      await refreshRoot();
      toastStore.show({
        message: targets.length === 1
          ? `Removed "${targets[0]?.entry.name ?? 'item'}".`
          : `Removed ${targets.length} items.`,
        tone: 'success',
      });
    } catch (err) {
      removeError = uploadErrorMessage(err);
    } finally {
      removeSubmitting = false;
    }
  }

  async function insertAs(row: Row, requestedType: 'image' | 'document' | 'file_path'): Promise<void> {
    await insertRowsAs([row], requestedType);
  }

  async function insertRowsAs(
    rowsToInsert: readonly Row[],
    requestedType: 'image' | 'document' | 'file_path' | 'auto',
  ): Promise<void> {
    contextMenu = null;
    if (rowsToInsert.length === 0) return;
    const invalid = rowsToInsert.find((row) => {
      if (requestedType === 'auto' || requestedType === 'file_path') return false;
      if (row.entry.kind === 'directory') return true;
      if (requestedType === 'image') return !isImagePath(row.path);
      return !isDocumentPath(row.path);
    });
    if (invalid !== undefined) {
      toastStore.show({
        message: `This file cannot be inserted as ${requestedType}.`,
        tone: 'warning',
      });
      return;
    }
    const zoom = sessionStore.viewport.zoom || 1;
    const origin = {
      x: -sessionStore.viewport.x / zoom + 120,
      y: -sessionStore.viewport.y / zoom + 120,
    };
    let created = 0;
    try {
      for (const [index, row] of rowsToInsert.entries()) {
        const item = createCanvasItemFromWorkspaceFile(
          {
            x: origin.x + index * 18,
            y: origin.y + index * 18,
          },
          {
            absolutePath: row.path,
            workspaceRoot: rootPath,
            kind: row.entry.kind,
            sizeBytes: row.entry.size_bytes,
          },
          requestedType === 'auto' ? undefined : requestedType,
        );
        const committed = await commitNewItem(item);
        if (committed !== null) created += 1;
      }
      if (created > 0) void sessionStore.reloadActiveLayout();
    } catch (err) {
      toastStore.show({
        message: `Insert failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
        durationMs: 6_000,
      });
      return;
    }
    if (created === 0) return;
    chromeStore.setLeftPanelTab('layers');
    toastStore.show({
      message: rowsToInsert.length === 1
        ? `Inserted ${rowsToInsert[0]?.entry.name ?? 'item'}.`
        : `Inserted ${created} workspace items.`,
      tone: 'success',
    });
  }

  function uploadErrorMessage(err: unknown): string {
    if (err instanceof UnauthorizedError) {
      window.location.href = '/auth';
      return 'Unauthorized.';
    }
    if (err instanceof FsApiUnavailableError) return 'Workspace file operation is not available on this server.';
    if (err instanceof DirNotAllowedError) return 'Directory is outside the server workspace.';
    if (err instanceof DirNotEmptyError) return 'Folder is not empty.';
    if (err instanceof FsAlreadyExistsError) return 'A file or folder with that name already exists.';
    if (err instanceof FsInvalidRequestError) return err.message;
    if (err instanceof FsInvalidNameError) return 'Name is not valid.';
    if (err instanceof FsMoveCycleError) return err.message;
    if (err instanceof FsNotFoundError) return 'File or folder not found.';
    if (err instanceof FsPayloadTooLargeError) return 'File is too large.';
    if (err instanceof FsUnsupportedMimeError) return 'File type is not supported.';
    if (err instanceof FsNameConflictError) return err.message;
    return err instanceof Error ? err.message : String(err);
  }

  async function onWorkspacePicked(path: string): Promise<void> {
    const name = activeName;
    if (name === null) return;
    changeOpen = false;
    try {
      filePreviewStore.clear();
      await changeWorkspace(name, path);
      sessionStore.setActiveWorkspaceRoot(path);
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

  function fileTreeStateKey(sessionName: string | null, root: string): string | null {
    return sessionName === null || root.length === 0 ? null : `${sessionName}:${root}`;
  }

  function readRestoredExpandedDirs(sessionName: string | null, root: string): Set<string> {
    const restored = readExpandedTreeState(
      FILE_TREE_EXPANSION_STORAGE_KEY,
      fileTreeStateKey(sessionName, root),
    );
    return new Set([...restored].filter((path) => isPathWithinRoot(path, root)));
  }

  function persistExpandedDirs(): void {
    writeExpandedTreeState(
      FILE_TREE_EXPANSION_STORAGE_KEY,
      fileTreeStateKey(activeName, rootPath),
      expandedDirs,
      MAX_FILE_TREE_EXPANSIONS,
    );
  }

  function isPathWithinRoot(path: string, root: string): boolean {
    if (root.length === 0) return false;
    const prefix = root.endsWith('/') ? root : `${root}/`;
    return path === root || path.startsWith(prefix);
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

<!-- ADR-0052 D4/D8 — text-safe highlight: split the name into plain-text and
     <mark> segments. NEVER innerHTML; every segment is a Svelte text node. -->
{#snippet highlightedName(name: string, ranges: [number, number][])}
  {#each highlightSegments(name, ranges) as segment}
    {#if segment.mark}<mark class="search-mark">{segment.text}</mark>{:else}{segment.text}{/if}
  {/each}
{/snippet}

<svelte:window
  onpointerdowncapture={onWindowPointerDown}
  onkeydown={onWindowKeydown}
  onblur={onWindowBlur}
  onresize={closeContextMenu}
/>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="file-tree"
  class:drop-root={dropTargetDir === rootDropTargetDir}
  onclick={onFileTreeClick}
  onkeydown={() => {}}
  oncontextmenu={onEmptyContextMenu}
  onscroll={closeContextMenu}
  ondragover={onRootDragOver}
  ondragleave={onRootDragLeave}
  ondrop={(e: DragEvent) => onRootDrop(e)}
>
  <header class="files-head">
    <span class="workspace-icon" aria-hidden="true">
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
        <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.2 1.5h5.2A1.5 1.5 0 0 1 14 6v5.5A1.5 1.5 0 0 1 12.5 13h-9A1.5 1.5 0 0 1 2 11.5z"/>
      </svg>
    </span>
    <div class="workspace-meta">
      <span class="workspace-base" title={displayPath(rootPath || targetRoot)}>
        {basename(rootPath || targetRoot)}
      </span>
      <span class="workspace-path mono" title={displayPath(rootPath || targetRoot)}>
        {compactPath(rootPath || targetRoot)}
      </span>
    </div>
    <div class="head-actions">
      <button
        type="button"
        class="icon-btn"
        disabled={activeName === null || rootLoading}
        title="Refresh files"
        aria-label="Refresh files"
        onclick={() => void refreshRoot()}
      >
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
          <path d="M13 8a5 5 0 1 1-1.5-3.5M13 3v2.5h-2.5"/>
        </svg>
      </button>
      <button
        type="button"
        class="icon-btn"
        disabled={activeName === null || rootLoading || (rootPath || targetRoot).length === 0}
        title="Upload here"
        aria-label="Upload here"
        onclick={(e: MouseEvent) => {
          e.stopPropagation();
          void uploadToDir(uploadTargetDir);
        }}
      >
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
          <path d="M8 11V3M5 6l3-3 3 3M3 13h10"/>
        </svg>
      </button>
      <button
        type="button"
        class="icon-btn"
        disabled={activeName === null}
        title="Change workspace"
        aria-label="Change workspace"
        onclick={() => (changeOpen = true)}
      >
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M6 11H4.5a3.5 3.5 0 0 1 0-7H6"/>
          <path d="M10 4h1.5a3.5 3.5 0 0 1 0 7H10"/>
          <path d="M5.5 7.5h5"/>
        </svg>
      </button>
    </div>
  </header>

  <!-- ADR-0052 D2 — the search input moved to the LeftPanel footer; this
       component renders only results. The footer passes the active query down
       via the `query` prop. -->

  {#if activeName === null}
    <PanelEmptyState
      icon="files"
      lead="No active session"
      description="Open or create a session to browse its workspace."
    />
  {:else if rootLoading}
    <div class="shimmer-stack" aria-label="Loading files">
      <span></span><span></span><span></span>
    </div>
  {:else if rootError !== null}
    <PanelEmptyState
      icon="alert"
      lead="Unable to read workspace"
      description={rootError}
      tone="danger"
      role="alert"
    />
  {:else if searching}
    <!-- ADR-0052 D4 — flat ranked results list replaces the tree while searching. -->
    <div class="search-results-wrap">
      {#if searchResults.length === 0}
        <PanelEmptyState
          icon="files"
          lead={searchLoading ? 'Searching…' : 'No matches'}
          description={searchLoading
            ? 'Searching the workspace.'
            : 'No files or folders match this search.'}
        />
      {:else}
        <ul class="search-results" role="listbox" aria-label="Search results">
          {#each searchResults as result (result.path)}
            {@const selected = filePreviewStore.selectedPaths.has(result.path)}
            {@const contextDir = resultContextDir(result)}
            <li class="search-result" class:selected role="presentation">
              <button
                type="button"
                class="search-result-button"
                role="option"
                aria-selected={selected}
                title={result.path}
                onclick={(e: MouseEvent) => {
                  e.stopPropagation();
                  onSearchResultClick(result);
                }}
              >
                {@render fileIconSvg(resultIconRow(result))}
                <span class="search-result-name">
                  {@render highlightedName(result.entry.name, result.ranges)}
                </span>
                {#if contextDir.length > 0}
                  <span class="search-result-dir mono">{contextDir}</span>
                {/if}
              </button>
            </li>
          {/each}
        </ul>
        {#if serverTruncated}
          <p class="search-truncated" role="status">
            Showing the first {searchResults.length} matches — refine your search to narrow results.
          </p>
        {/if}
      {/if}
    </div>
  {:else if rows.length === 0}
    <PanelEmptyState
      icon="files"
      lead="Empty workspace"
      description="This folder has no visible files."
    />
  {:else}
    <div class="tree-viewport">
      <!-- ADR-0052 D7 — sticky parent header overlay (hierarchy-only; hidden while
           searching). Pinned at the top of the scroll viewport, above normal rows. -->
      {#if stickyIndices.length > 0}
        <div class="sticky-stack">
          {#each stickyIndices as stickyIndex (stickyIndex)}
            {@const stickyRow = rows[stickyIndex]}
            {#if stickyRow !== undefined}
              <button
                type="button"
                class="sticky-row"
                style:padding-left={`${stickyRow.depth * 16 + 4}px`}
                title={stickyRow.path}
                onclick={(e: MouseEvent) => {
                  e.stopPropagation();
                  onStickyClick(stickyIndex);
                }}
              >
                {@render fileIconSvg(stickyRow)}
                <span class="label">{stickyRow.entry.name}</span>
              </button>
            {/if}
          {/each}
        </div>
      {/if}
      <ul
        bind:this={treeScrollEl}
        class="tree"
        role="tree"
        aria-label="Workspace file tree"
        onscroll={onTreeScroll}
      >
        {#each rows as row (row.path)}
        {@const selected = filePreviewStore.selectedPaths.has(row.path)}
        <li
          class="row"
          class:selected
          class:drop-inside={dropTargetDir === row.path}
          class:dragging={fileDragState !== null && fileDragState.sourcePaths.includes(row.path)}
          role="treeitem"
          aria-selected={selected}
          aria-expanded={row.entry.kind === 'directory' ? row.expanded : undefined}
          draggable={true}
          ondragstart={(e: DragEvent) => onFileDragStart(row, e)}
          ondragover={(e: DragEvent) => onFileRowDragOver(row, e)}
          ondragleave={(e: DragEvent) => onFileRowDragLeave(row, e)}
          ondrop={(e: DragEvent) => onFileRowDrop(row, e)}
          ondragend={clearFileDragState}
          oncontextmenu={(e: MouseEvent) => onRowContextMenu(row, e)}
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
                if (row.entry.kind !== 'directory') return;
                // ADR-0046 D6 amend ⑨ — replace-select the directory before toggling
                // so a selected child hidden by collapse moves selection to the visible
                // directory row (same caret contract as Layer tree, ADR-0024 D25). No
                // reveal-on-select effect here, so this is selection-visibility only.
                applyRowSelection([row], row.path, row.path);
                toggleDirectory(row.path);
              }}
              onkeydown={() => {}}
            >
              {#if row.entry.kind === 'directory'}
                {#if row.loading}
                  <span class="tiny-spin" aria-hidden="true"></span>
                {:else}
                  {row.expanded ? '▾' : '▸'}
                {/if}
              {/if}
            </span>
            <button
              type="button"
              class="row-button"
              title={row.path}
              onclick={(e: MouseEvent) => onRowClick(row, e)}
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
    </div>
  {/if}

  {#if contextMenu !== null}
    {@const menuRows = contextRowsForAction()}
    {@const isMultiMenu = menuRows.length >= 2}
    <div
      bind:this={contextMenuEl}
      class="file-context-menu"
      style:left={`${contextMenu.x}px`}
      style:top={`${contextMenu.y}px`}
      role="menu"
      tabindex="-1"
      oncontextmenu={(e: MouseEvent) => e.preventDefault()}
      onclick={(e: MouseEvent) => e.stopPropagation()}
      onkeydown={(e: KeyboardEvent) => {
        if (e.key === 'Escape') closeContextMenu();
      }}
    >
	      {#if isMultiMenu}
        <button type="button" role="menuitem" onclick={() => void insertRowsAs(menuRows, 'auto')}>
          Insert selected
        </button>
        <button type="button" role="menuitem" onclick={() => void insertRowsAs(menuRows, 'file_path')}>
          Insert as file paths
        </button>
        <div class="menu-separator" role="separator"></div>
        <button type="button" role="menuitem" onclick={() => copyRowsToWorkspaceClipboard(menuRows)}>
          Copy
        </button>
        <button type="button" role="menuitem" onclick={() => void copyPaths(menuRows)}>
          Copy paths
        </button>
        <button type="button" role="menuitem" class="danger" onclick={() => openRemoveRows(menuRows)}>
          Remove selected
        </button>
	      {:else if contextMenu.row === null}
	        <button type="button" role="menuitem" onclick={() => openCreateFolder(rootPath || targetRoot)}>
	          New folder
	        </button>
	        <button type="button" role="menuitem" onclick={() => void uploadToDir(rootPath || targetRoot)}>
	          Upload here...
	        </button>
        <button
          type="button"
          role="menuitem"
          disabled={!fileClipboardStore.hasEntries}
          onclick={() => void pasteWorkspaceClipboardTo(rootPath || targetRoot)}
        >
          Paste here
        </button>
	        <button type="button" role="menuitem" onclick={() => void refreshRoot()}>
	          Refresh
	        </button>
      {:else if contextMenu.row.entry.kind === 'directory'}
        {@const row = contextMenu.row}
        <button type="button" role="menuitem" onclick={() => selectEntry(row)}>
          Select folder
        </button>
	        <button type="button" role="menuitem" onclick={() => void uploadToDir(row.path)}>
	          Upload here...
	        </button>
        <button
          type="button"
          role="menuitem"
          disabled={!fileClipboardStore.hasEntries}
          onclick={() => void pasteWorkspaceClipboardTo(row.path)}
        >
          Paste here
        </button>
	        <button type="button" role="menuitem" onclick={() => void insertAs(row, 'file_path')}>
	          Insert as file path
	        </button>
        <button type="button" role="menuitem" onclick={() => openCreateFolder(row.path)}>
          New folder
        </button>
        <button type="button" role="menuitem" onclick={() => openRename(row)}>
          Rename
        </button>
	        <button type="button" role="menuitem" onclick={() => void copyPath(row.path)}>
	          Copy path
	        </button>
        <button type="button" role="menuitem" onclick={() => copyRowsToWorkspaceClipboard([row])}>
          Copy
        </button>
	        <button type="button" role="menuitem" class="danger" onclick={() => openRemove(row)}>
	          Remove
	        </button>
      {:else}
        {@const row = contextMenu.row}
        <button type="button" role="menuitem" onclick={() => selectEntry(row)}>
          Open in Preview
        </button>
        <button type="button" role="menuitem" onclick={() => void insertAs(row, 'image')} disabled={!isImagePath(row.path)}>
          Insert as image
        </button>
        <button type="button" role="menuitem" onclick={() => void insertAs(row, 'document')} disabled={!isDocumentPath(row.path)}>
          Insert as document
        </button>
        <button type="button" role="menuitem" onclick={() => void insertAs(row, 'file_path')}>
          Insert as file path
        </button>
	        <button type="button" role="menuitem" onclick={() => void copyPath(row.path)}>
	          Copy path
	        </button>
        <button type="button" role="menuitem" onclick={() => copyRowsToWorkspaceClipboard([row])}>
          Copy
        </button>
	        <button type="button" role="menuitem" onclick={() => openRename(row)}>
	          Rename
	        </button>
        <button type="button" role="menuitem" onclick={() => openRemove(row)} class="danger">
          Remove
        </button>
      {/if}
    </div>
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

<Modal
  open={mkdirOpen}
  onclose={closeCreateFolder}
  title="New folder"
  size="sm"
  dismissOnBackdrop={!mkdirSubmitting}
  dismissOnEsc={!mkdirSubmitting}
>
  {#snippet subtitle()}
    <span class="modal-path mono" title={displayPath(mkdirParentDir)}>
      {displayPath(mkdirParentDir)}
    </span>
  {/snippet}
  {#snippet body()}
    <div class="mkdir-form">
      <label class="mkdir-field" class:has-error={mkdirError !== null}>
        <span class="mkdir-label">Folder name</span>
        <input
          class="mkdir-input"
          bind:value={mkdirName}
          placeholder="New folder"
          autocomplete="off"
          disabled={mkdirSubmitting}
          oninput={() => (mkdirError = null)}
          onkeydown={onMkdirKeydown}
        />
      </label>
      {#if mkdirError !== null}
        <p class="mkdir-error" role="alert">{mkdirError}</p>
      {:else}
        <p class="mkdir-hint">Creates a directory inside the selected workspace folder.</p>
      {/if}
    </div>
  {/snippet}
  {#snippet footer()}
    <span class="modal-footer-status mkdir-footer-status" class:hidden={mkdirError === null}>
      {mkdirError ?? 'Ready'}
    </span>
    <Button variant="ghost" disabled={mkdirSubmitting} onclick={closeCreateFolder}>Cancel</Button>
    <Button variant="primary" disabled={!canSubmitMkdir} onclick={() => void submitCreateFolder()}>
      {mkdirSubmitting ? 'Creating...' : 'Create folder'}
    </Button>
  {/snippet}
</Modal>

<Modal
  open={renameOpen}
  onclose={closeRename}
  title="Rename"
  size="sm"
  dismissOnBackdrop={!renameSubmitting}
  dismissOnEsc={!renameSubmitting}
>
  {#snippet subtitle()}
    {#if renameTarget !== null}
      <span class="modal-path mono" title={renameTarget.path}>{renameTarget.path}</span>
    {/if}
  {/snippet}
  {#snippet body()}
    <div class="mkdir-form">
      <label class="mkdir-field" class:has-error={renameError !== null}>
        <span class="mkdir-label">Name</span>
        <input
          class="mkdir-input"
          bind:value={renameName}
          placeholder="Name"
          autocomplete="off"
          disabled={renameSubmitting}
          oninput={() => (renameError = null)}
          onkeydown={onRenameKeydown}
        />
      </label>
      {#if renameError !== null}
        <p class="mkdir-error" role="alert">{renameError}</p>
      {:else}
        <p class="mkdir-hint">Renames the selected file or folder in place.</p>
      {/if}
    </div>
  {/snippet}
  {#snippet footer()}
    <span class="modal-footer-status mkdir-footer-status" class:hidden={renameError === null}>
      {renameError ?? 'Ready'}
    </span>
    <Button variant="ghost" disabled={renameSubmitting} onclick={closeRename}>Cancel</Button>
    <Button variant="primary" disabled={!canSubmitRename} onclick={() => void submitRename()}>
      {renameSubmitting ? 'Renaming...' : 'Rename'}
    </Button>
  {/snippet}
</Modal>

<Modal
  open={removeTargets.length > 0}
  onclose={closeRemove}
  title="Remove"
  size="sm"
  dismissOnBackdrop={!removeSubmitting}
  dismissOnEsc={!removeSubmitting}
>
  {#snippet body()}
    {#if removeTargets.length > 0}
      <div class="confirm-copy">
        <span class="confirm-icon danger" aria-hidden="true">
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
            <path d="M3 4.5h10M6 4.5V3h4v1.5M5 4.5l.6 8.5h4.8L11 4.5"/>
          </svg>
        </span>
        <strong>
          {removeTargets.length === 1
            ? `Remove "${removeTargets[0]?.entry.name ?? 'item'}"?`
            : `Remove ${removeTargets.length} items?`}
        </strong>
        <small title={removeTargets.map((row) => row.path).join('\n')}>
          {removeTargets.length === 1
            ? removeTargets[0]?.path
            : 'Selected files and folders'}
        </small>
      </div>
      {#if removeError !== null}
        <p class="mkdir-error" role="alert">{removeError}</p>
      {:else}
        <p class="mkdir-hint">
          Files are removed permanently. Folders must be empty.
        </p>
      {/if}
    {/if}
  {/snippet}
  {#snippet footer()}
    <span class="modal-footer-status mkdir-footer-status" class:hidden={removeError === null}>
      {removeError ?? 'Ready'}
    </span>
    <Button variant="ghost" disabled={removeSubmitting} onclick={closeRemove}>Cancel</Button>
    <Button variant="danger" disabled={removeSubmitting} onclick={() => void submitRemove()}>
      {removeSubmitting ? 'Removing...' : 'Remove'}
    </Button>
  {/snippet}
</Modal>

<Modal
  open={uploadConflict !== null}
  onclose={closeUploadConflict}
  title="Upload conflict"
  size="sm"
  dismissOnBackdrop={!uploadConflictSubmitting}
  dismissOnEsc={!uploadConflictSubmitting}
>
  {#snippet body()}
    {#if uploadConflict !== null}
      <div class="confirm-copy">
        <span class="confirm-icon" aria-hidden="true">
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
            <path d="M8 5v3.5M8 11h.01"/>
            <path d="M2.5 13.5h11L8 2.5z"/>
          </svg>
        </span>
        <strong>A file with that name already exists.</strong>
        <small title={uploadConflict.dir}>{uploadConflict.dir}</small>
      </div>
      {#if uploadConflictError !== null}
        <p class="mkdir-error" role="alert">{uploadConflictError}</p>
      {:else}
        <p class="mkdir-hint">Choose how to finish this upload.</p>
      {/if}
    {/if}
  {/snippet}
  {#snippet footer()}
    <span class="modal-footer-status mkdir-footer-status" class:hidden={uploadConflictError === null}>
      {uploadConflictError ?? 'Ready'}
    </span>
    <Button variant="ghost" disabled={uploadConflictSubmitting} onclick={closeUploadConflict}>Cancel</Button>
    <Button variant="secondary" disabled={uploadConflictSubmitting} onclick={() => void submitUploadConflict('rename')}>
      Upload renamed
    </Button>
    <Button variant="primary" disabled={uploadConflictSubmitting} onclick={() => void submitUploadConflict('overwrite')}>
      Overwrite
    </Button>
  {/snippet}
</Modal>

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
    grid-template-columns: auto minmax(0, 1fr) auto;
    gap: var(--space-6);
    align-items: center;
    padding: var(--space-8) var(--space-10);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-2);
    flex: 0 0 auto;
  }

  .workspace-icon {
    width: 16px;
    height: 16px;
    display: grid;
    place-items: center;
    color: var(--color-accent);
  }

  .workspace-meta {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .mono {
    font-family: var(--font-mono);
  }

  .workspace-base {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg);
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
    letter-spacing: -0.1px;
  }

  .workspace-path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
    direction: rtl;
    text-align: left;
  }

  .head-actions {
    display: inline-flex;
    align-items: center;
    gap: 1px;
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

  .icon-btn:hover:not(:disabled) {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .icon-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* ── Search highlight (ADR-0052 D4) — the input itself lives in the
       LeftPanel footer (D2); only the result highlight styling stays here. ── */
  .search-mark {
    background: color-mix(in srgb, var(--color-accent) 28%, transparent);
    color: inherit;
    border-radius: 2px;
    padding: 0 1px;
  }

  /* ── Tree viewport (sticky overlay anchor) (ADR-0052 D7) ── */
  .tree-viewport {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  /* ── Search results (flat ranked list) (ADR-0052 D4) ── */
  .search-results-wrap {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
  }

  .search-results {
    list-style: none;
    margin: 0;
    padding: var(--space-4) 0;
  }

  .search-result {
    display: block;
    position: relative;
  }

  .search-result + .search-result {
    margin-top: 2px;
  }

  .search-result:hover {
    background: var(--color-glass-1);
  }

  .search-result.selected {
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    color: var(--color-accent);
    box-shadow: inset 2px 0 0 var(--color-accent);
  }

  .search-result.selected .type-icon {
    color: var(--color-accent);
  }

  .search-result-button {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    width: 100%;
    min-width: 0;
    padding: var(--space-4) var(--space-8) var(--space-4) var(--space-8);
    background: transparent;
    border: 0;
    color: inherit;
    text-align: left;
    cursor: pointer;
    font: inherit;
  }

  .search-result-name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .search-result-dir {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg-subtle);
    font-size: var(--text-sm);
    direction: rtl;
    text-align: left;
  }

  .search-truncated {
    margin: 0;
    padding: var(--space-6) var(--space-10);
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
    border-top: 1px solid var(--color-border);
  }

  /* ── Sticky parent headers (ADR-0052 D7) ── */
  .sticky-stack {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 2; /* above .tree rows; below the fixed context menu (--z-context-menu). */
    display: flex;
    flex-direction: column;
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    box-shadow: 0 4px 8px rgba(0, 0, 0, 0.12);
    pointer-events: none; /* container ignores events; rows opt back in below. */
  }

  .sticky-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    width: 100%;
    min-width: 0;
    height: 24px;
    padding-right: var(--space-8);
    border: 0;
    background: var(--color-surface);
    color: var(--color-fg-muted);
    text-align: left;
    cursor: pointer;
    font: inherit;
    pointer-events: auto; /* clickable even though the container opted out. */
  }

  .sticky-row:hover {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .tree {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    list-style: none;
    margin: 0;
    padding: var(--space-4) 0;
  }

  .file-tree.drop-root .tree,
  .file-tree.drop-root :global(.panel-empty) {
    background: color-mix(in srgb, var(--color-accent) 8%, transparent);
    outline: 1px dashed var(--color-accent);
    outline-offset: -1px;
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

  .row.dragging {
    opacity: 0.45;
  }

  .row.drop-inside {
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    outline: 1px dashed var(--color-accent);
    outline-offset: -1px;
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
    text-align: center;
    color: var(--color-fg-muted);
    cursor: pointer;
    user-select: none;
    transition: transform var(--motion-fast) var(--motion-easing);
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

  .row-error {
    color: var(--color-danger);
  }

  .row-error {
    margin: 2px 0 4px 28px;
    font-size: var(--text-sm);
  }

  .file-context-menu {
    position: fixed;
    z-index: var(--z-context-menu);
    min-width: 168px;
    padding: var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.18);
  }

  .file-context-menu button {
    width: 100%;
    height: 26px;
    display: flex;
    align-items: center;
    padding: 0 var(--space-8);
    border: 0;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--color-fg);
    font: inherit;
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }

  .file-context-menu button:hover:not(:disabled) {
    background: var(--color-glass-1);
  }

  .file-context-menu button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .file-context-menu button.danger {
    color: var(--color-danger);
  }

  .menu-separator {
    height: 1px;
    margin: var(--space-4) var(--space-2);
    background: var(--color-border);
  }

  .shimmer-stack {
    padding: var(--space-12);
    display: grid;
    gap: var(--space-8);
  }

  .shimmer-stack span {
    height: 20px;
    border-radius: var(--radius-sm);
    background: linear-gradient(
      90deg,
      var(--color-surface-2),
      var(--color-glass-1),
      var(--color-surface-2)
    );
    background-size: 180% 100%;
    animation: shimmer 1.1s linear infinite;
  }

  .tiny-spin {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    border: 1.5px solid var(--color-border-strong);
    border-top-color: var(--color-accent);
    animation: spin 900ms linear infinite;
  }

  .modal-path {
    display: block;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
  }

  .mkdir-form {
    display: grid;
    gap: var(--space-8);
  }

  .confirm-copy {
    display: grid;
    grid-template-columns: 32px minmax(0, 1fr);
    gap: var(--space-10);
    align-items: flex-start;
    color: var(--color-fg);
  }

  .confirm-icon {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    grid-row: 1 / span 2;
    background: color-mix(in srgb, var(--color-warning) 14%, transparent);
    color: var(--color-warning);
  }

  .confirm-icon.danger {
    background: color-mix(in srgb, var(--color-danger) 14%, transparent);
    color: var(--color-danger);
  }

  .confirm-copy strong,
  .confirm-copy small {
    display: block;
    grid-column: 2;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
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
    white-space: nowrap;
  }

  .mkdir-field {
    display: grid;
    gap: var(--space-6);
    min-width: 0;
  }

  .mkdir-label {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
  }

  .mkdir-input {
    box-sizing: border-box;
    width: 100%;
    height: 32px;
    padding: 0 var(--space-12);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    color: var(--color-fg);
    font: inherit;
    font-size: var(--text-base);
    letter-spacing: 0;
  }

  .mkdir-input:hover:not(:disabled) {
    border-color: var(--color-fg-subtle);
  }

  .mkdir-input:focus-visible {
    outline: 2px solid var(--color-info);
    outline-offset: 1px;
    border-color: var(--color-info);
  }

  .mkdir-field.has-error .mkdir-input {
    border-color: var(--color-danger);
  }

  .mkdir-hint,
  .mkdir-error,
  .mkdir-footer-status {
    margin: 0;
    font-size: var(--text-sm);
  }

  .mkdir-hint {
    color: var(--color-fg-muted);
  }

  .mkdir-error {
    color: var(--color-danger);
  }

  .mkdir-footer-status {
    min-width: 0;
    color: var(--color-danger);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mkdir-footer-status.hidden {
    visibility: hidden;
  }

  @keyframes shimmer {
    to {
      background-position: -180% 0;
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
