<script lang="ts">
  // xterm.js hand-rolled wrapper — R8 §F1 (ADR-0012 D4·O1).
  //
  // Lifecycle (R8 F1 + R2 F11):
  // 1. $effect mount: new Terminal(SECURE_XTERM_OPTIONS) + loadAddon(FitAddon) +
  //    loadAddon(Unicode11Addon) + unicode.activeVersion='11' + term.open(containerEl)
  //    + fitAddon.fit() (1회) + WS dispatcher 에 PaneId 등록.
  // 2. 사용자 키 입력: term.onData → WS PANE_IN (0x03) 송신 (ADR-0004 D4 — UTF-8
  //    바이트 그대로, 디코드/이스케이프 없음). WsClient 가 connected 가 아니면
  //    silently drop — 별도 buffering 없음 (D8 reconnect-then-resync 정합).
  // 3. 컨테이너 ResizeObserver → debounce 150ms (R2 F8 — fit() 폭주 방지) → fit() +
  //    fit() 결과 (cols, rows) 가 직전 송신값과 다르면 PANE_RESIZE (0x04) 송신
  //    (ADR-0004 D5). 송신 debounce 100ms 는 fit() debounce 와 별개 — fit() 직후
  //    의 미세 rebound 가 추가 송신을 만들지 않게 *per-pane lastSent dedup* 가 1차
  //    필터, 100ms debounce 가 2차 filter.
  // 4. $effect cleanup: dispatcher unregister + term.dispose (R2 F11 — 모든 리스너·
  //    DOM·내부 버퍼 해제) + resize/input debounce timer 해제.
  //
  // D16 Panel Streaming State: visibility=false 또는 minimized=true 면 PanelNode 가
  // 본 컴포넌트를 *마운트조차 하지 않는다* — 본 컴포넌트는 *Streaming 상태에서만
  // 살아 있음* 가정. unmount = Suspended 진입 (xterm 인스턴스 자체가 사라짐). 재진입
  // 시 ring buffer replay (D15) 로 catch-up.
  //
  // WsClient 주입 경로: `+page.svelte` 가 `setContext('wsClient', wsClient)` 로 단일
  // 인스턴스를 등록. 본 컴포넌트는 `getContext` 로 꺼내 사용 — PanelNode 가 prop 으로
  // threading 하지 않아 sub-tree 깊이와 무관하게 동작.

  import { getContext } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { Unicode11Addon } from '@xterm/addon-unicode11';
  // xterm v6 의 cell rendering 은 본 stylesheet 가 없으면 동작하지 않는다 — DOM 은
  // 만들어지지만 cell width/height 가 0 으로 잡혀 글자가 화면에 보이지 않음.
  import '@xterm/xterm/css/xterm.css';
  import { SECURE_XTERM_OPTIONS } from '$lib/xterm/options';
  import { xtermTheme } from '$lib/xterm/xtermTheme';
  import { registerPaneOut, unregisterPaneOut } from '$lib/ws/dispatcher.svelte';
  import { encodePaneIn, encodePaneResize, FRAME_TYPE } from '$lib/ws/decode';
  import { debugCount } from '$lib/common/debugCounts';
  import { themeStore } from '$lib/stores/theme.svelte';
  import type { WsClient } from '$lib/ws/client';

  // paneId 는 항상 numeric (legacy `%N` 의 N 또는 0x88 binding 으로 얻은 PaneId).
  // multi-session terminal item 의 UUID 는 본 컴포넌트에 도달하기 전에 PanelNode
  // 가 terminalPool.paneIdFor(uuid) 로 resolve 후 numeric 만 전달. binding 미도착
  // 동안 PanelNode 가 본 컴포넌트를 마운트 X (connecting placeholder 대신 표시).
  let { paneId }: { paneId: string } = $props();

  let containerEl: HTMLDivElement | undefined = $state(undefined);

  /** Component-scope ref to the live Terminal instance — exposed so a
   *  sibling $effect can hot-reload `options.theme` whenever the chrome
   *  theme flips. `null` while unmounted. */
  let termRef = $state<Terminal | null>(null);

  // R2 F8 resize debounce — fit() 폭주 방지 (DOM 측정 → reflow 비용).
  // NodeResizer 드래그 중에는 컨테이너만 커지고 xterm 내부 .xterm-screen 의
  // cell-정수배수 inline-px height 는 fit() 호출 후에만 갱신. 그동안 갭이
  // 노출되므로 debounce 짧게 — 50ms 면 인지 X + fit() 폭주 회피 모두 충족.
  const RESIZE_DEBOUNCE_MS = 50;
  // FE-2 송신 debounce — fit() 직후의 미세 rebound 흡수. lastSent dedup 과 함께 작동.
  const RESIZE_SEND_DEBOUNCE_MS = 100;

  // WS PANE_IN/RESIZE 송신 채널 — `+page.svelte` 의 setContext('wsClient', …) 가
  // 단일 진실. setContext 는 컴포넌트 init 시점에 단 한 번이라 *holder 객체*로
  // 간접 참조 → onMount 에서 채워진 wsClient 를 send 시점에 lazy lookup.
  // 미설정(테스트/SSR) 환경에서는 holder 가 null → 송신 skip.
  interface WsClientHolder { current: WsClient | null }
  const wsClientHolder = getContext<WsClientHolder | undefined>('wsClient') ?? null;

  // paneId 는 "37" 같은 정수 문자열 (PanelNode 가 SSoT pane_id 의 정수 부분만 전달).
  // wire 변환 시 number 로 캐스팅 — `Number.parseInt(…, 10)` 으로 명시.
  const paneIdNum = $derived(Number.parseInt(paneId, 10));

  // Single-byte encoder reuse — `term.onData` 가 키 입력마다 호출되므로 매번 새
  // TextEncoder 를 만들지 않는다.
  const encoder = new TextEncoder();

  $effect(() => {
    // containerEl 이 아직 bind 되지 않은 첫 tick 보호.
    if (!containerEl) return;
    // paneId 가 정수 파싱 실패 (NaN) 면 송신 채널을 disable — 입력은 무시.
    const paneIdNumLocal = paneIdNum;
    const paneIdValid = Number.isInteger(paneIdNumLocal) && paneIdNumLocal > 0;

    const rect0 = containerEl.getBoundingClientRect();
    console.debug('[xterm] mount pane=%s container=%dx%d', paneId, rect0.width, rect0.height);

    const term = new Terminal({
      ...SECURE_XTERM_OPTIONS,
      theme: xtermTheme(themeStore.resolved),
    });
    termRef = term;
    const fitAddon = new FitAddon();
    const unicode11Addon = new Unicode11Addon();
    term.loadAddon(fitAddon);
    term.loadAddon(unicode11Addon);
    term.unicode.activeVersion = '11';
    term.open(containerEl);
    try {
      fitAddon.fit();
      console.debug('[xterm] post-fit pane=%s cols=%d rows=%d', paneId, term.cols, term.rows);
    } catch (e) {
      console.debug('[xterm] initial fit failed pane=%s err=%o', paneId, e);
    }

    // PaneOut 등록 — WS dispatcher 가 이 paneId 로 도착한 PANE_OUT(0x02) 을 본
    // 핸들러로 라우팅. `cb` 는 R2 F4 백프레셔 watermark 갱신 콜백 (term.write 가
    // 내부 buffer 플러시 후 호출).
    //
    // Mirror (ADR-0021 D1): 같은 paneId 에 여러 XtermHost 가 동시 마운트될 수 있음 —
    // dispatcher 가 Set<handler> fan-out 으로 모두에게 같은 bytes 를 흘려보낸다.
    // unregister 시 *handler identity* 가 필요하므로 inline 화살표 함수를 변수로 캡처.
    const paneOutHandler = (buf: Uint8Array, cb: () => void) => term.write(buf, cb);
    registerPaneOut(paneId, paneOutHandler);

    // ── FE-1: 사용자 키 입력 → PANE_IN (0x03) ───────────────────────────────
    // ADR-0004 D4: UTF-8 바이트 그대로 송신. WsClient 가 connected 가 아니면
    // 내부에서 drop — buffering 하지 않는다 (재연결 후 서버 상태가 권위).
    const dataDisposable = term.onData((data) => {
      const client = wsClientHolder?.current ?? null;
      if (!client || !paneIdValid) return;
      const bytes = encoder.encode(data);
      client.sendFrame(FRAME_TYPE.PANE_IN, encodePaneIn(paneIdNumLocal, bytes));
    });

    // ── FE-2: ResizeObserver → fit() → debounced PANE_RESIZE (0x04) ────────
    // 단계:
    //   ResizeObserver fire → 150ms fit() debounce → fitAddon.fit() →
    //   (cols, rows) 가 lastSent 와 다르면 100ms send debounce → PANE_RESIZE.
    //
    // lastSent dedup 가 1차 필터, send debounce 가 2차 필터. 두 단계는 직렬.
    let lastSentCols: number | null = null;
    let lastSentRows: number | null = null;
    let fitTimer: ReturnType<typeof setTimeout> | null = null;
    let sendTimer: ReturnType<typeof setTimeout> | null = null;
    let pendingCols: number | null = null;
    let pendingRows: number | null = null;
    // 0045 P1-D — 직전 ResizeObserver entry 의 contentRect px. SvelteFlow
    // measurement loop 와의 증폭 방지: 같은 px 면 fit() 자체 skip (DOM 측정 비용
    // 0). cols/rows dedup 은 fit() *이후* 단계 — entry-level dedup 가 더 빠름.
    let lastObservedW = -1;
    let lastObservedH = -1;

    function flushResize(): void {
      if (pendingCols === null || pendingRows === null) return;
      const client = wsClientHolder?.current ?? null;
      if (!client || !paneIdValid) {
        // 송신 채널 없으면 dedup state 만 갱신.
        lastSentCols = pendingCols;
        lastSentRows = pendingRows;
        pendingCols = null;
        pendingRows = null;
        return;
      }
      const cols = pendingCols;
      const rows = pendingRows;
      pendingCols = null;
      pendingRows = null;
      // dedup: send 시점 다시 확인 — debounce 동안 rebound 로 동일값 회귀 가능.
      if (cols === lastSentCols && rows === lastSentRows) return;
      lastSentCols = cols;
      lastSentRows = rows;
      client.sendFrame(FRAME_TYPE.PANE_RESIZE, encodePaneResize(paneIdNumLocal, cols, rows));
    }

    const ro = new ResizeObserver((entries) => {
      // P1-D entry-level dedup — 직전 px 와 동일하면 fit() 진입 자체 skip.
      // SvelteFlow 의 nodeInternals update 가 동일 width/height 재측정을 트리거할
      // 때 fit() 호출 + 결과 비교 비용을 0 으로.
      const last = entries[entries.length - 1];
      if (last !== undefined) {
        const w = Math.round(last.contentRect.width);
        const h = Math.round(last.contentRect.height);
        if (w === lastObservedW && h === lastObservedH) return;
        lastObservedW = w;
        lastObservedH = h;
      }
      if (fitTimer) clearTimeout(fitTimer);
      fitTimer = setTimeout(() => {
        fitTimer = null;
        debugCount('xterm.fit');
        try {
          fitAddon.fit();
        } catch (e) {
          console.debug('[gtmux] xterm fit on resize failed', e);
          return;
        }
        const cols = term.cols;
        const rows = term.rows;
        // dedup 1차: 직전 송신값과 동일하면 send debounce 도 arm 하지 않음.
        if (cols === lastSentCols && rows === lastSentRows) return;
        pendingCols = cols;
        pendingRows = rows;
        if (sendTimer) clearTimeout(sendTimer);
        sendTimer = setTimeout(() => {
          sendTimer = null;
          flushResize();
        }, RESIZE_SEND_DEBOUNCE_MS);
      }, RESIZE_DEBOUNCE_MS);
    });
    ro.observe(containerEl);

    return () => {
      if (fitTimer) {
        clearTimeout(fitTimer);
        fitTimer = null;
      }
      if (sendTimer) {
        clearTimeout(sendTimer);
        sendTimer = null;
      }
      ro.disconnect();
      dataDisposable.dispose();
      unregisterPaneOut(paneId, paneOutHandler);
      termRef = null;
      term.dispose();
    };
  });

  /* ── G27: hot-reload xterm theme when chrome theme flips ─────────────
   * SECURE_XTERM_OPTIONS 의 theme 은 mount 시 1회 — 이후 themeStore.resolved
   * (system mode 의 OS preference 변경 포함) 가 바뀌면 본 effect 가 live
   * Terminal 의 `options.theme` 를 교체. xterm v6 의 `options` setter 는
   * shallow merge + 즉시 repaint. */
  $effect(() => {
    const term = termRef;
    const resolved = themeStore.resolved;
    if (term === null) return;
    term.options.theme = xtermTheme(resolved);
    // theme options swap 만으로는 v6 DOM renderer 의 cell DOM (.xterm-rows
    // > div > span) 의 inline color / background-color 가 stale — 새로고침
    // 외 복구 X. 가설: cell span 의 inline style.color / backgroundColor 가
    // *cell write 시점에 commit* 된 옛 theme 의 sRGB. theme swap 후
    // refresh(0, rows-1) 가 cached span fragment 를 *recycle* 못 함.
    //
    // 시도 C: cell span 의 inline color reset (= 옛 stale 색 제거) → refresh
    // 가 새 theme 으로 재 paint. ANSI class (.xterm-fg-N, .xterm-bg-N) 는
    // class 인데, *.xterm 의 inline <style>* 의 token 이 theme swap 시 자동
    // 변경 — 그러나 cell 의 *inline color* 가 더 우선 (specificity) → inline
    // 제거 후에야 class 의 새 색 falls through.
    try {
      term.clearTextureAtlas?.();
    } catch {
      // silent.
    }
    if (containerEl !== undefined) {
      // NOTE: querySelectorAll<HTMLElement>(...) generic 표기는 svelte parser
      // 가 HTML tag 로 오인 → script unclosed. cast 패턴으로 우회.
      const cells = containerEl.querySelectorAll('.xterm-rows span') as NodeListOf<HTMLElement>;
      cells.forEach((span) => {
        // color / background-color 만 reset, 다른 inline style (font-weight,
        // text-decoration 등) 은 보존.
        span.style.color = '';
        span.style.backgroundColor = '';
      });
    }
    try {
      term.refresh(0, term.rows - 1);
    } catch {
      // silent.
    }
  });
</script>

<div bind:this={containerEl} class="xterm-host nowheel nodrag"></div>

<style>
  /* xterm.js DOM 렌더러는 측정 시점에 *0 × 0* 컨테이너에서 ColMeasure 가 무효 (R2 F7).
     panel-body 측 flex: 1 + min-height: 0 이 본 컨테이너를 가시 크기로 끌어준다.
     `nowheel` / `nodrag` 클래스는 SvelteFlow 의 휠/드래그 인터셉터 차단 — 터미널 안에서
     마우스 휠 스크롤(scrollback), 드래그(선택)이 캔버스 pan/drag 와 충돌하지 않도록. */
  /* Background MUST match xterm theme.background — .xterm-screen 의
   * cell-정수배수 inline px height 와 컨테이너 사이의 잔여 영역 (특히
   * resize 중 fit() debounce window 동안) 이 같은 색이라 보이지 않게.
   * --xterm-bg 는 xtermTheme.ts 의 LIGHT/DARK.background 와 동기. */
  .xterm-host {
    width: 100%;
    height: 100%;
    background: var(--xterm-bg);
  }
  .xterm-host :global(.xterm) {
    height: 100%;
  }
  /* xterm.css 의 default 는 .xterm / .xterm-viewport 의 background 를
   * xterm.options.theme.background 로 inline-set 하거나 검은색으로 둠.
   * resize 중 .xterm-screen 의 inline px height 가 컨테이너보다 짧을 때
   * 노출되는 영역이 검게 보이지 않도록 세 layer 모두 강제 매칭. */
  .xterm-host :global(.xterm),
  .xterm-host :global(.xterm-viewport),
  .xterm-host :global(.xterm-screen) {
    background-color: var(--xterm-bg) !important;
  }
  .xterm-host :global(.xterm-viewport),
  .xterm-host :global(.xterm-screen) {
    height: 100%;
  }
</style>
