// layoutRefetch — ADR-0053 D7 의 side-effect 배선 (timer + store).
//
// 결정 로직은 `layoutRefetchGate.ts` (순수, vitest 대상), 본 모듈은:
// - dispatcher 의 0x80 수신 → debounce 후 `sessionStore.reloadActiveLayout()`.
// - Canvas node drag / NodeResizer resize 가 hold/release 로 defer 가드.
//
// Debounce 이유: 자기 발신 echo race — FE 의 PUT 이 유발한 0x80 broadcast 가
// HTTP 응답보다 *먼저* 도착할 수 있다 (WS/HTTP 순서 무보장). 150ms 지연 후
// `takeRunnable` 이 etag 를 재비교하므로, 그 사이 PUT 응답이 `layoutEtag` 를
// 갱신하면 refetch 가 생략된다. 외부발 (CLI ops) 변경엔 150ms 지연은 UX 무해.
//
// History 불변식: refetch 는 `reloadActiveLayout()` → `loadLayout()` 경유 —
// `historyStore.capture` 는 `applyMutation`/`applyDeletion` 만 수행하므로
// 외부발 반영은 undo stack 에 기록되지 않는다 (0x86 MOUNT_CASCADE 전례와
// 동일 정책, ADR-0028 D1.1).

import { layoutEtag } from '$lib/stores/layoutEtag';
import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { LayoutRefetchGate } from './layoutRefetchGate';

/** Echo-race 흡수 겸 다발 0x80 coalescing 용 debounce. */
export const LAYOUT_REFETCH_DEBOUNCE_MS = 150;

const gate = new LayoutRefetchGate();
let timer: ReturnType<typeof setTimeout> | null = null;

/**
 * Dispatcher 의 0x80 진입점. `etagHex` = payload 16B 의 hex. 이미 최신이면
 * no-op, 아니면 debounce 된 refetch 를 schedule (hold 중이면 defer).
 */
export function notifyLayoutChanged(etagHex: string): void {
  if (gate.receive(etagHex, layoutEtag.hex)) scheduleFlush();
}

/**
 * Interaction (node drag / panel resize) 시작. 반드시 정확히 1회의
 * `releaseLayoutRefetch()` 와 짝지어 호출할 것 — 미짝 시 refetch 가 영구
 * defer 된다.
 */
export function holdLayoutRefetch(): void {
  gate.hold();
}

/** Interaction 종료 — defer 된 refetch 가 있으면 debounce 후 수행. */
export function releaseLayoutRefetch(): void {
  if (gate.release()) scheduleFlush();
}

function scheduleFlush(): void {
  if (timer !== null) return;
  timer = setTimeout(() => {
    timer = null;
    void flush();
  }, LAYOUT_REFETCH_DEBOUNCE_MS);
}

async function flush(): Promise<void> {
  const target = gate.takeRunnable(layoutEtag.hex);
  if (target === null) return;
  // Silent best-effort (reloadActiveLayout 정책 그대로) — 실패 시 다음 0x80
  // 이 자연 재시도 트리거. active === null 이면 내부에서 false 반환.
  const ok = await sessionStore.reloadActiveLayout();
  if (!ok) {
    console.debug('[ws] external layout refetch failed (etag %s)', target);
  }
}
