<script lang="ts">
  /**
   * ReauthModal — step-up re-authentication gate (ADR-0020 D16).
   *
   * Sensitive Settings actions (server shutdown, cookie/token rotation) require
   * the user to re-prove presence by re-entering their current credential right
   * before the action runs. The backend re-verifies that credential *inline*
   * (in the action endpoint body), so this modal's only job is to collect it and
   * hand it to the caller's action.
   *
   * Credential field is decided by `credentialKind` (default `'auto'`):
   *   - `'auto'` (D16.2): mode-aware from `settingsStore.auth.password_set`
   *       (from `GET /api/settings`, D11.2):
   *         - `password_set === true`  → prompt for the account **password**.
   *         - `password_set === false` → prompt for the **token** (token mode).
   *   - `'either'` (D19.2 union step-up): a single **"Token or password"** field
   *       accepting EITHER credential — used by "Delete password", where the
   *       token recovers a lost password and a remembered password also works.
   * All three submit a single `credential` string. The field is always masked
   * (`type="password"`) to avoid shoulder-surfing.
   *
   * Flow / ownership:
   *   - Parent owns `open` + `onCancel` (Esc / backdrop / Cancel / success-close).
   *   - Parent passes `onSubmit(credential)` — the actual action call
   *     (`shutdownServer` / `rotateToken`). It runs while the modal shows a busy
   *     state.
   *   - On a step-up failure (wrong credential / rate limit / empty), the modal
   *     stays open and shows the error inline so the user can retry.
   *   - On success it calls `onCancel()` to dismiss (parent flips `open=false`).
   *   - Any other (unexpected) error is re-thrown for the parent's catch (e.g.
   *     UnauthorizedError → redirect to /auth) after closing the modal.
   *
   * Security: user input is bound via Svelte text binding only — never
   * `{@html}`. The credential is sent over the action fetch and not persisted.
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { settingsStore } from '$lib/stores/settings.svelte';
  import {
    InvalidCredentialError,
    CredentialRequiredError,
    RateLimitedError,
  } from '$lib/http/stepup';

  interface Props {
    /** Controlled visibility. Parent owns the boolean. */
    open: boolean;
    /** Heading, e.g. "Confirm shutdown". */
    title: string;
    /** Short context line under the title (what the user is authorising). */
    description?: string;
    /** Label on the confirm button, e.g. "Shutdown" / "Rotate". */
    confirmLabel?: string;
    /** Confirm button emphasis. Default 'primary'; use 'danger' for shutdown. */
    confirmVariant?: 'primary' | 'danger';
    /**
     * Which credential the single field collects (ADR-0020 D16.2 / D19.2):
     *   - `'auto'` (default) — mode-aware password|token from `password_set`.
     *   - `'either'` — union "Token or password" field (Remove-password reset).
     */
    credentialKind?: 'auto' | 'either';
    /**
     * Perform the gated action with the entered credential. Resolve → success
     * (modal closes). Reject with a step-up error (InvalidCredentialError /
     * RateLimitedError / CredentialRequiredError) → modal stays open + inline
     * error. Reject with anything else → modal closes + error re-thrown to the
     * parent's catch.
     */
    onSubmit: (credential: string) => Promise<void>;
    /** Close request — Esc / backdrop / Cancel / after success. */
    onCancel: () => void;
  }

  const {
    open,
    title,
    description,
    confirmLabel = 'Confirm',
    confirmVariant = 'primary',
    credentialKind = 'auto',
    onSubmit,
    onCancel,
  }: Props = $props();

  /** Union mode accepts either credential (D19.2). */
  const isEither = $derived(credentialKind === 'either');
  /**
   * In `'auto'` the field follows the settings snapshot (D16.2); in `'either'`
   * the union field is conceptually password-ish but accepts a token too, so it
   * is never treated as the strict password branch.
   */
  const usePassword = $derived(
    !isEither && settingsStore.auth?.password_set === true,
  );
  const fieldLabel = $derived(
    isEither ? 'Token or password' : usePassword ? 'Password' : 'Token',
  );
  const fieldPlaceholder = $derived(
    isEither
      ? 'Your token or password'
      : usePassword
        ? 'Your password'
        : 'Your token',
  );

  let credential = $state('');
  let inFlight = $state(false);
  let errorMessage = $state<string | null>(null);

  // Reset transient state whenever the modal is (re)opened or closed so a fresh
  // open never shows a stale credential or error.
  $effect(() => {
    if (!open) {
      credential = '';
      errorMessage = null;
      inFlight = false;
    }
  });

  const canSubmit = $derived(!inFlight && credential.length > 0);

  async function submit(): Promise<void> {
    if (!canSubmit) return;
    inFlight = true;
    errorMessage = null;
    try {
      await onSubmit(credential);
      // Success — parent closes via onCancel (flips `open`), which the reset
      // effect then clears.
      onCancel();
    } catch (err) {
      if (err instanceof InvalidCredentialError) {
        errorMessage = isEither
          ? 'Incorrect token or password.'
          : usePassword
            ? 'Incorrect password.'
            : 'Incorrect token.';
        credential = '';
        return;
      }
      if (err instanceof CredentialRequiredError) {
        errorMessage = `Enter your ${fieldLabel.toLowerCase()}.`;
        return;
      }
      if (err instanceof RateLimitedError) {
        const wait =
          err.retryAfterSecs !== null && err.retryAfterSecs > 0
            ? ` Try again in ${err.retryAfterSecs}s.`
            : ' Try again later.';
        errorMessage = `Too many attempts.${wait}`;
        return;
      }
      // Unexpected (e.g. UnauthorizedError, network) — close and let the parent
      // handle it (redirect / toast).
      onCancel();
      throw err;
    } finally {
      inFlight = false;
    }
  }

  /**
   * Native `<form>` submit (Enter or the footer button). `preventDefault` stops
   * the default GET navigation; the real action runs via `submit()`. Wrapping
   * the masked input in a `<form>` (vs. a bare input + keydown handler) silences
   * Chrome's "[DOM] Password field is not contained in a form" warning and lets
   * password managers offer save/fill. The footer Confirm button is
   * `type="submit"` and lives inside this same form (Modal renders the footer
   * snippet as a sibling, so the form spans body + footer via an id-less native
   * submit is not possible — instead the button calls `submit()` directly while
   * the form's onsubmit covers Enter). Focus-trap/Esc are unaffected: Modal's
   * trap queries focusables inside the dialog regardless of the nested form, and
   * Esc is handled at window level.
   */
  function onFormSubmit(e: SubmitEvent): void {
    e.preventDefault();
    void submit();
  }
</script>

<Modal
  {open}
  onclose={onCancel}
  {title}
  size="sm"
  dismissOnBackdrop={!inFlight}
  dismissOnEsc={!inFlight}
>
  {#snippet body()}
    <form class="reauth-stack" onsubmit={onFormSubmit}>
      <!--
        Hidden username field so Chrome associates the masked credential with an
        account (Chrome [DOM] "Password forms should have (optionally hidden)
        username fields", https://goo.gl/9p2vKq). The field is always masked
        (type="password") regardless of credentialKind, so this is present for
        every variant. gtmux is single-user → a constant gives a stable entry.
        display:none + tabindex=-1 + aria-hidden keep it out of layout and the
        Modal focus trap / a11y tree.
      -->
      <input
        type="text"
        name="username"
        autocomplete="username"
        value="gtmux"
        readonly
        tabindex="-1"
        aria-hidden="true"
        style="display:none"
      />
      {#if description}
        <p class="reauth-lead">{description}</p>
      {/if}
      <label class="field" class:has-error={errorMessage !== null}>
        <span class="field-label">{fieldLabel}</span>
        <input
          class="text-input"
          type="password"
          bind:value={credential}
          placeholder={fieldPlaceholder}
          disabled={inFlight}
          autocomplete={usePassword || isEither ? 'current-password' : 'off'}
          autocapitalize="off"
          autocorrect="off"
          spellcheck="false"
        />
        {#if errorMessage !== null}
          <span class="field-error" role="alert">
            <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
              <circle cx="8" cy="8" r="6" />
              <path d="M8 5v3.5M8 11h.01" />
            </svg>
            {errorMessage}
          </span>
        {/if}
      </label>
      <!--
        Real submit button so Enter submits the form natively and password
        managers recognise a complete form. Visually hidden — the visible
        confirm action is the footer button (rendered by Modal as a sibling
        outside this form), which calls submit() directly.
      -->
      <button type="submit" class="reauth-submit-proxy" tabindex="-1" aria-hidden="true" disabled={!canSubmit}></button>
    </form>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={onCancel} disabled={inFlight}>Cancel</Button>
    <Button variant={confirmVariant} onclick={submit} disabled={!canSubmit}>
      {inFlight ? 'Verifying…' : confirmLabel}
    </Button>
  {/snippet}
</Modal>

<style>
  .reauth-stack {
    display: grid;
    gap: var(--space-12);
    margin: 0;
  }

  /* Native submit target for Enter + password-manager form recognition.
     Visually hidden but still in the form (not display:none, which would drop
     it from submit handling in some engines). */
  .reauth-submit-proxy {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    border: 0;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  .reauth-lead {
    margin: 0;
    color: var(--color-fg-muted);
    font-size: var(--text-md);
    line-height: var(--leading-normal);
  }

  .field {
    display: grid;
    gap: var(--space-6);
    min-width: 0;
  }

  .field-label {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
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
    transition: border-color var(--motion-fast) var(--motion-easing);
  }

  .text-input:hover:not(:disabled) {
    border-color: var(--color-fg-subtle);
  }

  .text-input:focus-visible {
    outline: 2px solid var(--color-info);
    outline-offset: 1px;
    border-color: var(--color-info);
  }

  .text-input:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .field.has-error .text-input {
    border-color: var(--color-danger);
  }

  .field-error {
    display: inline-flex;
    align-items: center;
    gap: var(--space-4);
    margin: 0;
    color: var(--color-danger);
    font-size: var(--text-sm);
  }
</style>
