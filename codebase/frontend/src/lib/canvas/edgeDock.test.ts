import { describe, it, expect } from 'vitest';
import {
  eligibleForDock,
  nearestDockCandidate,
  computeDock,
  dockMinForType,
  type DockBox,
  type DockTarget,
  type DockDragged,
} from '$lib/canvas/edgeDock';

// ADR-0051 — edge-dock with size-match. Pure geometry coverage:
// detection (amend ②, pointer-based): pointer near each of the 4 side segments,
// deep-center → null, beyond-corner (segment-endpoint) within/over T, nearest of
// two targets, tie-break order. Placement (computeDock) is UNCHANGED:
// 4 sides × { normal, min-clamp, image-aspect-preserve }.

const T = 20; // proximity threshold (canvas px).

// Helper: a target box at a fixed location.
function target(box: DockBox, type: DockTarget['type'] = 'note'): DockTarget {
  return { id: 't1', box, type };
}

const TARGET: DockBox = { x: 100, y: 100, w: 200, h: 160 };
// TARGET sides: L x=100, R x=300, T y=100, B y=260; extents x∈[100,300] y∈[100,260].

describe('eligibleForDock', () => {
  it('accepts the 6 eligible types', () => {
    for (const t of ['terminal', 'note', 'document', 'image', 'file_path', 'snippets'] as const) {
      expect(eligibleForDock(t)).toBe(true);
    }
  });
  it('rejects figure types (D1)', () => {
    for (const t of ['rect', 'ellipse', 'line', 'path', 'text', 'free_draw'] as const) {
      expect(eligibleForDock(t)).toBe(false);
    }
  });
});

describe('nearestDockCandidate — pointer near each side segment (amend ②)', () => {
  it('RIGHT side: pointer just outside the right edge', () => {
    // R segment x=300, y∈[100,260]. Pointer (312, 180) → dist 12, within extent.
    const c = nearestDockCandidate({ x: 312, y: 180 }, [target(TARGET)], T);
    expect(c).not.toBeNull();
    expect(c?.side).toBe('R');
    expect(c?.gap).toBeCloseTo(12);
  });

  it('LEFT side: pointer just outside the left edge', () => {
    // L segment x=100, y∈[100,260]. Pointer (90, 180) → dist 10.
    const c = nearestDockCandidate({ x: 90, y: 180 }, [target(TARGET)], T);
    expect(c?.side).toBe('L');
    expect(c?.gap).toBeCloseTo(10);
  });

  it('BOTTOM side: pointer just below the bottom edge', () => {
    // B segment y=260, x∈[100,300]. Pointer (200, 275) → dist 15.
    const c = nearestDockCandidate({ x: 200, y: 275 }, [target(TARGET)], T);
    expect(c?.side).toBe('B');
    expect(c?.gap).toBeCloseTo(15);
  });

  it('TOP side: pointer just above the top edge', () => {
    // T segment y=100, x∈[100,300]. Pointer (200, 92) → dist 8.
    const c = nearestDockCandidate({ x: 200, y: 92 }, [target(TARGET)], T);
    expect(c?.side).toBe('T');
    expect(c?.gap).toBeCloseTo(8);
  });

  it('pointer just inside an edge still detects that side (line-band, both sides)', () => {
    // Pointer (305, 180): 5 outside the R line → R wins (T(20) band straddles edge).
    const c = nearestDockCandidate({ x: 305, y: 180 }, [target(TARGET)], T);
    expect(c?.side).toBe('R');
    expect(c?.gap).toBeCloseTo(5);
  });
});

describe('nearestDockCandidate — no candidate (line-only detection)', () => {
  it('deep center of a large target → null (away from all edges)', () => {
    // Center (200,180): nearest side distances are 80 (L/R reach) and 80 (T/B
    // reach) — all > T(20). Pointer is not near any line → no candidate.
    expect(nearestDockCandidate({ x: 200, y: 180 }, [target(TARGET)], T)).toBeNull();
  });

  it('pointer beyond the threshold from every side → null', () => {
    // (350,180) is 50 right of R line (x=300), far from L/T/B too → null.
    expect(nearestDockCandidate({ x: 350, y: 180 }, [target(TARGET)], T)).toBeNull();
  });

  it('empty target list (figure-excluded pre-filter) → null', () => {
    // Caller pre-filters figures; with all figures excluded the list is empty.
    expect(nearestDockCandidate({ x: 312, y: 180 }, [], T)).toBeNull();
  });
});

describe('nearestDockCandidate — corner / segment-endpoint distance (amend ②)', () => {
  it('pointer beyond a corner uses distance to the nearest endpoint (within T)', () => {
    // Top-right corner is (300,100). Pointer (306,92): beyond the R segment top
    // and beyond the T segment right → both clamp to the corner endpoint.
    // dist = hypot(6,8) = 10 ≤ T → candidate. Exact L,R,T,B tie-break: R before T.
    const c = nearestDockCandidate({ x: 306, y: 92 }, [target(TARGET)], T);
    expect(c).not.toBeNull();
    expect(c?.gap).toBeCloseTo(10);
    expect(c?.side).toBe('R');
  });

  it('pointer beyond a corner farther than T → null', () => {
    // Top-right corner (300,100). Pointer (320,80): hypot(20,20)=28.28 > T(20).
    expect(nearestDockCandidate({ x: 320, y: 80 }, [target(TARGET)], T)).toBeNull();
  });
});

describe('nearestDockCandidate — multiple targets + tie-break', () => {
  it('picks the nearest of two competing targets (min distance wins)', () => {
    // tA right line x=200 (10 from pointer x=210); tB left line x=470 — far.
    const tA: DockTarget = { id: 'A', box: { x: 100, y: 100, w: 100, h: 200 }, type: 'note' };
    const tB: DockTarget = { id: 'B', box: { x: 470, y: 100, w: 100, h: 200 }, type: 'note' };
    const c = nearestDockCandidate({ x: 210, y: 180 }, [tA, tB], T);
    expect(c?.targetId).toBe('A');
    expect(c?.side).toBe('R');
    expect(c?.gap).toBeCloseTo(10);
  });

  it('exact-distance tie within one target → deterministic side order L,R,T,B', () => {
    // Pointer at the exact center of a small square: equal distance to all 4
    // sides → L (rank 0) wins. Box 40×40 at (0,0), center (20,20), each dist 20.
    const sq: DockTarget = { id: 'sq', box: { x: 0, y: 0, w: 40, h: 40 }, type: 'note' };
    const c = nearestDockCandidate({ x: 20, y: 20 }, [sq], T);
    expect(c?.side).toBe('L');
    expect(c?.gap).toBeCloseTo(20);
  });
});

describe('computeDock — size match + flush (4 sides, normal)', () => {
  const dragged: DockDragged = {
    box: { x: 312, y: 120, w: 150, h: 100 },
    type: 'note',
    min: dockMinForType('note'),
  };

  it('RIGHT: matches height, flush at target right', () => {
    const p = computeDock(dragged, TARGET, 'R');
    expect(p.h).toBe(TARGET.h); // matched height
    expect(p.w).toBe(150); // width unchanged (non-image)
    expect(p.x).toBe(TARGET.x + TARGET.w); // flush right
    expect(p.y).toBe(TARGET.y);
  });

  it('LEFT: matches height, flush at target left (x = target.x - w)', () => {
    const p = computeDock(dragged, TARGET, 'L');
    expect(p.h).toBe(TARGET.h);
    expect(p.x).toBe(TARGET.x - p.w);
    expect(p.y).toBe(TARGET.y);
  });

  it('BOTTOM: matches width, flush at target bottom', () => {
    const p = computeDock(dragged, TARGET, 'B');
    expect(p.w).toBe(TARGET.w); // matched width
    expect(p.h).toBe(100); // height unchanged
    expect(p.y).toBe(TARGET.y + TARGET.h);
    expect(p.x).toBe(TARGET.x);
  });

  it('TOP: matches width, flush at target top (y = target.y - h)', () => {
    const p = computeDock(dragged, TARGET, 'T');
    expect(p.w).toBe(TARGET.w);
    expect(p.y).toBe(TARGET.y - p.h);
    expect(p.x).toBe(TARGET.x);
  });
});

describe('computeDock — min clamp (D6: always docks)', () => {
  it('vertical side: matched height below min → clamps to min, still docks flush', () => {
    // target height 40 < terminal min height 140 → clamp to 140.
    const smallTarget: DockBox = { x: 100, y: 100, w: 200, h: 40 };
    const dragged: DockDragged = {
      box: { x: 312, y: 100, w: 300, h: 200 },
      type: 'terminal',
      min: dockMinForType('terminal'),
    };
    const p = computeDock(dragged, smallTarget, 'R');
    expect(p.h).toBe(140); // clamped to min, larger than target
    expect(p.x).toBe(smallTarget.x + smallTarget.w); // still flush
    expect(p.y).toBe(smallTarget.y); // shares the side origin / corner
  });

  it('horizontal side: matched width below min → clamps to min', () => {
    const narrowTarget: DockBox = { x: 100, y: 100, w: 50, h: 160 };
    const dragged: DockDragged = {
      box: { x: 100, y: 275, w: 300, h: 100 },
      type: 'file_path',
      min: dockMinForType('file_path'),
    };
    const p = computeDock(dragged, narrowTarget, 'B');
    expect(p.w).toBe(200); // file_path min width
    expect(p.y).toBe(narrowTarget.y + narrowTarget.h);
  });
});

describe('computeDock — image aspect preserve (D6 image 특례)', () => {
  it('vertical side: set height, scale width by source aspect (no distortion)', () => {
    // source aspect 2.0 (e.g. 800×400). match height 160 → width 320.
    const dragged: DockDragged = {
      box: { x: 312, y: 120, w: 400, h: 300 },
      type: 'image',
      min: dockMinForType('image'),
      aspect: 2.0,
    };
    const p = computeDock(dragged, TARGET, 'R');
    expect(p.h).toBe(160); // matched height
    expect(p.w).toBeCloseTo(320); // 160 * 2.0 — aspect preserved
    expect(p.w / p.h).toBeCloseTo(2.0);
  });

  it('horizontal side: set width, scale height by source aspect', () => {
    // aspect 2.0, match width 200 → height 100.
    const dragged: DockDragged = {
      box: { x: 100, y: 275, w: 400, h: 200 },
      type: 'image',
      min: dockMinForType('image'),
      aspect: 2.0,
    };
    const p = computeDock(dragged, TARGET, 'B');
    expect(p.w).toBe(200); // matched width
    expect(p.h).toBeCloseTo(100); // 200 / 2.0
    expect(p.w / p.h).toBeCloseTo(2.0);
  });

  it('falls back to current box ratio when aspect missing', () => {
    const dragged: DockDragged = {
      box: { x: 312, y: 120, w: 300, h: 150 }, // ratio 2.0
      type: 'image',
      min: dockMinForType('image'),
    };
    const p = computeDock(dragged, TARGET, 'R');
    expect(p.h).toBe(160);
    expect(p.w).toBeCloseTo(320);
  });
});
