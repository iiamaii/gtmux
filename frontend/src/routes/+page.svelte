<script lang="ts">
  // 단일 라우트 — Canvas + Sidebar + Toolbar + ReconnectBanner 마운트 + WS bootstrap.
  //
  // Bootstrap 흐름 (post-0044 dual-source 제거):
  // 1. `?t=<token>` 가 있으면 cookie 발급. sessionStorage 에도 Bearer 보관
  //    (legacy WS subprotocol 호환 — BE Stage 5-A cookie-only handshake land 후 폐기).
  // 2. Auth gate — GET /api/sessions 로 cookie 검증, 401 시 /auth.
  // 3. Session attach — hint 있으면 reconnectGate.start, 없으면 workspaceSwitcher.
  // 4. WsClient 단일 인스턴스 생성 + setContext('wsClient'). cookie 만으로 upgrade.
  // 5. onDestroy: wsClient.stop().

  import { setContext, onDestroy, onMount } from 'svelte';
  import { SvelteFlowProvider } from '@xyflow/svelte';
  import Canvas from '$lib/canvas/Canvas.svelte';
  import Titlebar from '$lib/chrome/Titlebar.svelte';
  import LeftPanel from '$lib/sidebar/LeftPanel.svelte';
  import RightPanel from '$lib/chrome/RightPanel.svelte';
  import ViewportCtrl from '$lib/chrome/ViewportCtrl.svelte';
  import ContextMenu from '$lib/chrome/ContextMenu.svelte';
  import ChangeTerminalModal from '$lib/chrome/ChangeTerminalModal.svelte';
  import GroupCloseConfirmModal from '$lib/chrome/GroupCloseConfirmModal.svelte';
  import SettingsOverlay from '$lib/chrome/SettingsOverlay.svelte';
  import WorkspaceSwitcher from '$lib/chrome/WorkspaceSwitcher.svelte';
  import ReconnectModal from '$lib/chrome/ReconnectModal.svelte';
  import Toolbar2 from '$lib/toolbar/Toolbar2.svelte';
  import ReconnectBanner from '$lib/banner/ReconnectBanner.svelte';
  import Toast from '$lib/ui/Toast.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { createDispatcher } from '$lib/ws/dispatcher.svelte';
  import { heartbeatStore } from '$lib/ws/heartbeat.svelte';
  import { login } from '$lib/http/auth';
  import { bindZShortcuts } from '$lib/keyboard/zShortcuts.svelte';
  import { bindChromeShortcuts } from '$lib/keyboard/chromeShortcuts.svelte';
  import { themeStore } from '$lib/stores/theme.svelte';
  import { workspaceSwitcher } from '$lib/stores/workspaceSwitcher.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { sessionStorageHint } from '$lib/stores/sessionStorageHint';
  import { reconnectGate } from '$lib/stores/reconnectGate.svelte';
  import type { WsClient } from '$lib/ws/client';

  const TOKEN_STORAGE_KEY = 'gtmux_token';

  /**
   * Token 획득 정책 (Stage 2 cookie auth amend):
   * - Stage 2 cookie 인증 (ADR-0020) 이후로 *Bearer token 은 선택* — HTTP `/api/*`
   *   는 `credentials: 'include'` 의 `gtmux_auth` cookie 로 인증. Bearer token 은
   *   *legacy WS 핸드셰이크* (ws-server 가 아직 옛 subprotocol 사용) 시에만 필요.
   *   BE Stage 5-A cookie-only handshake 가 land 하면 본 토큰 경로도 폐기.
   */
  function acquireToken(): string | null {
    try {
      const fromStorage = sessionStorage.getItem(TOKEN_STORAGE_KEY);
      if (fromStorage !== null && fromStorage.length > 0) {
        return fromStorage;
      }
    } catch (e) {
      console.debug('[gtmux] sessionStorage unavailable', e);
    }
    return null;
  }

  // wsClient slot — XtermHost 등이 getContext 로 꺼낼 단일 인스턴스. token 획득
  // 후에야 채워지므로 holder 객체를 setContext 에 등록해 *간접 참조* 로 후속 갱신을
  // 자식이 볼 수 있게 한다. Svelte 5 의 setContext 는 컴포넌트 init 시점에 단 한 번
  // 호출되어야 하므로, holder 객체를 미리 등록하고 onMount 에서 `.current` 만 교체.
  //
  // 자식 (XtermHost) 측 계약: getContext<WsClientHolder>('wsClient')?.current.
  interface WsClientHolder { current: WsClient | null }
  const wsClientHolder: WsClientHolder = { current: null };
  setContext<WsClientHolder>('wsClient', wsClientHolder);

  // ContextMenu host — Canvas dispatches openAt(...) via this ref.
  // The same ref is exposed via setContext('contextMenu') so deep
  // descendants (PanelNode's header (…) button) can summon the menu
  // without a callback prop chain through SvelteFlow node data.
  interface ContextMenuHolder {
    openAt: (args: { clientX: number; clientY: number; paneId?: string | null; panelId?: string | null }) => void;
  }
  let contextMenuRef: ContextMenuHolder | undefined = $state();
  const contextMenuHolder = {
    openAt: (args: { clientX: number; clientY: number; paneId?: string | null; panelId?: string | null }) => {
      contextMenuRef?.openAt(args);
    },
  };
  setContext<ContextMenuHolder>('contextMenu', contextMenuHolder);

  /**
   * `?t=<token>` 진입 시 cookie 발급. 0036 §2 (P0-A) 정합.
   *
   * 이전 코드: token 을 sessionStorage 에 저장만 하고 URL clean → 이후 auth gate
   * 의 GET /api/sessions 가 cookie 없으니 401 → /auth redirect 루프. magic-link
   * 진입이 실질적으로 실패하던 버그.
   *
   * 수정 후: URL 의 `?t=` 가 있으면 *bootstrap 의 첫 단계* 로 `POST /auth/login
   * {token, redirect:false}` 호출 → BE 가 cookie 발급 → URL clean 은 *성공 후*.
   * 실패 (invalid/rate_limited/unavailable) 시 token 을 보존한 채 BE-rendered
   * `/auth/bootstrap` 로 위임 — 사용자가 password 또는 다른 token 으로 복구 가능.
   *
   * sessionStorage 의 Bearer 는 legacy WS subprotocol 용으로 유지 (BE Stage 5-A
   * 의 cookie-only handshake 가 land 하면 폐기 — 0035 §3.3 의 (α)/(β)/(γ) trajectory).
   */
  async function exchangeTokenForCookie(token: string): Promise<boolean> {
    try {
      // `redirect` omitted — BE 가 응답 body 에 redirect URL 을 echo 하더라도
      // FE 는 그 값을 무시 (root SPA 가 자체 라우팅). 핵심은 Set-Cookie 수신.
      const res = await login({ token });
      return res.kind === 'ok';
    } catch (e) {
      console.warn('[gtmux] /auth/login exchange threw', e);
      return false;
    }
  }

  function stripTokenFromUrl(): void {
    try {
      const params = new URLSearchParams(window.location.search);
      if (!params.has('t')) return;
      params.delete('t');
      const qs = params.toString();
      const cleanUrl =
        window.location.pathname + (qs.length > 0 ? `?${qs}` : '');
      window.history.replaceState({}, '', cleanUrl);
    } catch (e) {
      console.debug('[gtmux] URL clean failed', e);
    }
  }

  let unbindZShortcuts: (() => void) | null = null;
  let unbindChromeShortcuts: (() => void) | null = null;
  let unbindSystemTheme: (() => void) | null = null;
  let unbindVisibility: (() => void) | null = null;

  /**
   * Phase 2 (plan-0008 §6, Case II) — tab 이 background 에 있다가 다시 활성화
   * 되거나 사용자가 idle 후 첫 입력 시 silent reattach 시도. 결과 fail 이면
   * mutation guard 가 차후 mutation 진입 차단 + ReconnectModal 노출.
   *
   * Trigger 조건:
   *   - document.visibilityState === 'visible' 로 전환
   *   - heartbeat 의 isIdle (15s+ user idle) 가 true
   *   - reconnectGate.canMountApp (idle/success) 진행 중
   *   - sessionStore.active 가 있고 already in-flight 아니면
   */
  function maybeSilentReattach(): void {
    if (typeof document === 'undefined') return;
    if (document.visibilityState !== 'visible') return;
    if (!reconnectGate.canMountApp) return;
    const active = sessionStore.active;
    if (active === null) return;
    if (sessionStore.reattachInProgress) return;
    // 사용자가 활성 / 막 입력했으면 굳이 reattach 안 해도 됨 (server frame 이
    // 곧 흐를 가능성). isIdle 일 때만 — Phase 2 의 Case II 정의.
    if (!heartbeatStore.isIdle) return;
    void sessionStore.silentReattach(active.name).then((result) => {
      if (result.kind === 'success') {
        heartbeatStore.reset();
        return;
      }
      // Phase 2 fail — toast 로 silent 안내 + 사용자 명시 분기 (ReconnectModal
      // 까지는 escalation 안 함, Case II 의 무거운 modal 회피).
      if (result.kind === 'unauthorized') {
        window.location.href = '/auth';
        return;
      }
      const message =
        result.kind === 'in_use'
          ? `Session "${active.name}" is in use by another webpage.`
          : result.kind === 'not_found'
            ? `Session "${active.name}" no longer exists on the server.`
            : `Reconnect failed: ${result.message}`;
      toastStore.show({
        message,
        tone: result.kind === 'unreachable' ? 'warning' : 'error',
        durationMs: 8_000,
      });
    });
  }

  onMount(() => {
    // Theme — re-apply on mount so the in-memory state and <html class>
    // converge after Svelte hydrates. The inline FOUC-guard in index.html
    // already set the class for first paint; this is the idempotent
    // follow-up that also handles the (rare) case where localStorage
    // changed in another tab between page-load and hydration.
    themeStore.apply();
    // OS preference listener — keeps `system` mode in sync when the OS
    // flips between light/dark while the app is open.
    unbindSystemTheme = themeStore.bindSystemListener();
    // WS heartbeat watchdog (ADR-0021 D6) — activity / frame timestamp 추적.
    // Phase 2 silent reattach 와 stale detection 의 입력.
    heartbeatStore.start();

    // Phase 2 (plan-0008 §6) — visibility transition listener.
    const onVisibility = () => maybeSilentReattach();
    document.addEventListener('visibilitychange', onVisibility);
    unbindVisibility = () => {
      document.removeEventListener('visibilitychange', onVisibility);
    };

    // Z-index keyboard shortcuts ([/]/⇧[/⇧]). M.size === 1 일 때만, editable
    // focus 제외. ADR-0024 D2 + 0033 §8.2. Routed through shortcutRegistry.
    unbindZShortcuts = bindZShortcuts();
    // Chrome shortcuts (Cmd+Shift+L / Cmd+Shift+I) — frontend-handover-v2 G26 P1.
    unbindChromeShortcuts = bindChromeShortcuts();

    // Bootstrap pipeline — token cookie 교환 → auth gate → WS subscriptions.
    // 순차 실행이 중요: cookie 가 발급되기 전에 /api/sessions 를 부르면 401 이
    // 떨어져 사용자가 /auth 로 튕긴다 (0036 §2 의 root cause).
    void (async () => {
      // Step 1 — `?t=<token>` 가 있으면 cookie 발급.
      let tokenFromUrl: string | null = null;
      try {
        const params = new URLSearchParams(window.location.search);
        tokenFromUrl = params.get('t');
      } catch (e) {
        console.debug('[gtmux] ?t capture failed', e);
      }

      if (tokenFromUrl !== null && tokenFromUrl.length > 0) {
        // legacy WS subprotocol 용 Bearer 저장 — login 성공/실패와 무관히 보존
        // (실패 시 사용자가 /auth 로 갔다가 돌아오면 그 때 재사용).
        try {
          sessionStorage.setItem(TOKEN_STORAGE_KEY, tokenFromUrl);
        } catch (e) {
          console.debug('[gtmux] sessionStorage write failed', e);
        }
        const ok = await exchangeTokenForCookie(tokenFromUrl);
        if (ok) {
          stripTokenFromUrl();
        } else {
          // BE-rendered /auth/bootstrap 가 token 재시도 / password fallback 을
          // 일관 처리. URL 의 token 은 유지 (사용자 복구 경로 보존).
          window.location.href =
            `/auth/bootstrap?token=${encodeURIComponent(tokenFromUrl)}`;
          return;
        }
      }

      // Step 2 — Auth gate. cookie 가 있어야 200 통과.
      //
      // 본 블록의 모든 비-redirect 종료 경로에서 reconnectGate.state 가 'booting'
      // 을 벗어나도록 보장 — 그렇지 않으면 canMountApp=false 로 boot-screen 영구.
      // 5s timeout — BE 느리거나 hang 시 fallback markIdle (사용자가 SessionMenu
      // 또는 모달로 자체 복구 가능).
      try {
        const controller = new AbortController();
        const timeoutId = setTimeout(() => controller.abort(), 5_000);
        let res: Response;
        try {
          res = await fetch('/api/sessions', {
            method: 'GET',
            credentials: 'include',
            headers: { Accept: 'application/json' },
            signal: controller.signal,
          });
        } finally {
          clearTimeout(timeoutId);
        }
        if (res.status === 401) {
          window.location.href = '/auth';
          return;
        }
        if (res.ok && sessionStore.active === null) {
          // Step 2.5 — sessionStorage hint 검사 (ADR-0019 D5.4, plan-0008 §4.6).
          // hint 있음 = 직전 attach 한 session 이름 — 자동 reattach 시도 +
          //   ReconnectModal 로 본 화면 mount 차단.
          // hint 없음 = fresh tab / 명시 cancel 후 — 기존 workspaceSwitcher 흐름.
          const hint = sessionStorageHint.get();
          if (hint !== null) {
            void reconnectGate.start(hint);
          } else {
            reconnectGate.markIdle();
            workspaceSwitcher.open();
          }
        } else {
          // res 가 200 이지만 sessionStore.active !== null (defensive — 첫 mount
          // 에서는 발생 X), 또는 200 외 (e.g. 500/503). 어느 쪽이든 booting 에서
          // 벗어나야 하고, fresh attach UI 노출이 안전한 default.
          reconnectGate.markIdle();
          if (sessionStore.active === null) workspaceSwitcher.open();
          if (!res.ok) {
            toastStore.show({
              message: `Auth gate returned HTTP ${res.status}. Try again or sign in.`,
              tone: 'warning',
              durationMs: 6_000,
            });
          }
        }
      } catch (e) {
        const aborted = e instanceof DOMException && e.name === 'AbortError';
        console.warn('[gtmux] auth ping failed', e);
        reconnectGate.markIdle();
        if (sessionStore.active === null) workspaceSwitcher.open();
        toastStore.show({
          message: aborted
            ? 'Auth gate timed out. Use the menu to pick a session.'
            : 'Auth ping failed — network or server unreachable.',
          tone: 'warning',
          durationMs: 6_000,
        });
      }

      // Step 3 — WS bootstrap. Cookie-additive auth (0035 §3.3 α, BE 의 D10 α)
      // 가 land 되어 있으므로 Bearer token 부재 시에도 WS 가 cookie 만으로 upgrade.
      // WS 가 열려야 0x88 TERMINAL_SPAWNED catch-up 으로 UUID↔PaneId binding 복원,
      // 페이지 닫힘 시 disconnect_sink, PANE_OUT / PANE_IN streaming 동작.
      const token = acquireToken();
      const client = createDispatcher({ token });
      wsClientHolder.current = client;
      client.start();
    })();
  });

  onDestroy(() => {
    heartbeatStore.stop();
    if (unbindVisibility !== null) {
      unbindVisibility();
      unbindVisibility = null;
    }
    const client = wsClientHolder.current;
    if (client) {
      client.stop();
      wsClientHolder.current = null;
    }
    if (unbindZShortcuts !== null) {
      unbindZShortcuts();
      unbindZShortcuts = null;
    }
    if (unbindChromeShortcuts !== null) {
      unbindChromeShortcuts();
      unbindChromeShortcuts = null;
    }
    if (unbindSystemTheme !== null) {
      unbindSystemTheme();
      unbindSystemTheme = null;
    }
  });
</script>

<svelte:head>
  <title>gtmux</title>
</svelte:head>

<div class="app">
  <ReconnectBanner />
  {#if reconnectGate.canMountApp}
    <!-- ADR-0019 D5.4 / plan-0008 §4.6 — ReconnectModal 활성 동안에는 본 화면
         mount 차단. canMountApp = state ∈ {idle, success}. -->
    <Titlebar />
    <Toolbar2 />
    <SvelteFlowProvider>
      <div class="workspace">
        <main class="canvas-pane">
          <Canvas onContextMenuRequest={(args) => contextMenuRef?.openAt(args)} />
        </main>
        <LeftPanel />
        <RightPanel />
        <ViewportCtrl />
        <ContextMenu bind:this={contextMenuRef} />
      </div>
    </SvelteFlowProvider>
  {:else if reconnectGate.state === 'booting'}
    <div class="boot-screen" role="status" aria-live="polite">
      <span class="boot-spinner" aria-hidden="true"></span>
      <span>Preparing workspace…</span>
    </div>
  {/if}
</div>
<WorkspaceSwitcher />
<ChangeTerminalModal />
<GroupCloseConfirmModal />
<SettingsOverlay />
{#if reconnectGate.modalState !== null}
  <ReconnectModal
    mode={reconnectGate.modalState}
    name={reconnectGate.attemptName ?? ''}
    attempt={reconnectGate.attempt}
    error={reconnectGate.error}
    onSwitchSession={() => {
      reconnectGate.cancel();
      workspaceSwitcher.open();
    }}
    onRetry={() => void reconnectGate.retry()}
  />
{/if}
<Toast />

<style>
  /* html/body/font 는 styles/global.css 가 token 기반으로 단일 정본.
   * 본 파일은 layout-level 그리드만 담당. */
  :global(#app) {
    height: 100%;
  }

  .app {
    display: flex;
    flex-direction: column;
    width: 100vw;
    height: 100vh;
    overflow: hidden;
  }

  .boot-screen {
    flex: 1 1 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-12);
    color: var(--color-fg-muted);
    background: var(--canvas-bg);
    font-size: var(--text-sm);
  }

  .boot-spinner {
    width: 16px;
    height: 16px;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: boot-spin 0.7s linear infinite;
  }

  @keyframes boot-spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* Stage E — workspace is an `absolute` overlay host. Canvas fills the
   * area entirely; LeftPanel / PaneInfoPanel float on top with their
   * own absolute positions. LeftPanel hosts the Layers/Terminals tab
   * pair (ADR-0017 §D2 amend 2026-05-16). PaneInfoPanel keeps its
   * RailToggle on the right edge. This matches ref/frontend-design
   * §1.3 and ADR-0017 §D1. */
  .workspace {
    flex: 1 1 auto;
    position: relative;
    min-height: 0;
    min-width: 0;
    overflow: hidden;
  }

  .canvas-pane {
    position: absolute;
    inset: 0;
    min-width: 0;
    min-height: 0;
  }

  /* Responsive — narrow viewports auto-collapse the floating panels.
   * The user can still toggle via RailToggle (which now rides the
   * viewport edge). */
  @media (max-width: 900px) {
    .workspace :global(.right-panel) {
      transform: translateX(calc(var(--layout-sidebar-right-w) + var(--space-12)));
      opacity: 0;
      pointer-events: none;
    }
  }

  @media (max-width: 700px) {
    .workspace :global(.left-panel) {
      transform: translateX(calc(-1 * (var(--layout-sidebar-w) + var(--space-12))));
      opacity: 0;
      pointer-events: none;
    }
  }
</style>
