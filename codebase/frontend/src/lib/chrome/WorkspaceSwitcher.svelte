<script lang="ts">
  /**
   * WorkspaceSwitcher — multi-session modal stack 통합.
   *
   * 정본:
   * - plan-0007 §14 FE-NEW-1 (Session UI)
   * - ADR-0019 D7 (인증 후 Dialog)
   * - ADR-0018 D6 (match-or-spawn confirm)
   * - frontend-handover §6 Stage 2~3
   *
   * Stage 머신:
   *   closed → open() → choice (AuthDialog)
   *     ├── create  (NewSessionModal) → 성공 시 created session 으로 attach 시도
   *     └── list    (SessionListModal) → 사용자 선택 → attach 시도
   *                    └── attach 응답:
   *                        - ok → sessionStore.loadLayout + close
   *                        - confirm_required → attach_confirm (AttachConfirmModal)
   *                        - conflict → toast + list 유지
   *
   * Attach API (`POST /api/sessions/<name>/attach`) 는 BE Stage 3 (BE-NEW-3) —
   * Stage 2 BE 에서 still pending. 404 시 graceful toast.
   *
   * UnauthorizedError → /auth 로 redirect (BE server-rendered).
   */

  import AuthDialog from './AuthDialog.svelte';
  import NewSessionModal from './NewSessionModal.svelte';
  import SessionListModal from './SessionListModal.svelte';
  import AttachConfirmModal from './AttachConfirmModal.svelte';
  import {
    attachConfirm,
    attachSession,
    detachSession,
    getLayout,
    listSessions,
    UnauthorizedError,
  } from '$lib/http/sessions';
  import { reconnectGate } from '$lib/stores/reconnectGate.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { workspaceSwitcher } from '$lib/stores/workspaceSwitcher.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { settingsStore } from '$lib/stores/settings.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { getWebpageId } from '$lib/session/webpageId';
  import type {
    AttachResponse,
    SessionInfo,
  } from '$lib/types/sessions';

  // 기존 session 이름 list (NewSessionModal 의 duplicate pre-check 용).
  // list stage 진입 시점 또는 create stage 진입 시점에 lazy fetch.
  let existingNames = $state<readonly string[]>([]);
  let pendingAttachPreviousSession: string | null = null;
  let pendingAttachHasTentativeLock = false;

  $effect(() => {
    if (
      workspaceSwitcher.stage === 'create' ||
      workspaceSwitcher.stage === 'list'
    ) {
      void refreshNames();
    }
  });

  async function refreshNames(): Promise<void> {
    try {
      const res = await listSessions();
      existingNames = res.sessions.map((s) => s.name);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        redirectToAuth();
        return;
      }
      console.debug('[gtmux] refreshNames failed', err);
    }
  }

  function redirectToAuth(): void {
    workspaceSwitcher.close();
    window.location.href = '/auth';
  }

  /** WS conn id stub — Stage 3 BE-NEW-4 의 WS routing 정합 전까지 placeholder. */
  function makeWsConnId(): string {
    return getWebpageId();
  }

  /**
   * 0077 follow-up — session switch 완료 시 *조건부* full page reload.
   *
   * - 첫 attach (`previous === null`) 는 reload 안 함 — *진입 시점* 이라 의미 없음.
   * - 같은 session 재attach (`previous === next`) 도 reload 안 함 — 의미 0.
   * - `settingsStore.behavior.reload_on_session_switch === false` 면 skip.
   *
   * 호출 사이트: `tryAttach` 의 `kind:'ok'` + `confirmAttach` 의 success path.
   * `sessionStorage` 의 hint 가 이미 set 된 상태라 reload 후 `reconnectGate
   * .start(next)` 가 자동 재진입 (D5.4). webpage_id 는 sessionStorage 유지
   * → 같은 owner_key → BE 의 same-owner idempotent attach 200.
   */
  function maybeReloadOnSwitch(previous: string | null, next: string): void {
    if (previous === null || previous === next) return;
    if (!settingsStore.behavior.reload_on_session_switch) return;
    if (typeof window === 'undefined') return;
    window.location.reload();
  }

  async function tryAttach(name: string): Promise<void> {
    const previousSession = sessionStore.active?.name ?? null;
    let res: AttachResponse;
    try {
      res = await attachSession(name, { ws_conn_id: makeWsConnId() });
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        redirectToAuth();
        return;
      }
      const msg = err instanceof Error ? err.message : String(err);
      if (msg.includes('not found')) {
        toastStore.show({
          message: `Session "${name}" not found on server.`,
          tone: 'error',
        });
        return;
      }
      toastStore.show({
        message: `Attach failed: ${msg}`,
        tone: 'error',
      });
      return;
    }

    if (res.kind === 'ok') {
      sessionStore.setActiveSession({ name });
      sessionStore.loadLayout(res.layout);
      reconnectGate.markSuccess();
      pendingAttachPreviousSession = null;
      pendingAttachHasTentativeLock = false;
      toastStore.show({
        message: `Attached to session "${name}".`,
        tone: 'success',
      });
      workspaceSwitcher.close();
      maybeReloadOnSwitch(previousSession, name);
      return;
    }

    if (res.kind === 'confirm_required') {
      // BE Stage 4-C: attach 가 lock 은 잡았지만 unmatched UUID 존재.
      // 사용자가 confirm 누르면 attach/confirm 호출 → spawn → layout fetch.
      // 아직 layout 이 load 된 workspace 가 아니므로 active session 으로 올리지 않는다.
      // Cancel 시에는 이 tentative lock 을 release 하고, 이전 session 이 있었다면
      // 다시 attach 해 "switch 취소 = 이전 workspace 유지" 의미를 복원한다.
      pendingAttachPreviousSession = previousSession;
      pendingAttachHasTentativeLock = true;
      workspaceSwitcher.goAttachConfirm(name, res.summary);
      return;
    }

    // conflict
    const pidHint =
      res.active_server_pid !== undefined
        ? ` (in use by server-pid ${res.active_server_pid})`
        : '';
    toastStore.show({
      message: `Session "${name}" is already attached elsewhere${pidHint}.`,
      tone: 'warning',
    });
  }

  /**
   * Confirm-attach 흐름 — BE Stage 4-C 의 `POST attach/confirm`.
   * spawn 결과 toast → layout fetch → sessionStore.loadLayout → switcher close.
   */
  async function confirmAttach(name: string): Promise<void> {
    try {
      const result = await attachConfirm(name);
      if (result.failed.length > 0) {
        const summary = result.failed
          .map((f) => `${f.id.slice(0, 8)}: ${f.error}`)
          .join('; ');
        toastStore.show({
          message: `${result.failed.length} terminal(s) failed to spawn: ${summary}`,
          tone: 'error',
          durationMs: 8_000,
        });
        // 그래도 spawn 된 것은 layout 에 있으므로 진행.
      }
      if (result.spawned.length > 0) {
        toastStore.show({
          message: `Spawned ${result.spawned.length} terminal(s) for "${name}".`,
          tone: 'success',
        });
      }
      // Layout fetch — confirm 후엔 모두 match-able 상태.
      const { layout } = await getLayout(name);
      // 0077 follow-up — reset 전에 capture (maybeReloadOnSwitch 의 입력).
      const previousForReload = pendingAttachPreviousSession;
      sessionStore.setActiveSession({ name });
      sessionStore.loadLayout(layout);
      reconnectGate.markSuccess();
      void terminalPool.refresh();
      pendingAttachPreviousSession = null;
      pendingAttachHasTentativeLock = false;
      workspaceSwitcher.close();
      maybeReloadOnSwitch(previousForReload, name);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        redirectToAuth();
        return;
      }
      toastStore.show({
        message: `Confirm-attach failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  async function restorePreviousSession(name: string): Promise<void> {
    const res = await attachSession(name, { ws_conn_id: makeWsConnId() });
    if (res.kind === 'ok') {
      sessionStore.setActiveSession({ name });
      sessionStore.loadLayout(res.layout);
      reconnectGate.markSuccess();
      void terminalPool.refresh();
      return;
    }
    if (res.kind === 'confirm_required') {
      pendingAttachPreviousSession = null;
      pendingAttachHasTentativeLock = true;
      workspaceSwitcher.goAttachConfirm(name, res.summary);
      return;
    }
    const pidHint =
      res.active_server_pid !== undefined
        ? ` (server-pid ${res.active_server_pid})`
        : '';
    throw new Error(`previous session is already attached elsewhere${pidHint}`);
  }

  async function cancelAttachConfirm(): Promise<void> {
    const pending = workspaceSwitcher.pendingSession;
    const previous = pendingAttachPreviousSession;
    const shouldReleasePending =
      pendingAttachHasTentativeLock &&
      pending !== null &&
      sessionStore.active?.name !== pending;

    pendingAttachPreviousSession = null;
    pendingAttachHasTentativeLock = false;

    try {
      if (shouldReleasePending) {
        await detachSession(pending);
      }
      if (previous !== null && previous !== pending) {
        await restorePreviousSession(previous);
        if (workspaceSwitcher.stage === 'attach_confirm') return;
      } else if (sessionStore.active === null) {
        sessionStore.clear();
      }
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        redirectToAuth();
        return;
      }
      sessionStore.clear();
      toastStore.show({
        message: `Attach cancelled, but previous session could not be restored: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'warning',
        durationMs: 8_000,
      });
    }

    workspaceSwitcher.goList();
  }

  function onSessionCreated(session: SessionInfo): void {
    toastStore.show({
      message: `Created session "${session.name}".`,
      tone: 'success',
    });
    void tryAttach(session.name);
  }

  function onSessionPicked(name: string): void {
    void tryAttach(name);
  }

  function onConfirmAttach(): void {
    const name = workspaceSwitcher.pendingSession;
    if (name === null) return;
    void confirmAttach(name);
  }
</script>

<AuthDialog
  open={workspaceSwitcher.stage === 'choice'}
  onCreate={() => workspaceSwitcher.goCreate()}
  onSelect={() => workspaceSwitcher.goList()}
  onClose={() => workspaceSwitcher.close()}
  dismissable={sessionStore.active !== null}
/>

<NewSessionModal
  open={workspaceSwitcher.stage === 'create'}
  {existingNames}
  onClose={() => workspaceSwitcher.open()}
  onCreated={onSessionCreated}
/>

<SessionListModal
  open={workspaceSwitcher.stage === 'list'}
  onClose={() => workspaceSwitcher.closeList()}
  onSelect={onSessionPicked}
  onUnauthorized={redirectToAuth}
/>

<AttachConfirmModal
  open={workspaceSwitcher.stage === 'attach_confirm'}
  sessionName={workspaceSwitcher.pendingSession ?? ''}
  summary={workspaceSwitcher.pendingSummary}
  onCancel={() => void cancelAttachConfirm()}
  onConfirm={onConfirmAttach}
/>
