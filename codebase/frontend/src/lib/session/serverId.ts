// 0074 Phase 1 — Server boot identity observer.
//
// 문제 (`docs/reports/0074-webpage-auth-epoch-and-stale-tab.md` §1):
//   브라우저 탭 A 가 session 에 attach 한 채 열려있는 사이에 Server 가
//   종료/재시작 → 사용자가 다른 탭에서 재인증 → 새 cookie 발급 →
//   브라우저 cookie jar 공유로 탭 A 도 새 cookie 를 자동으로 싣게 됨.
//   탭 A 의 sessionStorage / store 는 그대로라 *bootstrap 을 다시
//   통과하지 않고도* attach/mutation 시도 가능. 이 stale 진입을 막아야
//   사용자가 *session 선택 흐름으로 돌아가도록* 보장된다.
//
// Phase 1 (FE detection only, 0074 §7.1):
//   1. BE 의 `GET /api/sessions` 응답이 `X-Gtmux-Server-Id` header 로
//      현재 boot 의 `server_id` (UUID v4) 를 송신.
//   2. FE 가 본 모듈로 *observed server_id* 를 `sessionStorage` 에
//      저장하고, 후속 응답의 server_id 와 비교.
//   3. mismatch 시 stale tab 으로 인식 — caller 가 등록한 *cleanup
//      handler* 를 1회 호출 + 저장된 id 를 새 값으로 갱신.
//
// 본 모듈은 *순수 detection*. 실제 cleanup 흐름 (sessionStore.clear /
// terminalPool.clear / reconnectGate.markIdle / workspaceSwitcher.open)
// 은 호출 측 (page mount + listSessions wrapper) 이 wire.
//
// 후속 (0074 Phase 2): BE 가 per-Webpage `webpage_boot_nonce` 발급 +
// write-sensitive endpoint guard 에서 검증. 본 Phase 1 는 *UX 회복* 만,
// Phase 2 가 *보안 강제*.

const STORAGE_KEY = 'gtmux_observed_server_id';

/** Cleanup callback. 동기 호출 — FE 측 store mutation 만 수행, 비동기 work
 *  (fetch 등) 가 필요하면 *발행 후* 호출 측에서 별도 처리. */
export type ServerIdMismatchHandler = () => void;

let mismatchHandler: ServerIdMismatchHandler | null = null;

/** Register the cleanup handler fired when an observed server_id differs
 *  from the one in sessionStorage. Call this once at page mount; the
 *  handler runs *every* mismatch (Server restart cascade) so it should be
 *  idempotent. Pass `null` to detach (test teardown). */
export function onServerIdMismatch(handler: ServerIdMismatchHandler | null): void {
  mismatchHandler = handler;
}

/** Pure observer — given an incoming server_id, compare with the stored
 *  one and fire the mismatch handler when they differ. Returns:
 *   - `'match'`     — same id (typical case, no side effect)
 *   - `'first'`     — first observation since the tab opened (stored, no handler call)
 *   - `'mismatch'`  — id differs from the stored one (handler fired, id updated)
 *   - `'no-storage'` — sessionStorage unavailable (SSR / privacy mode) — detection skipped
 *
 *  Idempotent: calling twice with the same id is a `'match'` second time. */
export function observeServerId(id: string | null | undefined): 'match' | 'first' | 'mismatch' | 'no-storage' {
  if (typeof id !== 'string' || id.length === 0) return 'no-storage';
  if (typeof sessionStorage === 'undefined') return 'no-storage';
  let stored: string | null;
  try {
    stored = sessionStorage.getItem(STORAGE_KEY);
  } catch {
    return 'no-storage';
  }
  if (stored === null) {
    try {
      sessionStorage.setItem(STORAGE_KEY, id);
    } catch {
      // best-effort — detection still runs in-memory next request
    }
    return 'first';
  }
  if (stored === id) return 'match';
  // Mismatch — Server restarted while this tab kept its local state.
  // Fire the handler *before* updating storage so the cleanup runs once
  // per fresh boot (subsequent requests will see the new id and match).
  try {
    if (mismatchHandler) mismatchHandler();
  } catch (e) {
    console.error('[server-id] mismatch handler threw', e);
  }
  try {
    sessionStorage.setItem(STORAGE_KEY, id);
  } catch {
    // ignore — next request will retry
  }
  return 'mismatch';
}

/** Test helper — drop the stored id (no handler call). Use between
 *  test cases that hit the observer. */
export function resetObservedServerId(): void {
  if (typeof sessionStorage === 'undefined') return;
  try {
    sessionStorage.removeItem(STORAGE_KEY);
  } catch {
    /* ignore */
  }
}

/** Test helper — read the stored id without observing. */
export function peekObservedServerId(): string | null {
  if (typeof sessionStorage === 'undefined') return null;
  try {
    return sessionStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}
