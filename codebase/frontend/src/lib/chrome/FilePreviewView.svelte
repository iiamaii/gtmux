<script lang="ts">
  /**
   * FilePreviewView — read-only preview for the selected Workspace file.
   */

  import { filePreviewStore } from '$lib/stores/filePreview.svelte';
  import { chromeStore } from '$lib/stores/chrome.svelte';
  import { uploadAssetFromPath, AssetUploadUnavailableError, type UploadedAsset } from '$lib/http/assets';
  import { UnauthorizedError } from '$lib/http/sessions';
  import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import {
    buildRenderedHtmlSrcdoc,
    renderHtml,
    renderMarkdown,
    RENDERED_HTML_IFRAME_SANDBOX,
  } from '$lib/canvas/documentRender';

  type PreviewKind = 'empty' | 'image' | 'pdf' | 'markdown' | 'html' | 'text' | 'unsupported';

  let loading = $state(false);
  let errorMessage = $state<string | null>(null);
  let asset = $state<UploadedAsset | null>(null);
  let textContent = $state<string | null>(null);
  let loadedPath = $state<string | null>(null);

  const selection = $derived(filePreviewStore.selection);
  const kind = $derived(classify(selection?.path ?? '', asset?.mime ?? ''));
  const assetUrl = $derived(asset === null ? '' : `/api/assets/${asset.asset_id}`);
  const renderedMarkdown = $derived(renderMarkdown(textContent ?? ''));
  const renderedHtml = $derived(buildRenderedHtmlSrcdoc(renderHtml(textContent ?? '')));
  const textLines = $derived((textContent ?? '').split('\n'));

  function basename(path: string): string {
    return path.split('/').filter(Boolean).pop() ?? path;
  }

  function extension(path: string): string {
    const name = basename(path).toLowerCase();
    const dot = name.lastIndexOf('.');
    return dot < 0 ? '' : name.slice(dot + 1);
  }

  function classify(path: string, mime: string): PreviewKind {
    if (path.length === 0) return 'empty';
    const ext = extension(path);
    if (mime.startsWith('image/')) return 'image';
    if (mime === 'application/pdf') return 'pdf';
    if (mime.startsWith('text/html')) return 'html';
    if (mime.startsWith('text/markdown')) return 'markdown';
    if (mime.startsWith('text/') || mime === 'application/json') return ext === 'md' ? 'markdown' : 'text';
    if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg'].includes(ext)) return 'image';
    if (ext === 'pdf') return 'pdf';
    if (['md', 'markdown'].includes(ext)) return 'markdown';
    if (['html', 'htm'].includes(ext)) return 'html';
    if (['txt', 'log', 'json', 'ts', 'tsx', 'js', 'jsx', 'css', 'rs', 'svelte', 'toml', 'yaml', 'yml'].includes(ext)) {
      return 'text';
    }
    return 'unsupported';
  }

  function assetKindFor(path: string): 'image' | 'document' | null {
    const predicted = classify(path, '');
    if (predicted === 'image') return 'image';
    if (predicted === 'pdf' || predicted === 'markdown' || predicted === 'html' || predicted === 'text') {
      return 'document';
    }
    return null;
  }

  $effect(() => {
    const current = selection;
    if (current === null) {
      loadedPath = null;
      asset = null;
      textContent = null;
      errorMessage = null;
      loading = false;
      return;
    }
    if (current.path === loadedPath) return;
    loadedPath = current.path;
    void loadPreview(current.path);
  });

  async function loadPreview(path: string): Promise<void> {
    const materializeKind = assetKindFor(path);
    asset = null;
    textContent = null;
    errorMessage = null;
    if (materializeKind === null) {
      loading = false;
      errorMessage = 'Preview is not available for this file type.';
      return;
    }
    loading = true;
    try {
      const nextAsset = await uploadAssetFromPath(path, materializeKind);
      asset = nextAsset;
      const nextKind = classify(path, nextAsset.mime);
      if (nextKind === 'markdown' || nextKind === 'html' || nextKind === 'text') {
        const res = await fetch(`/api/assets/${nextAsset.asset_id}`, {
          method: 'GET',
          credentials: 'include',
          headers: { Accept: nextAsset.mime || 'text/plain' },
        });
        if (res.status === 401) throw new UnauthorizedError();
        if (!res.ok) throw new Error(`GET /api/assets/${nextAsset.asset_id} returned ${res.status}`);
        textContent = await res.text();
      }
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      errorMessage = err instanceof AssetUploadUnavailableError
        ? 'File preview is not available on this server.'
        : err instanceof Error
          ? err.message
          : String(err);
    } finally {
      loading = false;
    }
  }

  function fmtSize(bytes: number | null | undefined): string {
    if (bytes === null || bytes === undefined) return '';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function extLabel(path: string): string {
    const ext = extension(path);
    if (ext.length === 0) return 'file';
    return ext.slice(0, 4);
  }

  function extClass(path: string): string {
    const ext = extension(path);
    if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'avif'].includes(ext)) return 'img';
    if (ext === 'pdf') return 'pdf';
    if (['md', 'markdown'].includes(ext)) return 'md';
    if (['ts', 'tsx', 'js', 'jsx', 'svelte', 'rs', 'css', 'html', 'json', 'toml', 'yaml', 'yml'].includes(ext)) return 'code';
    return 'file';
  }

  function compactPath(path: string): string {
    const parts = path.split('/').filter(Boolean);
    if (parts.length <= 2) return path;
    return `.../${parts.slice(-2).join('/')}`;
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

  function revealInFiles(): void {
    chromeStore.setLeftPanelTab('files');
  }
</script>

<div class="preview">
  {#if selection === null}
    <div class="empty">
      <p>No selection</p>
      <p class="hint">Click a file in Files to preview.</p>
    </div>
  {:else}
    <header class="preview-head">
      <div class="title-row">
        <span class="ext-chip {extClass(selection.path)}">{extLabel(selection.path)}</span>
        <span class="file-name" title={selection.path}>{basename(selection.path)}</span>
        <span class="actions">
          {#if assetUrl.length > 0}
            <a class="icon-btn" href={assetUrl} download title="Download" aria-label="Download">
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
          <button type="button" class="icon-btn" title="Reveal in Files" aria-label="Reveal in Files" onclick={revealInFiles}>
            <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.2 1.5h5.2A1.5 1.5 0 0 1 14 6v5.5A1.5 1.5 0 0 1 12.5 13h-9A1.5 1.5 0 0 1 2 11.5z"/>
            </svg>
          </button>
        </span>
      </div>
      <div class="file-meta" title={selection.path}>
        {[fmtSize(selection.entry.size_bytes), compactPath(selection.path)].filter(Boolean).join(' · ')}
      </div>
    </header>

    {#if loading}
      <div class="empty-state">
        <span class="spin" aria-hidden="true"></span>
        <span class="desc">Loading preview...</span>
      </div>
    {:else if errorMessage !== null}
      <div class="empty-state danger" role="alert">
        <span class="disc" aria-hidden="true">!</span>
        <span class="lead">Can't preview this type</span>
        <span class="desc">{errorMessage}</span>
      </div>
    {:else if kind === 'image' && assetUrl.length > 0}
      <div class="image-wrap">
        <img src={assetUrl} alt={basename(selection.path)} />
      </div>
    {:else if kind === 'pdf' && assetUrl.length > 0}
      <iframe class="pdf-frame" src={assetUrl} title={basename(selection.path)}></iframe>
    {:else if kind === 'markdown'}
      <article class="text-preview rendered">{@html renderedMarkdown}</article>
    {:else if kind === 'html'}
      <iframe
        class="html-frame"
        title={basename(selection.path)}
        sandbox={RENDERED_HTML_IFRAME_SANDBOX}
        srcdoc={renderedHtml}
      ></iframe>
    {:else if kind === 'text'}
      <div class="code-preview">
        {#each textLines as line, index (index)}
          <div class="code-line">
            <span class="gutter">{index + 1}</span>
            <code>{line}</code>
          </div>
        {/each}
      </div>
    {:else}
      <div class="empty-state">
        <span class="disc" aria-hidden="true">?</span>
        <span class="lead">Can't preview this type</span>
        <span class="desc">Download or open it from the project workspace.</span>
      </div>
    {/if}
  {/if}
</div>

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

  .empty {
    padding: var(--space-12);
    color: var(--color-fg-muted);
  }

  .empty p {
    margin: 0 0 var(--space-4);
    font-size: var(--text-md);
  }

  .empty .hint {
    font-size: var(--text-base);
    color: var(--color-fg-subtle);
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

  .empty-state.danger .disc {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 14%, transparent);
  }

  .disc {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--color-glass-1);
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
  }

  .lead {
    color: var(--color-fg);
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
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

  .image-wrap {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    place-items: center;
    padding: var(--space-12);
    overflow: auto;
    background:
      linear-gradient(45deg, var(--color-glass-1) 25%, transparent 25%, transparent 75%, var(--color-glass-1) 75%) 0 0/16px 16px,
      linear-gradient(45deg, var(--color-glass-1) 25%, transparent 25%, transparent 75%, var(--color-glass-1) 75%) 8px 8px/16px 16px,
      var(--color-surface);
  }

  .image-wrap img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }

  .pdf-frame,
  .html-frame {
    flex: 1 1 auto;
    min-height: 0;
    width: 100%;
    border: 0;
    background: white;
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

  .code-preview {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: var(--space-8) 0;
    color: var(--color-fg);
    background: var(--color-surface);
    font-family: var(--font-mono);
    font-size: 10.5px;
    line-height: 1.6;
  }

  .code-line {
    display: grid;
    grid-template-columns: 28px max-content;
    gap: var(--space-8);
    min-width: max-content;
    padding-right: var(--space-12);
  }

  .gutter {
    color: var(--color-fg-subtle);
    text-align: right;
    user-select: none;
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

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
