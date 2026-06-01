<script lang="ts">
  /**
   * NewSessionModal — project-first session creation.
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
  import FileExplorer from './FileExplorer.svelte';
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
  let workspaceRoot = $state('');
  let explorerOpen = $state(false);
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
    !submitting && name.length > 0 && workspaceRoot.trim().length > 0 && validationError === null,
  );

  // Modal close 시 form reset
  $effect(() => {
    if (!open) {
      name = '';
      workspaceRoot = '';
      explorerOpen = false;
      errorMessage = null;
      submitting = false;
    }
  });

  function suggestNameFromWorkspace(path: string): string {
    const raw = path.split('/').filter(Boolean).pop() ?? 'session';
    const base = raw
      .replace(/[^A-Za-z0-9_-]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 64) || 'session';
    if (!existingNames.includes(base)) return base;
    for (let i = 2; i < 1000; i += 1) {
      const suffix = `-${i}`;
      const candidate = `${base.slice(0, 64 - suffix.length)}${suffix}`;
      if (!existingNames.includes(candidate)) return candidate;
    }
    return base;
  }

  function onWorkspacePicked(path: string): void {
    workspaceRoot = path;
    explorerOpen = false;
    if (name.trim().length === 0) {
      name = suggestNameFromWorkspace(path);
    }
  }

  async function submit(): Promise<void> {
    if (!canSubmit) return;
    submitting = true;
    errorMessage = null;
    try {
      const res = await createSession({ name, workspace_root: workspaceRoot.trim() });
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
      Pick a project directory and a short session name.
    </p>
    <Input
      bind:value={name}
      label="Session name"
      placeholder="project-session"
      autofocus
      error={validationError ?? errorMessage}
      {onkeydown}
    />
    <div class="workspace-field">
      <Input
        bind:value={workspaceRoot}
        label="Workspace root"
        placeholder="/path/to/project"
        error={workspaceRoot.length === 0 ? null : null}
        {onkeydown}
      />
      <Button onclick={() => (explorerOpen = true)} disabled={submitting}>Browse</Button>
    </div>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={onClose} disabled={submitting}>Cancel</Button>
    <Button variant="primary" onclick={submit} disabled={!canSubmit}>
      {submitting ? 'Creating…' : 'Create'}
    </Button>
  {/snippet}
</Modal>

<FileExplorer
  open={explorerOpen}
  mode="dir"
  title="Choose workspace root"
  initialDir={workspaceRoot}
  onCancel={() => (explorerOpen = false)}
  onPick={onWorkspacePicked}
/>

<style>
  .lead {
    margin: 0 0 var(--space-12);
    font-size: var(--text-md);
    color: var(--color-fg-muted);
    line-height: var(--leading-normal);
  }

  .workspace-field {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--space-8);
    align-items: end;
    margin-top: var(--space-12);
  }
</style>
