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
    getLayout,
    listSessions,
    UnauthorizedError,
  } from '$lib/http/sessions';
  import { reconnectGate } from '$lib/stores/reconnectGate.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { workspaceSwitcher } from '$lib/stores/workspaceSwitcher.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import type {
    AttachResponse,
    SessionInfo,
  } from '$lib/types/sessions';

  // 기존 session 이름 list (NewSessionModal 의 duplicate pre-check 용).
  // list stage 진입 시점 또는 create stage 진입 시점에 lazy fetch.
  let existingNames = $state<readonly string[]>([]);

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
    return `webpage-${Math.random().toString(36).slice(2, 10)}`;
  }

  async function tryAttach(name: string): Promise<void> {
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
      toastStore.show({
        message: `Attached to session "${name}".`,
        tone: 'success',
      });
      workspaceSwitcher.close();
      return;
    }

    if (res.kind === 'confirm_required') {
      // BE Stage 4-C: attach 가 lock 은 잡았지만 unmatched UUID 존재.
      // 사용자가 confirm 누르면 attach/confirm 호출 → spawn → layout fetch.
      sessionStore.setActiveSession({ name });
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
      sessionStore.loadLayout(layout);
      reconnectGate.markSuccess();
      void terminalPool.refresh();
      workspaceSwitcher.close();
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
/>

<NewSessionModal
  open={workspaceSwitcher.stage === 'create'}
  {existingNames}
  onClose={() => workspaceSwitcher.open()}
  onCreated={onSessionCreated}
/>

<SessionListModal
  open={workspaceSwitcher.stage === 'list'}
  onClose={() => workspaceSwitcher.open()}
  onSelect={onSessionPicked}
  onUnauthorized={redirectToAuth}
/>

<AttachConfirmModal
  open={workspaceSwitcher.stage === 'attach_confirm'}
  sessionName={workspaceSwitcher.pendingSession ?? ''}
  summary={workspaceSwitcher.pendingSummary}
  onCancel={() => workspaceSwitcher.goList()}
  onConfirm={onConfirmAttach}
/>
