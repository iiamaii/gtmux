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
  let nameSuggested = $state(false);

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
  let disabledReason = $derived.by((): string | null => {
    if (submitting) return 'Creating session.';
    if (workspaceRoot.trim().length === 0) return 'Pick a workspace directory.';
    if (name.trim().length === 0) return 'Name the session.';
    if (validationError !== null) return validationError;
    return null;
  });

  // Modal close 시 form reset
  $effect(() => {
    if (!open) {
      name = '';
      workspaceRoot = '';
      explorerOpen = false;
      errorMessage = null;
      submitting = false;
      nameSuggested = false;
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
      nameSuggested = true;
    }
  }

  function onNameInput(): void {
    nameSuggested = false;
    errorMessage = null;
  }

  function onWorkspaceInput(): void {
    nameSuggested = false;
    errorMessage = null;
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
    <div class="form-stack">
      <label class="field">
        <span class="field-label">Workspace root <span class="required">*</span></span>
        <span class="path-row">
          <input
            class="text-input mono"
            bind:value={workspaceRoot}
            placeholder="Pick a project directory..."
            disabled={submitting}
            autocomplete="off"
            oninput={onWorkspaceInput}
            {onkeydown}
          />
          <Button onclick={() => (explorerOpen = true)} disabled={submitting}>
            <span class="btn-icon" aria-hidden="true">
              <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.2 1.5h5.2A1.5 1.5 0 0 1 14 6v5.5A1.5 1.5 0 0 1 12.5 13h-9A1.5 1.5 0 0 1 2 11.5z"/>
              </svg>
            </span>
            Browse
          </Button>
        </span>
        <span class="field-hint">
          Files, Preview, upload and download are scoped to this directory.
        </span>
      </label>

      <label class="field" class:has-error={validationError !== null}>
        <span class="field-label">Session name <span class="required">*</span></span>
        <input
          class="text-input"
          bind:value={name}
          placeholder="project-session"
          disabled={submitting}
          autocomplete="off"
          oninput={onNameInput}
          {onkeydown}
        />
        {#if nameSuggested}
          <span class="suggested">
            <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
              <path d="M8 2v12M2 8h12"/>
            </svg>
            suggested from folder - edit freely
          </span>
        {:else if validationError !== null}
          <span class="field-error" role="alert">
            <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
              <circle cx="8" cy="8" r="6"/>
              <path d="M8 5v3.5M8 11h.01"/>
            </svg>
            {validationError}
          </span>
        {/if}
      </label>

      {#if errorMessage !== null}
        <p class="field-error form-error" role="alert">
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
            <circle cx="8" cy="8" r="6"/>
            <path d="M8 5v3.5M8 11h.01"/>
          </svg>
          {errorMessage}
        </p>
      {/if}
    </div>
  {/snippet}
  {#snippet footer()}
    <span class="footer-reason" class:hidden={disabledReason === null}>
      <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
        <circle cx="8" cy="8" r="6"/>
        <path d="M8 5v3.5M8 11h.01"/>
      </svg>
      {disabledReason ?? 'Ready'}
    </span>
    <Button variant="ghost" onclick={onClose} disabled={submitting}>Cancel</Button>
    <Button variant="primary" onclick={submit} disabled={!canSubmit}>
      {submitting ? 'Creating...' : 'Create session'}
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
  .form-stack {
    display: grid;
    gap: var(--space-12);
  }

  .field {
    display: grid;
    gap: var(--space-6);
    min-width: 0;
  }

  .field-label {
    display: inline-flex;
    align-items: center;
    gap: var(--space-4);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
  }

  .required {
    color: var(--color-accent);
  }

  .path-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--space-8);
    align-items: center;
    min-width: 0;
  }

  .text-input {
    box-sizing: border-box;
    width: 100%;
    min-width: 0;
    height: 32px;
    padding: 0 var(--space-12);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    color: var(--color-fg);
    font-family: inherit;
    font-size: var(--text-base);
    line-height: var(--leading-normal);
    letter-spacing: 0;
    transition: border-color var(--motion-fast) var(--motion-easing);
  }

  .text-input:hover:not(:disabled) {
    border-color: var(--color-fg-subtle);
  }

  .text-input:focus-visible {
    outline: 2px solid var(--color-info);
    outline-offset: 1px;
    border-color: var(--color-info);
    background: var(--color-surface);
  }

  .field.has-error .text-input {
    border-color: var(--color-danger);
  }

  .mono {
    font-family: var(--font-mono);
    font-size: var(--text-base);
  }

  .field-hint,
  .suggested,
  .field-error,
  .footer-reason {
    font-size: var(--text-sm);
    letter-spacing: 0;
  }

  .field-hint {
    color: var(--color-fg-muted);
  }

  .suggested,
  .field-error,
  .footer-reason {
    display: inline-flex;
    align-items: center;
    gap: var(--space-4);
  }

  .suggested {
    color: var(--color-accent);
    font-family: var(--font-mono);
  }

  .field-error {
    margin: 0;
    color: var(--color-danger);
  }

  .form-error {
    padding: var(--space-8) var(--space-10);
    border: 1px solid color-mix(in srgb, var(--color-danger) 34%, transparent);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
  }

  .footer-reason {
    min-width: 0;
    margin-right: auto;
    color: var(--color-fg-muted);
  }

  .footer-reason.hidden {
    visibility: hidden;
  }

  .btn-icon {
    display: inline-flex;
    align-items: center;
  }
</style>
