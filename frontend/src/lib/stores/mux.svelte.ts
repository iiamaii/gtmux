// MuxStore — tmux-domain mirror state (windows / panes / session).
//
// 정본:
// - `docs/ssot/wire-protocol.md` §2.3 (NOTIFY_MIRROR kind enum 7개)
// - `docs/adr/0008-single-pane-window-and-group.md` D1 (single-pane-per-window 컨벤션)
// - `docs/sketch.md` §4.1.1 (두 state 도메인 분리 — tmux 측 / 웹 측)
// - R8 §F3 (svelte 5 runes store 패턴 — SvelteMap entry-level reactivity)
//
// 본 store 는 *tmux 가 authoritative* — 모든 mutation 은 dispatcher 가 NOTIFY_MIRROR
// 수신 시 단방향으로 호출한다. UI 는 read-only. tmux 가 보내지 않은 상태를 web 이
// authored 하는 일이 없다는 점이 sketch §4.1.1 의 *불변식 #1* 강제 보장.
//
// 본 store 의 entries 는 *처음 받은 신호를 진실로 신뢰* — backend 가 보내지 않는 한
// 추가 정합 검증은 하지 않는다 (forward-compat 보존, SSoT §6).

import { SvelteMap } from 'svelte/reactivity';

/** tmux window 한 개의 mirror. `id` 는 SvelteMap key (= `"@N"`) 와 같으므로 본 객체에 포함하지 않는다. */
export interface MirroredWindow {
  /** tmux window 이름 (`%window-renamed` 시 갱신). 빈 문자열 = 미지정. */
  name: string;
  /** tmux layout 문자열 — 캔버스 좌표로 변환 *금지* (불변식 #3, SSoT §4 layout-change). */
  layout: string;
}

/** tmux pane 한 개의 mirror. key 는 SvelteMap 의 `%N` 의 정수 `N`. */
export interface MirroredPane {
  /** 속한 window id (`"@N"`). 미지정 = `null`. */
  window_id: string | null;
  /** `%pane-died` 수신 후 `true`. zombie 상태. */
  dead: boolean;
  /** `%pane-mode-changed` 수신 시 갱신. `null` = 기본 모드. */
  mode: string | null;
}

/** tmux session 한 개. 동시에 단일 session 만 mirror 한다 (ADR-0007 1:1:1). */
export interface MirroredSession {
  /** `"$N"` 형식. */
  id: string;
  name: string;
}

class MuxStore {
  // Windows — key = `"@N"`. SvelteMap 으로 entry-level reactivity.
  windows = $state(new SvelteMap<string, MirroredWindow>());
  // Panes — key = `%N` 의 *정수 N*. wire protocol 의 paneId varint 와 동일 단위.
  panes = $state(new SvelteMap<number, MirroredPane>());
  // Active session — ADR-0007 D2/D3 의 1:1:1 바인딩 하에서 동시에 하나.
  session = $state<MirroredSession | null>(null);

  /** `window-add` 수신 — 이미 존재해도 idempotent (멱등). */
  addWindow(windowId: string, name: string): void {
    if (!this.windows.has(windowId)) {
      this.windows.set(windowId, { name, layout: '' });
    } else {
      // 기존 window 의 name 만 update (layout 보존).
      const cur = this.windows.get(windowId);
      if (cur && cur.name !== name) {
        this.windows.set(windowId, { ...cur, name });
      }
    }
  }

  /** `window-renamed` 수신. 없는 window 라면 silently 생성한다 (forward-compat). */
  renameWindow(windowId: string, name: string): void {
    const cur = this.windows.get(windowId);
    this.windows.set(windowId, { name, layout: cur?.layout ?? '' });
  }

  /** `window-close` 수신 — window 와 그에 속한 모든 pane 의 `window_id` 를 `null` 로 떨군다. */
  closeWindow(windowId: string): void {
    this.windows.delete(windowId);
    for (const [paneId, pane] of this.panes) {
      if (pane.window_id === windowId) {
        // window 만 사라지고 pane 자체는 `pane-died` 가 별도로 마무리한다. 본 step 에서는
        // orphan 상태로 표시만 (UI 분기는 panel-side 책임).
        this.panes.set(paneId, { ...pane, window_id: null });
      }
    }
  }

  /** `session-changed` 수신 — 이전 session 은 통째로 교체된다 (단일 session 정책). */
  setSession(sessionId: string, name: string): void {
    this.session = { id: sessionId, name };
  }

  /** `layout-change` 수신. layout 문자열은 trigger 로만 사용 — 캔버스 좌표로 변환 금지. */
  setLayout(windowId: string, layout: string): void {
    const cur = this.windows.get(windowId);
    this.windows.set(windowId, { name: cur?.name ?? '', layout });
  }

  /** `pane-mode-changed` 수신. 없는 pane 이라면 default(`window_id = null`) 로 생성. */
  setPaneMode(paneId: number, mode: string): void {
    const cur = this.panes.get(paneId);
    this.panes.set(paneId, {
      window_id: cur?.window_id ?? null,
      dead: cur?.dead ?? false,
      mode,
    });
  }

  /**
   * `pane-died` 수신 — pane entry 는 *지우지 않고* `dead = true` 로 marking 한다.
   * UI 의 zombie badge 가 *과거 pane id* 를 들고 있을 수 있으므로 entry 보존 (ADR-0001 D9).
   */
  killPane(paneId: number): void {
    const cur = this.panes.get(paneId);
    this.panes.set(paneId, {
      window_id: cur?.window_id ?? null,
      dead: true,
      mode: cur?.mode ?? null,
    });
  }

  /**
   * 새 pane 발견 — 현재 backend 는 별도 `pane-add` 이벤트를 emit 하지 않으므로,
   * dispatcher 가 PANE_OUT(0x02) 수신 시 *처음 보는 pane* 일 때 본 메서드를 호출한다.
   * Idempotent.
   *
   * single-pane convention (ADR-0008 D1) 하에서 `window_id` 매핑은 backend 의
   * `pane-add` event 정식 wire 시점에 정밀화된다. 본 MVP 는 `null` 로 두고 mux
   * 입력은 `window_id` 미지정 panes 도 허용한다.
   */
  addPane(paneId: number, windowId: string | null = null): void {
    if (this.panes.has(paneId)) {
      // 기존 pane 의 window_id 가 null 인 상태에서 새 정보가 오면 set, 아니면 보존.
      if (windowId !== null) {
        const cur = this.panes.get(paneId)!;
        if (cur.window_id === null) {
          this.panes.set(paneId, { ...cur, window_id: windowId });
        }
      }
      return;
    }
    this.panes.set(paneId, { window_id: windowId, dead: false, mode: null });
  }
}

export const muxStore = new MuxStore();
