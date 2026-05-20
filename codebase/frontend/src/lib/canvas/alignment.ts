// alignment.ts — node 간 정렬 / 분포 pure function (plan-0010 Task 5, ADR-0027 D4~D8).
//
// 입력: 다중 선택의 CanvasItem array. 출력: id → new (x, y) Map (변경된 item 만).
// caller (ItemInfoView 의 핸들러) 가 본 Map 을 받아 mutateLayout PUT 으로 broadcast.
//
// 정합:
// - 기준 = selection 의 union BBox (ADR-0027 D5)
// - locked item 의 position 은 갱신 안 함, 단 BBox 계산엔 포함 (D7)
// - line item 은 endpoint (x, y) 와 (x2, y2) 둘 다 평행 이동 (D7)
// - distribute 는 N ≥ 3 일 때만 정의 (D8) — N < 3 면 빈 Map 반환
// - group child / minimized 도 일반 item 으로 처리 (D7)

import type { CanvasItem } from '$lib/types/canvas';

export type AlignMode =
  | 'left'
  | 'center-x'
  | 'right'
  | 'top'
  | 'center-y'
  | 'bottom';

export type DistributeMode = 'horizontal' | 'vertical';

/** Item 의 BBox — line 은 endpoints, 그 외는 x/y/w/h. */
interface BBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

function itemBBox(it: CanvasItem): BBox {
  if (it.type === 'line') {
    const x2 = (it as unknown as { x2: number }).x2;
    const y2 = (it as unknown as { y2: number }).y2;
    const x = Math.min(it.x, x2);
    const y = Math.min(it.y, y2);
    return { x, y, w: Math.abs(x2 - it.x), h: Math.abs(y2 - it.y) };
  }
  return { x: it.x, y: it.y, w: it.w, h: it.h };
}

function selectionBBox(items: CanvasItem[]): BBox | null {
  if (items.length === 0) return null;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const it of items) {
    const b = itemBBox(it);
    if (b.x < minX) minX = b.x;
    if (b.y < minY) minY = b.y;
    if (b.x + b.w > maxX) maxX = b.x + b.w;
    if (b.y + b.h > maxY) maxY = b.y + b.h;
  }
  return { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
}

/**
 * Item 의 *display anchor* — itemBBox 의 top-left. line 은 box top-left.
 * 평행 이동 delta 가 cur (item.x / item.y / endpoints) 에 동일 적용된다.
 */
export interface MoveResult {
  /** new x for item.x (line 은 새 시작점 x). */
  x: number;
  /** new y for item.y. */
  y: number;
  /** line 만 — 새 끝점 (x2, y2). 다른 type 은 undefined. */
  x2?: number;
  y2?: number;
}

/** delta 평행 이동을 한 item 에 적용. line 은 endpoints 둘 다. */
function moveItem(it: CanvasItem, dx: number, dy: number): MoveResult {
  if (it.type === 'line') {
    const x2 = (it as unknown as { x2: number }).x2;
    const y2 = (it as unknown as { y2: number }).y2;
    return { x: it.x + dx, y: it.y + dy, x2: x2 + dx, y2: y2 + dy };
  }
  return { x: it.x + dx, y: it.y + dy };
}

/**
 * Align — selection BBox 기준 (D5). locked 는 새 position skip (D7).
 *
 * 반환: id → MoveResult Map. *변경된 item 만* — 기존 position 과 동일하면
 * 포함 안 함 (mutateLayout 의 idempotent path 와 정합).
 */
export function alignItems(
  items: CanvasItem[],
  mode: AlignMode,
): Map<string, MoveResult> {
  const out = new Map<string, MoveResult>();
  if (items.length < 2) return out;
  const bbox = selectionBBox(items);
  if (bbox === null) return out;

  for (const it of items) {
    if (it.locked === true) continue;
    const b = itemBBox(it);
    let dx = 0;
    let dy = 0;
    switch (mode) {
      case 'left':
        dx = bbox.x - b.x;
        break;
      case 'center-x':
        dx = bbox.x + bbox.w / 2 - (b.x + b.w / 2);
        break;
      case 'right':
        dx = bbox.x + bbox.w - (b.x + b.w);
        break;
      case 'top':
        dy = bbox.y - b.y;
        break;
      case 'center-y':
        dy = bbox.y + bbox.h / 2 - (b.y + b.h / 2);
        break;
      case 'bottom':
        dy = bbox.y + bbox.h - (b.y + b.h);
        break;
    }
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) continue;
    out.set(it.id, moveItem(it, dx, dy));
  }
  return out;
}

/**
 * Distribute — 두 극단 (leftmost / rightmost 또는 top / bottom) 의 BBox center 는
 * 고정, 중간 item 들의 center 가 균등 간격으로 분포 (D5 / D8). N ≥ 3.
 */
export function distributeItems(
  items: CanvasItem[],
  mode: DistributeMode,
): Map<string, MoveResult> {
  const out = new Map<string, MoveResult>();
  if (items.length < 3) return out;

  const sorted = [...items].sort((a, b) => {
    const ba = itemBBox(a);
    const bb = itemBBox(b);
    if (mode === 'horizontal') return ba.x + ba.w / 2 - (bb.x + bb.w / 2);
    return ba.y + ba.h / 2 - (bb.y + bb.h / 2);
  });
  const first = sorted[0];
  const last = sorted[sorted.length - 1];
  if (first === undefined || last === undefined) return out;
  const bFirst = itemBBox(first);
  const bLast = itemBBox(last);
  const startCenter =
    mode === 'horizontal' ? bFirst.x + bFirst.w / 2 : bFirst.y + bFirst.h / 2;
  const endCenter =
    mode === 'horizontal' ? bLast.x + bLast.w / 2 : bLast.y + bLast.h / 2;
  const step = (endCenter - startCenter) / (sorted.length - 1);

  for (let i = 1; i < sorted.length - 1; i += 1) {
    const it = sorted[i];
    if (it === undefined) continue;
    if (it.locked === true) continue;
    const b = itemBBox(it);
    const targetCenter = startCenter + step * i;
    let dx = 0;
    let dy = 0;
    if (mode === 'horizontal') {
      dx = targetCenter - (b.x + b.w / 2);
    } else {
      dy = targetCenter - (b.y + b.h / 2);
    }
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) continue;
    out.set(it.id, moveItem(it, dx, dy));
  }
  return out;
}
