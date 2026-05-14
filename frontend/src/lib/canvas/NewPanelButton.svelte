<script lang="ts">
  // "New Panel" button — Stage-B PTY-direct wire (S7-WS-PAYLOAD-SIMPLIFY).
  //
  // 흐름:
  //   1. 클릭 → WsClient 로 0x01 CTRL `{cmd: "new-pane", args: []}` 발사.
  //      UUID-v4 id 가 NOTIFY_MIRROR `pane-spawned.request_id` 로 echo 되어
  //      매칭. ADR-0013 D10 / 0026 §2.5 정합.
  //   2. backend (PtyBackend) 가 PaneId 발급 + `pane-spawned` NOTIFY broadcast.
  //      Pane 의 첫 출력 (shell prompt) 은 0x02 PANE_OUT 으로 즉시 도착 →
  //      dispatcher 의 `addPane` first-sight 가 mux store 를 갱신.
  //   3. captured pane_id 로 `putLayoutAppendPanel` — 좌표는 viewport center.
  //   4. 412 시 1회 자동 rebase (putLayoutAppendPanel 내부 처리). 그 이상은 사용자 알림.
  //
  // 정본:
  // - `docs/adr/0013-pty-direct-no-tmux.md` D10 (BackendCommand allowlist)
  // - `docs/reports/0026-stage-b-carry-forward.md` §2.5 (envelope mapping)
  // - `docs/ssot/canvas-layout-schema.md` §1 Panel (id pattern `^p[0-9a-zA-Z]{1,32}$`)
  //
  // 의도적 단순화:
  // - viewport center 는 `ephemeralStore.viewport` + container DOM 크기로 계산.
  //   useSvelteFlow 는 SvelteFlow 컴포넌트 내부 컨텍스트 필요 — 본 컴포넌트는
  //   *outside SvelteFlow* 에서 동작하므로 store-derived 좌표만 사용.
  // - pending action 의 timeout 은 ctrl-registry 가 5s 로 관리. PANE_OUT
  //   first-sight watcher 가 동일 5s 안에 깨어나기를 기대한다.

  import { getContext } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import { ephemeralStore } from '$lib/stores/ephemeral.svelte';
  import { muxStore } from '$lib/stores/mux.svelte';
  import { panelsStore } from '$lib/stores/panels.svelte';
  import { sendCtrl } from '$lib/ws/ctrl-registry';
  import { appendPanelIfMissing } from '$lib/http/layout';
  import type { WsClient } from '$lib/ws/client';

  interface WsClientHolder { current: WsClient | null }

  // +page.svelte 가 등록한 holder — token 획득 후 채워진다.
  const wsClientHolder = getContext<WsClientHolder>('wsClient');

  // token 은 sessionStorage 에서 동기적으로 꺼낸다 (+page.svelte 와 동일 정책).
  const TOKEN_STORAGE_KEY = 'gtmux_token';

  // Panel 디폴트 크기 (PanelNode 와 정합: 480 × 320).
  const PANEL_DEFAULT_W = 480;
  const PANEL_DEFAULT_H = 320;

  // 실행 중 (button disabled) state — 동시 클릭 보호.
  let inFlight = $state(false);
  let errorMessage = $state<string | null>(null);

  function genPanelId(): string {
    // schema §1 Panel.id pattern: `^p[0-9a-zA-Z]{1,32}$`. crypto.randomUUID() 의
    // 8-4-4-4-12 hex 는 hyphen 을 포함하므로 hyphen 만 제거해 32자 hex 로 만들고
    // `p` prefix 를 붙인다 (총 33 chars — 1+32, pattern 한계 정확히).
    const u = (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function')
      ? crypto.randomUUID()
      : `${Math.random().toString(16).slice(2)}${Math.random().toString(16).slice(2)}`;
    return `p${u.replace(/-/g, '').slice(0, 32)}`;
  }

  function computeViewportCenter(): { x: number; y: number } {
    // viewport.x / viewport.y / viewport.zoom 은 SvelteFlow 의 pan-zoom 미러.
    // world coordinate 의 *컨테이너 중심* = (containerW/2 - viewport.x) / zoom.
    // container DOM 크기는 .canvas-root 의 첫 자식을 query — 본 컴포넌트는 그 안에 있다.
    const vp = ephemeralStore.viewport;
    // DOM access 는 click 시점에만 (SSR 안전 영역 아님 — 본 컴포넌트는 클라이언트 컨텍스트).
    const root = document.querySelector('.canvas-root');
    const rect = root?.getBoundingClientRect();
    const cw = rect?.width ?? window.innerWidth ?? 1024;
    const ch = rect?.height ?? window.innerHeight ?? 768;
    const zoom = vp.zoom === 0 ? 1 : vp.zoom;
    // panel 좌상단을 viewport 중심에 정렬하려면 (centerX - W/2, centerY - H/2).
    const centerX = (cw / 2 - vp.x) / zoom - PANEL_DEFAULT_W / 2;
    const centerY = (ch / 2 - vp.y) / zoom - PANEL_DEFAULT_H / 2;
    return { x: centerX, y: centerY };
  }

  /** 현재 panels 의 최대 z + 1. 빈 set 이면 1. */
  function nextZ(): number {
    let max = 0;
    for (const p of panelsStore.panels.values() as Iterable<Record<string, unknown>>) {
      const z = typeof p['z'] === 'number' ? (p['z'] as number) : 0;
      if (z > max) max = z;
    }
    return max + 1;
  }

  /**
   * mux store 의 `panes` 에 *새 entry* 가 추가될 때까지 기다린다. 반환값은 그 entry 의 paneId.
   * timeoutMs 초과 시 reject.
   *
   * 본 watcher 는 setTimeout polling 이 아닌 *SvelteMap reference 비교* 로 동작 —
   * panes 는 `$state(new SvelteMap)` 이지만 SvelteMap 은 reactivity 를 직접 가지므로
   * 외부에서 `$effect` 로 `panes.size` 를 감시할 수도 있다. 다만 본 모듈은 컴포넌트
   * 내부에서 호출되므로 단순화를 위해 reference 비교 + microtask 폴링.
   *
   * NOTE: backend 가 success ack 와 함께 result.pane_id 를 보내면 ctrl-registry 의
   * resolve 가 먼저 깨어나므로 본 watcher 는 race 없이 사용되지 않은 채 timeout 만료.
   */
  function waitForNewPane(
    snapshot: ReadonlySet<number>,
    timeoutMs: number,
  ): Promise<number> {
    return new Promise((resolve, reject) => {
      const start = performance.now();
      const tick = () => {
        for (const id of muxStore.panes.keys()) {
          if (!snapshot.has(id)) {
            resolve(id);
            return;
          }
        }
        if (performance.now() - start > timeoutMs) {
          reject(new Error('waitForNewPane timeout'));
          return;
        }
        // 50ms 폴링 — 5s 안에 100회 정도. svelte 의 effect-rooted watcher 는
        // 컴포넌트 lifecycle 에 묶이지 않아 비동기 함수 안에서 다루기 부담스러움.
        setTimeout(tick, 50);
      };
      tick();
    });
  }

  async function onclick() {
    if (inFlight) return;
    const client = wsClientHolder?.current;
    if (!client) {
      errorMessage = 'WebSocket not ready';
      return;
    }
    const token = readToken();
    if (token === null) {
      errorMessage = 'No auth token';
      return;
    }
    inFlight = true;
    errorMessage = null;
    try {
      // 1) snapshot 현재 mux pane set — PANE_OUT first-sight 매칭의 기준점.
      const baseline = new Set<number>(muxStore.panes.keys());

      // 2) CTRL `new-pane` 요청. Stage B 의 BackendCommand allowlist 에 대응
      //    (ADR-0013 D10). args 는 비워두면 PtyBackend 가 $SHELL 기본값을
      //    spawn — gtmux Server 는 Session 1:1 binding 이므로 session-name
      //    인자는 더 이상 필요하지 않다 (Server 자체가 session).
      const { response } = sendCtrl(
        client,
        'new-pane',
        [],
        { timeoutMs: 5_000 },
      );

      // 3) 두 매칭 경로 중 *먼저 깨어나는* 쪽으로 진행.
      //    - response: backend 의 ok=true ack (현재 미배선, 미래 정식).
      //    - waitForNewPane: PANE_OUT first-sight 가 muxStore.addPane 을 깨움.
      let paneId: number | null = null;
      try {
        const winner = await Promise.race([
          response.then((r) => ({ kind: 'ctrl' as const, r })),
          waitForNewPane(baseline, 5_000).then((id) => ({ kind: 'mux' as const, id })),
        ]);
        if (winner.kind === 'ctrl') {
          const r = winner.r;
          if (!r.ok) {
            throw new Error(`CTRL error: ${r.code ?? '?'} ${r.error ?? ''}`);
          }
          const pid = typeof r.result?.['pane_id'] === 'string'
            ? (r.result['pane_id'] as string)
            : null;
          if (pid && /^%\d+$/.test(pid)) {
            paneId = Number.parseInt(pid.slice(1), 10);
          }
          // backend 가 result.pane_id 를 안 보냈으면 mux 경로로 한번 더 기다린다.
          if (paneId === null) {
            paneId = await waitForNewPane(baseline, 5_000);
          }
        } else {
          paneId = winner.id;
        }
      } catch (e) {
        throw new Error(`new-pane resolve failed: ${String((e as Error).message ?? e)}`);
      }
      if (paneId === null || Number.isNaN(paneId)) {
        throw new Error('pane_id not captured');
      }

      // 4) appendPanelIfMissing — 본 path 와 dispatcher 의 pane-spawned hook
      //    이 같은 helper 를 호출하므로 idempotent (ADR-0015 D3/D4). 첫 도착
      //    쪽의 좌표가 잠긴다 — 본 path 는 viewport center 를 전달해 사용자
      //    명시 클릭의 mental model 을 보존.
      const center = computeViewportCenter();
      await appendPanelIfMissing(paneId, { token, coords: center });
      // 성공 — LAYOUT_CHANGED broadcast 가 fetchLayoutAndHydrate 를 깨우므로
      // panelsStore 가 자동으로 새 panel 을 보이게 된다 (Pull-through-notify).
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      console.warn('[gtmux] New Panel failed:', msg);
      errorMessage = msg;
    } finally {
      inFlight = false;
    }
  }

  function readToken(): string | null {
    try {
      return sessionStorage.getItem(TOKEN_STORAGE_KEY);
    } catch {
      return null;
    }
  }

  // 미사용 import 회피 (SvelteMap 은 타입 hint 로만 사용 — runtime 사용 강제로 묶음).
  void SvelteMap;
</script>

<button
  type="button"
  class="new-panel-btn"
  disabled={inFlight}
  onclick={onclick}
  aria-label="Create a new Panel"
  title="Create a new tmux window + canvas Panel"
>
  {inFlight ? 'Creating…' : 'New Panel'}
</button>
{#if errorMessage !== null}
  <span class="new-panel-error" role="alert">{errorMessage}</span>
{/if}

<style>
  .new-panel-btn {
    display: inline-block;
    padding: var(--space-4) var(--space-10);
    background: var(--color-surface);
    color: var(--color-fg);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    font-family: inherit;
    font-size: var(--text-md);
    cursor: pointer;
    line-height: 1.4;
    box-shadow: var(--shadow-sm);
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .new-panel-btn:hover:not(:disabled) {
    background: var(--color-glass-1);
  }

  .new-panel-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .new-panel-error {
    margin-left: var(--space-8);
    color: var(--color-danger);
    font-size: var(--text-base);
    line-height: 1.4;
  }
</style>
