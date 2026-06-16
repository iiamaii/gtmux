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
// 4 sides × { normal, min-clamp, image-aspect-preserve } + no-overlap→null +
// figure-excluded→null.

const T = 20; // proximity threshold (canvas px).

// Helper: a target box at a fixed location.
function target(box: DockBox, type: DockTarget['type'] = 'note'): DockTarget {
  return { id: 't1', box, type };
}

const TARGET: DockBox = { x: 100, y: 100, w: 200, h: 160 };

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

describe('nearestDockCandidate — 4 sides with overlap', () => {
  it('RIGHT side: dragged left edge near target right edge', () => {
    // target right = 300. dragged at x=312 (gap 12), vertically overlapping.
    const dragged: DockBox = { x: 312, y: 120, w: 150, h: 100 };
    const c = nearestDockCandidate(dragged, [target(TARGET)], T);
    expect(c).not.toBeNull();
    expect(c?.side).toBe('R');
    expect(c?.gap).toBeCloseTo(12);
  });

  it('LEFT side: dragged right edge near target left edge', () => {
    // target left = 100. dragged right edge at 90 (x=-60+w=150 → right 90, gap 10).
    const dragged: DockBox = { x: -60, y: 120, w: 150, h: 100 };
    const c = nearestDockCandidate(dragged, [target(TARGET)], T);
    expect(c?.side).toBe('L');
    expect(c?.gap).toBeCloseTo(10);
  });

  it('BOTTOM side: dragged top edge near target bottom edge', () => {
    // target bottom = 260. dragged top at 275 (gap 15), horizontally overlapping.
    const dragged: DockBox = { x: 120, y: 275, w: 150, h: 100 };
    const c = nearestDockCandidate(dragged, [target(TARGET)], T);
    expect(c?.side).toBe('B');
    expect(c?.gap).toBeCloseTo(15);
  });

  it('TOP side: dragged bottom edge near target top edge', () => {
    // target top = 100. dragged bottom at 92 (y=-8+h=100 → bottom 92, gap 8).
    const dragged: DockBox = { x: 120, y: -8, w: 150, h: 100 };
    const c = nearestDockCandidate(dragged, [target(TARGET)], T);
    expect(c?.side).toBe('T');
    expect(c?.gap).toBeCloseTo(8);
  });

  it('picks the minimum gap across candidates (D3)', () => {
    // Two targets: t-right gap 12, t-left gap 4 → left wins.
    const tRight: DockTarget = { id: 'right', box: { x: 100, y: 100, w: 100, h: 100 }, type: 'note' };
    const tLeft: DockTarget = { id: 'left', box: { x: 470, y: 100, w: 100, h: 100 }, type: 'note' };
    // dragged occupies x 300..466 → right edge 466, near tLeft.x=470 (gap 4);
    // left edge 300, near tRight.right=200 (gap 100). Min gap = tLeft (4).
    const dragged: DockBox = { x: 300, y: 120, w: 166, h: 60 };
    const c = nearestDockCandidate(dragged, [tRight, tLeft], T);
    expect(c?.targetId).toBe('left');
    expect(c?.side).toBe('L');
    expect(c?.gap).toBeCloseTo(4);
  });
});

describe('nearestDockCandidate — no candidate', () => {
  it('no perpendicular overlap → null even when gap small', () => {
    // Horizontally near target right (gap 5) but far below → no vertical overlap.
    const dragged: DockBox = { x: 305, y: 400, w: 150, h: 100 };
    expect(nearestDockCandidate(dragged, [target(TARGET)], T)).toBeNull();
  });

  it('gap beyond threshold → null', () => {
    // Vertically overlapping but gap 50 > T(20).
    const dragged: DockBox = { x: 350, y: 120, w: 150, h: 100 };
    expect(nearestDockCandidate(dragged, [target(TARGET)], T)).toBeNull();
  });

  it('empty target list (figure-excluded pre-filter) → null', () => {
    // Caller pre-filters figures; with all figures excluded the list is empty.
    const dragged: DockBox = { x: 312, y: 120, w: 150, h: 100 };
    expect(nearestDockCandidate(dragged, [], T)).toBeNull();
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
