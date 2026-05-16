<script lang="ts">
  // 단일 라우트 — Canvas + Sidebar + Toolbar + ReconnectBanner 마운트 + WS bootstrap.
  //
  // Bootstrap 흐름 (D17 + D21 c3):
  // 1. token 획득 — sessionStorage('gtmux_token') 우선. 미존재 시 dev fallback 로
  //    prompt() 1회 (명백히 dev-only — 정식 구현은 server-side rendered HTML 이
  //    sessionStorage 에 토큰 주입). HttpOnly cookie 는 JS 에서 읽을 수 없어 WS
  //    서브프로토콜 경로(Sec-WebSocket-Protocol)에 토큰을 실을 수 없음 → 따라서
  //    sessionStorage 경로 채택.
  // 2. WsClient 단일 인스턴스 생성 (createDispatcher) + setContext('wsClient', …)
  //    로 sub-tree (XtermHost 등) 가 getContext 로 꺼내 사용.
  // 3. setLayoutRefetchHandler 등록 — 0x80 LAYOUT_CHANGED 수신 시 dispatcher 가
  //    HTTP GET /api/layout 을 트리거. (Pull-through-notify.)
  // 4. wsClient.start() → connecting → open. 동시에 GET /api/layout 으로 초기
  //    hydrate (etag/panels/groups).
  // 5. onDestroy: wsClient.stop() + setLayoutRefetchHandler(null) 로 cleanup.

  import { setContext, onDestroy, onMount } from 'svelte';
  import { SvelteFlowProvider } from '@xyflow/svelte';
  import Canvas from '$lib/canvas/Canvas.svelte';
  import Titlebar from '$lib/chrome/Titlebar.svelte';
  import LeftPanel from '$lib/sidebar/LeftPanel.svelte';
  import RightPanel from '$lib/chrome/RightPanel.svelte';
  import HelpBar from '$lib/chrome/HelpBar.svelte';
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
  import { createDispatcher, setLayoutRefetchHandler, setAutoMountHandler } from '$lib/ws/dispatcher.svelte';
  import { appendPanelIfMissing, createLayoutRefetchHandler, fetchLayoutAndHydrate } from '$lib/http/layout';
  import { login } from '$lib/http/auth';
  import { bindZShortcuts } from '$lib/keyboard/zShortcuts.svelte';
  import { bindChromeShortcuts } from '$lib/keyboard/chromeShortcuts.svelte';
  import { layoutStore } from '$lib/stores/layout.svelte';
  import { themeStore } from '$lib/stores/theme.svelte';
  import { chromeStore } from '$lib/stores/chrome.svelte';
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
   *   BE-NEW-4 (Stage 3 WS routing) 가 cookie 검증 통합하면 본 토큰 경로도 폐기.
   * - 본 함수는 *sessionStorage 에 있으면 반환, 없으면 null*. native prompt 폐기 —
   *   `/auth/bootstrap?token=…` 흐름으로 들어오면 cookie 가 이미 발행되어 있고,
   *   prompt 가 그 위에 또 뜨면 사용자 혼란. null 반환 시 onMount 가 WS bootstrap
   *   skip 하고 HTTP 경로 (cookie + 모달 stack) 만 활성.
   */
  function acquireToken(): string | null {
    try {
      const fromStorage = sessionStorage.getItem(TOKEN_STORAGE_KEY);
      if (fromStorage !== null && fromStorage.length > 0) {
        return fromStorage;
      }
    } catch (e) {
      // sessionStorage 접근 실패 (private browsing 등) — silent fall-through.
      console.debug('[gtmux] sessionStorage unavailable', e);
    }
    console.info(
      '[gtmux] No Bearer token in sessionStorage. Cookie auth handles ' +
        '/api/*. Legacy WS streaming (Bearer subprotocol) will not start ' +
        'until Stage 3 BE migrates the WS handshake to cookie.',
    );
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
      try {
        const res = await fetch('/api/sessions', {
          method: 'GET',
          credentials: 'include',
          headers: { Accept: 'application/json' },
        });
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
            workspaceSwitcher.open();
          }
        }
      } catch (e) {
        console.warn('[gtmux] auth ping failed', e);
      }

      // Step 3 — WS bootstrap. Cookie-additive auth (0035 §3.3 α, BE 의 D10 α)
      // 가 land 되어 있으므로 Bearer token 부재 시에도 WS 가 cookie 만으로 upgrade.
      // 본 WS 가 열려야:
      //   - 0x88 TERMINAL_SPAWNED catch-up 으로 UUID↔PaneId binding 복원
      //     (0040 §5 옵션 A). XtermHost 가 "connecting" placeholder 에서 벗어남.
      //   - 페이지 닫힘 시 disconnect_sink 가 cookie release_lock_for_cookie 트리거
      //     → session 의 active 가 자동 false.
      //   - PANE_OUT / PANE_IN streaming.
      // legacy v1 layout PUT/GET 의 refetch handler 와 auto-mount 은 Bearer token
      // 이 있을 때만 wire — multi-session 사용자는 token 없어도 정상 동작.
      const token = acquireToken();
      const client = createDispatcher({ token });
      wsClientHolder.current = client;

      if (token !== null) {
        const refetch = createLayoutRefetchHandler(token);
        setLayoutRefetchHandler(refetch);

        // 0036 §5 (P1-D) — legacy auto-mount 은 sessionStore.active 가 *없을 때만*.
        // multi-session active 동안 backend 의 server-wide `pane-spawned` notify
        // 가 v1 panelsStore 를 오염시키지 않도록 격리. Stage 5-D 의 MOUNT_CASCADE
        // 가 정식 경로.
        setAutoMountHandler(async (paneId) => {
          if (sessionStore.active !== null) {
            console.debug(
              '[gtmux] legacy pane-spawned auto-mount skipped — multi-session active',
            );
            return;
          }
          await appendPanelIfMissing(paneId, { token });
        });

        void fetchLayoutAndHydrate(token, layoutStore.etag);
      } else {
        console.debug(
          '[gtmux] cookie-only WS bootstrap — legacy v1 layout PUT/GET disabled',
        );
      }

      client.start();
    })();
  });

  onDestroy(() => {
    const client = wsClientHolder.current;
    if (client) {
      client.stop();
      wsClientHolder.current = null;
    }
    setLayoutRefetchHandler(null);
    setAutoMountHandler(null);
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
        <HelpBar />
        <ViewportCtrl />
        <ContextMenu bind:this={contextMenuRef} />
      </div>
    </SvelteFlowProvider>
  {/if}
</div>
<WorkspaceSwitcher />
<ChangeTerminalModal />
<GroupCloseConfirmModal />
<SettingsOverlay />
{#if reconnectGate.state !== 'idle' && reconnectGate.state !== 'success'}
  <ReconnectModal
    mode={reconnectGate.state}
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
