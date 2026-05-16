// SessionStore — session-scoped layout / viewport / M / I / maximize state.
//
// 정본:
// - ADR-0019 D5 (session-scoped store: 모든 layout/viewport/M/I 는 활성 session 단위)
// - ADR-0021 D5 (session switch 시 store reset)
// - ADR-0018 D1 (CanvasLayout envelope — schema v2)
// - ADR-0018 D7 + ADR-0024 D2 (z mutation 정책 — 4 액션은 별 store/util)
// - frontend-handover §3.1 (architectural invariants — store invariant 1)
// - frontend-handover §6 Stage 1.3
// - CONTEXT.md 의 "Manipulation Selection (M)" / "Input Target (I)" 정의
// - G20 amend: maximized 는 FE-only ephemeral (schema 외) — session 단위 1 item.
//
// Stage 1 skeleton 의 범위:
// - Type / 구조 / 기본 mutation method 만 정의.
// - 실제 HTTP fetch (`switchSession`) / WS frame dispatch 통합 / 기존
//   `panels.svelte.ts` 의 server-wide → session-scoped amend 는 Stage 2~4.
//
// Reactivity:
// - SvelteMap / SvelteSet — entry-level reactivity (`panels.svelte.ts` /
//   `mux.svelte.ts` 패턴 정합).
// - `$state` — primitive / object 의 reactive root.

import { SvelteMap, SvelteSet } from 'svelte/reactivity';

import { mutateLayout } from '$lib/http/sessions';
import { sessionStorageHint } from '$lib/stores/sessionStorageHint';
import type { CanvasItem, CanvasLayout, Viewport } from '$lib/types/canvas';
import type { Group } from '$lib/types/group';

/** Active session 의 식별 정보. attach 성공 시 set, detach 시 null. */
export interface ActiveSession {
  /** Session name (user-facing identifier, ADR-0019). */
  name: string;
}

/**
 * Reattach 결과 — `attemptReattach()` 의 분기 (ADR-0019 D5.1/D5.4 + plan-0008 §4).
 *
 * - `success`: 200 응답 + layout fetch + sessionStore.setActiveSession/loadLayout
 *              완료. 호출자는 본 화면 mount 진입 가능.
 * - `in_use`: 409. 다른 webpage 가 attach 보유.
 * - `not_found`: 404. session 이 BE 에서 사라짐. hint 도 자동 clear 됨.
 * - `unauthorized`: 401. cookie 만료 — caller 가 /auth redirect.
 * - `unreachable`: 5xx / network error / AbortError 외 fetch 실패. message 동봉.
 */
export type ReattachResult =
  | { kind: 'success' }
  | { kind: 'in_use'; holderPid?: number }
  | { kind: 'not_found' }
  | { kind: 'unauthorized' }
  | { kind: 'unreachable'; message: string };

/** Viewport 기본값 — session 없는 상태 / fresh layout 의 fallback. */
const DEFAULT_VIEWPORT: Viewport = { x: 0, y: 0, zoom: 1 };

/** `attemptReattach` 의 WS conn id stub — `WorkspaceSwitcher` 와 동일 패턴. */
function makeWsConnId(): string {
  return `webpage-${Math.random().toString(36).slice(2, 10)}`;
}

class SessionStore {
  /** 현 webpage 가 attach 한 session. null = pre-attach / post-detach. */
  active = $state<ActiveSession | null>(null);

  /** `items[]` 의 in-memory representation — id 키 SvelteMap. */
  items = $state(new SvelteMap<string, CanvasItem>());

  /** `groups[]` 의 in-memory representation — id 키 SvelteMap. */
  groups = $state(new SvelteMap<string, Group>());

  /** Viewport (panel/zoom). 양방향 sync 대상 (Stage 7 FE-9). */
  viewport = $state<Viewport>({ ...DEFAULT_VIEWPORT });

  /**
   * Manipulation Selection — 사용자가 제어 대상으로 잡은 Items 의 id 집합.
   * 다중 선택. CONTEXT.md 정의. session-scoped.
   */
  M = $state(new SvelteSet<string>());

  /**
   * Input Target — 키보드 입력이 라우팅되는 terminal id (또는 null).
   * 한 session 안 unique. CONTEXT.md 정의.
   */
  I = $state<string | null>(null);

  /**
   * FE-only ephemeral. Canvas viewport-fill 로 확대된 panel 의 id.
   * 한 session 1 panel (G20 amend). attach/switch 시 자동 null 로 reset.
   * schema 영속 X — 즉 어떤 PUT/GET 도 이 값을 노출하지 않음.
   */
  maximizedItemId = $state<string | null>(null);

  /**
   * Focus mode (ADR-0017 §D5). FE-only ephemeral — session 단위.
   * targetPanelId === null 이면 currently-selected M[0] 이 대상.
   * Stage 7 G27 의 시각 효과는 후속.
   */
  focusMode = $state<{ enabled: boolean; targetPanelId: string | null }>({
    enabled: false,
    targetPanelId: null,
  });

  /* ────────────────────────────────────────────────────────────────────── */
  /* Layout lifecycle (loaded ↔ cleared)                                    */
  /* ────────────────────────────────────────────────────────────────────── */

  /**
   * Layout 적용. attach 성공 / LAYOUT_CHANGED rebuild 시 호출.
   * 기존 state 는 모두 폐기 (M/I/maximize 포함) — session 단위 fresh load.
   */
  loadLayout(layout: CanvasLayout): void {
    this.items.clear();
    for (const it of layout.items) {
      this.items.set(it.id, it);
    }
    this.groups.clear();
    for (const g of layout.groups) {
      this.groups.set(g.id, g);
    }
    this.viewport = { ...layout.viewport };
    this.M.clear();
    this.I = null;
    this.maximizedItemId = null;
    this.focusMode = { enabled: false, targetPanelId: null };
  }

  /**
   * Session attach 진입. Stage 2~3 의 attach handler 가 호출 — 본 skeleton
   * 은 state set 만 (실 HTTP/WS 통합은 후속).
   *
   * sessionStorage hint 도 함께 set — ADR-0019 D5.4 + plan-0008 §4.5.
   * AppPage 다음 reload 시점에 ReconnectModal trigger 의 입력.
   */
  setActiveSession(session: ActiveSession): void {
    this.active = session;
    sessionStorageHint.set(session.name);
  }

  /**
   * Detach / pre-attach 상태로 reset. Session switch 흐름의 시작점.
   *
   * sessionStorage hint 도 함께 clear — 명시 detach / logout / [Switch session…]
   * / session [Delete] 흐름 모두 통과. 다음 reload 도 dialog 흐름으로 자연 회귀.
   */
  clear(): void {
    this.active = null;
    this.items.clear();
    this.groups.clear();
    this.viewport = { ...DEFAULT_VIEWPORT };
    this.M.clear();
    this.I = null;
    this.maximizedItemId = null;
    this.focusMode = { enabled: false, targetPanelId: null };
    sessionStorageHint.clear();
  }

  /**
   * Session switch — 현재 session detach + 새 session attach.
   *
   * Stage 1 skeleton 에서는 *reset 만* 처리. 실제 HTTP `POST /api/sessions/<name>/attach`
   * + match-or-spawn confirm dialog (ADR-0018 D6) 는 Stage 2~3 에서 추가.
   * Caller 가 attach 응답으로 받은 layout 을 `loadLayout()` 에 넘김.
   */
  switchSession(name: string): void {
    this.clear();
    this.active = { name };
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* M (Manipulation Selection) — multi-id set                              */
  /* ────────────────────────────────────────────────────────────────────── */

  setM(ids: Iterable<string>): void {
    this.M.clear();
    for (const id of ids) this.M.add(id);
  }

  addToM(id: string): void {
    this.M.add(id);
  }

  removeFromM(id: string): void {
    this.M.delete(id);
  }

  toggleM(id: string): void {
    if (this.M.has(id)) this.M.delete(id);
    else this.M.add(id);
  }

  clearM(): void {
    this.M.clear();
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* I (Input Target) — single terminal id                                  */
  /* ────────────────────────────────────────────────────────────────────── */

  setI(terminalId: string | null): void {
    this.I = terminalId;
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* Maximize — FE-only ephemeral, 1-at-a-time (G20)                        */
  /* ────────────────────────────────────────────────────────────────────── */

  maximize(itemId: string): void {
    this.maximizedItemId = itemId;
  }

  unmaximize(): void {
    this.maximizedItemId = null;
  }

  toggleMaximize(itemId: string): void {
    this.maximizedItemId = this.maximizedItemId === itemId ? null : itemId;
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* Viewport                                                               */
  /* ────────────────────────────────────────────────────────────────────── */

  /** Debounce timer for viewport PUT (Stage 7 FE-9). */
  #viewportTimer: ReturnType<typeof setTimeout> | null = null;
  static readonly VIEWPORT_DEBOUNCE_MS = 500;

  /**
   * Update the in-memory viewport and schedule a debounced PUT to
   * persist `viewport` into the session's layout.
   *
   * The debounce coalesces rapid pan/zoom into a single network round
   * trip. Failures are swallowed (logged) — viewport is "close enough"
   * ephemeral state; a missed write only costs the last 500ms of
   * panning on the next reload.
   */
  updateViewport(v: Viewport): void {
    const cur = this.viewport;
    if (
      Math.abs(cur.x - v.x) < 0.5 &&
      Math.abs(cur.y - v.y) < 0.5 &&
      Math.abs(cur.zoom - v.zoom) < 0.001
    ) {
      return;
    }
    this.viewport = { ...v };
    if (this.active === null) return;
    if (this.#viewportTimer !== null) clearTimeout(this.#viewportTimer);
    this.#viewportTimer = setTimeout(() => {
      this.#viewportTimer = null;
      void this.#flushViewport();
    }, SessionStore.VIEWPORT_DEBOUNCE_MS);
  }

  async #flushViewport(): Promise<void> {
    const active = this.active;
    if (active === null) return;
    const v = { ...this.viewport };
    try {
      await mutateLayout(active.name, (cur) => ({ ...cur, viewport: v }));
    } catch (err) {
      console.debug('[gtmux] viewport persist failed', err);
    }
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* Reattach — ADR-0019 D5.1 / D5.4, plan-0008 §4.2                        */
  /* ────────────────────────────────────────────────────────────────────── */

  /**
   * Reattach to `name` — silent / blocking 의 공통 utility.
   *
   * - POST /api/sessions/<name>/attach (cookie + AbortSignal)
   * - 200 → GET /layout → setActiveSession + loadLayout → `success`
   * - 409 → `in_use` (holder.pid 추출)
   * - 404 → `not_found` (hint 도 자동 clear — caller 도 `cancel()` 으로 재clear 가능)
   * - 401 → `unauthorized` (caller 가 /auth redirect)
   * - 5xx / network / fetch throw → `unreachable` (단 AbortError 는 caller 가
   *   signal.aborted 로 자체 detect — 본 method 도 `unreachable` 로 반환하나
   *   caller 가 signal 확인 후 무시)
   *
   * BE 가 200 + `unmatched.length > 0` 응답해도 reattach 흐름에서는 그대로
   * `success` — confirm dialog 진입 안 함 (plan-0008 §8 risk row).
   */
  async attemptReattach(
    name: string,
    signal?: AbortSignal,
  ): Promise<ReattachResult> {
    let attachRes: Response;
    try {
      attachRes = await fetch(
        `/api/sessions/${encodeURIComponent(name)}/attach`,
        {
          method: 'POST',
          headers: {
            Accept: 'application/json',
            'Content-Type': 'application/json',
          },
          credentials: 'include',
          body: JSON.stringify({ ws_conn_id: makeWsConnId() }),
          signal,
        },
      );
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      return { kind: 'unreachable', message };
    }

    if (attachRes.status === 401) {
      sessionStorageHint.clear();
      return { kind: 'unauthorized' };
    }
    if (attachRes.status === 404) {
      sessionStorageHint.clear();
      return { kind: 'not_found' };
    }
    if (attachRes.status === 409) {
      try {
        const body = (await attachRes.json()) as {
          holder?: { pid?: number };
        };
        return { kind: 'in_use', holderPid: body.holder?.pid };
      } catch {
        return { kind: 'in_use' };
      }
    }
    if (attachRes.status >= 500) {
      return {
        kind: 'unreachable',
        message: `server responded ${attachRes.status}`,
      };
    }
    if (!attachRes.ok) {
      return {
        kind: 'unreachable',
        message: `attach returned ${attachRes.status}`,
      };
    }

    // 200 — drain body (lock acquired). matched/unmatched 무시 — confirm dialog
    // 는 reattach 흐름의 책임 외 (plan-0008 §8).
    try {
      await attachRes.json();
    } catch {
      /* body 형식 변화 무관 — 다음 layout fetch 가 진실 */
    }

    let layoutRes: Response;
    try {
      layoutRes = await fetch(
        `/api/sessions/${encodeURIComponent(name)}/layout`,
        {
          method: 'GET',
          headers: { Accept: 'application/json' },
          credentials: 'include',
          signal,
        },
      );
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      return { kind: 'unreachable', message };
    }
    if (layoutRes.status === 401) {
      sessionStorageHint.clear();
      return { kind: 'unauthorized' };
    }
    if (!layoutRes.ok) {
      return {
        kind: 'unreachable',
        message: `layout fetch returned ${layoutRes.status}`,
      };
    }
    try {
      const layout = (await layoutRes.json()) as CanvasLayout;
      this.setActiveSession({ name });
      this.loadLayout(layout);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      return { kind: 'unreachable', message: `layout parse: ${message}` };
    }
    return { kind: 'success' };
  }

  /* ────────────────────────────────────────────────────────────────────── */
  /* Silent reattach + mutation guard — plan-0008 Phase 2 (Case II)         */
  /* ────────────────────────────────────────────────────────────────────── */

  /**
   * Phase 2 — silent reattach in-flight 여부. visibilitychange / WS reconnect
   * 등의 trigger 가 호출. mutation guard 가 본 promise 를 await.
   */
  reattachInProgress = $state<boolean>(false);

  /** Phase 2 의 마지막 silent reattach 결과 — failure 분기 입력. */
  lastSilentReattachResult = $state<ReattachResult | null>(null);

  /** Internal promise — 동시 호출 dedup. */
  #silentReattachPromise: Promise<ReattachResult> | null = null;

  /**
   * Silent reattach utility — Phase 2 trigger 가 호출.
   *
   * 동시 호출은 동일 promise share (dedup). 결과는 `lastSilentReattachResult`
   * 에 보관. 호출자가 결과에 따라:
   *  - 'success': mutation 계속 진행.
   *  - 'in_use'/'not_found'/'unauthorized'/'unreachable': caller 가 modal/toast.
   */
  silentReattach(name: string, signal?: AbortSignal): Promise<ReattachResult> {
    if (this.#silentReattachPromise !== null) return this.#silentReattachPromise;
    this.reattachInProgress = true;
    const promise = (async (): Promise<ReattachResult> => {
      try {
        const result = await this.attemptReattach(name, signal);
        this.lastSilentReattachResult = result;
        return result;
      } finally {
        this.reattachInProgress = false;
        this.#silentReattachPromise = null;
      }
    })();
    this.#silentReattachPromise = promise;
    return promise;
  }

  /**
   * Mutation guard — outgoing write 진입점 (mutateLayout / deleteItem /
   * attachConfirm 등) 의 *바로 직전* 에 await 하는 helper.
   *
   * 동작:
   *  - reattach in-flight 면 끝날 때까지 await + result === success 면 통과.
   *  - lastSilentReattachResult 가 fail 상태면 그 결과 그대로 반환 (caller
   *    가 modal/toast 분기). caller 는 `await ... ; if (!ok) return;` 패턴.
   *  - 아무 trigger 도 없었으면 ok 반환 (no-op).
   *
   * 호출 예:
   *   const guard = await sessionStore.guardOutgoingMutation();
   *   if (!guard.ok) return;
   *   await mutateLayout(...);
   */
  async guardOutgoingMutation(): Promise<{ ok: boolean; result?: ReattachResult }> {
    if (this.#silentReattachPromise !== null) {
      const result = await this.#silentReattachPromise;
      return { ok: result.kind === 'success', result };
    }
    if (this.lastSilentReattachResult !== null && this.lastSilentReattachResult.kind !== 'success') {
      return { ok: false, result: this.lastSilentReattachResult };
    }
    return { ok: true };
  }
}

/** Single session-scoped store instance. */
export const sessionStore = new SessionStore();
