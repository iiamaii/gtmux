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
  // D16 Panel Streaming State amend: visibility=false 면 PanelNode 가 본 컴포넌트를
  // 마운트하지 않는다. minimized=true 는 xterm 인스턴스를 유지하고 chrome 만 접는다.
  // 그래야 restore 시 xterm 의 screen/scrollback buffer 가 보존된다.
  //
  // WsClient 주입 경로: `+page.svelte` 가 `setContext('wsClient', wsClient)` 로 단일
  // 인스턴스를 등록. 본 컴포넌트는 `getContext` 로 꺼내 사용 — PanelNode 가 prop 으로
  // threading 하지 않아 sub-tree 깊이와 무관하게 동작.

  import { getContext, untrack } from 'svelte';
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
  import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
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

  /** ADR-0004 D6 amend ② (2026-05-21) — SvelteFlow viewport scale 안에서
   *  xterm.js v6 의 mouse 좌표 mismatch 보정. xterm 의 mouse handler 는
   *  `element.getBoundingClientRect()` + `clientX - rect.left` 로 character
   *  index 계산 — viewport scale(z) 안에서 rect 는 visual size (scaled),
   *  cell width 는 unscaled fontMetrics base. ratio mismatch = z factor →
   *  사용자 visual click N cells 옆이 N×z 의 char index 로 계산.
   *
   *  Fix: capture-phase mouse listener — z ≠ 1 일 때 좌표를 z 로 나눠
   *  synthetic event redispatch. WeakSet 으로 synth 재진입 차단. */
  const synthSet = new WeakSet<Event>();
  const MOUSE_TYPES = ['mousedown', 'mousemove', 'mouseup', 'click', 'contextmenu', 'dblclick'] as const;

  function isTerminalCopyShortcut(e: KeyboardEvent): boolean {
    return (e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 'c';
  }

  async function copyTextToClipboard(text: string): Promise<void> {
    if (text.length === 0) return;
    const result = await copyTextToSystemClipboard(text);
    if (!result.ok) throw new Error(result.reason ?? 'Clipboard copy failed');
  }

  function relayMouse(e: MouseEvent): void {
    if (synthSet.has(e)) return;
    if (!containerEl) return;
    // ADR-0004 D6 amend ② 정정 (2026-05-21) — sessionStore.viewport.zoom 직접
    // 사용하면 maximize 모달 (XtermHost DOM 이 modal 로 reparent — SvelteFlow
    // viewport transform *밖*) 에서도 zoom 적용되어 좌표 어긋남 (회귀). 대신
    // 실제 effective transform 측정 — getBoundingClientRect().width 와
    // offsetWidth 의 비율 = 실효 scale. modal 에서는 transform 없으니 비율 1.0
    // → natural handling. viewport 안에서만 zoom != 1.
    const rect = containerEl.getBoundingClientRect();
    const layoutW = containerEl.offsetWidth;
    if (layoutW === 0) return;
    const z = rect.width / layoutW;
    if (Math.abs(z - 1) < 0.001) return; // identity transform — natural handling.
    const synthClientX = rect.left + (e.clientX - rect.left) / z;
    const synthClientY = rect.top + (e.clientY - rect.top) / z;
    e.preventDefault();
    e.stopImmediatePropagation();
    const synth = new MouseEvent(e.type, {
      bubbles: true,
      cancelable: true,
      view: e.view ?? window,
      detail: e.detail,
      screenX: e.screenX,
      screenY: e.screenY,
      clientX: synthClientX,
      clientY: synthClientY,
      button: e.button,
      buttons: e.buttons,
      ctrlKey: e.ctrlKey,
      shiftKey: e.shiftKey,
      altKey: e.altKey,
      metaKey: e.metaKey,
      relatedTarget: e.relatedTarget,
    });
    synthSet.add(synth);
    (e.target as Element | null)?.dispatchEvent(synth);
  }

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
      theme: { ...xtermTheme(untrack(() => themeStore.resolved)) },
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

    // ADR-0004 D6 amend ② — Capture-phase mouse coord 변환 listener.
    // viewport zoom ≠ 1 일 때 좌표를 z 로 나눠 synthetic redispatch → xterm
    // 의 unscaled cell width 계산과 정합 → 정확한 char index.
    for (const type of MOUSE_TYPES) {
      containerEl.addEventListener(type, relayMouse, { capture: true });
    }

    function onTerminalKeyDown(e: KeyboardEvent): void {
      if (!isTerminalCopyShortcut(e)) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      const selectedText = term.getSelection();
      if (selectedText.length > 0) {
        void copyTextToClipboard(selectedText).catch((err) => {
          console.debug('[gtmux] terminal copy failed', err);
        });
      }
    }

    // Ctrl/Cmd+Shift+C conflicts with browser DevTools inspect shortcuts.
    // Scope the interception to xterm only, then copy xterm's internal
    // selection instead of letting the browser default action run.
    containerEl.addEventListener('keydown', onTerminalKeyDown, { capture: true });

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
        // Minimized panels keep XtermHost mounted under display:none so the
        // xterm buffer survives. Ignore hidden 0x0 measurements and force the
        // next visible measurement to refit even if it matches the old size.
        if (w <= 0 || h <= 0) {
          lastObservedW = -1;
          lastObservedH = -1;
          return;
        }
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
          term.refresh(0, Math.max(0, term.rows - 1));
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
      // ADR-0004 D6 amend ② cleanup — capture-phase mouse listener 해제.
      if (containerEl) {
        for (const type of MOUSE_TYPES) {
          containerEl.removeEventListener(type, relayMouse, { capture: true });
        }
        containerEl.removeEventListener('keydown', onTerminalKeyDown, { capture: true });
      }
      termRef = null;
      term.dispose();
    };
  });

  /* ── G27: hot-reload xterm theme when chrome theme flips ─────────────
   * SECURE_XTERM_OPTIONS 의 theme 은 mount 시 1회 — 이후 themeStore.resolved
   * (system mode 의 OS preference 변경 포함) 가 바뀌면 본 effect 가 live
   * Terminal 의 `options.theme` 를 교체. xterm v6 의 `options` setter 는
   * object reference 비교를 하므로 새 object 를 전달해야 한다. */
  $effect(() => {
    const term = termRef;
    const resolved = themeStore.resolved;
    if (term === null) return;
    term.options.theme = { ...xtermTheme(resolved) };
    term.refresh(0, Math.max(0, term.rows - 1));
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
