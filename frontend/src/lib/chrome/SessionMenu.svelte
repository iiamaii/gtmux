<script lang="ts">
  /**
   * Session menu — Titlebar 좌측 kebab dropdown (plan 0005 Stage C, ADR-0017 §D2).
   *
   * Items:
   *   - Session shutdown → ShutdownModal (destructive, danger class)
   *   - Rotate token (P1+ — currently shows toast hint)
   *   - About → simple alert (P1+ proper About modal)
   */

  import Dropdown from '$lib/ui/Dropdown.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';
  import ShutdownModal from './ShutdownModal.svelte';
  import SessionDeleteConfirmModal from './SessionDeleteConfirmModal.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { workspaceSwitcher } from '$lib/stores/workspaceSwitcher.svelte';
  import { settingsDialog } from '$lib/stores/settingsDialog.svelte';
  import { shutdownDialog } from '$lib/stores/shutdownDialog.svelte';
  import { sessionIODialog } from '$lib/stores/sessionIOdialog.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { sessionStorageHint } from '$lib/stores/sessionStorageHint';
  import { reconnectGate } from '$lib/stores/reconnectGate.svelte';
  import { deleteSession, UnauthorizedError } from '$lib/http/sessions';
  import { logout } from '$lib/http/auth';

  interface Props {
    sessionName: string;
  }

  const { sessionName }: Props = $props();

  async function onLogout(): Promise<void> {
    // ADR-0019 D5.4 / plan-0008 §4.5 — sessionStorage hint 제거. logout 은
    // sessionStore.clear() 를 거치지 않고 즉시 page redirect 하므로 명시 clear.
    sessionStorageHint.clear();
    try {
      await logout();
    } catch (e) {
      console.debug('[gtmux] logout failed', e);
    }
    // BE 가 Set-Cookie Max-Age=0 으로 cookie 폐기 — 반드시 reload 로 깨끗한
    // 상태에서 /auth (BE server-rendered) 로 진입.
    window.location.href = '/auth';
  }

  function onRotateToken(): void {
    toastStore.show({
      message: `Run \`gtmux rotate-token --session ${sessionName}\` in the CLI`,
      tone: 'info',
      durationMs: 6_000,
    });
  }

  function onAbout(): void {
    toastStore.show({
      message: 'gtmux — tmux-backed web canvas workspace. ADR-0013 PTY-direct.',
      tone: 'info',
    });
  }

  // ADR-0019 D10.1 (G51 amend) — "Delete current session…" entry point.
  // 현 attached session 만 대상. 승인 후: deleteSession → sessionStore.clear()
  // → reconnectGate.cancel() (sessionStorageHint.clear 포함) → workspaceSwitcher.open().
  let deleteConfirmOpen = $state(false);

  function onRequestDelete(): void {
    if (sessionStore.active === null) return;
    deleteConfirmOpen = true;
  }

  async function onConfirmDelete(): Promise<void> {
    const active = sessionStore.active;
    if (active === null) {
      deleteConfirmOpen = false;
      return;
    }
    const name = active.name;
    deleteConfirmOpen = false;
    try {
      await deleteSession(name);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Delete failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
      return;
    }
    // D5.4 cancel 흐름 + D10 "현 attached 였으면 dialog 회귀" 정합.
    // (reconnectGate.cancel 가 sessionStorageHint.clear 도 호출하지만 ADR-0019
    // D10.1 명세 정합 위해 명시 4-step.)
    sessionStore.clear();
    reconnectGate.cancel();
    sessionStorageHint.clear();
    workspaceSwitcher.open();
    toastStore.show({
      message: `Session "${name}" deleted. Terminals remain in the server pool.`,
      tone: 'success',
    });
  }
</script>

<Dropdown placement="bottom-start">
  {#snippet trigger({ toggle })}
    <IconButton aria-label="Session menu" size="sm" onclick={toggle}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <line x1="3" y1="6" x2="21" y2="6"/>
        <line x1="3" y1="12" x2="21" y2="12"/>
        <line x1="3" y1="18" x2="21" y2="18"/>
      </svg>
    </IconButton>
  {/snippet}
  {#snippet menu({ close })}
    <button
      type="button"
      onclick={() => {
        workspaceSwitcher.open();
        close();
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="3" y="4" width="18" height="16" rx="2"/>
        <line x1="3" y1="10" x2="21" y2="10"/>
        <line x1="9" y1="14" x2="15" y2="14"/>
      </svg>
      <span>Switch workspace session…</span>
    </button>
    <button
      type="button"
      onclick={() => {
        sessionIODialog.openImport();
        close();
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
        <polyline points="17 8 12 3 7 8"/>
        <line x1="12" y1="3" x2="12" y2="15"/>
      </svg>
      <span>Import session…</span>
    </button>
    <button
      type="button"
      disabled={sessionStore.active === null}
      title={sessionStore.active === null ? 'No active session to export' : ''}
      onclick={() => {
        if (sessionStore.active === null) return;
        sessionIODialog.openExport();
        close();
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
        <polyline points="7 10 12 15 17 10"/>
        <line x1="12" y1="15" x2="12" y2="3"/>
      </svg>
      <span>Export session…</span>
    </button>
    <button
      type="button"
      onclick={() => {
        void onLogout();
        close();
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/>
        <polyline points="16 17 21 12 16 7"/>
        <line x1="21" y1="12" x2="9" y2="12"/>
      </svg>
      <span>Sign out</span>
    </button>
    <button
      type="button"
      class="danger"
      disabled={sessionStore.active === null}
      title={sessionStore.active === null ? 'No active session to delete' : ''}
      onclick={() => {
        if (sessionStore.active === null) return;
        onRequestDelete();
        close();
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <polyline points="3 6 5 6 21 6"/>
        <path d="M19 6l-2 14a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2L5 6"/>
        <path d="M10 11v6"/>
        <path d="M14 11v6"/>
        <path d="M9 6V4a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v2"/>
      </svg>
      <span>Delete current session…</span>
    </button>
    <button
      type="button"
      class="danger"
      onclick={() => {
        shutdownDialog.show();
        close();
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M18.36 6.64a9 9 0 1 1-12.73 0"/>
        <line x1="12" y1="2" x2="12" y2="12"/>
      </svg>
      <span>Session shutdown</span>
    </button>
    <button
      type="button"
      onclick={() => {
        onRotateToken();
        close();
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <polyline points="23 4 23 10 17 10"/>
        <polyline points="1 20 1 14 7 14"/>
        <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/>
      </svg>
      <span>Rotate token</span>
    </button>
    <button
      type="button"
      onclick={() => {
        settingsDialog.show();
        close();
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="3"/>
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
      </svg>
      <span>Settings…</span>
    </button>
    <button
      type="button"
      onclick={() => {
        onAbout();
        close();
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="10"/>
        <line x1="12" y1="16" x2="12" y2="12"/>
        <line x1="12" y1="8" x2="12.01" y2="8"/>
      </svg>
      <span>About</span>
    </button>
  {/snippet}
</Dropdown>

<ShutdownModal
  open={shutdownDialog.open}
  {sessionName}
  onclose={() => shutdownDialog.close()}
/>

<SessionDeleteConfirmModal
  open={deleteConfirmOpen}
  sessionName={sessionStore.active?.name ?? ''}
  onCancel={() => (deleteConfirmOpen = false)}
  onConfirm={() => void onConfirmDelete()}
/>
