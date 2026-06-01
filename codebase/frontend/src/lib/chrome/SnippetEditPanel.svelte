<script lang="ts">
  // SnippetEditPanel — ADR-0038 (2026-05-24 amend). Floating popover form for
  // creating or editing a single SnippetEntry. Anchored to a viewport rect
  // provided by the caller (canvas pill, inspector add, layer context menu).
  //
  // Invariants:
  //   - ADR-0028: save/delete go through `sessionStore.applyMutation` (single
  //     entry, priorSnapshot rollback, 1 Cmd+Z step).
  //   - escRouter convention: Esc dismisses (silent discard); click-outside
  //     also dismisses.
  //   - Mount once globally (in +page.svelte). Singleton store gates visibility.

  import { onMount, untrack } from 'svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { snippetEditPanel } from '$lib/stores/snippetEditPanel.svelte';
  import { generateUuidV4 } from '$lib/uuid';
  import type { CanvasItem, SnippetEntry, SnippetsItem } from '$lib/types/canvas';

  let panelEl = $state<HTMLDivElement | null>(null);
  let keyInputEl = $state<HTMLInputElement | null>(null);

  // Draft form state — local to the panel. Initialized from store on open.
  let draftKey = $state('');
  let draftBody = $state('');
  let keyError = $state(false);
  let saving = $state(false);

  // Position (viewport pixel coords). Default off-screen until anchor resolves.
  let pos = $state({ x: -9999, y: -9999 });
  const PANEL_WIDTH = 320;
  const PANEL_GAP = 6;
  const PANEL_MARGIN = 8; // viewport edge breathing room

  const isOpen = $derived(snippetEditPanel.open);
  const nodeId = $derived(snippetEditPanel.nodeId);
  const entryId = $derived(snippetEditPanel.entryId);
  const isNew = $derived(entryId === null);

  // Re-initialize draft + measure position whenever the panel opens. untrack
  // the store reads inside the side-effect body so we only reseed on open
  // transitions, not on every store mutation.
  $effect(() => {
    if (!isOpen) return;
    untrack(() => {
      draftKey = snippetEditPanel.prefillKey;
      draftBody = snippetEditPanel.prefillBody;
      keyError = false;
      saving = false;
    });
    // Defer focus + position to next tick — element must be mounted first.
    queueMicrotask(() => {
      if (keyInputEl !== null) {
        keyInputEl.focus();
        keyInputEl.select();
      }
      computePosition();
    });
  });

  function computePosition(): void {
    if (panelEl === null) return;
    const anchor = snippetEditPanel.anchor;
    if (anchor === null) return;
    const panelRect = panelEl.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const source = snippetEditPanel.source;

    let x: number;
    let y: number;

    if (source === 'inspector') {
      // Inspector trigger sits in the right panel — placing below or aligned
      // to anchor would overlap the inspector chrome. Compute the actual
      // inspector left edge at runtime so the panel never crosses into it
      // (the anchor button's own coords aren't enough — anchor sits *inside*
      // the inspector, so anchor.x - PANEL_WIDTH may still overlap).
      const inspectorEl = document.querySelector('.right-panel');
      const inspectorLeft =
        inspectorEl !== null ? inspectorEl.getBoundingClientRect().left : vw;
      x = anchor.x - PANEL_WIDTH - PANEL_GAP;
      // Ensure the panel's right edge clears the inspector left edge by GAP.
      const maxRight = inspectorLeft - PANEL_GAP;
      if (x + PANEL_WIDTH > maxRight) {
        x = maxRight - PANEL_WIDTH;
      }
      // Vertical: align panel top to trigger top, clamped.
      y = anchor.y;
      // Fallback when there is no room on the left (very narrow viewport):
      // fall through to default below/above placement.
      if (x < PANEL_MARGIN) {
        x = anchor.x; // restart default placement
      } else {
        // Inspector-specific path resolved — clamp y only.
        if (y + panelRect.height + PANEL_MARGIN > vh) y = vh - panelRect.height - PANEL_MARGIN;
        if (y < PANEL_MARGIN) y = PANEL_MARGIN;
        pos = { x, y };
        return;
      }
    } else {
      x = anchor.x;
    }

    // Default placement: prefer below the anchor, above as fallback.
    const belowY = anchor.y + anchor.height + PANEL_GAP;
    const aboveY = anchor.y - panelRect.height - PANEL_GAP;
    if (belowY + panelRect.height + PANEL_MARGIN <= vh) {
      y = belowY;
    } else if (aboveY >= PANEL_MARGIN) {
      y = aboveY;
    } else {
      y = Math.max(PANEL_MARGIN, vh - panelRect.height - PANEL_MARGIN);
    }
    if (x + PANEL_WIDTH + PANEL_MARGIN > vw) x = vw - PANEL_WIDTH - PANEL_MARGIN;
    if (x < PANEL_MARGIN) x = PANEL_MARGIN;
    pos = { x, y };
  }

  function cancel(): void {
    snippetEditPanel.close();
  }

  async function save(): Promise<void> {
    if (saving) return;
    const trimmedKey = draftKey.trim();
    if (trimmedKey.length === 0) {
      keyError = true;
      keyInputEl?.focus();
      return;
    }
    const targetNodeId = nodeId;
    const targetEntryId = entryId;
    if (targetNodeId === null) return;
    const bodyValue = draftBody;
    saving = true;
    const result = await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) => {
          if (it.id !== targetNodeId || it.type !== 'snippets') return it;
          const sit = it as SnippetsItem;
          if (targetEntryId === null) {
            const newEntry: SnippetEntry = {
              id: generateUuidV4(),
              key: trimmedKey,
              body: bodyValue,
            };
            return { ...sit, entries: [...sit.entries, newEntry] };
          }
          return {
            ...sit,
            entries: sit.entries.map((e) =>
              e.id === targetEntryId
                ? { ...e, key: trimmedKey, body: bodyValue }
                : e,
            ),
          };
        }),
      }),
      {
        abortMessage: 'Snippet save aborted — session reconnect failed.',
        failMessage: 'Snippet save failed',
      },
    );
    saving = false;
    if (result.ok) snippetEditPanel.close();
  }

  async function remove(): Promise<void> {
    if (saving || isNew) return;
    const targetNodeId = nodeId;
    const targetEntryId = entryId;
    if (targetNodeId === null || targetEntryId === null) return;
    saving = true;
    const result = await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) => {
          if (it.id !== targetNodeId || it.type !== 'snippets') return it;
          const sit = it as SnippetsItem;
          return { ...sit, entries: sit.entries.filter((e) => e.id !== targetEntryId) };
        }),
      }),
      {
        abortMessage: 'Snippet delete aborted — session reconnect failed.',
        failMessage: 'Snippet delete failed',
      },
    );
    saving = false;
    if (result.ok) snippetEditPanel.close();
  }

  function onKeydown(e: KeyboardEvent): void {
    if (!isOpen) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      cancel();
      return;
    }
    // Cmd/Ctrl + Enter saves from anywhere inside the form.
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      e.stopPropagation();
      void save();
    }
  }

  function onWindowPointerDown(e: PointerEvent): void {
    if (!isOpen) return;
    if (panelEl === null) return;
    const target = e.target as Node | null;
    if (target !== null && panelEl.contains(target)) return;
    cancel();
  }

  onMount(() => {
    window.addEventListener('pointerdown', onWindowPointerDown, true);
    return () => {
      window.removeEventListener('pointerdown', onWindowPointerDown, true);
    };
  });
</script>

<svelte:window onkeydown={onKeydown} onresize={computePosition} />

{#if isOpen}
  <div
    bind:this={panelEl}
    class="snippet-edit-panel"
    role="dialog"
    aria-label={isNew ? 'Add snippet' : 'Edit snippet'}
    style="left: {pos.x}px; top: {pos.y}px; width: {PANEL_WIDTH}px;"
  >
    <div class="sep-head">
      <span class="sep-title">{isNew ? 'New snippet' : 'Edit snippet'}</span>
      <button
        type="button"
        class="sep-close"
        aria-label="Cancel"
        title="Cancel"
        onclick={cancel}
      >
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
          <path d="M3 3l6 6M9 3l-6 6"/>
        </svg>
      </button>
    </div>
    <form class="sep-body" onsubmit={(e) => { e.preventDefault(); void save(); }}>
      <label class="sep-row">
        <span class="sep-label">Key</span>
        <input
          class="sep-input"
          class:error={keyError}
          type="text"
          bind:value={draftKey}
          bind:this={keyInputEl}
          placeholder="e.g. deploy"
          maxlength={256}
          disabled={saving}
        />
      </label>
      <label class="sep-row sep-row-grow">
        <span class="sep-label">Body</span>
        <textarea
          class="sep-textarea"
          bind:value={draftBody}
          placeholder="Body to copy on click"
          rows={5}
          disabled={saving}
        ></textarea>
      </label>
      <div class="sep-actions">
        {#if !isNew}
          <button
            type="button"
            class="sep-btn delete"
            onclick={() => void remove()}
            disabled={saving}
          >Delete</button>
        {/if}
        <span class="sep-spacer"></span>
        <button
          type="button"
          class="sep-btn cancel"
          onclick={cancel}
          disabled={saving}
        >Cancel</button>
        <button
          type="submit"
          class="sep-btn save"
          disabled={saving}
        >Save</button>
      </div>
    </form>
  </div>
{/if}

<style>
  /* ColorPicker-style chassis: single surface bg (no surface-2 strip), head
     and body share the same surface so the panel reads as a *non-movable*
     popover rather than a chrome-strip window. */
  .snippet-edit-panel {
    position: fixed;
    z-index: var(--z-popover, 100);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md, 0 12px 40px rgba(0, 0, 0, 0.18));
    display: grid;
    grid-template-rows: 32px 1fr;
    overflow: hidden;
    font-family: var(--font-sans);
    color: var(--color-fg);
    user-select: none;
  }
  .sep-head {
    display: flex;
    align-items: center;
    height: 32px;
    padding: 0 4px 0 12px;
    border-bottom: 1px solid var(--color-border);
  }
  .sep-title {
    font-size: 12px;
    font-weight: var(--weight-medium);
    letter-spacing: 0;
  }
  .sep-close {
    margin-left: auto;
    width: 24px;
    height: 24px;
    display: grid;
    place-items: center;
    border: 0;
    background: transparent;
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    transition: background 0.12s, color 0.12s;
  }
  .sep-close:hover {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }
  .sep-body {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
  }
  .sep-row {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .sep-row-grow {
    flex: 1;
    min-height: 0;
  }
  .sep-label {
    font-family: var(--font-mono);
    font-size: 9.5px;
    color: var(--color-fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.6px;
  }
  .sep-input,
  .sep-textarea {
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: var(--color-fg);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 5px 8px;
    outline: none;
    width: 100%;
    box-sizing: border-box;
    letter-spacing: 0;
    line-height: 1.4;
    transition: border-color 0.12s;
  }
  .sep-input:focus,
  .sep-textarea:focus {
    border-color: var(--color-accent);
  }
  .sep-input.error {
    border-color: var(--color-danger);
  }
  .sep-textarea {
    resize: none;
    min-height: 90px;
    line-height: 1.5;
  }
  .sep-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .sep-spacer {
    flex: 1;
  }
  .sep-btn {
    font-family: var(--font-mono);
    font-size: 10.5px;
    padding: 4px 10px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    color: var(--color-fg);
    cursor: pointer;
    letter-spacing: 0.3px;
    line-height: 1.3;
    transition: background 0.12s, color 0.12s, border-color 0.12s;
  }
  .sep-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .sep-btn:hover:not(:disabled) {
    background: var(--color-glass-1);
  }
  .sep-btn.delete {
    background: transparent;
    color: var(--color-danger);
    border-color: color-mix(in srgb, var(--color-danger) 30%, transparent);
  }
  .sep-btn.delete:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
  }
  .sep-btn.save {
    background: var(--color-accent);
    color: var(--color-accent-fg);
    border-color: var(--color-accent);
    font-weight: 540;
  }
  .sep-btn.save:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-accent) 85%, black);
  }
</style>
