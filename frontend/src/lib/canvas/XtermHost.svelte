<script lang="ts">
  // xterm.js hand-rolled wrapper — R8 §F1 (ADR-0012 D4·O1).
  //
  // Lifecycle (R8 F1 + R2 F11):
  // 1. $effect mount: new Terminal(SECURE_XTERM_OPTIONS) + loadAddon(FitAddon) +
  //    loadAddon(Unicode11Addon) + unicode.activeVersion='11' + term.open(containerEl)
  //    + fitAddon.fit() (1회) + WS dispatcher에 PaneId 등록.
  // 2. 사용자 키 입력: term.onData → WS PANE_IN (0x03) 송신 (현재 dispatcher 인터페이스가
  //    PaneOut 한 방향만 구현되어 있어, MVP 단계는 console.debug에 입력 바이트만 기록.
  //    실제 송신은 ws/client.ts 가 sendPaneInput을 노출하는 시점에 배선).
  // 3. 컨테이너 ResizeObserver → debounce 150ms (R2 F8) → fit() + 변경된 cols/rows를
  //    WS PANE_RESIZE (0x04) 로 송신 (MVP는 fit만 — 실제 송신은 client.ts 의존).
  // 4. $effect cleanup: dispatcher unregister + term.dispose (R2 F11 — 모든 리스너·DOM·
  //    내부 버퍼 해제).
  //
  // D16 Panel Streaming State: visibility=false 또는 minimized=true 면 PanelNode가 본
  // 컴포넌트를 *마운트조차 하지 않는다* — 본 컴포넌트는 *Streaming 상태에서만 살아 있음*
  // 가정. unmount = Suspended 진입 (xterm 인스턴스 자체가 사라짐). 재진입 시 ring buffer
  // replay (D15)로 catch-up. P1+에서 *retain (display:none)* 패턴 검토 (R8 §F8 sketch
  // 주석 + R8-O3 측정 결과 따라).

  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { Unicode11Addon } from '@xterm/addon-unicode11';
  import { SECURE_XTERM_OPTIONS } from '$lib/xterm/options';
  import { registerPaneOut, unregisterPaneOut } from '$lib/ws/dispatcher.svelte';

  let { paneId }: { paneId: string } = $props();

  let containerEl: HTMLDivElement | undefined = $state(undefined);

  // R2 F8 resize debounce — 150ms. fit() 폭주 방지.
  const RESIZE_DEBOUNCE_MS = 150;

  $effect(() => {
    // containerEl 이 아직 bind 되지 않은 첫 tick 보호.
    if (!containerEl) return;

    const term = new Terminal(SECURE_XTERM_OPTIONS);
    const fitAddon = new FitAddon();
    const unicode11Addon = new Unicode11Addon();
    term.loadAddon(fitAddon);
    term.loadAddon(unicode11Addon);
    term.unicode.activeVersion = '11';
    term.open(containerEl);
    try {
      fitAddon.fit();
    } catch (e) {
      // fit() 은 컨테이너가 0×0이면 throw 가능 — 디버그 로그만 남기고 진행.
      console.debug('xterm initial fit failed', e);
    }

    // PaneOut 등록 — WS dispatcher가 이 paneId로 도착한 PANE_OUT(0x02)을 본 핸들러로
    // 라우팅. `cb`는 R2 F4 백프레셔 watermark 갱신 콜백 (term.write가 내부 buffer
    // 플러시 후 호출).
    registerPaneOut(paneId, (buf, cb) => term.write(buf, cb));

    // 사용자 키 입력 → PANE_IN (0x03). WS client의 sendPaneInput 인터페이스가
    // 정의되기 전까지는 console.debug 로 stub.
    const dataDisposable = term.onData((data) => {
      console.debug('pane input', paneId, data.length);
    });

    // 컨테이너 ResizeObserver — 디바운스된 fit().
    let resizeTimer: ReturnType<typeof setTimeout> | null = null;
    const ro = new ResizeObserver(() => {
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        resizeTimer = null;
        try {
          fitAddon.fit();
          console.debug('pane resize', paneId, term.cols, term.rows);
        } catch (e) {
          console.debug('xterm fit on resize failed', e);
        }
      }, RESIZE_DEBOUNCE_MS);
    });
    ro.observe(containerEl);

    return () => {
      if (resizeTimer) {
        clearTimeout(resizeTimer);
        resizeTimer = null;
      }
      ro.disconnect();
      dataDisposable.dispose();
      unregisterPaneOut(paneId);
      term.dispose();
    };
  });
</script>

<div bind:this={containerEl} class="xterm-host nowheel nodrag"></div>

<style>
  /* xterm.js DOM 렌더러는 측정 시점에 *0 × 0* 컨테이너에서 ColMeasure 가 무효 (R2 F7).
     panel-body 측 flex: 1 + min-height: 0 이 본 컨테이너를 가시 크기로 끌어준다.
     `nowheel` / `nodrag` 클래스는 SvelteFlow의 휠/드래그 인터셉터 차단 — 터미널 안에서
     마우스 휠 스크롤(scrollback), 드래그(선택)이 캔버스 pan/drag와 충돌하지 않도록. */
  .xterm-host {
    width: 100%;
    height: 100%;
    background: #000;
  }
  .xterm-host :global(.xterm) {
    height: 100%;
  }
  .xterm-host :global(.xterm-viewport),
  .xterm-host :global(.xterm-screen) {
    height: 100%;
  }
</style>
