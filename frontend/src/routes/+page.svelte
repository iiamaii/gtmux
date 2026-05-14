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
  import Sidebar from '$lib/sidebar/Sidebar.svelte';
  import PaneInfoPanel from '$lib/chrome/PaneInfoPanel.svelte';
  import RailToggle from '$lib/chrome/RailToggle.svelte';
  import HelpBar from '$lib/chrome/HelpBar.svelte';
  import ViewportCtrl from '$lib/chrome/ViewportCtrl.svelte';
  import ContextMenu from '$lib/chrome/ContextMenu.svelte';
  import ReconnectBanner from '$lib/banner/ReconnectBanner.svelte';
  import Toast from '$lib/ui/Toast.svelte';
  import { createDispatcher, setLayoutRefetchHandler } from '$lib/ws/dispatcher.svelte';
  import { createLayoutRefetchHandler, fetchLayoutAndHydrate } from '$lib/http/layout';
  import { layoutStore } from '$lib/stores/layout.svelte';
  import { themeStore } from '$lib/stores/theme.svelte';
  import { chromeStore } from '$lib/stores/chrome.svelte';
  import type { WsClient } from '$lib/ws/client';

  const TOKEN_STORAGE_KEY = 'gtmux_token';

  /**
   * Token 획득 정책:
   * - 정식: server-side rendered HTML 이 inline script 로 sessionStorage 에 주입
   *   (HttpOnly cookie 는 JS 에서 읽을 수 없으므로 WS Sec-WebSocket-Protocol 에 실릴
   *    토큰은 sessionStorage 경로가 정본).
   * - Dev fallback: prompt(). 사용자 환경에서만 출력 — 정식 구현은 Sprint 4.
   */
  function acquireToken(): string | null {
    try {
      const fromStorage = sessionStorage.getItem(TOKEN_STORAGE_KEY);
      if (fromStorage !== null && fromStorage.length > 0) {
        return fromStorage;
      }
    } catch (e) {
      // sessionStorage 접근 실패 (private browsing 등) — fallback 시도.
      console.debug('[gtmux] sessionStorage unavailable', e);
    }
    // Dev-only fallback. 정식 환경에서는 절대 reach 하지 않음.
    if (typeof window !== 'undefined' && typeof window.prompt === 'function') {
      const fromPrompt = window.prompt(
        'gtmux dev token (set sessionStorage.gtmux_token to skip):'
      );
      if (fromPrompt !== null && fromPrompt.length > 0) {
        try {
          sessionStorage.setItem(TOKEN_STORAGE_KEY, fromPrompt);
        } catch (e) {
          console.debug('[gtmux] sessionStorage write failed', e);
        }
        return fromPrompt;
      }
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
  let contextMenuRef: { openAt: (args: { clientX: number; clientY: number; paneId?: string | null; panelId?: string | null }) => void } | undefined = $state();

  onMount(() => {
    // Theme — re-apply on mount so the in-memory state and <html class>
    // converge after Svelte hydrates. The inline FOUC-guard in index.html
    // already set the class for first paint; this is the idempotent
    // follow-up that also handles the (rare) case where localStorage
    // changed in another tab between page-load and hydration.
    themeStore.apply();

    const token = acquireToken();
    if (token === null) {
      console.debug('[gtmux] no token — bootstrap skipped');
      return;
    }

    // WS dispatcher — onMessage / onStateChange / onClose 는 dispatcher 기본 어댑터.
    const client = createDispatcher({ token });
    wsClientHolder.current = client;

    // Pull-through-notify hookup — 0x80 수신 시 GET /api/layout 발급.
    const refetch = createLayoutRefetchHandler(token);
    setLayoutRefetchHandler(refetch);

    // 초기 hydrate — WS connect 와 병렬. 실패해도 banner 가 사용자에게 안내.
    void fetchLayoutAndHydrate(token, layoutStore.etag);

    client.start();
  });

  onDestroy(() => {
    const client = wsClientHolder.current;
    if (client) {
      client.stop();
      wsClientHolder.current = null;
    }
    setLayoutRefetchHandler(null);
  });
</script>

<svelte:head>
  <title>gtmux</title>
</svelte:head>

<div class="app">
  <ReconnectBanner />
  <Titlebar />
  <SvelteFlowProvider>
    <div class="workspace">
      <main class="canvas-pane">
        <Canvas onContextMenuRequest={(args) => contextMenuRef?.openAt(args)} />
      </main>
      <Sidebar collapsed={chromeStore.state.sidebarCollapsed} />
      <PaneInfoPanel collapsed={chromeStore.state.paneInfoCollapsed} />
      <RailToggle
        side="left"
        collapsed={chromeStore.state.sidebarCollapsed}
        onclick={() => chromeStore.toggleSidebar()}
        aria-label="Toggle layer panel"
      />
      <RailToggle
        side="right"
        collapsed={chromeStore.state.paneInfoCollapsed}
        onclick={() => chromeStore.togglePaneInfo()}
        aria-label="Toggle pane info panel"
      />
      <HelpBar />
      <ViewportCtrl />
      <ContextMenu bind:this={contextMenuRef} />
    </div>
  </SvelteFlowProvider>
</div>
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
   * area entirely; Sidebar / PaneInfoPanel / RailToggle ×2 float on top
   * with their own absolute positions. This matches ref/frontend-design
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
    .workspace :global(.pane-info:not(.collapsed)) {
      transform: translateX(calc(var(--layout-sidebar-right-w) + var(--space-12)));
      opacity: 0;
      pointer-events: none;
    }
  }

  @media (max-width: 700px) {
    .workspace :global(.sidebar:not(.collapsed)) {
      transform: translateX(calc(-1 * (var(--layout-sidebar-w) + var(--space-12))));
      opacity: 0;
      pointer-events: none;
    }
  }
</style>
