<script lang="ts">
  /**
   * AttachConfirmModal (FE-NEW-5).
   *
   * 정본:
   * - ADR-0018 D6 (match-or-spawn — confirm_required 분기)
   * - plan-0007 §14 FE-NEW-5
   * - frontend-handover §6 Stage 3
   *
   * 흐름:
   * 1. 사용자가 SessionListModal 에서 session 선택 → 부모가 `attachSession()`
   *    호출 → 응답 `kind:"confirm_required"` 시 본 modal 진입.
   * 2. summary 표시:
   *    - spawn_count: 새로 spawn 될 terminal 수
   *    - unmatched_item_ids: layout 의 terminal 중 pool 에 없는 것 (= fresh spawn)
   *    - unmatched_terminal_ids: pool 의 terminal 중 layout 에 없는 것 (= 다른
   *      session 의 alive terminal — 그대로 두고 본 session 은 touch 안 함)
   * 3. [Confirm] → 부모가 `attachConfirm()` 호출 후 layout fetch.
   *    [Cancel] → 부모가 tentative attach 를 정리하고 SessionListModal 로 회귀.
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import type { AttachConfirmSummary } from '$lib/types/sessions';

  interface Props {
    open: boolean;
    sessionName: string;
    summary: AttachConfirmSummary | null;
    onCancel: () => void;
    onConfirm: () => void;
  }

  const { open, sessionName, summary, onCancel, onConfirm }: Props = $props();

  /** 사용자에게 spawn 영향이 있는가 — 0 이면 confirm 무용이지만 BE 가 보낼 수도 있음. */
  let hasSpawn = $derived((summary?.spawn_count ?? 0) > 0);
  let matchedCount = $derived(summary?.matched_item_ids.length ?? 0);
</script>

<Modal
  {open}
  onclose={onCancel}
  title="Attach session ‘{sessionName}’?"
  dismissOnBackdrop={false}
>
  {#snippet body()}
    {#if summary === null}
      <p class="modal-state">Loading attach summary…</p>
    {:else}
      <p class="modal-lead lead">
        This session's layout doesn't fully match the server's live terminals.
        Continuing will adjust the canvas:
      </p>

      <ul class="changes">
        {#if hasSpawn}
          <li>
            <span class="badge spawn">spawn</span>
            <span class="text">
              <strong>{summary.spawn_count}</strong>
              new terminal{summary.spawn_count === 1 ? '' : 's'} will be
              started for missing panels.
            </span>
          </li>
          <li class="note-row">
            <span class="badge note">note</span>
            <span class="text">
              New terminals start fresh — previous output cannot be restored.
            </span>
          </li>
        {/if}

        {#if matchedCount > 0}
          <li>
            <span class="badge keep">keep</span>
            <span class="text">
              <strong>{matchedCount}</strong>
              panel{matchedCount === 1 ? '' : 's'} already match
              live terminal{matchedCount === 1 ? '' : 's'} — reconnected
              without change.
            </span>
          </li>
        {/if}

        {#if !hasSpawn && matchedCount === 0}
          <li>
            <span class="badge keep">keep</span>
            <span class="text">Everything matches. Attach without changes.</span>
          </li>
        {/if}
      </ul>
    {/if}
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={onCancel}>Cancel</Button>
    <Button variant="primary" onclick={onConfirm} disabled={summary === null}>
      Confirm attach
    </Button>
  {/snippet}
</Modal>

<style>
  .lead {
    margin: 0 0 var(--space-12);
  }

  .changes {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-8);
  }

  .changes li {
    display: grid;
    grid-template-columns: 64px 1fr;
    gap: var(--space-12);
    align-items: start;
    padding: var(--space-10) var(--space-12);
    background: var(--color-surface-2);
    border-radius: var(--radius-md);
  }

  .badge {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding: 2px 8px;
    border-radius: var(--radius-pill);
    text-align: center;
    align-self: center;
  }

  .badge.spawn {
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    color: var(--color-accent);
  }

  .badge.keep {
    background: var(--color-surface);
    color: var(--color-fg-muted);
    border: 1px solid var(--color-border);
  }

  .badge.note {
    background: transparent;
    color: var(--color-fg-muted);
    border: 1px dashed var(--color-border);
  }

  .note-row .text {
    color: var(--color-fg-muted);
    font-style: italic;
  }

  .text {
    font-size: var(--text-md);
    color: var(--color-fg);
    line-height: var(--leading-normal);
  }

  .text strong {
    font-weight: var(--weight-semibold);
  }
</style>
