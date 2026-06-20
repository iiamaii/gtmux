<script lang="ts">
  /**
   * FilePreviewView — read-only preview for the selected Workspace file.
   */

  import { filePreviewStore } from '$lib/stores/filePreview.svelte';
  import { chromeStore } from '$lib/stores/chrome.svelte';
  import { fsFileUrl } from '$lib/http/fs';
  import { UnauthorizedError } from '$lib/http/sessions';
  import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { escRouter } from '$lib/common/escRouter.svelte';
  import CodeViewer from '$lib/canvas/CodeViewer.svelte';
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
  let previewMaximized = $state(false);
  let contentMenu = $state<PreviewContentMenu | null>(null);
  let contentMenuEl: HTMLDivElement | undefined = $state();
  // ADR-0046 D6 amend ⑩ — ref to the currently-mounted preview surface so the
  // Cmd/Ctrl+C shortcut can scope the selection to it (same root the right-click
  // menu uses). The previewSurface snippet renders in two places (inline +
  // maximize body) but only one is shown at a time, so a single ref is correct.
  let previewSurfaceEl: HTMLDivElement | undefined = $state();

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

  $effect(() => {
    const current = selection;
    if (isMultiSelection) {
      closeContentMenu();
      loadedPath = null;
      textContent = null;
      errorMessage = null;
      loading = false;
      return;
    }
    if (current === null) {
      closeContentMenu();
      loadedPath = null;
      textContent = null;
      errorMessage = null;
      loading = false;
      previewMaximized = false;
      return;
    }
    if (current.path === loadedPath) return;
    loadedPath = current.path;
    void loadPreview(current.path);
  });

  $effect(() => {
    if (!previewMaximized) return;
    return escRouter.register({
      priority: 2,
      handler: () => {
        previewMaximized = false;
        return true;
      },
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
      if (!res.ok) throw new Error(`GET /api/fs/file returned ${res.status}`);
      const nextText = await res.text();
      if (loadedPath !== path) return;
      textContent = nextText;
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

  // ADR-0046 D6 amend ⑩ — Cmd/Ctrl+C in the active Preview = Copy path (with
  // selection location), mirroring the right-click "Copy path". Handled in
  // CAPTURE phase so it runs before the global bubble keydown handlers
  // (shortcutRegistry / canvas) and before the native copy event, guaranteeing
  // the path-copy wins when the Preview is the active surface with a selected
  // file. The gate returns false everywhere else, so normal Cmd/Ctrl+C and the
  // right-click menu's Copy (raw text) / Copy path are unaffected.
  function onWindowKeydownCapture(e: KeyboardEvent): void {
    if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey &&
      !e.altKey &&
      (e.key === 'c' || e.key === 'C')
    ) {
      if (!canKeyboardCopyPath()) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      void copyPathViaShortcut();
    }
  }

  /** Gate for the amend ⑩ keyboard copy-path: Preview active + single file +
   *  focus not in an editable field / xterm (so normal field copy still works). */
  function canKeyboardCopyPath(): boolean {
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
</script>

<svelte:window
  onpointerdowncapture={onWindowPointerDown}
  onkeydowncapture={onWindowKeydownCapture}
  onkeydown={onWindowKeydown}
  onresize={closeContentMenu}
  onblur={closeContentMenu}
/>

{#snippet previewSurface(current: FilePreviewSelection)}
  <div
    bind:this={previewSurfaceEl}
    class="preview-surface"
    role="region"
    aria-label="Preview content"
    style:--preview-content-scale={componentSettings.previewScale}
    oncontextmenu={(e: MouseEvent) => openContentMenu(e, current)}
    onscroll={closeContentMenu}
  >
    {#if loading}
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
            {#if previewMaximized}
              <svg width="13" height="13" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
                <rect x="2" y="3.6" width="6.5" height="6.4" rx="0.5"/>
                <path d="M4 3.6V2.4h6.5v6.4H9"/>
              </svg>
            {:else}
              <svg width="13" height="13" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
                <rect x="2.5" y="2.5" width="7" height="7" rx="0.5"/>
              </svg>
            {/if}
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
          {#if previewUrl.length > 0}
            <a class="icon-btn" href={previewUrl} download title="Download" aria-label="Download">
              <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
                <path d="M8 3v8M5 8l3 3 3-3M3 13h10"/>
              </svg>
            </a>
          {:else}
            <button type="button" class="icon-btn" disabled title="Download unavailable" aria-label="Download">
              <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
                <path d="M8 3v8M5 8l3 3 3-3M3 13h10"/>
              </svg>
            </button>
          {/if}
          <button type="button" class="icon-btn" title="Copy path" aria-label="Copy path" onclick={() => void copyPath()}>
            <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <rect x="5" y="5" width="8" height="9" rx="1.2"/>
              <path d="M3 11V3a1 1 0 0 1 1-1h6"/>
            </svg>
          </button>
          <button
            type="button"
            class="icon-btn"
            class:is-active={previewMaximized}
            title={previewMaximized ? 'Restore' : 'Maximize'}
            aria-label={previewMaximized ? 'Restore' : 'Maximize'}
            onclick={previewMaximized ? closePreviewMaximize : openPreviewMaximize}
          >
            {#if previewMaximized}
              <svg width="13" height="13" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
                <rect x="2" y="3.6" width="6.5" height="6.4" rx="0.5"/>
                <path d="M4 3.6V2.4h6.5v6.4H9"/>
              </svg>
            {:else}
              <svg width="13" height="13" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
                <rect x="2.5" y="2.5" width="7" height="7" rx="0.5"/>
              </svg>
            {/if}
          </button>
        </span>
      </div>
      <div class="file-meta" title={selection.path}>
        {[fmtSize(selection.entry.size_bytes), compactPath(selection.path)].filter(Boolean).join(' · ')}
      </div>
    </header>

    {@render previewSurface(selection)}
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
            {#if previewUrl.length > 0}
              <a class="icon-btn" href={previewUrl} download title="Download" aria-label="Download">
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
                  <path d="M8 3v8M5 8l3 3 3-3M3 13h10"/>
                </svg>
              </a>
            {/if}
            <button type="button" class="icon-btn" title="Copy path" aria-label="Copy path" onclick={() => void copyPath()}>
              <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <rect x="5" y="5" width="8" height="9" rx="1.2"/>
                <path d="M3 11V3a1 1 0 0 1 1-1h6"/>
              </svg>
            </button>
          {/if}
          <button
            type="button"
            class="icon-btn"
            title="Restore (Esc)"
            aria-label="Restore"
            onclick={closePreviewMaximize}
          >
            <svg width="13" height="13" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
              <rect x="2" y="3.6" width="6.5" height="6.4" rx="0.5"/>
              <path d="M4 3.6V2.4h6.5v6.4H9"/>
            </svg>
          </button>
        </div>
      </header>
      <div class="preview-max-body">
        {#if isMultiSelection}
          {@render multiSelectionSurface(multiSummary)}
        {:else if selection !== null}
          {@render previewSurface(selection)}
        {/if}
      </div>
    </div>
  </div>
{/if}

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

  .file-name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg);
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
    letter-spacing: -0.1px;
  }

  .file-meta {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg-subtle);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
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

  .icon-btn.is-active {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .icon-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
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

  .preview-surface {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
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
