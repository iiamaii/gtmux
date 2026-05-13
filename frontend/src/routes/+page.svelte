<script lang="ts">
  // 단일 라우트 — Canvas + Sidebar + Toolbar + ReconnectBanner 마운트.
  //
  // Bootstrap 흐름 (D17 + D21 c3):
  // 1. token 획득 — sessionStorage('gtmux_token') 우선. 미존재 시 dev fallback로 prompt()
  //    1회 (명백히 dev-only — 정식 구현은 Sprint 4에서 server-side rendered HTML이
  //    sessionStorage에 토큰 주입). HttpOnly cookie는 JS에서 읽을 수 없어 WS 서브프로토콜
  //    경로(Sec-WebSocket-Protocol)에 토큰을 실을 수 없음 → 따라서 sessionStorage 경로 채택.
  // 2. HTTP GET /api/layout → layoutStore.hydrate (현재 stores가 placeholder라 console.debug
  //    까지만 진행).
  // 3. WS dispatcher 시작 (현재 client/dispatcher가 placeholder라 console.debug까지만).
  //
  // 본 task 범위(FE-2)는 Canvas mount + 단일 panel render 가능성. 실제 hydrate/connect 배선은
  // FE-3 (WS dispatcher) + bootstrap 트랙에서 완성.

  import { onMount } from 'svelte';
  import Canvas from '$lib/canvas/Canvas.svelte';
  import Toolbar from '$lib/toolbar/Toolbar.svelte';
  import Sidebar from '$lib/sidebar/Sidebar.svelte';
  import ReconnectBanner from '$lib/banner/ReconnectBanner.svelte';

  const TOKEN_STORAGE_KEY = 'gtmux_token';

  /**
   * Token 획득 정책:
   * - 정식: server-side rendered HTML이 inline script로 sessionStorage에 주입
   *   (HttpOnly cookie는 JS에서 읽을 수 없으므로 WS Sec-WebSocket-Protocol에 실릴 토큰은
   *    sessionStorage 경로가 정본).
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
      console.debug('sessionStorage unavailable', e);
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
          console.debug('sessionStorage write failed', e);
        }
        return fromPrompt;
      }
    }
    return null;
  }

  onMount(() => {
    const token = acquireToken();
    if (token === null) {
      console.debug('gtmux: no token — bootstrap skipped');
      return;
    }
    // Hydration + WS connect — 본 task 범위 외 (FE-3 트랙에서 fetchLayout / dispatcher.start
    // 가 노출되면 본 onMount에서 호출). 현재는 placeholder 진단 로그만.
    console.debug('gtmux: token acquired, bootstrap pending FE-3');
  });
</script>

<svelte:head>
  <title>gtmux</title>
</svelte:head>

<div class="app">
  <ReconnectBanner />
  <Toolbar />
  <div class="workspace">
    <Sidebar />
    <main class="canvas-pane">
      <Canvas />
    </main>
  </div>
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    padding: 0;
    height: 100%;
    background: #020617;
    color: #e5e7eb;
    font-family:
      ui-sans-serif,
      system-ui,
      -apple-system,
      'Segoe UI',
      Roboto,
      sans-serif;
  }

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

  .workspace {
    flex: 1 1 auto;
    display: flex;
    flex-direction: row;
    min-height: 0;
    min-width: 0;
  }

  .canvas-pane {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    position: relative;
  }
</style>
