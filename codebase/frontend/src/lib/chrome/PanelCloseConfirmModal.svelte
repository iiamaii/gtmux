<script lang="ts">
  /**
   * PanelCloseConfirmModal — Panel close 의 3-option confirm (G25 amend).
   *
   * 정본:
   * - ADR-0021 D9.3 (Panel/Terminal close 분리)
   * - ADR-0010 G25 amend (multi-session 의 confirm 정책)
   * - plan-0007 §14.7 FE-7 (Panel header V2)
   *
   * 3 옵션:
   *   - [Cancel]            — modal 닫음
   *   - [Panel only]        — Layout 에서 item 제거. Terminal 은 pool 유지.
   *                           (다른 session 의 mirror panel 영향 X)
   *   - [Panel + Terminal]  — Item 제거 + Terminal SIGTERM.
   *                           Mirror 가 있으면 다른 session 의 panel 들이 dangling.
   *
   * `Settings.behavior.auto_kill_terminal_on_panel_close = true` (P1+) 면 본
   * modal 생략 + [Panel + Terminal] 즉시 실행 (G25 amend). 부모에서 분기.
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';

  interface Props {
    open: boolean;
    /** 표시용 panel 라벨 (single = label/short-id, batch = "N items"). */
    panelLabel: string;
    /** Batch item count — 1 = single, >1 = batch (title 분기). */
    count?: number;
    /** 이 panel 들의 terminal UUID 가 다른 session 에서 reference 되는 수 합. */
    attachCount: number;
    /** Mirror 인 attached session 이름 list union (현 session 제외). */
    otherSessions: string[];
    onCancel: () => void;
    onPanelOnly: () => void;
    onPanelAndTerminal: () => void;
  }

  const {
    open,
    panelLabel,
    count = 1,
    attachCount,
    otherSessions,
    onCancel,
    onPanelOnly,
    onPanelAndTerminal,
  }: Props = $props();

  const isBatch = $derived(count > 1);
  const modalTitle = $derived(
    isBatch ? `Remove ${panelLabel} from canvas?` : `Close panel ‘${panelLabel}’?`,
  );

  let mirrorHint = $derived(otherSessions.length > 0);
  // 다른 session 의 mirror 가 있으면 Panel+Terminal 차단 — kill 시 그 session
  // 들의 panel 도 dangling. UI 차원 가드 (BE attach_index 의 진실 기준).
  let killBlocked = $derived(mirrorHint);
</script>

<Modal
  {open}
  onclose={onCancel}
  title={modalTitle}
  dismissOnBackdrop={false}
>
  {#snippet body()}
    <p class="modal-lead lead">
      {isBatch
        ? 'Selection includes terminal(s). Choose what happens to them:'
        : 'Choose what happens to the underlying terminal:'}
    </p>

    <div class="choice-list">
      <div class="choice">
        <div class="choice-title">{isBatch ? 'Panels only' : 'Panel only'}</div>
        <div class="choice-desc">
          {isBatch
            ? 'Remove items from this canvas. Terminals stay running in the server pool — re-attach later from the Terminals list.'
            : 'Remove panel from this canvas. The terminal stays running in the server pool — re-attach later from the Terminals list.'}
        </div>
      </div>
      <div class="choice danger" class:disabled={killBlocked}>
        <div class="choice-title">{isBatch ? 'Panels + Terminals' : 'Panel + Terminal'}</div>
        <div class="choice-desc">
          {isBatch ? 'Remove items and stop the terminals (SIGTERM).' : 'Remove panel and stop the terminal (SIGTERM).'}
          {#if killBlocked}
            <strong class="warn">
              ⚠ Disabled — this terminal is also attached to
              {otherSessions.length} other session{otherSessions.length === 1 ? '' : 's'}
              ({otherSessions.join(', ')}). Killing it here would leave those
              panels dangling. Remove the panel from each of those sessions
              first, or use “Panel only” to keep the terminal running.
            </strong>
          {:else if attachCount > 1}
            ⚠ This terminal is referenced by {attachCount} layout items —
            others may go dangling.
          {/if}
        </div>
      </div>
    </div>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={onCancel}>Cancel</Button>
    <Button variant="secondary" onclick={onPanelOnly}>{isBatch ? 'Panels only' : 'Panel only'}</Button>
    <Button
      variant="danger"
      onclick={onPanelAndTerminal}
      disabled={killBlocked}
      title={killBlocked
        ? `Disabled — terminal is mirrored in: ${otherSessions.join(', ')}`
        : undefined}
    >
      {isBatch ? 'Panels + Terminals' : 'Panel + Terminal'}
    </Button>
  {/snippet}
</Modal>

<style>
  .lead {
    margin: 0 0 var(--space-12);
  }

  .choice-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-8);
  }

  .choice {
    padding: var(--space-10) var(--space-12);
    background: var(--color-surface-2);
    border-left: 3px solid var(--color-border-strong);
    border-radius: var(--radius-md);
  }

  .choice.danger {
    border-left-color: var(--color-danger);
  }

  .choice.disabled {
    opacity: 0.7;
  }

  .choice-title {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    color: var(--color-fg);
    margin-bottom: 2px;
  }

  .choice-desc {
    font-size: var(--text-base);
    color: var(--color-fg-muted);
    line-height: var(--leading-normal);
  }

  .warn {
    display: block;
    margin-top: 4px;
    color: var(--color-danger);
    font-weight: var(--weight-medium);
  }
</style>
