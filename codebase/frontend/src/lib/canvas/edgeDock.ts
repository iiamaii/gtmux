// edgeDock.ts — edge-dock with size-match pure geometry (ADR-0051).
//
// 정본: docs/adr/0051-edge-dock-size-match.md (D1~D8).
//
// DOM / store 무관 순수 함수 모듈 (node vitest 로 단위 테스트). Canvas.svelte 의
// onnodedrag / onnodedragstop 가 본 함수들을 호출해 proximity+dwell 판정과 drop
// commit 을 수행한다. 좌표는 모두 *canvas (flow) space* — caller 가 screen px 를
// viewport zoom 으로 나눠 canvas 좌표로 변환한 뒤 넘긴다 (ADR-0051 D2).
//
// Detection (amend ②): side 판정은 dragged-box-edge 가 아니라 *마우스 포인터*
// (canvas 좌표) ↔ target 4 side line segment 의 point-to-segment 거리 기반이다.
// 최소 거리 side 가 후보이며 grab-offset/박스 크기에 무관해 전 컴포넌트 동일
// 민감도를 갖는다 (ADR-0051 D2/D3 amend ②). 배치/커밋(computeDock) 은 불변.
//
// 재사용: 박스 수학은 `alignment.ts` 의 BBox 규약(x/y/w/h, top-left origin)과
// 정합. min-size 는 per-node NodeResizer minimum(아래 DOCK_MIN_SIZE)과 일치.

import type { CanvasItemType } from '$lib/types/canvas';

/** Axis-aligned box in canvas (flow) coordinates. */
export interface DockBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Side of the *target* that the dragged item docks against. */
export type DockSide = 'L' | 'R' | 'T' | 'B';

/** Minimum dimensions of the dragged item, used for clamp (ADR-0051 D6). */
export interface DockMin {
  w: number;
  h: number;
}

/** A target the dragged item may dock to. Pre-filtered by the caller. */
export interface DockTarget {
  id: string;
  box: DockBox;
  type: CanvasItemType;
}

/** Result of proximity scan — the chosen (target, side) and its line distance. */
export interface DockCandidate {
  targetId: string;
  side: DockSide;
  /**
   * Point-to-segment distance (canvas px) between the mouse pointer and the
   * chosen target side line segment (ADR-0051 D2/D3 amend ②). Smaller = closer.
   * Named `gap` for continuity with the prior box-edge model; the value now
   * measures pointer ↔ side-line proximity, not dragged-edge ↔ side gap.
   */
  gap: number;
}

/** A point in canvas (flow) coordinates. */
export interface DockPoint {
  x: number;
  y: number;
}

/** Final landing box after size-match + flush placement (ADR-0051 D5). */
export interface DockPlacement {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Eligible item types for docking (ADR-0051 D1).
 *
 * Figure types (rect / ellipse / line / path / text / free_draw) are excluded —
 * their aspect / angle semantics (ADR-0031) conflict with plain size matching.
 */
const ELIGIBLE_TYPES: ReadonlySet<CanvasItemType> = new Set<CanvasItemType>([
  'terminal',
  'note',
  'document',
  'image',
  'file_path',
  'snippets',
]);

/** True iff `type` is one of the 6 dock-eligible types (ADR-0051 D1). */
export function eligibleForDock(type: CanvasItemType): boolean {
  return ELIGIBLE_TYPES.has(type);
}

/**
 * Per-type minimum dimensions — mirrors each node's NodeResizer minWidth /
 * minHeight (PanelNode 240×140, NoteNode 160×60, DocumentNode 220×160,
 * ImageNode 120×80, FilePathNode 200×80, SnippetsNode 200×75). Used to clamp
 * the matched dimension (ADR-0051 D6). Caller may override via `computeDock`'s
 * `min` argument; this map is the canonical fallback for the 6 eligible types.
 */
export const DOCK_MIN_SIZE: Readonly<Record<string, DockMin>> = {
  terminal: { w: 240, h: 140 },
  note: { w: 160, h: 60 },
  document: { w: 220, h: 160 },
  image: { w: 120, h: 80 },
  file_path: { w: 200, h: 80 },
  snippets: { w: 200, h: 75 },
};

/** Min-size lookup for an eligible type. Falls back to {w:1,h:1} if unknown. */
export function dockMinForType(type: CanvasItemType): DockMin {
  return DOCK_MIN_SIZE[type] ?? { w: 1, h: 1 };
}

/** Vertical sides (L/R) match height; horizontal (T/B) match width. */
function isVerticalSide(side: DockSide): boolean {
  return side === 'L' || side === 'R';
}

/**
 * Perpendicular distance from point (px,py) to the axis-aligned line segment
 * [(ax,ay)→(bx,by)], clamped to the segment endpoints. For our 4 sides the
 * segment is always axis-aligned, so this reduces to: distance along the
 * constant axis + (if the point projects beyond the segment) the overshoot
 * along the varying axis. A pointer beyond a corner therefore measures the
 * straight-line distance to the nearest segment endpoint (ADR-0051 D3 amend ②).
 */
function pointToSegmentDist(
  px: number,
  py: number,
  ax: number,
  ay: number,
  bx: number,
  by: number,
): number {
  const dx = bx - ax;
  const dy = by - ay;
  const lenSq = dx * dx + dy * dy;
  // Degenerate (zero-length) segment → distance to the point.
  if (lenSq === 0) return Math.hypot(px - ax, py - ay);
  // Project (p − a) onto the segment, clamped to [0,1].
  let t = ((px - ax) * dx + (py - ay) * dy) / lenSq;
  if (t < 0) t = 0;
  else if (t > 1) t = 1;
  const cx = ax + t * dx;
  const cy = ay + t * dy;
  return Math.hypot(px - cx, py - cy);
}

/**
 * Find the nearest dock candidate for the mouse `pointer` across `targets`
 * (ADR-0051 D2/D3 amend ②).
 *
 * Detection is now POINTER-BASED, not dragged-box-edge-based. Each target's 4
 * sides are treated as line SEGMENTS (their actual edge extents); for every
 * side the point-to-segment distance from the pointer is measured (clamped to
 * the segment endpoints — a pointer beyond a corner measures distance to the
 * nearest endpoint). A side is a candidate only if its distance ≤ threshold
 * `T`. Among all candidates across all targets the MINIMUM distance wins; exact
 * ties broken by deterministic side order L,R,T,B (target iteration is stable).
 *
 * There is NO overlap requirement anymore — the pointer fully determines both
 * the target and the side. If the pointer is farther than `T` from every side
 * segment (e.g. deep inside a large target, away from all edges) there is no
 * candidate → null ("라인 위에서만 인지", user decision 2026-06-16 ②). Because
 * the metric is pointer-to-line distance it is size-independent and free of
 * grab-offset bias → uniform on-screen sensitivity across all components.
 *
 * Side segments (target box = x,y,w,h):
 *   L: x = x,     y ∈ [y, y+h]
 *   R: x = x+w,   y ∈ [y, y+h]
 *   T: y = y,     x ∈ [x, x+w]
 *   B: y = y+h,   x ∈ [x, x+w]
 *
 * Targets MUST be pre-filtered by the caller to: eligible type, not locked,
 * visible, not minimized, not the dragged item itself (ADR-0051 D8). This keeps
 * the high-frequency onnodedrag path linear in the (small) target count.
 *
 * @param pointer mouse pointer in canvas (flow) coordinates.
 * @param T proximity threshold in canvas coords (caller: screen px / zoom).
 * @returns the chosen candidate, or null if no side is within `T`.
 */
export function nearestDockCandidate(
  pointer: DockPoint,
  targets: readonly DockTarget[],
  T: number,
): DockCandidate | null {
  const px = pointer.x;
  const py = pointer.y;

  let best: DockCandidate | null = null;
  // Deterministic side precedence on exact distance ties (ADR-0051 D3).
  const sideRank: Record<DockSide, number> = { L: 0, R: 1, T: 2, B: 3 };

  for (const target of targets) {
    const tb = target.box;
    const tl = tb.x;
    const tr = tb.x + tb.w;
    const tt = tb.y;
    const tbot = tb.y + tb.h;

    // L side: vertical segment at x = tl, y ∈ [tt, tbot].
    considerCandidate(target.id, 'L', pointToSegmentDist(px, py, tl, tt, tl, tbot));
    // R side: vertical segment at x = tr, y ∈ [tt, tbot].
    considerCandidate(target.id, 'R', pointToSegmentDist(px, py, tr, tt, tr, tbot));
    // T side: horizontal segment at y = tt, x ∈ [tl, tr].
    considerCandidate(target.id, 'T', pointToSegmentDist(px, py, tl, tt, tr, tt));
    // B side: horizontal segment at y = tbot, x ∈ [tl, tr].
    considerCandidate(target.id, 'B', pointToSegmentDist(px, py, tl, tbot, tr, tbot));
  }

  function considerCandidate(targetId: string, side: DockSide, dist: number): void {
    if (dist > T) return;
    if (best === null) {
      best = { targetId, side, gap: dist };
      return;
    }
    if (dist < best.gap - 1e-6) {
      best = { targetId, side, gap: dist };
      return;
    }
    // Exact-distance tie → deterministic side order.
    if (Math.abs(dist - best.gap) <= 1e-6 && sideRank[side] < sideRank[best.side]) {
      best = { targetId, side, gap: dist };
    }
  }

  return best;
}

/** Input to `computeDock` describing the dragged item. */
export interface DockDragged {
  box: DockBox;
  type: CanvasItemType;
  /** Min dimensions for clamp (ADR-0051 D6). */
  min: DockMin;
  /**
   * Source aspect (w/h) for `image` (ADR-0051 D6 image 특례). When present and
   * the type is image, the orthogonal dimension is scaled to preserve aspect.
   * Ignored for non-image types.
   */
  aspect?: number;
}

/**
 * Compute the dragged item's landing box: size-match the matched dimension to
 * the target side, clamp to min, place flush (zero gap) at the side origin
 * (ADR-0051 D5 / D6).
 *
 * - Vertical side (L/R): match height to `targetBox.h`.
 * - Horizontal side (T/B): match width to `targetBox.w`.
 * - min-size CLAMP — docking always succeeds even if min is violated; on the
 *   clamped axis the dragged item stays larger than the target and shares the
 *   side origin / corner (ADR-0051 D6).
 * - IMAGE special case: set the matched dimension, then scale the orthogonal
 *   dimension by the source aspect (no distortion). The matched dimension is
 *   still clamped to min first; the orthogonal dimension is then derived from
 *   it and ALSO clamped to its own min (which may break exact aspect, but min
 *   wins — distortion-free yet never below the resizer floor).
 *
 * The dragged item's current size is otherwise preserved on the non-matched
 * axis (non-image) — only x/y/w/h are returned (ADR-0051 D7: z / type unchanged).
 */
export function computeDock(
  dragged: DockDragged,
  targetBox: DockBox,
  side: DockSide,
): DockPlacement {
  const vertical = isVerticalSide(side);
  const isImage = dragged.type === 'image';

  let w = dragged.box.w;
  let h = dragged.box.h;

  if (vertical) {
    // Match height to target; clamp to min height.
    h = Math.max(targetBox.h, dragged.min.h);
    if (isImage) {
      const aspect = resolveAspect(dragged);
      // Derive width from matched height, then clamp width to its own min.
      w = Math.max(h * aspect, dragged.min.w);
    }
  } else {
    // Match width to target; clamp to min width.
    w = Math.max(targetBox.w, dragged.min.w);
    if (isImage) {
      const aspect = resolveAspect(dragged);
      // Derive height from matched width, then clamp height to its own min.
      h = Math.max(w / aspect, dragged.min.h);
    }
  }

  // Flush placement at the target side origin (ADR-0051 D5).
  let x: number;
  let y: number;
  switch (side) {
    case 'R':
      x = targetBox.x + targetBox.w;
      y = targetBox.y;
      break;
    case 'L':
      x = targetBox.x - w;
      y = targetBox.y;
      break;
    case 'B':
      x = targetBox.x;
      y = targetBox.y + targetBox.h;
      break;
    case 'T':
      x = targetBox.x;
      y = targetBox.y - h;
      break;
  }

  return { x, y, w, h };
}

/** Resolve a usable image aspect (>0). Falls back to current box ratio. */
function resolveAspect(dragged: DockDragged): number {
  if (dragged.aspect !== undefined && Number.isFinite(dragged.aspect) && dragged.aspect > 0) {
    return dragged.aspect;
  }
  if (dragged.box.h > 0) return dragged.box.w / dragged.box.h;
  return 1;
}
