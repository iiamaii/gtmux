<script lang="ts">
  /**
   * SessionDeleteConfirmModal — Session 삭제 confirm.
   *
   * 정본:
   * - ADR-0019 D10 (Session delete cascade — terminal 은 server-pool 에 남음)
   * - ADR-0019 D10.1 (G51 amend — 2 FE entry points: SessionListModal hover-kebab
   *   + SessionMenu "Delete current session…"). 두 entry 모두 본 modal 재사용.
   *
   * Copy = D10 정합 "Delete session '<name>'? (Terminal 들은 server-pool 에 남음)".
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';

  interface Props {
    open: boolean;
    sessionName: string;
    onCancel: () => void;
    onConfirm: () => void;
  }

  const { open, sessionName, onCancel, onConfirm }: Props = $props();
</script>

<Modal
  {open}
  onclose={onCancel}
  title="Delete session ‘{sessionName}’?"
  dismissOnBackdrop={false}
>
  {#snippet body()}
    <p class="modal-lead">
      The session file will be removed.
    </p>
    <p class="modal-note">
      Terminals stay running in the server pool — re-attach them from the
      Terminals list, or stop them explicitly.
    </p>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={onCancel}>Cancel</Button>
    <Button variant="danger" onclick={onConfirm}>Delete session</Button>
  {/snippet}
</Modal>

<style>
  .modal-lead {
    margin-bottom: var(--space-8);
    color: var(--color-fg);
  }
</style>
