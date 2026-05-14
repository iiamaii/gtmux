// HTTP `GET/PUT /api/layout` 클라이언트 — Pull-through-notify 의 GET 절반.
//
// 정본:
// - `docs/adr/0006-persistence-storage.md` (HTTP GET/PUT, ETag, 412 rebase 정책)
// - `docs/ssot/canvas-layout-schema.md` §2 (ETag 정규화 — HTTP 구간 hex)
// - `docs/ssot/wire-protocol.md` §2.2 (0x80 LAYOUT_CHANGED 신호 → re-fetch trigger)
// - `docs/reports/0008-frontend-stack.md` §F5 (HTTPClient + 디바운스 commit 패턴)
//
// 본 모듈은 *읽기 경로 (GET)* 만 구현한다. PUT (디바운스 300ms commit) 은 별도
// 트랙. dispatcher 가 0x80 LAYOUT_CHANGED 수신 시 `setLayoutRefetchHandler` 로
// 본 모듈의 `fetchLayoutAndHydrate` 를 트리거.
//
// 인증: ADR-0003 D5 의 Bearer 토큰을 `Authorization: Bearer <token>` 헤더로 전달.
// WS Sec-WebSocket-Protocol 과 동일 토큰 — `+page.svelte` 의 sessionStorage 진실.

import { connectionStore } from '$lib/stores/connection.svelte';
import { groupsStore, type Group } from '$lib/stores/groups.svelte';
import { layoutStore } from '$lib/stores/layout.svelte';
import { panelsStore, type Panel } from '$lib/stores/panels.svelte';
import { SvelteMap } from 'svelte/reactivity';

/** API 응답 shape — `docs/ssot/canvas-layout-schema.md` 의 LayoutSnapshot. */
export interface LayoutSnapshot {
  readonly schema_version: number;
  readonly panels: readonly Panel[];
  readonly groups: readonly Group[];
}

export interface FetchLayoutResult {
  /** `null` = 304 Not Modified (현재 etag 유지). */
  readonly snapshot: LayoutSnapshot | null;
  readonly etag: string;
}

/**
 * `GET /api/layout` — If-None-Match 로 현재 store etag 동봉. 304 시 store 갱신 없이
 * 반환만, 200 시 panels / groups / etag 를 store 에 hydrate.
 *
 * dispatcher 의 `LAYOUT_CHANGED` 트리거가 본 함수를 호출한다 (Pull-through-notify).
 *
 * @param token base64url 인증 토큰
 * @param currentEtag store 가 보유한 hex etag (없으면 `null`)
 */
export async function fetchLayoutAndHydrate(
  token: string,
  currentEtag: string | null,
): Promise<FetchLayoutResult | null> {
  const headers: Record<string, string> = {
    Accept: 'application/json',
    Authorization: `Bearer ${token}`,
  };
  if (currentEtag !== null) {
    // canvas-layout-schema §2: HTTP 구간 ETag 는 hex 문자열. quoted ETag 헤더
    // 컨벤션에 맞춰 `"<hex>"` 형식으로 wrap.
    headers['If-None-Match'] = `"${currentEtag}"`;
  }

  let res: Response;
  try {
    res = await fetch('/api/layout', { method: 'GET', headers, credentials: 'same-origin' });
  } catch (e) {
    console.debug('[gtmux] /api/layout fetch failed', e);
    return null;
  }

  // 304: server 가 그대로라고 통보 — store 갱신 없이 반환.
  if (res.status === 304) {
    return { snapshot: null, etag: currentEtag ?? '' };
  }
  if (!res.ok) {
    console.warn('[gtmux] /api/layout returned', res.status);
    return null;
  }

  const etag = stripQuotes(res.headers.get('ETag') ?? '');
  let snapshot: LayoutSnapshot;
  try {
    snapshot = (await res.json()) as LayoutSnapshot;
  } catch (e) {
    console.warn('[gtmux] /api/layout JSON parse failed', e);
    return null;
  }

  // Hydrate stores. SvelteMap 은 entry-level reactivity 를 위해 *지우고 다시 채움*
  // 대신 *diff* 가 이상적이나, MVP 는 전체 교체 (R8 §F3 에 따라 후속 최적화).
  hydratePanels(snapshot.panels);
  hydrateGroups(snapshot.groups);
  layoutStore.setEtag(etag);
  layoutStore.schemaVersion = snapshot.schema_version;
  return { snapshot, etag };
}

/**
 * 0x80 LAYOUT_CHANGED 수신 시 dispatcher 가 호출하는 *adapter* — Pull-through-notify
 * 의 마지막 단계. token 은 외부 클로저로 주입된다 (`+page.svelte` 가 createDispatcher
 * 호출 시 함께 setLayoutRefetchHandler).
 *
 * `etag` 인자는 broadcast 페이로드의 raw 16B — 현재 구현은 사용하지 않고 GET 의
 * 응답 ETag 를 권위로 삼는다 (broadcast 와 GET 사이 race 가 발생해도 GET 결과가
 * 최종). 인자는 향후 412 rebase 시 If-Match optimistic concurrency 의 입력으로
 * 활용 가능 — 그 때 본 시그니처가 의미를 가진다.
 */
export function createLayoutRefetchHandler(
  token: string,
): (etag: Uint8Array) => Promise<void> {
  return async (_etag: Uint8Array): Promise<void> => {
    await fetchLayoutAndHydrate(token, layoutStore.etag);
  };
}

// ── helpers ────────────────────────────────────────────────────────────────

function hydratePanels(panels: readonly Panel[]): void {
  const next = new SvelteMap<string, Panel>();
  for (const p of panels) {
    next.set(p.id, p);
  }
  panelsStore.panels = next;
}

function hydrateGroups(groups: readonly Group[]): void {
  const next = new SvelteMap<string, Group>();
  for (const g of groups) {
    next.set(g.id, g);
  }
  groupsStore.groups = next;
}

function stripQuotes(s: string): string {
  if (s.length >= 2 && s.startsWith('"') && s.endsWith('"')) {
    return s.slice(1, -1);
  }
  return s;
}

// ── PUT /api/layout — append-panel helper ──────────────────────────────────

/**
 * S5-FE-NEW-PANEL 의 PUT 절반 — *append-only* mutation. 현재 store 상태에 panel
 * 한 개를 끝에 추가하고 `If-Match` 와 함께 PUT.
 *
 * 412 (ETag mismatch) 수신 시 1회만 자동 rebase 한다: GET 으로 최신 snapshot 을
 * 받아 *그 위에* 다시 append 한다. 두 번째도 412 이면 호출 측에 throw — 사용자
 * 알림 트리거.
 *
 * 본 helper 는 *concurrent edit 모델 없이* 단일 사용자 가정 — `panels`/`groups`
 * 의 다른 mutation 이 동시에 in-flight 인 경우는 본 sprint 범위 밖 (P1+).
 */
export interface NewPanelInput {
  /** `p` prefix + ULID/UUID 36자 이내 (schema §1 Panel.id pattern). */
  readonly id: string;
  /** `%N` 형식. tmux mirror 와 정합. */
  readonly pane_id: string;
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
  readonly z: number;
}

export async function putLayoutAppendPanel(
  token: string,
  input: NewPanelInput,
): Promise<{ etag: string }> {
  // 첫 시도 — 현재 store 의 panels/groups 를 그대로 가져와 새 panel 을 추가.
  const firstEtag = layoutStore.etag;
  let result = await attemptAppend(token, input, firstEtag);
  if (result.kind === 'ok') return { etag: result.etag };
  if (result.kind === 'fatal') throw new Error(result.message);
  // 412 — GET 으로 rebase 후 1회 재시도.
  const refreshed = await fetchLayoutAndHydrate(token, null);
  if (refreshed === null) {
    throw new Error('layout rebase fetch failed');
  }
  const secondEtag = layoutStore.etag;
  result = await attemptAppend(token, input, secondEtag);
  if (result.kind === 'ok') return { etag: result.etag };
  if (result.kind === 'fatal') throw new Error(result.message);
  throw new Error('layout PUT 412 after rebase — concurrent writer suspected');
}

type AttemptResult =
  | { kind: 'ok'; etag: string }
  | { kind: 'rebase' } // 412
  | { kind: 'fatal'; message: string };

async function attemptAppend(
  token: string,
  input: NewPanelInput,
  currentEtag: string | null,
): Promise<AttemptResult> {
  // schema §1: panels[] 는 모든 기존 entries + 새 entry. groups 는 그대로 유지.
  const panels = [...panelsStore.panels.values()];
  const newPanel: Panel = {
    id: input.id,
    // 잠정 PanelsStore.Panel 의 placeholder 정의에는 미정 필드가 많으므로 cast.
    // schema §1 의 모든 required 필드를 명시한다 — visibility/minimized/locked/parent_id.
    ...({
      parent_id: null,
      pane_id: input.pane_id,
      x: input.x,
      y: input.y,
      w: input.w,
      h: input.h,
      z: input.z,
      visibility: true,
      minimized: false,
      locked: false,
      label: null,
      note: null,
    } as Record<string, unknown>),
  };
  panels.push(newPanel);
  const groups = [...groupsStore.groups.values()];

  // 본 helper 는 schema_version 을 store 값에서 가져온다 (hydrate 직후 값 유지).
  const body = JSON.stringify({
    schema_version: layoutStore.schemaVersion,
    etag: currentEtag ?? '00000000000000000000000000000000',
    panels,
    groups,
  });

  const headers: Record<string, string> = {
    Accept: 'application/json',
    'Content-Type': 'application/json',
    Authorization: `Bearer ${token}`,
  };
  if (currentEtag !== null) {
    headers['If-Match'] = `"${currentEtag}"`;
  }

  let res: Response;
  try {
    res = await fetch('/api/layout', {
      method: 'PUT',
      headers,
      credentials: 'same-origin',
      body,
    });
  } catch (e) {
    return { kind: 'fatal', message: `PUT /api/layout network failure: ${String(e)}` };
  }
  if (res.status === 204) {
    const etag = stripQuotes(res.headers.get('ETag') ?? '');
    return { kind: 'ok', etag };
  }
  if (res.status === 412) {
    return { kind: 'rebase' };
  }
  return { kind: 'fatal', message: `PUT /api/layout returned ${res.status}` };
}

// ── PUT /api/layout — commit current store helper ───────────────────────────

/**
 * Send the *current* panels/groups store snapshot to the server. Used by
 * the drag-stop handler so a moved panel persists across the next
 * LAYOUT_CHANGED-driven re-hydrate (SvelteFlow re-renders from the store,
 * so without this commit the next selection event snaps the panel back
 * to its pre-drag position).
 *
 * 412 (ETag mismatch) is auto-rebased once: fetchLayoutAndHydrate replaces
 * the local store with the server view, after which the local edits are
 * lost — the caller should re-issue the move. Higher-level UX is P1+.
 */
export async function putLayoutCommitCurrent(token: string): Promise<{ etag: string }> {
  const firstEtag = layoutStore.etag;
  let result = await commitCurrent(token, firstEtag);
  if (result.kind === 'ok') return { etag: result.etag };
  if (result.kind === 'fatal') throw new Error(result.message);
  const refreshed = await fetchLayoutAndHydrate(token, null);
  if (refreshed === null) throw new Error('layout rebase fetch failed');
  result = await commitCurrent(token, layoutStore.etag);
  if (result.kind === 'ok') return { etag: result.etag };
  if (result.kind === 'fatal') throw new Error(result.message);
  throw new Error('layout PUT 412 after rebase — concurrent writer suspected');
}

async function commitCurrent(token: string, currentEtag: string | null): Promise<AttemptResult> {
  const panels = [...panelsStore.panels.values()];
  const groups = [...groupsStore.groups.values()];
  const body = JSON.stringify({
    schema_version: layoutStore.schemaVersion,
    etag: currentEtag ?? '00000000000000000000000000000000',
    panels,
    groups,
  });
  const headers: Record<string, string> = {
    Accept: 'application/json',
    'Content-Type': 'application/json',
    Authorization: `Bearer ${token}`,
  };
  if (currentEtag !== null) {
    headers['If-Match'] = `"${currentEtag}"`;
  }
  let res: Response;
  try {
    res = await fetch('/api/layout', {
      method: 'PUT',
      headers,
      credentials: 'same-origin',
      body,
    });
  } catch (e) {
    return { kind: 'fatal', message: `PUT /api/layout network failure: ${String(e)}` };
  }
  if (res.status === 204) {
    const etag = stripQuotes(res.headers.get('ETag') ?? '');
    layoutStore.setEtag(etag);
    return { kind: 'ok', etag };
  }
  if (res.status === 412) return { kind: 'rebase' };
  return { kind: 'fatal', message: `PUT /api/layout returned ${res.status}` };
}

// connectionStore 는 향후 fetch 실패 시 banner 트리거에 사용 — 본 task 범위에서는
// 단순 console.warn 으로 그치고, store 와의 wiring 은 P1+ 의 별도 트랙.
void connectionStore;

// ── appendPanelIfMissing — Stage I (ADR-0015) auto-mount entry ──────────────
//
// dispatcher 의 `pane-spawned` NOTIFY hook 과 NewPanelButton 의 명시 spawn
// path 두 곳에서 같은 가드를 통과한다. layout 에 이미 같은 pane_id 의 panel
// 이 있으면 *no-op return* — 다중 탭 / two-path race 시 두 번째 호출이
// 자연스럽게 흡수된다 (ADR-0015 D4 + D6).
//
// coords:
//   - 'cascade' (default) → origin + N×40px (N = 현 panels.size)
//   - { x, y } → 명시 좌표 (NewPanelButton 의 viewport-center 등)
//
// Panel.id 는 helper 내부에서 자동 생성. UUIDv4 의 hyphen 제거 + `p` prefix.

const PANEL_DEFAULT_W = 480;
const PANEL_DEFAULT_H = 320;
const CASCADE_STEP = 40;

export interface AppendPanelIfMissingOpts {
  /** Token for the PUT. Pass the same value the rest of the app uses. */
  readonly token: string;
  /** Optional explicit coords; otherwise cascade. */
  readonly coords?: { x: number; y: number };
}

function genPanelId(): string {
  const u =
    typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
      ? crypto.randomUUID()
      : `${Math.random().toString(16).slice(2)}${Math.random().toString(16).slice(2)}`;
  return `p${u.replace(/-/g, '').slice(0, 32)}`;
}

function nextZ(): number {
  let max = 0;
  for (const p of panelsStore.panels.values() as Iterable<Record<string, unknown>>) {
    const z = typeof p['z'] === 'number' ? (p['z'] as number) : 0;
    if (z > max) max = z;
  }
  return max + 1;
}

function cascadeCoords(): { x: number; y: number } {
  const n = panelsStore.panels.size;
  return { x: n * CASCADE_STEP, y: n * CASCADE_STEP };
}

/** Idempotent auto-mount entry. Returns the panel id (existing or newly
 *  created), or `null` if the underlying PUT failed beyond rebase. */
export async function appendPanelIfMissing(
  paneId: number,
  opts: AppendPanelIfMissingOpts,
): Promise<string | null> {
  const expected = `%${paneId}`;
  // Idempotent guard — search panels for matching pane_id.
  for (const p of panelsStore.panels.values() as Iterable<Record<string, unknown>>) {
    if (p['pane_id'] === expected) {
      return typeof p['id'] === 'string' ? (p['id'] as string) : null;
    }
  }
  const coords = opts.coords ?? cascadeCoords();
  const panelId = genPanelId();
  try {
    await putLayoutAppendPanel(opts.token, {
      id: panelId,
      pane_id: expected,
      x: coords.x,
      y: coords.y,
      w: PANEL_DEFAULT_W,
      h: PANEL_DEFAULT_H,
      z: nextZ(),
    });
    return panelId;
  } catch (e) {
    // After 412 rebase the panel may already exist (another tab won the
    // race) — re-check the guard before declaring failure.
    for (const p of panelsStore.panels.values() as Iterable<Record<string, unknown>>) {
      if (p['pane_id'] === expected) {
        return typeof p['id'] === 'string' ? (p['id'] as string) : null;
      }
    }
    console.warn('[gtmux] appendPanelIfMissing failed', e);
    return null;
  }
}
