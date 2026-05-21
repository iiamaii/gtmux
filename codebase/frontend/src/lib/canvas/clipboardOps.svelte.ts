// clipboardOps — paste 본체 op (ADR-0030 D4/D6/D9).
//
// 정본:
// - ADR-0030 D3 — terminal paste = clone (fresh UUID → BE unmatched-spawn)
// - ADR-0030 D4 — bbox top-left + (dx, dy), 상대 위치 보존
// - ADR-0030 D6 — 모든 item 의 새 UUID
// - ADR-0030 D9 — applyMutation 통과 → historyStore 자동 capture
//
// clipboardShortcuts / ContextMenu / (Phase B) editingShortcuts 의 공통 helper.

import type { CanvasItem, CanvasLayout, LineItem } from '$lib/types/canvas';
import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { generateUuidV4 } from '$lib/uuid';

export interface PasteOptions {
  /** 누적 offset (dx, dy). clipboardStore.consumePasteOffset() 결과 또는 Duplicate 의 고정 offset. */
  offset: { dx: number; dy: number };
  /** Paste 후 selection 을 새 item 으로 교체 (default true, Figma 패턴). */
  setSelection?: boolean;
  failMessage?: string;
}

/**
 * Paste — source items 를 bbox + offset 으로 clone, active session 의 layout 에 append.
 * Terminal 도 fresh UUID → BE 의 unmatched-spawn 분기 자연 활용 (D3 clone).
 */
export async function pasteItems(
  sources: readonly CanvasItem[],
  options: PasteOptions,
): Promise<boolean> {
  if (sources.length === 0) return false;
  if (sessionStore.active === null) return false;

  const { dx, dy } = options.offset;
  const bboxX = sources.reduce((m, it) => Math.min(m, it.x), Number.POSITIVE_INFINITY);
  const bboxY = sources.reduce((m, it) => Math.min(m, it.y), Number.POSITIVE_INFINITY);
  const fresh = sources.map((src) => cloneWithOffset(src, bboxX, bboxY, dx, dy));

  const res = await sessionStore.applyMutation(
    (cur: CanvasLayout) => {
      const maxZ = cur.items.reduce((m, it) => Math.max(m, it.z), 0);
      const appended = fresh.map((it, i) => ({ ...it, z: maxZ + 1 + i }));
      return { ...cur, items: [...cur.items, ...appended] };
    },
    { failMessage: options.failMessage ?? 'Paste failed' },
  );

  if (res.ok && options.setSelection !== false) {
    sessionStore.setM(fresh.map((it) => it.id));
  }
  return res.ok;
}

/**
 * Full clone of `src` with a fresh UUID, repositioned by `(dx, dy)` from the
 * source group's bounding-box top-left.
 *
 * **Invariant** (ADR-0030 D3 amend ① — batch-5 R8 / Grill #17): every type,
 * *including `terminal`*, is full-cloned — `id` 만 새 UUID, 나머지 모든 필드
 * (label / description / visibility / locked / minimized / restored_geom /
 * parent_id / terminal_overrides) 는 source 에서 그대로 보존. Terminal 의 새
 * BE spawn 은 *id 가 unmatched* 라는 사실로 자연 trigger 됨 (D6 + ADR-0021
 * D6 match-or-spawn). 사용자 mental model = "복제 = 시각·메타 동일한 새
 * 인스턴스" — 따라서 terminal type 도 별도 metadata strip 분기 *없음*.
 *
 * Line 만 추가 처리: x2/y2 endpoint 도 offset 적용 (geometry 의 일부, 다른
 * type 의 w/h 같은 의미).
 *
 * 이 invariant 가 회귀되면 (e.g. terminal 분기로 label="" reset) batch-5 R8
 * 의 user-intent 가 깨지고, 0079 connector / 0080 asset 같은 후속 batch 의
 * "복제 = 동일" 가정이 무너짐.
 */
function cloneWithOffset(
  src: CanvasItem,
  bboxX: number,
  bboxY: number,
  dx: number,
  dy: number,
): CanvasItem {
  const clone = structuredClone($state.snapshot(src)) as CanvasItem;
  const out = {
    ...clone,
    id: generateUuidV4(),
    x: bboxX + dx + (src.x - bboxX),
    y: bboxY + dy + (src.y - bboxY),
  } as CanvasItem;
  if (out.type === 'line') {
    const lineSrc = src as LineItem;
    const lineOut = out as LineItem;
    lineOut.x2 = lineSrc.x2 + dx;
    lineOut.y2 = lineSrc.y2 + dy;
  }
  return out;
}
