<script lang="ts">
  /**
   * FilePreviewView — read-only preview for the selected Workspace file.
   */

  import { tick, untrack } from 'svelte';
  import { filePreviewStore } from '$lib/stores/filePreview.svelte';
  import { chromeStore } from '$lib/stores/chrome.svelte';
  import { fsFileUrl, fsDownloadUrl, fsFileWrite, fsFileGetEtag } from '$lib/http/fs';
  import {
    canEnterEdit,
    isDraftDirty,
    planConflictResolution,
    type ConflictChoice,
  } from './filePreviewEdit';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { UnauthorizedError } from '$lib/http/sessions';
  import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { escRouter } from '$lib/common/escRouter.svelte';
  import FindBar from '$lib/common/FindBar.svelte';
  import { DocumentFindController } from '$lib/common/documentFind.svelte';
  import {
    registerDocumentFindSurface,
    type DocumentFindOpenRequest,
  } from '$lib/common/findController';
  import CodeViewer from '$lib/canvas/CodeViewer.svelte';
  import CanvasGlyph from '$lib/canvas/CanvasGlyph.svelte';
  import CodeEditArea from './CodeEditArea.svelte';
  import DocumentMarkdownView from '$lib/viewers/DocumentMarkdownView.svelte';
  import HtmlViewer from '$lib/viewers/HtmlViewer.svelte';
  import ImageViewer from '$lib/viewers/ImageViewer.svelte';
  import PdfViewer from '$lib/viewers/PdfViewer.svelte';
  import PanelEmptyState from './PanelEmptyState.svelte';
  import { componentSettings } from '$lib/stores/componentSettings.svelte';
  import { basename, extension, previewMetaForPath, type WorkspacePreviewKind } from '$lib/files/workspaceAssets';
  import { formatPathWithLocation, selectionToRange } from '$lib/files/sourceLocation';
  import {
    buildRenderedHtmlSrcdoc,
    renderMarkdown,
    RENDERED_HTML_IFRAME_SANDBOX,
  } from '$lib/canvas/documentRender';
  import type { FilePreviewSelection } from '$lib/stores/filePreview.svelte';

  type PreviewKind = WorkspacePreviewKind | 'directory' | 'unsupported';

  const SUMMARY_ROW_LIMIT = 12;

  interface MultiSelectionSummary {
    count: number;
    fileCount: number;
    folderCount: number;
    knownSizeBytes: number;
    knownSizeCount: number;
    rows: FilePreviewSelection[];
    hiddenCount: number;
  }

  interface PreviewContentMenu {
    x: number;
    y: number;
    copyText: string;
    pathWithLocation: string;
  }

  let loading = $state(false);
  let errorMessage = $state<string | null>(null);
  let textContent = $state<string | null>(null);
  let loadedPath = $state<string | null>(null);
  // ADR-0057 D3/D4 — current ETag ("<mtime-nanos>-<size>") captured at load time
  // and refreshed on each successful save; the If-Match for the next write.
  let currentEtag = $state<string | null>(null);
  let previewMaximized = $state(false);

  // ── Edit mode (ADR-0057 D1–D5) ──
  let editing = $state(false);
  // Draft lives in component state so it survives the inline↔maximize snippet
  // swap (ADR-0057 D1). Baseline for dirty is `textContent` (last saved/loaded).
  let draft = $state('');
  let saving = $state(false);
  // Inline save-error surface (reuses PanelEmptyState-style error copy).
  let saveError = $state<string | null>(null);
  // Selection captured when edit began — used to revert on a "keep editing"
  // choice when the file selection changes underneath a dirty edit (D4).
  let editSelection: FilePreviewSelection | null = null;
  // One-shot suppression so the programmatic revert-select doesn't re-trigger
  // the dirty guard (avoids a confirm loop).
  let suppressSelectionGuard = false;

  interface PendingDiscard {
    title: string;
    message: string;
    confirmLabel: string;
    onConfirm: () => void;
    onCancel: () => void;
  }
  let pendingDiscard = $state<PendingDiscard | null>(null);

  interface ConflictState {
    path: string;
    /** Set once the draft has been copied to the clipboard (reload affordance). */
    copied: boolean;
    busy: boolean;
  }
  let conflict = $state<ConflictState | null>(null);

  const dirty = $derived(editing && isDraftDirty(draft, textContent ?? ''));
  const anyEditModalOpen = $derived(pendingDiscard !== null || conflict !== null);
  let contentMenu = $state<PreviewContentMenu | null>(null);
  let contentMenuEl: HTMLDivElement | undefined = $state();
  // ADR-0046 D6 amend ⑩ — ref to the currently-mounted preview surface so the
  // Cmd/Ctrl+C shortcut can scope the selection to it (same root the right-click
  // menu uses). The previewSurface snippet renders in two places (inline +
  // maximize body) but only one is shown at a time, so a single ref is correct.
  let previewSurfaceEl: HTMLDivElement | undefined = $state();
  // ADR-0057 D1 amend 2026-07-27 — imperative handle to the active CodeEditArea
  // so the edit action bar's Undo/Redo can drive its native textarea stack.
  let editAreaRef = $state<{ undo: () => void; redo: () => void } | null>(null);

  const selection = $derived(filePreviewStore.selection);
  const selectedEntries = $derived(filePreviewStore.selectedEntries);
  const selectedCount = $derived(selectedEntries.length);
  const isMultiSelection = $derived(selectedCount > 1);
  const previewMeta = $derived(previewMetaForPath(selection?.path ?? ''));
  const kind = $derived(
    selection?.entry.kind === 'directory' ? 'directory' : previewMeta.kind,
  );
  const previewUrl = $derived(selection === null ? '' : fsFileUrl(selection.path));
  const renderedMarkdown = $derived(renderMarkdown(textContent ?? ''));
  const renderedHtml = $derived(buildRenderedHtmlSrcdoc(textContent ?? ''));
  const codeLang = $derived(previewMeta.shikiLang);
  // ADR-0057 D1/D2 — pencil entry gate. No client-side size gate (the FE has no
  // config surface exposing assets.max_size_bytes); oversized writes surface via
  // the BE 413 instead. See report / D2 note.
  const canEdit = $derived(
    canEnterEdit({
      multiSelection: isMultiSelection,
      hasSelection: selection !== null,
      loading,
      hasError: errorMessage !== null,
      contentLoaded: textContent !== null,
      kind,
    }),
  );
  const multiSummary = $derived.by((): MultiSelectionSummary => {
    let fileCount = 0;
    let folderCount = 0;
    let knownSizeBytes = 0;
    let knownSizeCount = 0;
    for (const selected of selectedEntries) {
      if (selected.entry.kind === 'directory') {
        folderCount += 1;
        continue;
      }
      fileCount += 1;
      if (typeof selected.entry.size_bytes === 'number') {
        knownSizeBytes += selected.entry.size_bytes;
        knownSizeCount += 1;
      }
    }
    return {
      count: selectedEntries.length,
      fileCount,
      folderCount,
      knownSizeBytes,
      knownSizeCount,
      rows: selectedEntries.slice(0, SUMMARY_ROW_LIMIT),
      hiddenCount: Math.max(0, selectedEntries.length - SUMMARY_ROW_LIMIT),
    };
  });

  function resetPreviewState(resetMax: boolean): void {
    closeContentMenu();
    loadedPath = null;
    textContent = null;
    errorMessage = null;
    loading = false;
    currentEtag = null;
    if (resetMax) previewMaximized = false;
    resetEditState();
    scrollByPath.clear();
  }

  function performSelectionTransition(): void {
    const current = filePreviewStore.selection;
    if (isMultiSelection) {
      resetPreviewState(false);
      return;
    }
    if (current === null) {
      resetPreviewState(true);
      return;
    }
    if (current.path === loadedPath) return;
    loadedPath = current.path;
    resetEditState();
    // ADR-0056 D7 — drop the previous file's scroll on a real file switch.
    scrollByPath.clear();
    void loadPreview(current.path);
  }

  $effect(() => {
    const current = selection;
    void isMultiSelection;
    // The revert-select performed by the dirty guard re-enters this effect; the
    // one-shot flag lets that cycle pass through without re-prompting.
    if (suppressSelectionGuard) {
      suppressSelectionGuard = false;
      performSelectionTransition();
      return;
    }
    // ADR-0057 D4 — a selection change under a dirty edit must confirm before
    // the draft is dropped (file switch / deselect / multi-select / workspace
    // change all surface here as a filePreviewStore change).
    if (editing && dirty) {
      const leaving = isMultiSelection || current === null || current.path !== loadedPath;
      if (leaving) {
        promptDiscardForSelectionChange();
        return;
      }
    }
    performSelectionTransition();
  });

  function promptDiscardForSelectionChange(): void {
    const base = editSelection;
    // Where the user tried to navigate — only a single-file target can be resumed.
    const target: FilePreviewSelection | null =
      !isMultiSelection && selection !== null && (base === null || selection.path !== base.path)
        ? { path: selection.path, entry: selection.entry }
        : null;

    // Revert the store back to the edited file so the panel keeps showing the
    // in-progress edit behind the modal. Deferred to a microtask so we don't
    // mutate the store while inside the effect that reads it.
    if (base !== null) {
      queueMicrotask(() => {
        suppressSelectionGuard = true;
        filePreviewStore.select(base.path, base.entry);
      });
    }

    pendingDiscard = {
      title: 'Discard unsaved changes?',
      message:
        base !== null
          ? `You have unsaved changes to ${basename(base.path)}. Discard them?`
          : 'You have unsaved changes. Discard them?',
      confirmLabel: 'Discard changes',
      onConfirm: () => {
        pendingDiscard = null;
        resetEditState();
        if (target !== null) {
          filePreviewStore.select(target.path, target.entry);
        } else {
          filePreviewStore.clear();
        }
      },
      onCancel: () => {
        pendingDiscard = null;
      },
    };
  }

  $effect(() => {
    if (!previewMaximized) return;
    return escRouter.register({
      priority: 2,
      handler: () => {
        // Stand down while an edit modal owns Esc (ADR-0057 D1).
        if (anyEditModalOpen) return false;
        previewMaximized = false;
        return true;
      },
    });
  });

  // ADR-0057 D1 — Esc cancels the edit (inline-edit tier, priority 1). When a
  // confirm/conflict modal is open, defer to the Modal's own Esc handling.
  $effect(() => {
    if (!editing) return;
    return escRouter.register({
      priority: 1,
      handler: () => {
        if (anyEditModalOpen) return false;
        // Find (also priority 1) owns Esc while its bar is open — defer so the
        // first Esc closes find, not the edit (ADR-0058 amend 2026-07-24).
        if (findCtl.open) return false;
        requestCancelEdit();
        return true;
      },
    });
  });

  // ADR-0057 D4 — browser-native dirty guard (reload / close tab).
  $effect(() => {
    if (!dirty) return;
    if (typeof window === 'undefined') return;
    const handler = (e: BeforeUnloadEvent): void => {
      e.preventDefault();
      e.returnValue = '';
    };
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  });

  // ── Edit mode actions (ADR-0057 D1–D5) ──

  function resetEditState(): void {
    editing = false;
    draft = '';
    saving = false;
    saveError = null;
    editSelection = null;
    conflict = null;
  }

  function enterEdit(): void {
    if (!canEdit || selection === null) return;
    draft = textContent ?? '';
    editSelection = { path: selection.path, entry: selection.entry };
    saveError = null;
    editing = true;
    // ADR-0057 D5 / ADR-0058 D3 REVERSED (2026-07-24) — find stays available
    // while editing and now searches the draft. An open bar survives the
    // read→edit surface swap: getRoot() is the stable `.preview-surface`, so
    // the controller's MutationObserver catches the CodeViewer→CodeEditArea
    // child swap and recomputes against the draft (belt-and-suspenders retarget
    // effect below). No close() here.
  }

  function requestCancelEdit(): void {
    if (!editing) return;
    if (dirty) {
      pendingDiscard = {
        title: 'Discard unsaved changes?',
        message: 'You have unsaved changes. Discard them?',
        confirmLabel: 'Discard changes',
        onConfirm: () => {
          pendingDiscard = null;
          resetEditState();
        },
        onCancel: () => {
          pendingDiscard = null;
        },
      };
      return;
    }
    resetEditState();
  }

  async function saveEdit(): Promise<void> {
    if (!editing || saving || selection === null) return;
    const path = selection.path;
    const etag = currentEtag;
    if (etag === null) {
      saveError = 'Cannot save: missing file version. Reload the file and try again.';
      return;
    }
    saving = true;
    saveError = null;
    try {
      const result = await fsFileWrite(path, draft, etag);
      if (loadedPath !== path) return;
      if (result.ok) {
        // ADR-0057 D5 — advance the baseline + etag, stay in edit mode; the read
        // view (markdown/html re-render) uses the saved content on exit.
        currentEtag = result.etag;
        textContent = draft;
        toastStore.show({ message: 'Saved.', tone: 'success' });
        return;
      }
      if (result.kind === 'conflict') {
        conflict = { path, copied: false, busy: false };
        return;
      }
      saveError = result.message;
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      saveError = err instanceof Error ? err.message : String(err);
    } finally {
      if (loadedPath === path) saving = false;
    }
  }

  async function copyDraftToClipboard(): Promise<void> {
    const result = await copyTextToSystemClipboard(draft);
    if (conflict !== null) conflict = { ...conflict, copied: result.ok };
    toastStore.show({
      message: result.ok ? 'Copied draft to clipboard.' : (result.reason ?? 'Copy failed.'),
      tone: result.ok ? 'success' : 'error',
    });
  }

  // ADR-0057 D4 — resolve a 412 conflict: "reload" drops the draft for server
  // content; "overwrite" re-GETs the fresh etag and re-PUTs (last-writer-wins).
  async function resolveConflict(choice: ConflictChoice): Promise<void> {
    const c = conflict;
    if (c === null || c.busy) return;
    const path = c.path;
    const plan = planConflictResolution(choice);
    conflict = { ...c, busy: true };
    try {
      const freshEtag = plan.refetch ? await fsFileGetEtag(path) : currentEtag;
      if (loadedPath !== path) {
        conflict = null;
        return;
      }
      if (plan.replaceDraft) {
        // Reload branch — drop the draft, re-render the saved content.
        conflict = null;
        resetEditState();
        await loadPreview(path);
        return;
      }
      // Overwrite branch.
      if (freshEtag === null) {
        saveError = 'Could not read the current file version.';
        conflict = null;
        return;
      }
      const result = await fsFileWrite(path, draft, freshEtag);
      if (loadedPath !== path) {
        conflict = null;
        return;
      }
      if (result.ok) {
        currentEtag = result.etag;
        textContent = draft;
        conflict = null;
        toastStore.show({ message: 'Saved. Overwrote external changes.', tone: 'success' });
        return;
      }
      if (result.kind === 'conflict') {
        // Raced again — reopen the dialog for another attempt.
        conflict = { path, copied: c.copied, busy: false };
        return;
      }
      saveError = result.message;
      conflict = null;
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      saveError = err instanceof Error ? err.message : String(err);
      conflict = null;
    }
  }

  // ── Read-surface scroll continuity across inline↔maximize (ADR-0056 D7) ──
  // In-memory only, keyed by file path; never persisted. Text/code (CodeViewer)
  // and markdown surfaces only.
  const scrollByPath = new Map<string, number>();

  function recordScroll(e: Event): void {
    const t = e.target as HTMLElement | null;
    if (t === null || typeof t.className !== 'string') return;
    if (!t.classList.contains('code-viewer') && !t.classList.contains('document-markdown-view')) {
      return;
    }
    const p = selection?.path;
    if (p !== undefined) scrollByPath.set(p, t.scrollTop);
  }

  $effect(() => {
    const variant = activeSurfaceVariant;
    const path = selection?.path;
    void textContent; // re-run once the surface renders content
    if (path === undefined || editing) return;
    const saved = scrollByPath.get(path);
    if (saved === undefined) return;
    void tick().then(() => {
      const surf = surfaceEls[variant];
      const scroller = surf?.querySelector<HTMLElement>('.code-viewer, .document-markdown-view');
      if (scroller !== null && scroller !== undefined) scroller.scrollTop = saved;
    });
  });

  async function loadPreview(path: string): Promise<void> {
    textContent = null;
    errorMessage = null;
    const nextKind = previewMetaForPath(path).kind;
    if (nextKind === 'image' || nextKind === 'pdf') {
      loading = false;
      return;
    }
    loading = true;
    try {
      const res = await fetch(fsFileUrl(path), {
        method: 'GET',
        credentials: 'include',
        headers: { Accept: 'text/plain,application/json,text/html,text/markdown,*/*' },
      });
      if (res.status === 401) throw new UnauthorizedError();
      if (res.status === 404) {
        // The selected file no longer exists (moved/deleted). Drop the stale
        // selection so it doesn't re-fetch and 404 on every Files-tab entry
        // (ADR-0046 amend ⑪ persists the selection), and fall back to the empty
        // state instead of surfacing an error. The selection $effect resets the
        // rest of the preview once `selection` becomes null.
        if (loadedPath === path) filePreviewStore.clear();
        return;
      }
      if (!res.ok) throw new Error(`GET /api/fs/file returned ${res.status}`);
      const nextEtag = res.headers.get('etag');
      const nextText = await res.text();
      if (loadedPath !== path) return;
      textContent = nextText;
      // ADR-0057 D3 — capture the ETag for a later If-Match write.
      currentEtag = nextEtag;
    } catch (err) {
      if (loadedPath !== path) return;
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      errorMessage = err instanceof Error
          ? err.message
          : String(err);
    } finally {
      if (loadedPath === path) loading = false;
    }
  }

  function fmtSize(bytes: number | null | undefined): string {
    if (bytes === null || bytes === undefined) return '';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function extLabel(path: string): string {
    const ext = extension(path).slice(1);
    if (ext.length > 0) return ext.slice(0, 4);
    return previewMetaForPath(path).fileTypeLabel.slice(0, 4) || 'file';
  }

  function extClass(path: string): string {
    return previewMetaForPath(path).chipClass;
  }

  function compactPath(path: string): string {
    const parts = path.split('/').filter(Boolean);
    if (parts.length <= 2) return path;
    return `.../${parts.slice(-2).join('/')}`;
  }

  function summaryMeta(summary: MultiSelectionSummary): string {
    const parts = [
      `${summary.fileCount} file${summary.fileCount === 1 ? '' : 's'}`,
      `${summary.folderCount} folder${summary.folderCount === 1 ? '' : 's'}`,
    ];
    if (summary.knownSizeCount > 0) {
      parts.push(fmtSize(summary.knownSizeBytes));
    }
    return parts.join(' · ');
  }

  async function copyPath(): Promise<void> {
    const path = selection?.path;
    if (path === undefined) return;
    const result = await copyTextToSystemClipboard(path);
    toastStore.show({
      message: result.ok ? 'Copied file path.' : (result.reason ?? 'Copy failed.'),
      tone: result.ok ? 'success' : 'error',
    });
  }

  function closeContentMenu(): void {
    contentMenu = null;
  }

  function openContentMenu(e: MouseEvent, current: FilePreviewSelection): void {
    e.preventDefault();
    e.stopPropagation();
    const root = e.currentTarget as HTMLElement;
    const selectedText = selectedTextWithin(root);
    const sourceRange = kind === 'text'
      ? selectionToRange(root, window.getSelection())
      : null;
    contentMenu = {
      x: e.clientX,
      y: e.clientY,
      copyText: selectedText,
      pathWithLocation: formatPathWithLocation(current.path, sourceRange),
    };
    queueMicrotask(clampContentMenu);
  }

  function selectedTextWithin(root: HTMLElement): string {
    const sel = window.getSelection();
    if (sel === null || sel.rangeCount === 0 || sel.isCollapsed) return '';
    const range = sel.getRangeAt(0);
    const node = range.commonAncestorContainer;
    if (node !== root && !root.contains(node)) return '';
    return sel.toString();
  }

  function clampContentMenu(): void {
    if (contentMenu === null || contentMenuEl === undefined) return;
    const rect = contentMenuEl.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let nx = contentMenu.x;
    let ny = contentMenu.y;
    if (nx + rect.width > vw) nx = Math.max(0, vw - rect.width - 4);
    if (ny + rect.height > vh) ny = Math.max(0, vh - rect.height - 4);
    if (nx !== contentMenu.x || ny !== contentMenu.y) {
      contentMenu = { ...contentMenu, x: nx, y: ny };
    }
  }

  function onWindowPointerDown(e: PointerEvent): void {
    if (contentMenu === null || contentMenuEl === undefined) return;
    if (contentMenuEl.contains(e.target as Node)) return;
    closeContentMenu();
  }

  function onWindowKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') closeContentMenu();
  }

  // ADR-0046 D6 amend ⑬ (supersedes ⑩) — in the active Preview:
  //   Cmd/Ctrl+C        = copy the SELECTED TEXT (only when a real selection lies
  //                       in the preview surface; otherwise native copy passes).
  //   Cmd/Ctrl+Shift+C  = Copy path (+ selection location), the old ⑩ behavior.
  // Handled in CAPTURE phase so it wins over the global bubble keydown handlers
  // (shortcutRegistry / canvas) and the native copy event when the Preview is the
  // active surface with a single selected file. The gate returns false everywhere
  // else; the right-click menu's Copy / Copy path (amend ⑥) are unchanged.
  function onWindowKeydownCapture(e: KeyboardEvent): void {
    // ADR-0057 D1 — Cmd/Ctrl+S saves the current edit. Capture phase + a
    // preview-active/editing gate (mirrors the keyboard-copy gate) so it wins
    // over the browser's native save dialog only when this preview is editing.
    const isSaveChord =
      (e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && (e.key === 's' || e.key === 'S');
    if (isSaveChord) {
      if (chromeStore.state.rightPanelTab === 'preview' && editing && !anyEditModalOpen) {
        e.preventDefault();
        e.stopImmediatePropagation();
        void saveEdit();
      }
      return;
    }
    const isCopyChord =
      (e.metaKey || e.ctrlKey) && !e.altKey && (e.key === 'c' || e.key === 'C');
    if (!isCopyChord) return;
    if (!canKeyboardCopy()) return;
    if (e.shiftKey) {
      // Cmd/Ctrl+Shift+C → Copy path (amend ⑬.3).
      e.preventDefault();
      e.stopImmediatePropagation();
      void copyPathViaShortcut();
      return;
    }
    // Cmd/Ctrl+C → copy selected text, but only hijack when there IS a selection
    // in the preview surface; with no selection, let native copy pass (no-op).
    const root = previewSurfaceEl;
    if (root !== undefined && selectedTextWithin(root).length > 0) {
      e.preventDefault();
      e.stopImmediatePropagation();
      void copySelectedTextViaShortcut();
    }
  }

  /** Gate for the amend ⑬ keyboard copy: Preview active + single file + focus not
   *  in an editable field / xterm (so normal field copy still works). */
  function canKeyboardCopy(): boolean {
    if (chromeStore.state.rightPanelTab !== 'preview') return false;
    if (selection === null || isMultiSelection) return false;
    const el = document.activeElement as HTMLElement | null;
    if (el !== null) {
      const tag = el.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return false;
      if (el.isContentEditable) return false;
      if (el.classList.contains('xterm-helper-textarea')) return false;
    }
    return true;
  }

  async function copyPathViaShortcut(): Promise<void> {
    const current = selection;
    if (current === null) return;
    closeContentMenu();
    const root = previewSurfaceEl;
    // Include the selection's source location only when a real text selection
    // lies within the preview surface (same rule the right-click menu uses);
    // otherwise copy the plain absolute path.
    const hasSelInPreview =
      root !== undefined && kind === 'text' && selectedTextWithin(root).length > 0;
    const sourceRange = hasSelInPreview ? selectionToRange(root, window.getSelection()) : null;
    const value = formatPathWithLocation(current.path, sourceRange);
    const result = await copyTextToSystemClipboard(value);
    toastStore.show({
      message: result.ok ? 'Copied file path.' : (result.reason ?? 'Copy failed.'),
      tone: result.ok ? 'success' : 'error',
    });
  }

  /** Cmd/Ctrl+C (amend ⑬.2) — copy the live text selection within the preview. */
  async function copySelectedTextViaShortcut(): Promise<void> {
    const root = previewSurfaceEl;
    if (root === undefined) return;
    const text = selectedTextWithin(root);
    if (text.length === 0) return;
    const result = await copyTextToSystemClipboard(text);
    toastStore.show({
      message: result.ok ? 'Copied selection.' : (result.reason ?? 'Copy failed.'),
      tone: result.ok ? 'success' : 'error',
    });
  }

  async function copySelectedText(): Promise<void> {
    const menu = contentMenu;
    if (menu === null || menu.copyText.length === 0) return;
    closeContentMenu();
    const result = await copyTextToSystemClipboard(menu.copyText);
    toastStore.show({
      message: result.ok ? 'Copied selection.' : (result.reason ?? 'Copy failed.'),
      tone: result.ok ? 'success' : 'error',
    });
  }

  async function copyContentPath(): Promise<void> {
    const menu = contentMenu;
    if (menu === null) return;
    closeContentMenu();
    const result = await copyTextToSystemClipboard(menu.pathWithLocation);
    toastStore.show({
      message: result.ok ? 'Copied file path.' : (result.reason ?? 'Copy failed.'),
      tone: result.ok ? 'success' : 'error',
    });
  }

  function openPreviewMaximize(e: MouseEvent): void {
    e.stopPropagation();
    e.preventDefault();
    if (selection === null && !isMultiSelection) return;
    previewMaximized = true;
  }

  function closePreviewMaximize(e: MouseEvent): void {
    e.stopPropagation();
    e.preventDefault();
    previewMaximized = false;
  }

  function blockBackdropEvent(e: Event): void {
    if (e.target !== e.currentTarget) return;
    e.preventDefault();
    e.stopPropagation();
  }

  // ── In-document find (ADR-0058) — preview surface ──
  // The previewSurface snippet renders in two spots (inline + maximized
  // overlay); the find targets whichever is active, so each mount registers
  // itself per variant and the controller resolves the active one.
  type PreviewSurfaceVariant = 'inline' | 'max';
  let surfaceEls = $state<{ inline: HTMLElement | null; max: HTMLElement | null }>({
    inline: null,
    max: null,
  });
  let findBar = $state<{
    focusInput: (opts?: { selectAll?: boolean }) => void;
    prefill: (text: string) => void;
  } | null>(null);

  function captureSurface(
    node: HTMLElement,
    variant: PreviewSurfaceVariant,
  ): { destroy: () => void } {
    surfaceEls[variant] = node;
    return {
      destroy: () => {
        if (surfaceEls[variant] === node) surfaceEls[variant] = null;
      },
    };
  }

  const activeSurfaceVariant = $derived(previewMaximized ? 'max' : 'inline');

  const findCtl = new DocumentFindController({
    getRoot: () => (previewMaximized ? surfaceEls.max : surfaceEls.inline),
    // ADR-0057/0058 amend (2026-07-24) — while editing, find the LIVE draft (the
    // CodeEditArea overlay reflects `draft`); otherwise the saved text.
    getCodeText: () => (editing ? draft : (textContent ?? '')),
  });

  /** Searchable kinds only (ADR-0058 D3/D4): markdown rendered + text/code, and
   *  — per the 2026-07-24 amend (reverses ADR-0057 D5 v1 / ADR-0058 D3) — the
   *  edit-mode draft too. Not html rendered / pdf / image / directory /
   *  multi-select. */
  const findSearchable = $derived(
    !isMultiSelection &&
      selection !== null &&
      !loading &&
      errorMessage === null &&
      textContent !== null &&
      (kind === 'markdown' || kind === 'text'),
  );

  function openFindSurface(req?: DocumentFindOpenRequest): void {
    // ADR-0058 D5 amend (2026-07-24) — a textarea selection is a form-control
    // selection, not a DOM Selection, so the global Cmd/Ctrl+F routing cannot
    // prefill from it. Read selectionStart/End off the CodeEditArea textarea
    // here (before focus moves to the FindBar) and use it as the query seed.
    let seed = req?.prefill ?? '';
    if (seed.length === 0 && editing) {
      const root = previewMaximized ? surfaceEls.max : surfaceEls.inline;
      const ta = root?.querySelector<HTMLTextAreaElement>('.cea-textarea') ?? null;
      if (ta !== null && ta.selectionStart !== ta.selectionEnd) {
        seed = ta.value.slice(ta.selectionStart, ta.selectionEnd);
      }
    }
    findCtl.openBar();
    void tick().then(() => {
      // First line only — line-based code matching never crosses newlines
      // (ADR-0058 D2).
      const prefill = seed.split('\n')[0]?.trim() ?? '';
      if (prefill.length > 0) findBar?.prefill(prefill);
      findBar?.focusInput({ selectAll: true });
    });
  }

  // Cmd/Ctrl+F routing target (ADR-0058 D5 branches 2/4) — while searchable.
  $effect(() => {
    if (!findSearchable) return;
    return registerDocumentFindSurface('preview', openFindSurface);
  });

  // FILE SWITCH closes the find (ADR-0058 D1 override 2026-07-23 — reverses the
  // v1 "close on any transition"): the bar would re-target an unrelated
  // document, so a fresh file starts with find closed.
  //
  // `findCtl.close()` reads `findCtl.open` internally (its idempotency guard),
  // so calling it untracked keeps this effect keyed on `selection.path` ONLY.
  // Without untrack, `openBar()` setting `open = true` re-fires this effect and
  // instantly closes the bar — the find button would appear dead (same untrack
  // discipline as the MaximizedItemModal flush effect, commit 4eec92f).
  $effect(() => {
    void selection?.path;
    untrack(() => findCtl.close());
  });

  // MAXIMIZE/RESTORE keeps find open but RE-TARGETS the newly mounted surface
  // (ADR-0058 D1 override 2026-07-23 — survive maximize/restore). The controller
  // already resolves getRoot() by previewMaximized, but the observer was started
  // on the old root and the ranges point at the now-unmounted DOM; retarget()
  // re-observes the new root and recomputes index-preserving once the new
  // variant's surface has mounted (after tick). Keyed on previewMaximized only —
  // the open-state read is untracked (tracked-read bug class, commit 4eec92f).
  $effect(() => {
    void previewMaximized;
    if (!untrack(() => findCtl.open)) return;
    void tick().then(() => untrack(() => findCtl.retarget()));
  });

  $effect(() => {
    if (!findSearchable && findCtl.open) findCtl.close();
  });

  // EDIT↔READ swap keeps find open and RE-TARGETS the newly mounted surface
  // (ADR-0058 amend 2026-07-24 — find survives edit transitions). getRoot() is
  // the stable `.preview-surface`, so the observer already sees the child swap;
  // this retarget makes the recompute deterministic after the new surface has
  // mounted. Keyed on `editing` only — the open-state read is untracked
  // (tracked-read bug class, commit 4eec92f).
  $effect(() => {
    void editing;
    if (!untrack(() => findCtl.open)) return;
    void tick().then(() => untrack(() => findCtl.retarget()));
  });

  $effect(() => {
    return () => findCtl.destroy();
  });
</script>

<svelte:window
  onpointerdowncapture={onWindowPointerDown}
  onkeydowncapture={onWindowKeydownCapture}
  onkeydown={onWindowKeydown}
  onresize={closeContentMenu}
  onblur={closeContentMenu}
/>

<!-- ADR-0057 D1 (icon toolbar) — shared toolbar glyphs. icon system
     unification 2026-07-27 (ADR-0016 정합): every glyph now routes through
     the shared CanvasGlyph (24-unit viewBox, stroke 2, 13px preview/modal
     chrome tier), so same-function glyphs are byte-identical to the canvas
     node headers and the maximized modal. -->
{#snippet saveIcon()}
  <CanvasGlyph name="save" size={13} />
{/snippet}

{#snippet cancelIcon()}
  <!-- book-open — the destination mode (viewer). Eye collides with the canvas
       visibility glyph (user feedback 2026-07-24); book-open matches
       DocumentNode's "show rendered" toggle, same "back to reading" family. -->
  <CanvasGlyph name="book-open" size={13} />
{/snippet}

{#snippet maximizeIcon(isMax: boolean)}
  {#if isMax}
    <!-- while maximized → lucide minimize (corner brackets in) -->
    <CanvasGlyph name="restore-max" size={13} />
  {:else}
    <!-- lucide maximize (corner brackets out) -->
    <CanvasGlyph name="maximize" size={13} />
  {/if}
{/snippet}

{#snippet findIcon()}
  <CanvasGlyph name="search" size={13} />
{/snippet}

{#snippet editIcon()}
  <!-- pencil — the "Mode" slot in read mode (enter edit). -->
  <CanvasGlyph name="pencil" size={13} />
{/snippet}

{#snippet downloadIcon()}
  <CanvasGlyph name="download" size={13} />
{/snippet}

{#snippet copyPathIcon()}
  <CanvasGlyph name="copy" size={13} />
{/snippet}

{#snippet undoIcon()}
  <CanvasGlyph name="undo" size={13} />
{/snippet}

{#snippet redoIcon()}
  <CanvasGlyph name="redo" size={13} />
{/snippet}

<!-- ADR-0057 D1 amend 2026-07-27 — UNIFIED single-file toolbar. Same slot order
     in BOTH read/edit and BOTH inline/maximized: Find · Mode · Download ·
     Copy path · Maximize/Restore. "Mode" is one slot whose glyph/action swaps:
     read → pencil (enter edit); edit → book-open with is-active (back to
     viewer, dirty-discard confirm + Esc semantics via requestCancelEdit).
     Save/Undo/Redo now live in the separate edit action bar (editActionBar).
     A button that doesn't apply (Mode for non-editable kinds, Find for
     non-searchable) simply disappears — order of the rest is unchanged. -->
{#snippet fileToolbar(current: FilePreviewSelection, variant: PreviewSurfaceVariant)}
  {#if canEdit || editing}
    <!-- ADR-0057 D1 / ADR-0037 D1 UI amend 2026-07-27 — Mode is a two-button
         segmented control [Viewer | Edit] leading the toolbar (same vocabulary
         as SnippetsNode's .snip-mode-group). Active side is color-filled like
         the snippet modes: Viewer = accent, Edit = purple (--color-mode-edit).
         Clicking the active side is a no-op; clicking Viewer while editing
         runs the dirty-discard path (requestCancelEdit). Hidden entirely for
         non-editable kinds. -->
    <div class="mode-group" role="group" aria-label="View mode">
      <button
        type="button"
        class="icon-btn mode-seg-btn"
        class:is-active={!editing}
        title="Viewer"
        aria-label="Viewer mode"
        aria-pressed={!editing}
        disabled={saving}
        onclick={() => { if (editing) requestCancelEdit(); }}
      >
        {@render cancelIcon()}
      </button>
      <button
        type="button"
        class="icon-btn mode-seg-btn is-edit"
        class:is-active={editing}
        title="Edit"
        aria-label="Edit mode"
        aria-pressed={editing}
        onclick={() => { if (!editing) enterEdit(); }}
      >
        {@render editIcon()}
      </button>
    </div>
  {/if}
  {#if findSearchable}
    <button
      type="button"
      class="icon-btn"
      class:is-active={findCtl.open}
      title="Find in file"
      aria-label="Find in file"
      onclick={() => openFindSurface()}
    >
      {@render findIcon()}
    </button>
  {/if}
  {#if previewUrl.length > 0}
    <a class="icon-btn" href={fsDownloadUrl(current.path)} download={basename(current.path)} title="Download" aria-label="Download">
      {@render downloadIcon()}
    </a>
  {:else}
    <button type="button" class="icon-btn" disabled title="Download unavailable" aria-label="Download">
      {@render downloadIcon()}
    </button>
  {/if}
  <button type="button" class="icon-btn" title="Copy path" aria-label="Copy path" onclick={() => void copyPath()}>
    {@render copyPathIcon()}
  </button>
  {#if variant === 'max'}
    <button type="button" class="icon-btn is-active" title="Restore (Esc)" aria-label="Restore" onclick={closePreviewMaximize}>
      {@render maximizeIcon(true)}
    </button>
  {:else}
    <button
      type="button"
      class="icon-btn"
      class:is-active={previewMaximized}
      title={previewMaximized ? 'Restore' : 'Maximize'}
      aria-label={previewMaximized ? 'Restore' : 'Maximize'}
      onclick={previewMaximized ? closePreviewMaximize : openPreviewMaximize}
    >
      {@render maximizeIcon(previewMaximized)}
    </button>
  {/if}
{/snippet}

<!-- ADR-0057 D1 amend 2026-07-27 — edit action bar: a second header row shown
     below the main toolbar (in both inline and maximized edit views), holding
     Save · Undo · Redo as icon-btns. Save moved OUT of the main toolbar into
     this bar (Cmd/Ctrl+S still saves; disabled while saving || !dirty). The
     dirty dot lives here, right-aligned as an "unsaved" status. Undo/Redo
     preventDefault mousedown so focus stays in the textarea for execCommand;
     they stay enabled while editing (native stack has no reliable
     canUndo/canRedo introspection). -->
{#snippet editActionBar()}
  <div class="edit-action-bar" role="toolbar" aria-label="Edit actions">
    <button
      type="button"
      class="icon-btn"
      title="Save (⌘S)"
      aria-label="Save"
      disabled={saving || !dirty}
      onclick={() => void saveEdit()}
    >
      {@render saveIcon()}
    </button>
    <button
      type="button"
      class="icon-btn"
      title="Undo (⌘Z)"
      aria-label="Undo"
      onmousedown={(e: MouseEvent) => e.preventDefault()}
      onclick={() => editAreaRef?.undo()}
    >
      {@render undoIcon()}
    </button>
    <button
      type="button"
      class="icon-btn"
      title="Redo (⇧⌘Z)"
      aria-label="Redo"
      onmousedown={(e: MouseEvent) => e.preventDefault()}
      onclick={() => editAreaRef?.redo()}
    >
      {@render redoIcon()}
    </button>
    {#if dirty}
      <span class="dirty-dot" title="Unsaved changes" aria-label="Unsaved changes">●</span>
    {/if}
  </div>
{/snippet}

{#snippet previewSurface(current: FilePreviewSelection, variant: PreviewSurfaceVariant)}
  <!-- ADR-0058 D1 — FindBar sits as a SIBLING of the observed surface (the
       captured `.preview-surface`), not inside it: its live counter text must
       not fire the surface MutationObserver (subtree/characterData → rAF
       re-match per navigation). The wrapper is the positioned anchor so the
       floating overlay still lands at the same top-right box. -->
  <div class="preview-surface-wrap">
    {#if findCtl.open && findSearchable && variant === activeSurfaceVariant}
      <!-- floating find overlay; rendered only in the visible copy of the
           snippet (inline vs maximized overlay). -->
      <FindBar
        bind:this={findBar}
        matchCount={findCtl.count}
        currentIndex={findCtl.currentIndex}
        capped={findCtl.capped}
        initialQuery={findCtl.query}
        onQueryChange={(q: string) => findCtl.setQuery(q)}
        onNavigate={(dir: 1 | -1) => findCtl.navigate(dir)}
        onClose={() => findCtl.close()}
      />
    {/if}
    <div
      bind:this={previewSurfaceEl}
      use:captureSurface={variant}
      class="preview-surface"
      role="region"
      aria-label="Preview content"
      data-find-surface={findSearchable ? 'preview' : undefined}
      style:--preview-content-scale={componentSettings.previewScale}
      oncontextmenu={(e: MouseEvent) => openContentMenu(e, current)}
      onscroll={closeContentMenu}
      onscrollcapture={recordScroll}
    >
    {#if editing}
      <!-- ADR-0057 D1 — textarea-based editor (R1: no CodeMirror) with the
           IDE overlay (gutter + Shiki layer) matching the read-mode CodeViewer.
           `lang` comes from the same workspaceAssets meta as read mode. -->
      {#if saveError !== null}
        <div class="edit-error" role="alert">{saveError}</div>
      {/if}
      <CodeEditArea
        bind:this={editAreaRef}
        bind:value={draft}
        lang={codeLang}
        ariaLabel={`Editing ${basename(current.path)}`}
      />
    {:else if loading}
      <div class="empty-state">
        <span class="spin" aria-hidden="true"></span>
        <span class="desc">Loading preview...</span>
      </div>
    {:else if kind === 'directory'}
      <PanelEmptyState
        icon="files"
        lead="Folder selected"
        description="Use Files actions to upload here, rename, remove, or add it to canvas."
      />
    {:else if errorMessage !== null}
      <PanelEmptyState
        icon="preview"
        lead="Preview unavailable"
        description={errorMessage}
        role="alert"
      />
    {:else if kind === 'image' && previewUrl.length > 0}
      <ImageViewer src={previewUrl} alt={basename(current.path)} />
    {:else if kind === 'pdf' && previewUrl.length > 0}
      <PdfViewer src={previewUrl} title={basename(current.path)} />
    {:else if kind === 'markdown'}
      <DocumentMarkdownView
        html={renderedMarkdown}
        label={basename(current.path)}
        scale={componentSettings.previewScale}
      />
    {:else if kind === 'html'}
      <HtmlViewer
        title={basename(current.path)}
        srcdoc={renderedHtml}
        sandbox={RENDERED_HTML_IFRAME_SANDBOX}
      />
    {:else if kind === 'text'}
      <CodeViewer text={textContent ?? ''} lang={codeLang} filename={basename(current.path)} />
    {:else}
      <PanelEmptyState
        icon="preview"
        lead="Preview unavailable"
        description="Download or open it from the project workspace."
      />
    {/if}
    </div>
  </div>
{/snippet}

{#snippet multiSelectionSurface(summary: MultiSelectionSummary)}
  <div class="multi-summary">
    <div class="summary-strip" aria-label="Selection summary">
      <div class="summary-cell">
        <span class="summary-value">{summary.count}</span>
        <span class="summary-label">selected</span>
      </div>
      <div class="summary-cell">
        <span class="summary-value">{summary.fileCount}</span>
        <span class="summary-label">files</span>
      </div>
      <div class="summary-cell">
        <span class="summary-value">{summary.folderCount}</span>
        <span class="summary-label">folders</span>
      </div>
    </div>
    <div class="summary-section">
      <div class="summary-section-head">
        <span>Selection</span>
        {#if summary.knownSizeCount > 0}
          <span>{fmtSize(summary.knownSizeBytes)}</span>
        {/if}
      </div>
      <div class="summary-list" role="list" aria-label="Selected files">
        {#each summary.rows as selected (selected.path)}
          <div class="summary-row" role="listitem">
            <span class="summary-kind" class:is-folder={selected.entry.kind === 'directory'} aria-hidden="true">
              {selected.entry.kind === 'directory' ? 'dir' : extLabel(selected.path)}
            </span>
            <span class="summary-row-text">
              <span class="summary-row-name" title={selected.path}>{basename(selected.path)}</span>
              <span class="summary-row-path" title={selected.path}>{compactPath(selected.path)}</span>
            </span>
            <span class="summary-row-size">
              {selected.entry.kind === 'directory' ? 'folder' : fmtSize(selected.entry.size_bytes)}
            </span>
          </div>
        {/each}
        {#if summary.hiddenCount > 0}
          <div class="summary-more">+ {summary.hiddenCount} more</div>
        {/if}
      </div>
    </div>
  </div>
{/snippet}

<div class="preview">
  {#if isMultiSelection}
    <header class="preview-head">
      <div class="title-row">
        <span class="ext-chip multi">sel</span>
        <span class="file-name" title={`${multiSummary.count} selected`}>{multiSummary.count} items selected</span>
        <span class="actions">
          <button
            type="button"
            class="icon-btn"
            class:is-active={previewMaximized}
            title={previewMaximized ? 'Restore' : 'Maximize'}
            aria-label={previewMaximized ? 'Restore' : 'Maximize'}
            onclick={previewMaximized ? closePreviewMaximize : openPreviewMaximize}
          >
            {@render maximizeIcon(previewMaximized)}
          </button>
        </span>
      </div>
      <div class="file-meta">
        {summaryMeta(multiSummary)}
      </div>
    </header>
    {@render multiSelectionSurface(multiSummary)}
  {:else if selection === null}
    <PanelEmptyState
      icon="preview"
      lead="No file selected"
      description="Select a file in Files to preview it here."
    />
  {:else}
    <header class="preview-head">
      <div class="title-row">
        <span class="ext-chip {extClass(selection.path)}">{extLabel(selection.path)}</span>
        <span class="file-name" title={selection.path}>{basename(selection.path)}</span>
        <span class="actions">
          {@render fileToolbar(selection, 'inline')}
        </span>
      </div>
      <div class="file-meta" title={selection.path}>
        {[fmtSize(selection.entry.size_bytes), compactPath(selection.path)].filter(Boolean).join(' · ')}
      </div>
    </header>

    {#if editing}
      {@render editActionBar()}
    {/if}
    {@render previewSurface(selection, 'inline')}
  {/if}
</div>

{#if contentMenu !== null}
  <div
    bind:this={contentMenuEl}
    class="preview-content-menu"
    style:left={`${contentMenu.x}px`}
    style:top={`${contentMenu.y}px`}
    role="menu"
    tabindex="-1"
    oncontextmenu={(e: MouseEvent) => e.preventDefault()}
    onkeydown={(e: KeyboardEvent) => {
      if (e.key === 'Escape') closeContentMenu();
    }}
  >
    <button
      type="button"
      role="menuitem"
      disabled={contentMenu.copyText.length === 0}
      onclick={() => void copySelectedText()}
    >Copy</button>
    <button type="button" role="menuitem" onclick={() => void copyContentPath()}>Copy path</button>
  </div>
{/if}

{#if previewMaximized && (selection !== null || isMultiSelection)}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="preview-max-backdrop"
    role="dialog"
    aria-modal="true"
    aria-label="Maximized preview"
    tabindex="-1"
    onpointerdown={blockBackdropEvent}
    onpointerup={blockBackdropEvent}
    onmousedown={blockBackdropEvent}
    onmouseup={blockBackdropEvent}
    onclick={blockBackdropEvent}
    ondblclick={blockBackdropEvent}
    oncontextmenu={blockBackdropEvent}
    onwheel={blockBackdropEvent}
  >
    <div class="preview-max-card">
      <header class="preview-max-header">
        {#if isMultiSelection}
          <span class="ext-chip multi">sel</span>
          <div class="preview-max-title-group">
            <span class="preview-max-title" title={`${multiSummary.count} selected`}>{multiSummary.count} items selected</span>
            <span class="preview-max-meta">{summaryMeta(multiSummary)}</span>
          </div>
        {:else if selection !== null}
          <span class="ext-chip {extClass(selection.path)}">{extLabel(selection.path)}</span>
          <div class="preview-max-title-group">
            <span class="preview-max-title" title={selection.path}>{basename(selection.path)}</span>
            <span class="preview-max-meta" title={selection.path}>
              {[fmtSize(selection.entry.size_bytes), compactPath(selection.path)].filter(Boolean).join(' · ')}
            </span>
          </div>
        {/if}
        <div class="preview-max-actions">
          {#if !isMultiSelection && selection !== null}
            <!-- ADR-0057 D1 amend 2026-07-27 — same unified toolbar as inline
                 (Find · Mode · Download · Copy path · Restore). -->
            {@render fileToolbar(selection, 'max')}
          {:else}
            <!-- Multi-selection maximized: restore only. -->
            <button
              type="button"
              class="icon-btn is-active"
              title="Restore (Esc)"
              aria-label="Restore"
              onclick={closePreviewMaximize}
            >
              {@render maximizeIcon(true)}
            </button>
          {/if}
        </div>
      </header>
      {#if !isMultiSelection && selection !== null && editing}
        {@render editActionBar()}
      {/if}
      <div class="preview-max-body">
        {#if isMultiSelection}
          {@render multiSelectionSurface(multiSummary)}
        {:else if selection !== null}
          {@render previewSurface(selection, 'max')}
        {/if}
      </div>
    </div>
  </div>
{/if}

<!-- ADR-0057 D4 — dirty-guard discard confirm (reuses the Modal primitive). -->
<Modal
  open={pendingDiscard !== null}
  onclose={() => pendingDiscard?.onCancel()}
  title={pendingDiscard?.title ?? 'Discard unsaved changes?'}
  dismissOnBackdrop={false}
>
  {#snippet body()}
    <p class="modal-lead">{pendingDiscard?.message ?? ''}</p>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => pendingDiscard?.onCancel()}>Keep editing</Button>
    <Button variant="danger" onclick={() => pendingDiscard?.onConfirm()}>
      {pendingDiscard?.confirmLabel ?? 'Discard changes'}
    </Button>
  {/snippet}
</Modal>

<!-- ADR-0057 D4 — 412 conflict resolution (reload vs overwrite). -->
<Modal
  open={conflict !== null}
  onclose={() => { if (conflict !== null && !conflict.busy) conflict = null; }}
  title="File changed on disk"
  dismissOnBackdrop={false}
>
  {#snippet body()}
    <p class="modal-lead">
      This file was modified by another program since you started editing. Choose how to
      resolve it. You can copy your draft to the clipboard first.
    </p>
    <p class="modal-note note">
      <button type="button" class="link-btn" onclick={() => void copyDraftToClipboard()}>
        Copy my draft to clipboard
      </button>
      {#if conflict?.copied}<span class="copied-tag">copied</span>{/if}
    </p>
  {/snippet}
  {#snippet footer()}
    <Button
      variant="ghost"
      disabled={conflict?.busy ?? false}
      onclick={() => void resolveConflict('reload')}
    >Reload (drop my changes)</Button>
    <Button
      variant="danger"
      disabled={conflict?.busy ?? false}
      onclick={() => void resolveConflict('overwrite')}
    >Overwrite</Button>
  {/snippet}
</Modal>

<style>
  .preview {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
    background: var(--color-surface);
  }

  .preview-head {
    display: grid;
    gap: var(--space-6);
    padding: var(--space-8) var(--space-10);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-2);
    flex: 0 0 auto;
  }

  .title-row {
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-6);
  }

  .ext-chip {
    flex: 0 0 auto;
    max-width: 42px;
    padding: 1px 5px;
    border-radius: var(--radius-sm);
    background: var(--color-fg-muted);
    color: var(--color-bg);
    font-family: var(--font-mono);
    font-size: 8.5px;
    line-height: var(--leading-normal);
    letter-spacing: 0.4px;
    text-transform: uppercase;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ext-chip.code {
    background: #3178c6;
    color: #fff;
  }

  .ext-chip.md {
    background: #555;
    color: #fff;
  }

  .ext-chip.img {
    background: #d98b2b;
    color: #fff;
  }

  .ext-chip.pdf {
    background: #c4282c;
    color: #fff;
  }

  .ext-chip.multi {
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }

  /* Header title — NoteNode-anchored micro-label family (icon system
     unification 2026-07-27, ADR-0016 정합): mono · 9.5px · 540 · 0.6px.
     NO uppercase — filename is case-bearing (semantic distortion).
     Matches MaximizedItemModal .header-title. */
  .file-name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: 9.5px;
    font-weight: var(--weight-semibold);
    letter-spacing: 0.6px;
  }

  /* Meta sub-text — icon system unification 2026-07-27 (ADR-0016 정합):
     aligned with the sibling canvas meta lines (DocumentNode .doc-head /
     SnippetsNode meta): mono · 10px · muted · 0.4px. */
  .file-meta {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    letter-spacing: 0.4px;
  }

  .actions {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 1px;
    flex: 0 0 auto;
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
    text-decoration: none;
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .icon-btn:hover:not(:disabled) {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  /* Active-toggle treatment = neutral glass (SoT §3 — the 2026-07-27 accent
     tint experiment was reverted the same day on user review). */
  .icon-btn.is-active {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .icon-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  /* ADR-0057 D1 / ADR-0037 D1 UI amend 2026-07-27 — [Viewer | Edit] segmented
     control. Faint container + inter-button gap = one segmented unit (same
     visual vocabulary as SnippetsNode's .snip-mode-group). Buttons drop to
     22×22 inside the group so the 2-button Mode slot still fits a narrow
     (~275px) preview toolbar. */
  .mode-group {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    padding: 1px;
    background: var(--color-glass-1);
    border-radius: var(--radius-sm);
    flex: 0 0 auto;
    /* Mode-group ↔ neighbouring buttons = 8px (SoT §1.1: group-adjacency gap,
       2026-07-27 ×2 re-adjust). 7px side margin + the .actions 1px flex gap
       = 8px to the next button; plain button↔button stays 1px. */
    margin: 0 7px;
  }
  .mode-seg-btn {
    width: 22px;
    height: 22px;
  }
  /* Active side is color-filled like the snippet modes (2026-07-27):
     Viewer = accent, Edit = purple ("edit = purple" app-wide,
     --color-mode-edit). Active fill is kept on hover (no ghost fallback). */
  .mode-seg-btn.is-active,
  .mode-seg-btn.is-active:hover:not(:disabled) {
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }
  .mode-seg-btn.is-edit.is-active,
  .mode-seg-btn.is-edit.is-active:hover:not(:disabled) {
    background: var(--color-mode-edit);
    color: #fff;
  }

  .dirty-dot {
    flex: 0 0 auto;
    color: var(--color-accent);
    font-size: 10px;
    line-height: 1;
  }

  /* ADR-0057 D1 amend 2026-07-27 — edit action bar: a compact second header
     row (Save · Undo · Redo) matching the header chrome (border-bottom,
     surface-2 bg, ~30px). The dirty dot is right-aligned here as an "unsaved"
     status marker. Present in both inline and maximized edit views. */
  .edit-action-bar {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 1px;
    height: 30px;
    padding: 0 6px;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-2);
    user-select: none;
  }

  .edit-action-bar .dirty-dot {
    margin-left: auto;
    padding-right: 2px;
  }

  .edit-error {
    flex: 0 0 auto;
    padding: var(--space-8) var(--space-12);
    border-bottom: 1px solid var(--color-border);
    background: color-mix(in srgb, var(--color-danger, #c4282c) 12%, transparent);
    color: var(--color-fg);
    font-size: var(--text-sm);
    line-height: var(--leading-normal);
  }

  .link-btn {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-accent);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }

  .copied-tag {
    margin-left: 6px;
    color: var(--color-fg-subtle);
    font-size: var(--text-xs);
  }

  .empty-state {
    flex: 1 1 auto;
    display: grid;
    place-items: center;
    align-content: center;
    gap: var(--space-10);
    min-height: 150px;
    padding: var(--space-24) var(--space-16);
    text-align: center;
    color: var(--color-fg-muted);
  }

  .desc {
    max-width: 200px;
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
    letter-spacing: -0.1px;
    line-height: var(--leading-normal);
  }

  .spin {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid var(--color-border-strong);
    border-top-color: var(--color-accent);
    animation: spin 900ms linear infinite;
  }

  /* Positioned anchor for the floating FindBar overlay (ADR-0058 D1). FindBar
     is a sibling of the surface so its counter mutations stay out of the
     surface MutationObserver. Occupies the same box the surface used to. */
  .preview-surface-wrap {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .preview-surface {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    /* ADR-0046 D6 amend ⑬.1 — Preview content must be drag-selectable; override
     * the right-panel chrome's `user-select: none` (RightPanel) so highlight works. */
    user-select: text;
    -webkit-user-select: text;
    --code-viewer-font-size: calc(10.5px * var(--preview-content-scale, 1));
    --code-viewer-line-height: 1.6;
    --code-viewer-gutter-width: 28px;
  }

  .text-preview {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    margin: 0;
    padding: var(--space-12);
    color: var(--color-fg);
    background: var(--color-surface);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .text-preview.rendered {
    font-family: inherit;
    white-space: normal;
  }

  .text-preview.rendered :global(h1),
  .text-preview.rendered :global(h2),
  .text-preview.rendered :global(h3) {
    margin-top: 0;
  }

  .multi-summary {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-12);
    padding: var(--space-12);
    overflow: hidden;
    background: var(--color-surface);
    color: var(--color-fg);
  }

  .summary-strip {
    flex: 0 0 auto;
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    overflow: hidden;
    background: var(--color-surface-2);
  }

  .summary-cell {
    min-width: 0;
    display: grid;
    gap: 1px;
    padding: var(--space-8) var(--space-10);
    border-left: 1px solid var(--color-border);
  }

  .summary-cell:first-child {
    border-left: 0;
  }

  .summary-value {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
    line-height: var(--leading-tight);
  }

  .summary-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg-subtle);
    font-size: var(--text-sm);
    line-height: var(--leading-normal);
  }

  .summary-section {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .summary-section-head {
    flex: 0 0 auto;
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-8);
    padding: var(--space-8) var(--space-10);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-2);
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }

  .summary-section-head span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .summary-list {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    background: var(--color-surface);
  }

  .summary-row {
    min-width: 0;
    display: grid;
    grid-template-columns: 34px minmax(0, 1fr) max-content;
    align-items: center;
    gap: var(--space-8);
    min-height: 34px;
    padding: var(--space-6) var(--space-10);
    border-bottom: 1px solid var(--color-border);
  }

  .summary-kind {
    width: 28px;
    max-width: 28px;
    justify-self: start;
    padding: 1px 4px;
    border-radius: var(--radius-sm);
    background: var(--color-fg-muted);
    color: var(--color-bg);
    font-family: var(--font-mono);
    font-size: 8px;
    line-height: var(--leading-normal);
    text-align: center;
    text-transform: uppercase;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .summary-kind.is-folder {
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }

  .summary-row-text {
    min-width: 0;
    display: grid;
    gap: 1px;
  }

  .summary-row-name,
  .summary-row-path,
  .summary-row-size,
  .summary-more {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .summary-row-name {
    color: var(--color-fg);
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }

  .summary-row-path,
  .summary-row-size {
    color: var(--color-fg-subtle);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }

  .summary-row-size {
    justify-self: end;
  }

  .summary-more {
    padding: var(--space-8) var(--space-10);
    color: var(--color-fg-subtle);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }

  .preview-content-menu {
    position: fixed;
    z-index: var(--z-context-menu);
    min-width: 132px;
    padding: 4px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
    box-shadow: 0 10px 24px rgba(0, 0, 0, 0.18);
  }

  .preview-content-menu button {
    width: 100%;
    height: 26px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-fg);
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }

  .preview-content-menu button:hover:not(:disabled) {
    background: var(--color-glass-1);
  }

  .preview-content-menu button:disabled {
    color: var(--color-fg-subtle);
    cursor: not-allowed;
  }

  .preview-max-backdrop {
    position: fixed;
    top: calc(var(--layout-titlebar-h) + var(--layout-toolbar-h));
    right: 0;
    bottom: 0;
    left: 0;
    z-index: var(--z-modal);
    display: flex;
    align-items: stretch;
    justify-content: stretch;
    background: transparent;
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
  }

  .preview-max-card {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    margin: var(--space-12);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: var(--radius-md);
    background: var(--color-surface);
    color: var(--color-fg);
    box-shadow: 0 20px 48px rgba(0, 0, 0, 0.22), 0 0 0 1px var(--color-border);
  }

  .preview-max-header {
    position: relative;
    z-index: 2;
    flex: 0 0 36px;
    min-width: 0;
    height: 36px;
    display: flex;
    align-items: center;
    gap: var(--space-8);
    padding: 0 6px 0 var(--space-12);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-2);
    user-select: none;
  }

  .preview-max-title-group {
    min-width: 0;
    display: grid;
    gap: 1px;
    flex: 1 1 auto;
  }

  .preview-max-title,
  .preview-max-meta {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .preview-max-title {
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
  }

  .preview-max-meta {
    color: var(--color-fg-subtle);
    font-family: var(--font-mono);
    font-size: 9px;
  }

  .preview-max-actions {
    display: flex;
    align-items: center;
    gap: 1px;
    flex: 0 0 auto;
  }

  .preview-max-body {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--color-bg);
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
