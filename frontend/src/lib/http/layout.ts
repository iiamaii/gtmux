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

// ── 미사용 export 제거 (호환성) ────────────────────────────────────────────
//
// 기존 `getLayout()` placeholder 는 본 모듈의 fetchLayoutAndHydrate 로 대체되었다.
// 외부 caller 가 아직 없으므로 export 만 유지하지 않는다.

// connectionStore 는 향후 fetch 실패 시 banner 트리거에 사용 — 본 task 범위에서는
// 단순 console.warn 으로 그치고, store 와의 wiring 은 P1+ 의 별도 트랙.
void connectionStore;
