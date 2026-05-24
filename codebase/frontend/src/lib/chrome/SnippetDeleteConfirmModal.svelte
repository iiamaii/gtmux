<script lang="ts">
  /**
   * SnippetDeleteConfirmModal — confirms a single Snippet entry deletion.
   *
   * 정본: ADR-0038 (2026-05-24 amend) — delete-mode 의 pill click 시 노출.
   * 패턴은 SessionDeleteConfirmModal 과 동일 (Modal + Button + lead/note).
   * Cmd+Z 로 되돌릴 수 있다는 정보를 note 로 명시.
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { snippetDeleteDialog } from '$lib/stores/snippetDeleteDialog.svelte';

  const open = $derived(snippetDeleteDialog.open);
  const entryKey = $derived(snippetDeleteDialog.entryKey);
  const displayKey = $derived(
    entryKey.length > 48 ? `${entryKey.slice(0, 48)}…` : entryKey,
  );
</script>

<Modal
  {open}
  onclose={() => snippetDeleteDialog.cancel()}
  title="Delete snippet ‘{displayKey}’?"
  dismissOnBackdrop={false}
>
  {#snippet body()}
    <p class="lead">
      The snippet entry will be removed from this node.
    </p>
    <p class="note">
      You can undo with <kbd>⌘</kbd><kbd>Z</kbd> immediately after.
    </p>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => snippetDeleteDialog.cancel()}>Cancel</Button>
    <Button variant="danger" onclick={() => snippetDeleteDialog.confirm()}>Delete snippet</Button>
  {/snippet}
</Modal>

<style>
  .lead {
    margin: 0 0 var(--space-8);
    font-size: var(--text-md);
    color: var(--color-fg);
    line-height: var(--leading-normal);
  }
  .note {
    margin: 0;
    padding: var(--space-10) var(--space-12);
    background: var(--color-surface-2);
    border-left: 3px solid var(--color-warning);
    border-radius: var(--radius-sm);
    font-size: var(--text-base);
    color: var(--color-fg-muted);
    line-height: var(--leading-normal);
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .note kbd {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    padding: 1px 5px;
    border: 1px solid var(--color-border);
    border-radius: 3px;
    background: var(--color-surface);
    color: var(--color-fg);
    letter-spacing: 0;
  }
</style>
