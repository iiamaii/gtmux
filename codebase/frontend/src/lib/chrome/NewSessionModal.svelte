<script lang="ts">
  /**
   * NewSessionModal — 이름 입력 + Create.
   *
   * 정본:
   * - ADR-0019 D8 (session name validation `^[A-Za-z0-9_-]{1,64}$`)
   * - plan-0007 §14 FE-NEW-1 + §14.20 (inline edit / Esc routing)
   *
   * 흐름:
   * 1. 사용자 이름 입력 → real-time regex validate + duplicate check (FE-side
   *    의 기존 list 와 비교, BE 가 추가로 409 반환).
   * 2. [Create] 클릭 → `POST /api/sessions` → 성공 시 `onCreated(session)`.
   * 3. BE 409 (duplicate) / 기타 에러 → 본 modal 안 inline error.
   *
   * 부모 책임:
   * - `existingNames` 전달 (FE-side duplicate pre-check)
   * - `onCreated(session)` 안에서 attach 흐름 호출 (AttachConfirmModal 진입)
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import { createSession, SESSION_NAME_REGEX } from '$lib/http/sessions';
  import type { SessionInfo } from '$lib/types/sessions';

  interface Props {
    open: boolean;
    /** 현재 알려진 session 이름들 — duplicate pre-check 용. */
    existingNames?: readonly string[];
    onClose: () => void;
    onCreated: (session: SessionInfo) => void;
  }

  const { open, existingNames = [], onClose, onCreated }: Props = $props();

  let name = $state('');
  let submitting = $state(false);
  let errorMessage = $state<string | null>(null);

  /** 실시간 validation — 빈 / pattern / duplicate. */
  let validationError = $derived.by((): string | null => {
    if (name.length === 0) return null; // 빈 = error 표시 안 함 (placeholder 단계)
    if (!SESSION_NAME_REGEX.test(name)) {
      return 'Use A–Z, 0–9, _, - (1–64 chars).';
    }
    if (existingNames.includes(name)) {
      return `"${name}" already exists.`;
    }
    return null;
  });

  let canSubmit = $derived(
    !submitting && name.length > 0 && validationError === null,
  );

  // Modal close 시 form reset
  $effect(() => {
    if (!open) {
      name = '';
      errorMessage = null;
      submitting = false;
    }
  });

  async function submit(): Promise<void> {
    if (!canSubmit) return;
    submitting = true;
    errorMessage = null;
    try {
      const res = await createSession({ name });
      onCreated(res.session);
    } catch (err) {
      errorMessage = err instanceof Error ? err.message : String(err);
    } finally {
      submitting = false;
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && canSubmit) {
      e.preventDefault();
      void submit();
    }
  }
</script>

<Modal
  {open}
  onclose={onClose}
  title="New session"
  dismissOnBackdrop={false}
>
  {#snippet body()}
    <p class="lead">
      Pick a short name. Letters, digits, <code>_</code>, <code>-</code> only.
    </p>
    <Input
      bind:value={name}
      label="Session name"
      placeholder="my-workspace"
      autofocus
      error={validationError ?? errorMessage}
      {onkeydown}
    />
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={onClose} disabled={submitting}>Cancel</Button>
    <Button variant="primary" onclick={submit} disabled={!canSubmit}>
      {submitting ? 'Creating…' : 'Create'}
    </Button>
  {/snippet}
</Modal>

<style>
  .lead {
    margin: 0 0 var(--space-12);
    font-size: var(--text-md);
    color: var(--color-fg-muted);
    line-height: var(--leading-normal);
  }

  .lead code {
    font-family: var(--font-mono);
    font-size: var(--text-base);
    padding: 1px 5px;
    border-radius: 3px;
    background: var(--color-surface-2);
    color: var(--color-fg);
  }
</style>
