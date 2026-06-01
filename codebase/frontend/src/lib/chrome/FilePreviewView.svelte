<script lang="ts">
  /**
   * FilePreviewView — read-only preview for the selected Workspace file.
   */

  import { filePreviewStore } from '$lib/stores/filePreview.svelte';
  import { uploadAssetFromPath, AssetUploadUnavailableError, type UploadedAsset } from '$lib/http/assets';
  import { UnauthorizedError } from '$lib/http/sessions';
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
</script>

<div class="preview">
  {#if selection === null}
    <p class="state">Select a file in Files to preview it.</p>
  {:else}
    <header class="preview-head">
      <span class="file-name" title={selection.path}>{basename(selection.path)}</span>
      <span class="file-meta">{fmtSize(selection.entry.size_bytes)}</span>
    </header>

    {#if loading}
      <p class="state">Loading preview…</p>
    {:else if errorMessage !== null}
      <p class="state error">{errorMessage}</p>
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
      <pre class="text-preview"><code>{textContent}</code></pre>
    {:else}
      <p class="state">Preview is not available for this file type.</p>
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
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--space-8);
    padding: var(--space-10) var(--space-12);
    border-bottom: 1px solid var(--color-border);
    flex: 0 0 auto;
  }

  .file-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-fg);
    font-weight: var(--weight-medium);
  }

  .file-meta {
    color: var(--color-fg-subtle);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }

  .state {
    margin: 0;
    padding: var(--space-16) var(--space-12);
    text-align: center;
    color: var(--color-fg-muted);
  }

  .state.error {
    color: var(--color-danger);
  }

  .image-wrap {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    place-items: center;
    padding: var(--space-12);
    overflow: auto;
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

  .text-preview.rendered {
    font-family: inherit;
    white-space: normal;
  }

  .text-preview.rendered :global(h1),
  .text-preview.rendered :global(h2),
  .text-preview.rendered :global(h3) {
    margin-top: 0;
  }
</style>
