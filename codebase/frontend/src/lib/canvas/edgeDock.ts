// edgeDock.ts — edge-dock with size-match pure geometry (ADR-0051).
//
// 정본: docs/adr/0051-edge-dock-size-match.md (D1~D8).
//
// DOM / store 무관 순수 함수 모듈 (node vitest 로 단위 테스트). Canvas.svelte 의
// onnodedrag / onnodedragstop 가 본 함수들을 호출해 proximity+dwell 판정과 drop
// commit 을 수행한다. 좌표는 모두 *canvas (flow) space* — caller 가 screen px 를
// viewport zoom 으로 나눠 canvas 좌표로 변환한 뒤 넘긴다 (ADR-0051 D2).
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

/** Result of proximity scan — the chosen (target, side) and its edge gap. */
export interface DockCandidate {
  targetId: string;
  side: DockSide;
  /** Perpendicular distance between dragged edge and target side (canvas px). */
  gap: number;
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

/** Overlap length of [a0,a1] ∩ [b0,b1]. ≤0 means no overlap. */
function overlapLength(a0: number, a1: number, b0: number, b1: number): number {
  return Math.min(a1, b1) - Math.max(a0, b0);
}

/**
 * Find the nearest dock candidate for the dragged box across `targets`
 * (ADR-0051 D3).
 *
 * For each target side the dragged item's matching edge gap is measured and the
 * perpendicular-axis overlap is required (so a box far above/below a target's
 * right side does NOT dock). Among candidates within threshold `T` the minimum
 * gap wins; ties broken by deterministic side order L,R,T,B.
 *
 * Targets MUST be pre-filtered by the caller to: eligible type, not locked,
 * visible, not minimized, not the dragged item itself (ADR-0051 D8). This keeps
 * the high-frequency onnodedrag path linear in the (small) target count.
 *
 * @returns the chosen candidate, or null if none is within `T` with overlap.
 */
export function nearestDockCandidate(
  draggedBox: DockBox,
  targets: readonly DockTarget[],
  T: number,
): DockCandidate | null {
  const dl = draggedBox.x;
  const dr = draggedBox.x + draggedBox.w;
  const dt = draggedBox.y;
  const db = draggedBox.y + draggedBox.h;

  let best: DockCandidate | null = null;
  // Deterministic side precedence on exact gap ties (ADR-0051 D3).
  const sideRank: Record<DockSide, number> = { L: 0, R: 1, T: 2, B: 3 };

  for (const target of targets) {
    const tb = target.box;
    const tl = tb.x;
    const tr = tb.x + tb.w;
    const tt = tb.y;
    const tbot = tb.y + tb.h;

    // Vertical sides (L/R): require vertical-axis overlap.
    const vOverlap = overlapLength(dt, db, tt, tbot);
    if (vOverlap > 0) {
      // Right side: dragged left edge approaches target right edge.
      considerCandidate(target.id, 'R', Math.abs(dl - tr));
      // Left side: dragged right edge approaches target left edge.
      considerCandidate(target.id, 'L', Math.abs(dr - tl));
    }

    // Horizontal sides (T/B): require horizontal-axis overlap.
    const hOverlap = overlapLength(dl, dr, tl, tr);
    if (hOverlap > 0) {
      // Bottom side: dragged top edge approaches target bottom edge.
      considerCandidate(target.id, 'B', Math.abs(dt - tbot));
      // Top side: dragged bottom edge approaches target top edge.
      considerCandidate(target.id, 'T', Math.abs(db - tt));
    }
  }

  function considerCandidate(targetId: string, side: DockSide, gap: number): void {
    if (gap > T) return;
    if (best === null) {
      best = { targetId, side, gap };
      return;
    }
    if (gap < best.gap - 1e-6) {
      best = { targetId, side, gap };
      return;
    }
    // Exact-gap tie → deterministic side order.
    if (Math.abs(gap - best.gap) <= 1e-6 && sideRank[side] < sideRank[best.side]) {
      best = { targetId, side, gap };
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
