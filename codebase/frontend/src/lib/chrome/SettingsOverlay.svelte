<script lang="ts">
  /**
   * SettingsOverlay — full-screen settings surface (FE-8 / G19).
   *
   * 정본:
   * - frontend-handover-v2 FE-8 + G19 (full-screen overlay + auto-save)
   * - ADR-0017 amend ④ (2026-05-16 Settings overlay shape)
   *
   * Layout: left-side grouped rail + right-side grouped section content.
   * Behaviour: Esc / [×] close. Auto-save — each persisted control saves
   * on change, no [Save] button.
   *
   * This round wires implemented settings plus lightweight system/account
   * actions. Future component-specific settings should extend the grouped
   * left rail and section groups rather than adding menu-bar actions.
   */

  import { themeStore, type ThemeMode } from '$lib/stores/theme.svelte';
  import { settingsDialog, type SettingsSection } from '$lib/stores/settingsDialog.svelte';
  import { settingsStore } from '$lib/stores/settings.svelte';
  import {
    COMPONENT_SCALE_MAX,
    COMPONENT_SCALE_MIN,
    COMPONENT_SCALE_STEP,
    componentSettings,
    type ComponentScaleKey,
  } from '$lib/stores/componentSettings.svelte';
  import {
    shortcutRegistry,
    type ShortcutAction,
    type ShortcutBinding,
  } from '$lib/keyboard/shortcutRegistry.svelte';
  import { formatShortcutBinding } from '$lib/keyboard/shortcutDisplay';
  import { shortcutOverrides, normalizeShortcutBinding } from '$lib/stores/shortcutOverrides.svelte';
  import { UnauthorizedError } from '$lib/http/sessions';
  import type { BehaviorSettings } from '$lib/http/settings';
  import {
    setPassword,
    changePassword,
    resetPassword,
    PasswordError,
  } from '$lib/http/settings';
  import { logout, rotateToken } from '$lib/http/auth';
  import {
    InvalidCredentialError,
    CredentialRequiredError,
    RateLimitedError,
  } from '$lib/http/stepup';
  import brandLogoUrl from '$lib/assets/brand.png';
  import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import ShutdownModal from './ShutdownModal.svelte';
  import ReauthModal from './ReauthModal.svelte';
  import { sessionIODialog } from '$lib/stores/sessionIOdialog.svelte';
  import { sessionStorageHint } from '$lib/stores/sessionStorageHint';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { shutdownDialog } from '$lib/stores/shutdownDialog.svelte';

  const open = $derived(settingsDialog.open);
  const section = $derived(settingsDialog.section);
  const activeSessionName = $derived(sessionStore.active?.name ?? 'current session');

  /**
   * ADR-0049 D4 — OSC 52 terminal clipboard write needs a secure context
   * (HTTPS or localhost). We keep the consent toggle enabled regardless so the
   * user can grant it ahead of an HTTPS switch, but surface an inline warning
   * when the current page is non-secure so it's clear the setting won't take
   * effect yet. SSR-safe: `window` may be undefined during prerender.
   */
  const isSecureContext = $derived(
    typeof window !== 'undefined' && window.isSecureContext === true,
  );

  /* ── Section nav ─────────────────────────────────────────────────── */

  const SECTION_GROUPS: {
    label: string;
    items: { id: SettingsSection; label: string; badge?: string }[];
  }[] = [
    {
      label: 'Workspace',
      items: [
        { id: 'storage', label: 'Storage' },
        { id: 'behavior', label: 'Behavior' },
      ],
    },
    {
      label: 'Preferences',
      items: [
        { id: 'theme', label: 'Appearance' },
        { id: 'components', label: 'Components' },
        { id: 'shortcuts', label: 'Keyboard' },
      ],
    },
    {
      label: 'System',
      items: [
        { id: 'auth', label: 'Auth' },
        { id: 'about', label: 'About' },
      ],
    },
  ];

  /* ── Theme section ───────────────────────────────────────────────── */

  /**
   * ADR-0017 amend ④ D2 의 Auto-save 정책 정합 — change 즉시 persist + modal
   * 유지. chrome theme (token swap) 은 setMode 한 번에 즉시 반영. xterm 은
   * XtermHost 의 live theme effect 가 repaint 하며 terminal buffer 를 보존한다.
   */
  function setMode(mode: ThemeMode): void {
    themeStore.setMode(mode);
  }

  /* ── Behavior section ───────────────────────────────────────────── */

  /**
   * Behavior 토글 공통 처리 — auto_kill (ADR-0021 G25.1.b), picker_show_hidden
   * (ADR-0035 D7), reload_on_session_switch (0077 follow-up).
   *
   * PATCH 실패 시 input.checked 를 store 값으로 되돌린다 — `checked` 는 one-way
   * 바인딩이라 store 가 안 바뀌면 Svelte 가 DOM 을 재동기화하지 않고, 실패한
   * 토글이 화면에 뒤집힌 채 남는다. 실패 자체는 toast 로 surface — 이후 동작이
   * default 로 fallback 되므로 silent 보다 안전.
   */
  async function setBehaviorFlag(
    key: keyof BehaviorSettings,
    input: HTMLInputElement,
  ): Promise<void> {
    const next = input.checked;
    try {
      await settingsStore.setBehavior({ [key]: next });
    } catch (err) {
      input.checked = settingsStore.behavior[key];
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Setting save failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  function openLayoutImport(): void {
    sessionIODialog.openImport();
    close();
  }

  function openLayoutExport(): void {
    if (sessionStore.active === null) return;
    sessionIODialog.openExport();
    close();
  }

  async function onLogout(): Promise<void> {
    sessionStorageHint.clear();
    try {
      await logout();
    } catch (e) {
      console.debug('[gtmux] logout failed', e);
    }
    window.location.href = '/auth';
  }

  /* ── Rotate token (step-up re-auth, ADR-0020 D18.3 + D16) ────────── */

  let rotateReauthOpen = $state(false);

  function openRotateReauth(): void {
    rotateReauthOpen = true;
  }

  /**
   * Gated action — runs with the credential from ReauthModal. Step-up errors
   * propagate so the ReauthModal stays open + shows them inline; everything
   * else is handled here (redirect / toast).
   *
   * On success the rotation reissues the SERVER token and signs *everyone* out,
   * including this caller (BE revoke_all + WS close 4001, ADR-0020 D18.3). So
   * the session is already dead: copy the fresh login URL/token to the clipboard
   * (best-effort — the toast still carries the value via text binding when the
   * copy is blocked), then full-reload to `/auth` to re-login — the same
   * window.location pattern as onLogout (ADR-0020 D9.1).
   */
  async function runRotate(credential: string): Promise<void> {
    try {
      const res = await rotateToken(credential);
      const value = res.url ?? res.new_token;
      const copy = await copyTextToSystemClipboard(value);
      const copyNote = copy.ok
        ? 'New link copied — re-login with it or your password.'
        : `New link: ${value} — re-login with it or your password.`;
      toastStore.show({
        message: `Server token rotated. All sessions signed out. ${copyNote}`,
        tone: 'success',
        durationMs: 12_000,
      });
    } catch (err) {
      if (
        err instanceof InvalidCredentialError ||
        err instanceof CredentialRequiredError ||
        err instanceof RateLimitedError
      ) {
        throw err; // ReauthModal branches + stays open for retry.
      }
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Rotate token failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
        durationMs: 7_000,
      });
      return;
    }
    rotateReauthOpen = false;
    // Session is dead (revoke_all) — full reload into /auth, like onLogout.
    window.location.href = '/auth';
  }

  /* ── Password set / change (ADR-0020 D17) ────────────────────────── */

  const passwordIsSet = $derived(settingsStore.auth?.password_set === true);

  let pwCurrent = $state('');
  let pwNew = $state('');
  let pwConfirm = $state('');
  let pwSubmitting = $state(false);
  let pwError = $state<string | null>(null);

  /**
   * Lightweight client-side policy mirror (ADR-0020 D5 / D12: len ≥ 8 + letter
   * + digit). The server is the authority — this only spares an obvious
   * round-trip and guides the user. `weak_password` from the BE still wins.
   */
  function newPasswordPolicyError(pw: string): string | null {
    if (pw.length === 0) return null;
    if (pw.length < 8) return 'Use at least 8 characters.';
    if (!/[A-Za-z]/.test(pw)) return 'Include at least one letter.';
    if (!/[0-9]/.test(pw)) return 'Include at least one digit.';
    return null;
  }

  const pwPolicyError = $derived(newPasswordPolicyError(pwNew));
  const pwMismatch = $derived(
    pwConfirm.length > 0 && pwNew !== pwConfirm,
  );
  const canSubmitPassword = $derived(
    !pwSubmitting &&
      pwNew.length > 0 &&
      pwConfirm.length > 0 &&
      pwPolicyError === null &&
      !pwMismatch &&
      (!passwordIsSet || pwCurrent.length > 0),
  );

  function resetPasswordForm(): void {
    pwCurrent = '';
    pwNew = '';
    pwConfirm = '';
    pwError = null;
  }

  async function submitPassword(): Promise<void> {
    if (!canSubmitPassword) return;
    pwSubmitting = true;
    pwError = null;
    try {
      if (passwordIsSet) {
        await changePassword(pwCurrent, pwNew);
      } else {
        await setPassword(pwNew);
      }
      // Refresh the snapshot so `password_set` (and the form mode) updates.
      await settingsStore.load();
      resetPasswordForm();
      toastStore.show({
        message: passwordIsSet ? 'Password changed.' : 'Password set.',
        tone: 'success',
      });
    } catch (err) {
      if (err instanceof PasswordError) {
        pwError =
          err.code === 'current_password_mismatch'
            ? 'Current password is incorrect.'
            : 'New password is too weak (8+ chars, a letter, and a digit).';
        return;
      }
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      pwError = err instanceof Error ? err.message : String(err);
    } finally {
      pwSubmitting = false;
    }
  }

  /**
   * Native `<form>` submit for the set/change password form. `preventDefault`
   * stops the default GET navigation; `submitPassword` runs the real action.
   * Wrapping the password inputs in a `<form>` silences Chrome's "[DOM]
   * Password field is not contained in a form" warning and enables password-
   * manager save/fill (S1-c). The per-input Enter handler is retained as a
   * belt-and-suspenders fallback (and to allow `preventDefault` on Enter inside
   * the multi-field form), but the form's onsubmit is the primary path.
   */
  function onPasswordFormSubmit(e: SubmitEvent): void {
    e.preventDefault();
    void submitPassword();
  }

  function onPasswordKeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && canSubmitPassword) {
      e.preventDefault();
      void submitPassword();
    }
  }

  /* ── Delete password (union step-up reset, ADR-0020 D19) ─────────── */

  let deletePwReauthOpen = $state(false);

  function openDeletePwReauth(): void {
    deletePwReauthOpen = true;
  }

  /**
   * Gated action — runs with the credential from the union ReauthModal
   * (`credentialKind="either"`). The credential is EITHER the current password
   * OR the server token (D19.2), re-verified inline by the BE. Step-up errors
   * propagate so the modal stays open + shows them inline; everything else is
   * handled here.
   *
   * On success the BE deletes the password hash → `password_set` flips false.
   * The cookie/session is unchanged (D19.3 — token still valid), so no redirect:
   * we just refresh the snapshot (the set/change form switches to "Set
   * password") and toast.
   */
  async function runDeletePassword(credential: string): Promise<void> {
    try {
      await resetPassword(credential);
    } catch (err) {
      if (
        err instanceof InvalidCredentialError ||
        err instanceof CredentialRequiredError ||
        err instanceof RateLimitedError
      ) {
        throw err; // ReauthModal branches + stays open for retry.
      }
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Delete password failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
        durationMs: 7_000,
      });
      return;
    }
    deletePwReauthOpen = false;
    // Refresh so `password_set` flips false → the form switches to "Set
    // password" and the Status row updates.
    await settingsStore.load();
    resetPasswordForm();
    toastStore.show({
      message: 'Password deleted — token-only sign-in.',
      tone: 'success',
    });
  }

  /* ── Components section ─────────────────────────────────────────── */

  const COMPONENT_SCALE_CONTROLS: {
    key: ComponentScaleKey;
    label: string;
    ariaLabel: string;
    title: string;
  }[] = [
    {
      key: 'document_scale',
      label: 'Doc',
      ariaLabel: 'Document font size',
      title: 'Canvas documents and maximized document views.',
    },
    {
      key: 'preview_scale',
      label: 'Preview',
      ariaLabel: 'Preview font size',
      title: 'Files Preview tab and maximized file preview.',
    },
    {
      key: 'note_scale',
      label: 'Note',
      ariaLabel: 'Note font size',
      title: 'Note body text in canvas and maximized views.',
    },
  ];

  function setComponentScale(key: ComponentScaleKey, value: string | number): void {
    const next = typeof value === 'number' ? value : Number(value);
    componentSettings.setScale(key, next);
  }

  function componentScaleValue(key: ComponentScaleKey): number {
    switch (key) {
      case 'document_scale':
        return componentSettings.documentScale;
      case 'preview_scale':
        return componentSettings.previewScale;
      case 'note_scale':
        return componentSettings.noteScale;
    }
  }

  /* ── About section ─────────────────────────────────────────────── */

  function shortSha(sha: string | null | undefined): string {
    if (sha === null || sha === undefined || sha.length === 0) return 'local';
    if (sha === 'unknown') return 'unknown';
    return sha.slice(0, 12);
  }

  function runtimeEndpoint(): string {
    if (settingsStore.server === null) return 'unknown';
    return `${settingsStore.server.bind}:${settingsStore.server.port}`;
  }

  function logTarget(): string {
    return settingsStore.server?.log_path ?? 'stderr';
  }

  function authMode(): string {
    if (settingsStore.auth === null) return 'unknown';
    if (settingsStore.auth.password_set && settingsStore.auth.token_present) {
      return 'Password + token';
    }
    if (settingsStore.auth.password_set) return 'Password';
    if (settingsStore.auth.token_present) return 'Token';
    return 'Not configured';
  }

  function argon2Summary(): string {
    if (settingsStore.auth === null) return 'unknown';
    const argon2 = settingsStore.auth.argon2;
    const memoryMiB = Math.round(argon2.m_cost_kib / 1024);
    return `m=${memoryMiB}MiB · t=${argon2.t_cost} · p=${argon2.p_cost}`;
  }

  /* ── Shortcuts section ───────────────────────────────────────────── */

  let capturingActionId = $state<string | null>(null);
  let captureError = $state<string | null>(null);

  function bindingFromEvent(e: KeyboardEvent): ShortcutBinding | null {
    if (['Meta', 'Control', 'Alt', 'Shift'].includes(e.key)) return null;
    return normalizeShortcutBinding({
      key: e.key,
      meta: e.metaKey,
      ctrl: e.ctrlKey,
      alt: e.altKey,
      shift: e.shiftKey,
    });
  }

  function startCapture(actionId: string): void {
    capturingActionId = actionId;
    captureError = null;
  }

  function cancelCapture(): void {
    capturingActionId = null;
    captureError = null;
  }

  function resetShortcut(actionId: string): void {
    shortcutOverrides.reset(actionId);
    if (capturingActionId === actionId) cancelCapture();
  }

  function setSection(next: SettingsSection): void {
    cancelCapture();
    settingsDialog.setSection(next);
  }

  function commitShortcutCapture(action: ShortcutAction, e: KeyboardEvent): void {
    e.preventDefault();
    e.stopPropagation();
    e.stopImmediatePropagation();
    if (e.key === 'Escape') {
      cancelCapture();
      return;
    }
    const binding = bindingFromEvent(e);
    if (binding === null) return;
    const conflict = shortcutRegistry.conflictFor(action.actionId, binding);
    if (conflict !== null) {
      captureError =
        conflict.kind === 'reserved'
          ? conflict.description
          : `Already used by ${conflict.description}.`;
      return;
    }
    shortcutOverrides.set(action.actionId, binding);
    cancelCapture();
  }

  /** Group action descriptors by category. */
  const grouped = $derived.by(() => {
    const list = shortcutRegistry.listActions();
    const map = new Map<string, ShortcutAction[]>();
    for (const d of list) {
      const cat = d.category;
      const bucket = map.get(cat);
      if (bucket) bucket.push(d);
      else map.set(cat, [d]);
    }
    return Array.from(map.entries()).sort((a, b) => a[0].localeCompare(b[0]));
  });

  /* ── Close / keyboard ────────────────────────────────────────────── */

  let overlayEl = $state<HTMLDivElement | undefined>();

  function close(): void {
    cancelCapture();
    settingsDialog.close();
  }

  /** Modal primitive 와 동일한 단순 focus trap — Tab 이 overlay 안에서 순환. */
  function trapTab(e: KeyboardEvent): void {
    if (overlayEl === undefined) return;
    const focusables = Array.from(
      overlayEl.querySelectorAll<HTMLElement>(
        'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'
      )
    );
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    if (!first || !last) return;
    if (document.activeElement === null || !overlayEl.contains(document.activeElement)) {
      e.preventDefault();
      first.focus();
    } else if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  function onWindowKey(e: KeyboardEvent): void {
    if (!open) return;
    // ShutdownModal 이 위에 떠 있는 동안 Esc/Tab 소유권은 Modal primitive 쪽
    // (bubble phase) 에 있다. capture phase 인 이 리스너가 먼저 받으므로
    // 명시적으로 양보해야 Esc 가 confirm 만 닫고 settings 는 유지된다.
    if (shutdownDialog.open) return;
    // Same yield for the step-up re-auth modals mounted by this overlay (rotate
    // session, delete-password) — let their Modal primitive own Esc/Tab while
    // open.
    if (rotateReauthOpen) return;
    if (deletePwReauthOpen) return;
    if (capturingActionId !== null) {
      const action = shortcutRegistry
        .listActions()
        .find((candidate) => candidate.actionId === capturingActionId);
      if (action !== undefined) commitShortcutCapture(action, e);
      else cancelCapture();
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
      return;
    }
    if (e.key === 'Tab') trapTab(e);
  }

  $effect(() => {
    if (typeof window === 'undefined') return;
    if (!open) {
      // Cmd+, toggle 등 close() 를 거치지 않는 경로로 닫혀도 ShutdownModal 의
      // dialog state 를 함께 정리 — 안 하면 다음 open 때 confirm 이 곧바로
      // 다시 뜬다 (modal 은 {#if open} 안에 있어 unmount 만 된다).
      shutdownDialog.close();
      // Tear down any step-up gate + clear the password form so a fresh open
      // never shows stale credential input.
      rotateReauthOpen = false;
      deletePwReauthOpen = false;
      resetPasswordForm();
      return;
    }
    window.addEventListener('keydown', onWindowKey, { capture: true });
    // 초기 focus — overlay 안 첫 focusable (Modal primitive 와 동일 정책).
    queueMicrotask(() => {
      if (!settingsDialog.open || overlayEl === undefined) return;
      if (overlayEl.contains(document.activeElement)) return;
      const focusable = overlayEl.querySelector<HTMLElement>(
        'button:not([disabled]), [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
      focusable?.focus();
    });
    return () => window.removeEventListener('keydown', onWindowKey, { capture: true });
  });
</script>

{#snippet resetIcon()}
  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M13 4.5V1.8h-2.7" />
    <path d="M12.6 4.2A5.2 5.2 0 1 0 13.2 9" />
  </svg>
{/snippet}

{#snippet packageIcon()}
  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M2.5 5 8 2.2 13.5 5v6L8 13.8 2.5 11V5Z" />
    <path d="M2.8 5.2 8 8l5.2-2.8M8 8v5.5" />
  </svg>
{/snippet}

{#snippet hashIcon()}
  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" aria-hidden="true">
    <path d="M5.6 2.5 4.8 13.5M11.2 2.5l-.8 11M3 6h10M2.5 10h10" />
  </svg>
{/snippet}

{#snippet codeIcon()}
  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="m5.6 4.2-3 3.8 3 3.8M10.4 4.2l3 3.8-3 3.8" />
  </svg>
{/snippet}

{#snippet serverIcon()}
  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <rect x="2.5" y="3" width="11" height="4" rx="1" />
    <rect x="2.5" y="9" width="11" height="4" rx="1" />
    <path d="M5 5h.1M5 11h.1" />
  </svg>
{/snippet}

{#snippet cpuIcon()}
  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <rect x="4.5" y="4.5" width="7" height="7" rx="1" />
    <path d="M6.5 2.5v2M9.5 2.5v2M6.5 11.5v2M9.5 11.5v2M2.5 6.5h2M2.5 9.5h2M11.5 6.5h2M11.5 9.5h2" />
  </svg>
{/snippet}

{#snippet logIcon()}
  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M4 2.5h5.5L12 5v8.5H4V2.5Z" />
    <path d="M9.5 2.5V5H12M6 8h4M6 10.5h3" />
  </svg>
{/snippet}

{#snippet shieldIcon()}
  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M8 2.3 13 4v3.8c0 3-2 5-5 6-3-1-5-3-5-6V4l5-1.7Z" />
    <path d="m5.8 8 1.5 1.5 3-3" />
  </svg>
{/snippet}

{#snippet lockIcon()}
  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <rect x="3.5" y="7" width="9" height="6" rx="1.2" />
    <path d="M5.5 7V5.4a2.5 2.5 0 0 1 5 0V7" />
  </svg>
{/snippet}

{#snippet powerIcon()}
  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M8 2.5v5" />
    <path d="M4.7 4.9a5 5 0 1 0 6.6 0" />
  </svg>
{/snippet}

{#if open}
  <div
    class="settings-backdrop"
    role="presentation"
    onclick={close}
    onkeydown={() => {}}
  >
    <div
      bind:this={overlayEl}
      class="settings-overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby="settings-title"
      tabindex="-1"
      onclick={(e: MouseEvent) => e.stopPropagation()}
      onkeydown={() => {}}
    >
      <header class="settings-head">
        <span class="gear" aria-hidden="true">
          <svg width="17" height="17" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="9" cy="9" r="2.4" />
            <path d="M9 1.6v2M9 14.4v2M1.6 9h2M14.4 9h2M3.8 3.8l1.4 1.4M12.8 12.8l1.4 1.4M3.8 14.2l1.4-1.4M12.8 5.2l1.4-1.4" />
          </svg>
        </span>
        <div class="head-copy">
          <h2 id="settings-title">Settings</h2>
          <span>gtmux · workspace</span>
        </div>
        <button type="button" class="close" aria-label="Close settings" title="Close" onclick={close}>
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
            <path d="M4 4l8 8M12 4l-8 8" />
          </svg>
        </button>
      </header>

      <div class="settings-body">
        <nav class="section-nav" aria-label="Settings sections">
          {#each SECTION_GROUPS as group (group.label)}
            <div class="nav-head">{group.label}</div>
            {#each group.items as s (s.id)}
              <button
                type="button"
                class="nav-btn"
                class:active={section === s.id}
                onclick={() => setSection(s.id)}
              >
                <span class="nav-ico" aria-hidden="true"></span>
                <span class="nav-label">{s.label}</span>
                {#if s.badge}
                  <span class="nav-badge">{s.badge}</span>
                {/if}
              </button>
            {/each}
          {/each}
        </nav>

        <section class="section-pane" aria-live="polite">
          {#if section === 'theme'}
            <h3 class="section-head">Appearance</h3>
            <p class="section-hint">
              Theme applies immediately and follows the same token system as the canvas chrome.
            </p>
            <div class="sgroup-head">Theme</div>
            <div class="srow">
              <div>
                <div class="lbl">Interface theme</div>
                <div class="dsc">System follows your OS light/dark preference.</div>
              </div>
              <div class="ctl">
                <div class="seg" role="radiogroup" aria-label="Theme mode">
                  {#each ['light', 'dark', 'system'] as mode (mode)}
                    <button
                      type="button"
                      class:on={themeStore.mode === mode}
                      aria-pressed={themeStore.mode === mode}
                      onclick={() => setMode(mode as ThemeMode)}
                    >
                      {mode === 'system' ? 'System' : mode === 'light' ? 'Light' : 'Dark'}
                    </button>
                  {/each}
                </div>
              </div>
            </div>
          {:else if section === 'shortcuts'}
            <h3 class="section-head">Keyboard</h3>
            <p class="section-hint">
              Click a shortcut to record a replacement. Esc cancels recording.
            </p>
            {#if grouped.length === 0}
              <p class="placeholder">No shortcuts registered yet.</p>
            {:else}
              {#each grouped as [category, items] (category)}
                <div class="shortcut-group">
                  <h4 class="group-head">{category}</h4>
                  <table class="shortcut-table">
                    <tbody>
                      {#each items as d (d.actionId)}
                        <tr>
                          <td class="desc">
                            <span>{d.description}</span>
                            {#if d.overridden}
                              <span class="shortcut-state">custom</span>
                            {/if}
                            {#if d.customizable === false && d.protectedReason}
                              <span class="shortcut-sub">{d.protectedReason}</span>
                            {/if}
                          </td>
                          <td class="combo mono">
                            {#if capturingActionId === d.actionId}
                              <button
                                type="button"
                                class="shortcut-capture"
                                onkeydown={(e) => commitShortcutCapture(d, e)}
                              >
                                Press keys…
                              </button>
                            {:else}
                              <button
                                type="button"
                                class="shortcut-button mono"
                                disabled={!d.customizable}
                                title={d.defaultBindings[0]
                                  ? `Default: ${formatShortcutBinding(d.defaultBindings[0])}`
                                  : undefined}
                                onclick={() => startCapture(d.actionId)}
                              >
                                {formatShortcutBinding(d.activeBindings[0])}
                              </button>
                            {/if}
                          </td>
                          <td class="shortcut-actions">
                            <button
                              type="button"
                              class="reset-btn"
                              aria-label={`Reset shortcut for ${d.description}`}
                              title="Reset shortcut"
                              disabled={!d.overridden}
                              onclick={() => resetShortcut(d.actionId)}
                            >
                              {@render resetIcon()}
                            </button>
                          </td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </div>
              {/each}
              {#if captureError !== null}
                <p class="shortcut-error" role="alert">{captureError}</p>
              {/if}
            {/if}
          {:else if section === 'storage'}
            <h3 class="section-head">Storage</h3>
            <p class="section-hint">Layout files, workspace file visibility, and storage-facing defaults.</p>
            <div class="sgroup-head">Layout</div>
            <div class="srow">
              <div>
                <div class="lbl">Import layout</div>
                <div class="dsc">Create a new session from a gtmux layout export file.</div>
              </div>
              <div class="ctl">
                <button type="button" class="btn" onclick={openLayoutImport}>Import…</button>
              </div>
            </div>
            <div class="srow">
              <div>
                <div class="lbl">Export current layout</div>
                <div class="dsc">Download the active canvas layout and references.</div>
              </div>
              <div class="ctl">
                <button
                  type="button"
                  class="btn"
                  disabled={sessionStore.active === null}
                  title={sessionStore.active === null ? 'No active session to export' : 'Export current layout'}
                  onclick={openLayoutExport}
                >
                  Export…
                </button>
              </div>
            </div>
            <div class="sgroup-head">Files</div>
            <label class="srow">
              <div>
                <div class="lbl">Show hidden files</div>
                <div class="dsc">Include dot-prefixed files in workspace file pickers.</div>
              </div>
              <div class="ctl">
                <input
                  class="native-toggle"
                  type="checkbox"
                  checked={settingsStore.behavior.picker_show_hidden}
                  onchange={(e) => void setBehaviorFlag('picker_show_hidden', e.currentTarget as HTMLInputElement)}
                />
              </div>
            </label>
          {:else if section === 'auth'}
            <h3 class="section-head">Auth</h3>
            <p class="section-hint">Authentication actions that affect this browser session and token access.</p>
            <div class="sgroup-head">Account</div>
            <div class="srow">
              <div>
                <div class="lbl">Sign out</div>
                <div class="dsc">Clear the auth cookie and return to the auth page.</div>
              </div>
              <div class="ctl">
                <button type="button" class="btn" onclick={() => void onLogout()}>Sign out</button>
              </div>
            </div>
            <div class="srow">
              <div>
                <div class="lbl">Rotate token</div>
                <div class="dsc">
                  Reissue the server token and sign out every session, including
                  this one. The old token link stops working; you get a fresh
                  link to re-login. Requires re-entering your
                  {passwordIsSet ? 'password' : 'token'}.
                </div>
              </div>
              <div class="ctl">
                <button type="button" class="btn" onclick={openRotateReauth}>Rotate</button>
              </div>
            </div>

            <div class="sgroup-head">Password</div>
            <div class="srow pw-row">
              <div>
                <div class="lbl">{passwordIsSet ? 'Change password' : 'Set password'}</div>
                <div class="dsc">
                  {#if passwordIsSet}
                    Update the password used to sign in and to confirm sensitive
                    actions. Enter your current password to authorise the change.
                  {:else}
                    Set a password to sign in and to confirm sensitive actions.
                    At least 8 characters with a letter and a digit.
                  {/if}
                </div>
                <form class="pw-form" onsubmit={onPasswordFormSubmit}>
                  <!--
                    Hidden username field for password managers (Chrome [DOM]
                    "Password forms should have (optionally hidden) username
                    fields", https://goo.gl/9p2vKq). gtmux is single-user with no
                    real username, so a constant gives a stable saved entry.
                    display:none keeps it out of layout; tabindex=-1 + aria-hidden
                    keep it out of the focus trap and a11y tree.
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
                  {#if passwordIsSet}
                    <label class="pw-field">
                      <span class="pw-label">Current password</span>
                      <input
                        class="pw-input"
                        type="password"
                        bind:value={pwCurrent}
                        placeholder="Current password"
                        disabled={pwSubmitting}
                        autocomplete="current-password"
                        autocapitalize="off"
                        autocorrect="off"
                        spellcheck="false"
                        onkeydown={onPasswordKeydown}
                      />
                    </label>
                  {/if}
                  <label class="pw-field" class:has-error={pwPolicyError !== null}>
                    <span class="pw-label">New password</span>
                    <input
                      class="pw-input"
                      type="password"
                      bind:value={pwNew}
                      placeholder="New password"
                      disabled={pwSubmitting}
                      autocomplete="new-password"
                      autocapitalize="off"
                      autocorrect="off"
                      spellcheck="false"
                      onkeydown={onPasswordKeydown}
                    />
                    {#if pwPolicyError !== null}
                      <span class="pw-hint pw-hint-error" role="alert">{pwPolicyError}</span>
                    {/if}
                  </label>
                  <label class="pw-field" class:has-error={pwMismatch}>
                    <span class="pw-label">Confirm new password</span>
                    <input
                      class="pw-input"
                      type="password"
                      bind:value={pwConfirm}
                      placeholder="Re-enter new password"
                      disabled={pwSubmitting}
                      autocomplete="new-password"
                      autocapitalize="off"
                      autocorrect="off"
                      spellcheck="false"
                      onkeydown={onPasswordKeydown}
                    />
                    {#if pwMismatch}
                      <span class="pw-hint pw-hint-error" role="alert">Passwords do not match.</span>
                    {/if}
                  </label>
                  {#if pwError !== null}
                    <p class="pw-error" role="alert">{pwError}</p>
                  {/if}
                  <div class="pw-actions">
                    <button
                      type="submit"
                      class="btn"
                      disabled={!canSubmitPassword}
                    >
                      {#if pwSubmitting}
                        Saving…
                      {:else}
                        {passwordIsSet ? 'Change password' : 'Set password'}
                      {/if}
                    </button>
                  </div>
                </form>
              </div>
            </div>

            <div class="sgroup-head">Delete password</div>
            <div class="srow">
              <div>
                <div class="lbl">Delete password</div>
                <div class="dsc">
                  {#if passwordIsSet}
                    Delete the saved password and revert to token-only sign-in.
                    You'll confirm with your token or current password. You can
                    set a new password again later.
                  {:else}
                    No password is set.
                  {/if}
                </div>
              </div>
              <div class="ctl">
                <button
                  type="button"
                  class="btn danger"
                  onclick={openDeletePwReauth}
                  disabled={!passwordIsSet || pwSubmitting}
                >
                  Delete password
                </button>
              </div>
            </div>

            {#if settingsStore.auth !== null}
              <div class="sgroup-head">Status</div>
              <div class="kv-row"><span>Token</span><strong>{settingsStore.auth.token_present ? 'Present' : 'Missing'}</strong></div>
              <div class="kv-row"><span>Password</span><strong>{settingsStore.auth.password_set ? 'Set' : 'Not set'}</strong></div>
            {/if}
          {:else if section === 'behavior'}
            <h3 class="section-head">Behavior</h3>
            <p class="section-hint">
              Per-action defaults. Settings persist for the lifetime of the
              server process.
            </p>
            <div class="sgroup-head">Safety</div>
            <label class="srow">
              <div>
                <div class="lbl">Auto-kill terminal on panel close</div>
                <div class="dsc">Skip the confirm dialog and SIGTERM the terminal whenever a panel is closed.</div>
              </div>
              <div class="ctl">
                <input
                  class="native-toggle"
                  type="checkbox"
                  checked={settingsStore.behavior.auto_kill_terminal_on_panel_close}
                  onchange={(e) => void setBehaviorFlag('auto_kill_terminal_on_panel_close', e.currentTarget as HTMLInputElement)}
                />
              </div>
            </label>
            <div class="sgroup-head">Session lifecycle</div>
            <label class="srow">
              <div>
                <div class="lbl">Reload page on session switch</div>
                <div class="dsc">After switching sessions, reload the page to reset caches, WS state, and attach state.</div>
              </div>
              <div class="ctl">
                <input
                  class="native-toggle"
                  type="checkbox"
                  checked={settingsStore.behavior.reload_on_session_switch}
                  onchange={(e) => void setBehaviorFlag('reload_on_session_switch', e.currentTarget as HTMLInputElement)}
                />
              </div>
            </label>
            <div class="sgroup-head">Terminal</div>
            <label class="srow">
              <div>
                <div class="lbl">Allow terminal clipboard copy (OSC 52)</div>
                <div class="dsc">
                  Lets terminal apps (e.g. claude) copy to your clipboard via OSC 52.
                  Requires HTTPS or localhost; over plain HTTP it stays inactive.
                </div>
                {#if !isSecureContext}
                  <div class="dsc-warn" role="note">
                    This page is not a secure context, so this setting won't take
                    effect until you reach gtmux over HTTPS or localhost.
                  </div>
                {/if}
              </div>
              <div class="ctl">
                <input
                  class="native-toggle"
                  type="checkbox"
                  checked={settingsStore.behavior.osc52_clipboard_write_enabled}
                  onchange={(e) => void setBehaviorFlag('osc52_clipboard_write_enabled', e.currentTarget as HTMLInputElement)}
                />
              </div>
            </label>
          {:else if section === 'components'}
            <h3 class="section-head">Components</h3>
            <p class="section-hint">Browser-local presentation defaults for component viewers.</p>
            <div class="sgroup-head">Font size</div>
            {#each COMPONENT_SCALE_CONTROLS as control (control.key)}
              <label class="srow" title={control.title}>
                <div>
                  <div class="lbl">{control.label}</div>
                  <div class="dsc">{control.title}</div>
                </div>
                <div class="ctl">
                  <div class="slider">
                    <input
                      class="scale-slider"
                      type="range"
                      min={COMPONENT_SCALE_MIN}
                      max={COMPONENT_SCALE_MAX}
                      step={COMPONENT_SCALE_STEP}
                      value={componentScaleValue(control.key)}
                      aria-label={control.ariaLabel}
                      oninput={(e) => setComponentScale(control.key, (e.currentTarget as HTMLInputElement).value)}
                    />
                    <span class="val">{Math.round(componentScaleValue(control.key) * 100)}%</span>
                  </div>
                </div>
              </label>
            {/each}
            <div class="srow">
              <div>
                <div class="lbl">Reset font size</div>
                <div class="dsc">Restore Doc, Preview, and Note to 100%.</div>
              </div>
              <div class="ctl">
                <button
                  type="button"
                  class="btn icon-btn"
                  aria-label="Reset all font sizes"
                  title="Reset all font sizes"
                  onclick={() => componentSettings.reset()}
                >
                  {@render resetIcon()}
                </button>
              </div>
            </div>
          {:else if section === 'about'}
            <h3 class="section-head">About</h3>
            <p class="section-hint">
              Product identity, build metadata, local server status, and system-level actions.
            </p>
            <div class="about-id">
              <img class="about-mark" src={brandLogoUrl} alt="" aria-hidden="true" />
              <div>
                <div class="about-name">gtmux</div>
                <div class="about-tagline">tmux-backed Web Canvas Workspace</div>
                <div class="about-version mono">
                  Version {settingsStore.build?.version ?? 'dev'} · {shortSha(settingsStore.build?.sha)}
                </div>
              </div>
            </div>
            <p class="about-desc">
              gtmux renders your tmux sessions, windows, and panes as draggable
              panels on an infinite web canvas. tmux owns the processes and
              session lifecycle; the canvas owns the visual layout — terminals
              keep running even when the browser is closed.
            </p>
            <div class="sgroup-head">Build</div>
            <div class="info-row">
              <span class="info-icon">{@render packageIcon()}</span>
              <span class="info-key">Version</span>
              <strong class="info-val mono">{settingsStore.build?.version ?? 'dev'}</strong>
            </div>
            <div class="info-row">
              <span class="info-icon">{@render hashIcon()}</span>
              <span class="info-key">Commit</span>
              <strong class="info-val mono" title={settingsStore.build?.sha ?? 'local'}>
                {shortSha(settingsStore.build?.sha)}
              </strong>
            </div>
            <div class="info-row">
              <span class="info-icon">{@render codeIcon()}</span>
              <span class="info-key">Rust</span>
              <strong class="info-val mono">{settingsStore.build?.rust ?? 'unknown'}</strong>
            </div>
            <div class="sgroup-head">Runtime</div>
            <div class="info-row">
              <span class="info-icon">{@render serverIcon()}</span>
              <span class="info-key">Endpoint</span>
              <strong class="info-val mono">{runtimeEndpoint()}</strong>
            </div>
            <div class="info-row">
              <span class="info-icon">{@render cpuIcon()}</span>
              <span class="info-key">Process</span>
              <strong class="info-val mono">{settingsStore.server?.pid ?? 'unknown'}</strong>
            </div>
            <div class="info-row">
              <span class="info-icon">{@render logIcon()}</span>
              <span class="info-key">Logs</span>
              <strong class="info-val mono" title={logTarget()}>{logTarget()}</strong>
            </div>
            <div class="sgroup-head">Security</div>
            <div class="info-row">
              <span class="info-icon">{@render shieldIcon()}</span>
              <span class="info-key">Auth</span>
              <strong class="info-val">{authMode()}</strong>
            </div>
            <div class="info-row">
              <span class="info-icon">{@render lockIcon()}</span>
              <span class="info-key">Argon2id</span>
              <strong class="info-val mono">{argon2Summary()}</strong>
            </div>
            <div class="sgroup-head">Danger zone</div>
            <div class="srow danger-row">
              <div>
                <div class="lbl with-icon">
                  <span class="label-icon" aria-hidden="true">{@render powerIcon()}</span>
                  <span>Shutdown server</span>
                </div>
                <div class="dsc">Stop the local gtmux server process. Layout is preserved on disk.</div>
              </div>
              <div class="ctl">
                <button type="button" class="btn danger" onclick={() => shutdownDialog.show()}>
                  Shutdown…
                </button>
              </div>
            </div>
          {/if}
        </section>
      </div>
      <footer class="settings-foot">
        <div class="dstate"><span class="dot"></span><span>All changes save automatically</span></div>
      </footer>
    </div>
  </div>

  <ShutdownModal
    open={shutdownDialog.open}
    sessionName={activeSessionName}
    onclose={() => shutdownDialog.close()}
  />

  <ReauthModal
    open={rotateReauthOpen}
    title="Confirm rotate"
    description="Re-enter your credential to reissue the server token. This signs out every session, including this one."
    confirmLabel="Rotate"
    confirmVariant="primary"
    onSubmit={runRotate}
    onCancel={() => (rotateReauthOpen = false)}
  />

  <ReauthModal
    open={deletePwReauthOpen}
    title="Delete password"
    description="Enter your token or current password to delete it. Sign-in reverts to token-only; you can set a new password later."
    confirmLabel="Delete password"
    confirmVariant="danger"
    credentialKind="either"
    onSubmit={runDeletePassword}
    onCancel={() => (deletePwReauthOpen = false)}
  />
{/if}

<style>
  .settings-backdrop {
    position: fixed;
    inset: 0;
    background: transparent;
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    z-index: var(--z-modal);
    display: grid;
    place-items: center;
  }

  .settings-overlay {
    position: relative;
    width: min(884px, calc(100vw - 40px));
    height: min(624px, calc(100vh - 40px));
    background: var(--color-surface);
    color: var(--color-fg);
    border: 1px solid var(--color-border);
    border-radius: 10px;
    box-shadow: var(--shadow-lg);
    display: grid;
    grid-template-rows: 53px 1fr 57px;
    overflow: hidden;
    font-size: var(--text-base);
    line-height: var(--leading-normal);
    letter-spacing: 0;
  }

  .settings-overlay :global(svg) {
    display: block;
    flex: 0 0 auto;
  }

  .settings-head {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 0 14px 0 16px;
    border-bottom: 1px solid var(--color-border);
  }

  .gear {
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    color: var(--color-fg);
  }

  .head-copy {
    display: grid;
    gap: 1px;
  }

  .settings-head h2 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: var(--weight-semibold);
    letter-spacing: 0;
  }

  .head-copy span {
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    letter-spacing: 0.5px;
    text-transform: uppercase;
  }

  .close {
    box-sizing: border-box;
    width: 32px;
    height: 32px;
    margin-left: auto;
    padding: 0;
    border: 0;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--color-fg-muted);
    display: grid;
    place-items: center;
    line-height: 1;
    cursor: pointer;
  }

  .close:hover {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .settings-body {
    display: grid;
    grid-template-columns: 236px 1fr;
    min-height: 0;
  }

  .section-nav {
    border-right: 1px solid var(--color-border);
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    overflow-y: auto;
  }

  .nav-head {
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    letter-spacing: 0.6px;
    text-transform: uppercase;
    padding: 13px 10px 5px;
  }

  .nav-btn {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 32px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--color-fg);
    font: inherit;
    font-size: var(--text-base);
    letter-spacing: 0;
    text-align: left;
    cursor: pointer;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .nav-btn:hover {
    background: var(--color-glass-1);
  }

  .nav-btn.active {
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    color: var(--color-accent);
    font-weight: var(--weight-medium);
  }

  .nav-ico {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-fg-subtle);
    flex: 0 0 auto;
  }

  .nav-btn.active .nav-ico {
    background: var(--color-accent);
  }

  .nav-label {
    flex: 1 1 auto;
  }

  .nav-badge {
    padding: 0 6px;
    border-radius: var(--radius-pill);
    background: var(--color-surface-2);
    color: var(--color-fg-subtle);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    letter-spacing: 0.4px;
  }

  .section-pane {
    overflow-y: auto;
    padding: 6px 30px 36px;
    min-height: 0;
  }

  .section-head {
    margin: 0;
    padding: 24px 0 5px;
    font-size: var(--text-xl);
    font-weight: var(--weight-semibold);
    letter-spacing: 0;
  }

  .section-hint {
    margin: 0;
    color: var(--color-fg-muted);
    font-size: var(--text-base);
    line-height: var(--leading-normal);
    max-width: 62ch;
  }

  .sgroup-head {
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    letter-spacing: 0.6px;
    text-transform: uppercase;
    padding: 26px 0 2px;
  }

  .srow {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 28px;
    align-items: center;
    padding: 14px 0;
    border-bottom: 1px solid var(--color-border);
  }

  .srow:last-child {
    border-bottom: 0;
  }

  .lbl {
    color: var(--color-fg);
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    letter-spacing: 0;
  }

  .lbl.with-icon {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .label-icon {
    width: 18px;
    height: 18px;
    display: inline-grid;
    place-items: center;
    color: var(--color-danger);
  }

  .dsc {
    color: var(--color-fg-muted);
    font-size: var(--text-base);
    line-height: var(--leading-normal);
    margin-top: 3px;
    max-width: 50ch;
  }

  .dsc-warn {
    color: var(--color-warning, var(--color-danger));
    font-size: var(--text-base);
    line-height: var(--leading-normal);
    margin-top: 5px;
    max-width: 50ch;
  }

  .ctl {
    display: flex;
    align-items: center;
    gap: 8px;
    justify-self: end;
  }

  .seg {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-md);
    background: var(--color-surface-2);
  }

  .seg button {
    height: 24px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-fg-muted);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: var(--text-base);
    font-weight: var(--weight-medium);
    letter-spacing: 0;
    line-height: 1;
  }

  .seg button:hover {
    color: var(--color-fg);
  }

  .seg button.on {
    background: var(--color-surface);
    color: var(--color-fg);
    box-shadow: var(--shadow-sm);
  }

  .btn {
    box-sizing: border-box;
    height: 32px;
    min-width: 64px;
    padding: 0 var(--space-12);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    background: var(--color-surface-2);
    color: var(--color-fg);
    cursor: pointer;
    font-size: var(--text-base);
    font-weight: var(--weight-medium);
    letter-spacing: 0;
    line-height: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }

  .btn:hover:not(:disabled) {
    background: var(--color-glass-2);
    border-color: var(--color-fg-subtle);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn.danger {
    background: var(--color-danger);
    color: var(--color-fg);
    border-color: var(--color-danger);
  }

  .btn.danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-danger) 88%, black);
    border-color: var(--color-danger);
  }

  .icon-btn {
    width: 32px;
    min-width: 32px;
    padding: 0;
  }

  .native-toggle {
    box-sizing: border-box;
    width: 28px;
    height: 16px;
    margin: 0;
    display: block;
    position: relative;
    flex: 0 0 28px;
    border: 0;
    border-radius: var(--radius-pill);
    background: var(--color-border-strong);
    cursor: pointer;
    appearance: none;
    -webkit-appearance: none;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .native-toggle::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--color-surface);
    box-shadow: 0 1px 2px color-mix(in srgb, black 24%, transparent);
    transition: left var(--motion-fast) var(--motion-easing);
  }

  .native-toggle:checked {
    background: var(--color-accent);
  }

  .native-toggle:checked::after {
    left: 14px;
    background: var(--color-accent-fg);
  }

  .native-toggle:focus-visible {
    outline: 2px solid var(--color-info);
    outline-offset: 2px;
  }

  .slider {
    display: flex;
    align-items: center;
    gap: 13px;
  }

  .slider input[type='range'] {
    width: 166px;
    height: 16px;
    margin: 0;
    display: block;
    border: 0;
    background: transparent;
    appearance: none;
    -webkit-appearance: none;
    outline: none;
    cursor: pointer;
  }

  .slider input[type='range']::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: var(--color-border-strong);
  }

  .slider input[type='range']::-webkit-slider-thumb {
    width: 12px;
    height: 12px;
    margin-top: -4px;
    border: 2px solid var(--color-surface);
    border-radius: 50%;
    background: var(--color-accent);
    box-shadow: 0 0 0 1px var(--color-accent);
    appearance: none;
  }

  .slider input[type='range']::-moz-range-track {
    height: 4px;
    border: 0;
    border-radius: 2px;
    background: var(--color-border-strong);
  }

  .slider input[type='range']::-moz-range-thumb {
    width: 10px;
    height: 10px;
    border: 2px solid var(--color-surface);
    border-radius: 50%;
    background: var(--color-accent);
    box-shadow: 0 0 0 1px var(--color-accent);
  }

  .slider .val {
    min-width: 46px;
    text-align: right;
    font-family: var(--font-mono);
    font-size: var(--text-base);
    color: var(--color-fg-muted);
  }

  .kv-row {
    display: grid;
    grid-template-columns: 118px minmax(0, 1fr);
    gap: 16px;
    align-items: baseline;
    padding: 10px 0;
    border-bottom: 1px solid var(--color-border);
    font-size: var(--text-base);
  }

  .kv-row span {
    color: var(--color-fg-muted);
  }

  .kv-row strong {
    min-width: 0;
    color: var(--color-fg);
    font-weight: 540;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Password set/change form (ADR-0020 D17) ─────────────────────── */

  .pw-row {
    align-items: start;
  }

  .pw-form {
    display: grid;
    gap: var(--space-10);
    margin-top: var(--space-12);
    max-width: 360px;
  }

  .pw-field {
    display: grid;
    gap: var(--space-6);
    min-width: 0;
  }

  .pw-label {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
  }

  .pw-input {
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

  .pw-input:hover:not(:disabled) {
    border-color: var(--color-fg-subtle);
  }

  .pw-input:focus-visible {
    outline: 2px solid var(--color-info);
    outline-offset: 1px;
    border-color: var(--color-info);
  }

  .pw-input:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .pw-field.has-error .pw-input {
    border-color: var(--color-danger);
  }

  .pw-hint {
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
  }

  .pw-hint-error {
    color: var(--color-danger);
  }

  .pw-error {
    margin: 0;
    padding: var(--space-8) var(--space-10);
    border: 1px solid color-mix(in srgb, var(--color-danger) 34%, transparent);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    color: var(--color-danger);
    font-size: var(--text-sm);
  }

  .pw-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-8);
    justify-content: flex-start;
    margin-top: var(--space-4);
  }

  .about-id {
    margin-top: 18px;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 13px 0 0;
  }

  /* Brand-mark — Titlebar/.auth 와 동일 asset (src/lib/assets/brand.png). */
  .about-mark {
    width: 40px;
    height: 40px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    object-fit: cover;
    display: block;
    flex: 0 0 auto;
  }

  .about-desc {
    margin: 0;
    padding: 12px 0 15px;
    border-bottom: 1px solid var(--color-border);
    color: var(--color-fg-muted);
    font-size: var(--text-base);
    line-height: var(--leading-normal);
    max-width: 58ch;
  }

  .about-name {
    font-size: var(--text-lg);
    font-weight: var(--weight-semibold);
  }

  .about-tagline {
    margin-top: 1px;
    color: var(--color-fg);
    font-size: var(--text-base);
  }

  .about-version {
    margin-top: 2px;
    color: var(--color-fg-muted);
    font-size: var(--text-base);
  }

  .info-row {
    display: grid;
    grid-template-columns: 28px 92px minmax(0, 1fr);
    gap: 12px;
    align-items: center;
    padding: 10px 0;
    border-bottom: 1px solid var(--color-border);
    font-size: var(--text-base);
  }

  .info-icon {
    width: 24px;
    height: 24px;
    display: grid;
    place-items: center;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
    color: var(--color-fg-muted);
  }

  .info-key {
    color: var(--color-fg-muted);
  }

  .info-val {
    min-width: 0;
    color: var(--color-fg);
    font-weight: var(--weight-medium);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .danger-row {
    border-bottom: 0;
  }

  .settings-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 16px;
    border-top: 1px solid var(--color-border);
    background: color-mix(in srgb, var(--color-surface-2) 62%, transparent);
  }

  .dstate {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--color-fg-muted);
    font-size: var(--text-base);
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--color-success);
  }

  /* ── Shortcuts ───────────────────────────────────────────────── */
  .shortcut-group {
    margin-bottom: var(--space-16);
  }

  .group-head {
    margin: 0 0 var(--space-6);
    font-size: var(--text-md);
    font-family: var(--font-mono);
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--color-fg-muted);
    font-weight: var(--weight-regular);
  }

  .shortcut-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-md);
  }

  .shortcut-table td {
    padding: var(--space-4) var(--space-8);
    border-bottom: 1px solid var(--color-border);
    vertical-align: middle;
  }

  .shortcut-table tr:last-child td {
    border-bottom: 0;
  }

  .shortcut-table .desc {
    color: var(--color-fg);
  }

  .shortcut-table .desc > span {
    display: block;
  }

  .shortcut-table .desc > span + span {
    margin-top: 2px;
  }

  .shortcut-table .combo {
    text-align: right;
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    white-space: nowrap;
  }

  .shortcut-sub,
  .shortcut-state {
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
  }

  .shortcut-state {
    color: var(--color-accent);
  }

  .shortcut-button,
  .shortcut-capture {
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
    color: var(--color-fg);
    font: inherit;
    line-height: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 3px 8px;
    cursor: pointer;
  }

  .reset-btn {
    box-sizing: border-box;
    width: 24px;
    height: 24px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
    color: var(--color-fg-muted);
    display: inline-grid;
    place-items: center;
    padding: 0;
    line-height: 1;
    cursor: pointer;
  }

  .shortcut-button:hover:not(:disabled),
  .reset-btn:hover:not(:disabled) {
    border-color: var(--color-accent);
    color: var(--color-fg);
  }

  .shortcut-button:disabled,
  .reset-btn:disabled {
    cursor: not-allowed;
    opacity: 0.45;
  }

  .shortcut-capture {
    border-color: var(--color-accent);
    color: var(--color-accent);
  }

  .shortcut-actions {
    width: 76px;
    text-align: right;
  }

  .shortcut-error {
    margin: var(--space-8) 0 0;
    color: var(--color-danger, #d33);
    font-size: var(--text-md);
  }

  .mono {
    font-family: var(--font-mono);
  }

  @media (max-width: 720px) {
    .settings-overlay {
      width: calc(100vw - 20px);
      height: calc(100vh - 20px);
    }

    .settings-body {
      grid-template-columns: 168px 1fr;
    }

    .section-pane {
      padding-inline: 18px;
    }

    .srow {
      grid-template-columns: 1fr;
      gap: 10px;
    }

    .ctl {
      justify-self: start;
    }
  }
</style>
