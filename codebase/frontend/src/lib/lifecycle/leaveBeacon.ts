// Page-unload lock release — `POST /api/leave?webpage_id=<id>` via
// `navigator.sendBeacon`. ADR-0021 D6 amend ② / 0071 §D-5 의 FE 짝.
//
// 정상 탭 close (또는 navigate-away) 시 BE attach lock 의 즉시 release.
// 본 helper 가 없으면 30s heartbeat timeout 까지 lock 잔존 — 다른 webpage
// 가 같은 session 진입 시 그 동안 409.
//
// 설계 결정 (0073 §D Q5/Q6):
// - `beforeunload` + `pagehide` 둘 다 listen — `beforeunload` 가 fire 안
//   하는 iOS Safari + page cache (BFCache) 케이스도 cover.
// - `webpage_id` 는 URL query — `sendBeacon` 의 custom header 제한 우회
//   (`X-Gtmux-Webpage-Id` header 못 씀). BE 의 `webpage_id_from_query`
//   (ADR-0019 D5.6) 와 정합.
// - best-effort: sendBeacon 의 return value (boolean) 무시. BE 의 30s
//   heartbeat fallback (ADR-0021 D6.2) 이 안전망 — 호출 실패해도 lock
//   leak 영구화 위험 0.
// - idempotent: 두 listener 가 같은 cycle 에 fire 해도 BE 의 `leave_handler`
//   가 보유 lock 없으면 no-op. 중복 호출 안 막음.
// - bound flag 로 동일 module 의 bind 중복 방지 — page navigation 시
//   onMount 가 다시 호출되어 listener leak 되는 케이스 차단.
// - module 위치 `lib/lifecycle/` — session lifecycle (`lib/session/`) 과
//   page lifecycle 의 책임 분리.

import { getWebpageId } from '$lib/session/webpageId';

let bound = false;

function sendLeave(): void {
  if (typeof navigator === 'undefined' || typeof navigator.sendBeacon !== 'function') {
    return;
  }
  try {
    const webpageId = encodeURIComponent(getWebpageId());
    const url = `/api/leave?webpage_id=${webpageId}`;
    // sendBeacon 의 default Content-Type 은 `text/plain;charset=UTF-8`.
    // BE 는 body 무시 + query 의 webpage_id 만 본다.
    navigator.sendBeacon(url, new Blob([], { type: 'text/plain;charset=UTF-8' }));
  } catch (e) {
    // page unload 직전이라 toast / console.warn 둘 다 무의미. console.debug
    // 만 — 개발자가 devtools 열어두면 trace 가능.
    console.debug('[gtmux] leaveBeacon: send failed', e);
  }
}

export function bind(): void {
  if (bound) return;
  if (typeof window === 'undefined') return; // SSR/test guard
  window.addEventListener('beforeunload', sendLeave);
  window.addEventListener('pagehide', sendLeave);
  bound = true;
}

export function unbind(): void {
  if (!bound) return;
  if (typeof window === 'undefined') return;
  window.removeEventListener('beforeunload', sendLeave);
  window.removeEventListener('pagehide', sendLeave);
  bound = false;
}
