<script lang="ts">
  /**
   * ChangeWebViewModal — set / change a Web View node's address (ADR-0059 D3).
   *
   * Two commit paths (user-confirmed design):
   *   1. URL text input — normalizeWebViewInput applied on commit (D8 scheme
   *      auto-fix), the normalized value shown before it is written.
   *   2. Workspace file pick — the integrated FileExplorer (via filePicker),
   *      restricted to renderable types (html / md / images). The picked file
   *      is converted to a clean workspace-relative path.
   *
   * Commit → applyMutation (undoable, history-included — D8). FE preflight
   * validation surfaces the same rejections the BE would (WebViewUrlInvalid /
   * WebViewOwnOrigin) as an inline error before the round-trip; anything that
   * still slips through is surfaced by applyMutation's failure toast.
   */

  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { changeWebViewDialog } from '$lib/stores/changeWebViewDialog.svelte';
  import { filePicker } from '$lib/stores/filePicker.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import CanvasGlyph from '$lib/canvas/CanvasGlyph.svelte';
  import { classifyWebViewSource, normalizeWebViewInput } from '$lib/canvas/webViewSource';
  import { workspaceRelativePath } from '$lib/files/workspaceAssets';
  import { commitNewItem, createWebViewItem } from '$lib/canvas/itemFactory';
  import { toolStore } from '$lib/stores/toolStore.svelte';
  import type { CanvasItem, WebViewItem } from '$lib/types/canvas';

  /** Renderable local types the picker offers (ADR-0059 D2 matrix). */
  const WEB_VIEW_PICK_EXTENSIONS = [
    '.html', '.htm', '.md', '.markdown',
    '.png', '.jpg', '.jpeg', '.gif', '.webp', '.svg', '.bmp', '.ico', '.avif',
  ];

  let inputValue = $state('');
  let inlineError = $state<string | null>(null);
  let committing = $state(false);
  let lastSyncedId = $state<string | null>(null);

  const open = $derived(changeWebViewDialog.open);
  const itemId = $derived(changeWebViewDialog.itemId);
  const createAt = $derived(changeWebViewDialog.createAt);
  const isCreate = $derived(createAt !== null);
  const item = $derived.by((): WebViewItem | null => {
    if (itemId === null) return null;
    const it = sessionStore.items.get(itemId);
    return it !== undefined && it.type === 'web_view' ? it : null;
  });
  const modalOpen = $derived(open && (isCreate || item !== null));

  // Seed the input each time the dialog opens (keyed on the target so a
  // re-render doesn't clobber the user's typing). Create mode seeds empty.
  $effect(() => {
    const key = isCreate ? 'create' : itemId;
    if (modalOpen && key !== null && key !== lastSyncedId) {
      inputValue = isCreate ? '' : (item?.url ?? '');
      inlineError = null;
      lastSyncedId = key;
    }
    if (!modalOpen) lastSyncedId = null;
  });

  const normalizedPreview = $derived.by((): string => {
    const n = normalizeWebViewInput(inputValue);
    return n;
  });
  const showNormalizedHint = $derived(
    normalizedPreview.length > 0 && normalizedPreview !== inputValue.trim(),
  );

  function close(): void {
    if (committing) return;
    // Cancelling a create-mode open reverts the one-shot web_view tool.
    if (isCreate) toolStore.consume();
    changeWebViewDialog.close();
  }

  async function commitUrl(nextUrl: string): Promise<void> {
    if (committing) return;
    if (sessionStore.active === null) {
      close();
      return;
    }
    // Create mode (ADR-0059 D3) — spawn a new node with the chosen url. The BE
    // rejects an empty url, so the address is collected here first.
    if (isCreate && createAt !== null) {
      committing = true;
      try {
        const item0 = { ...createWebViewItem(createAt), url: nextUrl };
        const created = await commitNewItem(item0);
        if (created === null) {
          inlineError = 'The server rejected that address.';
          return;
        }
        toolStore.consume();
        changeWebViewDialog.close();
      } finally {
        committing = false;
      }
      return;
    }
    const targetId = itemId;
    if (targetId === null) {
      close();
      return;
    }
    committing = true;
    try {
      const result = await sessionStore.applyMutation(
        (cur) => ({
          ...cur,
          items: cur.items.map((it: CanvasItem) =>
            it.id === targetId && it.type === 'web_view'
              ? ({ ...it, url: nextUrl } as WebViewItem)
              : it,
          ),
        }),
        {
          abortMessage: 'Address change aborted — session reconnect failed.',
          failMessage: 'The server rejected that address.',
        },
      );
      if (!result.ok) {
        // BE 400 (WebViewUrlInvalid / WebViewOwnOrigin) — the failure toast
        // carries the specific code; surface a concise inline hint too.
        inlineError = 'The server rejected that address.';
        return;
      }
      changeWebViewDialog.close();
    } finally {
      committing = false;
    }
  }

  function commitTypedUrl(): void {
    const normalized = normalizeWebViewInput(inputValue);
    if (normalized.length === 0) {
      inlineError = 'Enter an address.';
      return;
    }
    const source = classifyWebViewSource(normalized);
    if (source.kind === 'invalid') {
      inlineError = 'Use an http(s):// URL or a workspace-relative file path.';
      return;
    }
    if (source.kind === 'remote' && isOwnOrigin(normalized)) {
      inlineError = "That is this app's own address.";
      return;
    }
    inlineError = null;
    void commitUrl(normalized);
  }

  function isOwnOrigin(url: string): boolean {
    if (typeof window === 'undefined') return false;
    try {
      return new URL(url).origin === window.location.origin;
    } catch {
      return false;
    }
  }

  function onPickWorkspaceFile(): void {
    const workspaceRoot = sessionStore.effectiveWorkspaceRoot;
    if (workspaceRoot.length === 0) {
      toastStore.show({ message: 'Workspace root is not available yet.', tone: 'error' });
      return;
    }
    filePicker.openFor(workspaceRoot, (absolutePath) => {
      const rel = workspaceRelativePath(workspaceRoot, absolutePath);
      if (rel === null) {
        inlineError = 'The file must be inside the active project workspace.';
        return;
      }
      inputValue = rel;
      inlineError = null;
      void commitUrl(rel);
    }, {
      accept: { extensions: WEB_VIEW_PICK_EXTENSIONS, description: 'web view files' },
      rootKind: 'workspace',
      rootPath: workspaceRoot,
    });
  }

  function onInputKeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitTypedUrl();
    }
  }
</script>

<Modal
  open={modalOpen}
  onclose={close}
  title={isCreate ? 'New web view' : 'Set address'}
  dismissOnBackdrop={!committing}
  dismissOnEsc={!committing}
>
  {#snippet body()}
    <div class="modal-stack">
      <p class="modal-copy">
        Link an <strong>http(s)://</strong> web address or a workspace file
        (HTML, Markdown, or an image) to render it live in this view.
      </p>
      <div class="url-row">
        <Input
          bind:value={inputValue}
          label="Address"
          placeholder="example.com  ·  notes/readme.md"
          autofocus={true}
          error={inlineError}
          onkeydown={onInputKeydown}
        />
      </div>
      {#if showNormalizedHint}
        <p class="normalized-hint">
          Will be saved as <span class="mono">{normalizedPreview}</span>
        </p>
      {/if}
      <div class="pick-row">
        <button type="button" class="pick-file" onclick={onPickWorkspaceFile} disabled={committing}>
          <CanvasGlyph name="folder" size={13} />
          <span>Pick a workspace file…</span>
        </button>
      </div>
    </div>
  {/snippet}

  {#snippet footer()}
    <Button variant="ghost" onclick={close} disabled={committing}>Cancel</Button>
    <Button variant="primary" onclick={commitTypedUrl} disabled={committing}>Set address</Button>
  {/snippet}
</Modal>

<style>
  .url-row :global(.input-control) {
    font-family: var(--font-mono);
    font-size: var(--text-base);
  }

  .normalized-hint {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
  }

  .normalized-hint .mono {
    font-family: var(--font-mono);
    color: var(--color-fg);
  }

  .pick-row {
    display: flex;
  }

  .pick-file {
    display: inline-flex;
    align-items: center;
    gap: var(--space-6);
    padding: var(--space-6) var(--space-10);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
    color: var(--color-fg);
    font-size: var(--text-base);
    cursor: pointer;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .pick-file:hover:not(:disabled) {
    background: var(--color-glass-1);
  }

  .pick-file:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
