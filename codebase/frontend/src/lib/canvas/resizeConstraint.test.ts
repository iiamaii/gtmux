import { describe, it, expect } from 'vitest';
import {
  resizeEventShiftKey,
  constrainResizeAspect,
  constrainResizeAspectIfShift,
  constrainResizeSquare,
  scheduleLiveAspectResize,
  scheduleLiveSquareResize,
  squarePointFromDrag,
  projectPointToAngle,
} from '$lib/canvas/resizeConstraint';

// ADR-0031 (2026-05-29 amend) — Shift-constrained scale.

describe('resizeEventShiftKey', () => {
  it('reads direct shiftKey', () => {
    expect(resizeEventShiftKey({ shiftKey: true })).toBe(true);
    expect(resizeEventShiftKey({ shiftKey: false })).toBe(false);
  });
  it('reads nested sourceEvent.shiftKey', () => {
    expect(resizeEventShiftKey({ sourceEvent: { shiftKey: true } })).toBe(true);
  });
  it('null / missing => false', () => {
    expect(resizeEventShiftKey(null)).toBe(false);
    expect(resizeEventShiftKey({})).toBe(false);
  });
});

describe('constrainResizeSquare (rect/ellipse 1:1)', () => {
  it('forces a square from a non-square drag, top-left anchored', () => {
    const out = constrainResizeSquare(
      { x: 0, y: 0, width: 150, height: 120 },
      { x: 0, y: 0, w: 100, h: 100 },
      20,
    );
    expect(out.width).toBe(out.height);
    expect(out.width).toBe(150); // dominant (width) axis wins
    expect(out.x).toBe(0);
    expect(out.y).toBe(0);
  });
});

describe('constrainResizeAspect (image source-aspect / current-aspect lock)', () => {
  it('maintains a 2:1 aspect ratio', () => {
    const out = constrainResizeAspect(
      { x: 0, y: 0, width: 300, height: 110 },
      { x: 0, y: 0, w: 200, h: 100 },
      2,
      20,
      10,
    );
    expect(out.width / out.height).toBeCloseTo(2, 5);
  });
  it('passes through on non-finite/zero aspect', () => {
    const params = { x: 1, y: 2, width: 30, height: 40 };
    expect(constrainResizeAspect(params, { x: 0, y: 0, w: 10, h: 10 }, 0, 1, 1)).toEqual(params);
  });
});

describe('constrainResizeAspectIfShift', () => {
  it('only constrains when the resize event carries Shift', () => {
    const params = { x: 0, y: 0, width: 300, height: 110 };
    const current = { x: 0, y: 0, w: 200, h: 100 };
    expect(constrainResizeAspectIfShift({ shiftKey: false }, params, current, 2, 20, 10)).toEqual(params);
    const constrained = constrainResizeAspectIfShift({ shiftKey: true }, params, current, 2, 20, 10);
    expect(constrained.width / constrained.height).toBeCloseTo(2, 5);
  });
});

describe('scheduleLiveAspectResize', () => {
  it('schedules live constrained geometry while Shift is held', async () => {
    const calls: { x: number; y: number; width: number; height: number }[] = [];
    scheduleLiveAspectResize(
      { shiftKey: true },
      { x: 0, y: 0, width: 300, height: 110 },
      { x: 0, y: 0, w: 200, h: 100 },
      2,
      20,
      10,
      (next) => calls.push(next),
    );
    expect(calls).toEqual([]);
    await Promise.resolve();
    expect(calls[0]!.width / calls[0]!.height).toBeCloseTo(2, 5);
  });
});

describe('scheduleLiveSquareResize', () => {
  it('schedules live square geometry while Shift is held', async () => {
    const calls: { x: number; y: number; width: number; height: number }[] = [];
    scheduleLiveSquareResize(
      { shiftKey: true },
      { x: 0, y: 0, width: 170, height: 120 },
      { x: 0, y: 0, w: 100, h: 100 },
      20,
      (next) => calls.push(next),
    );
    expect(calls).toEqual([]);
    await Promise.resolve();
    expect(calls[0]!.width).toBe(calls[0]!.height);
    expect(calls[0]!.width).toBe(170);
  });
});

describe('squarePointFromDrag (figure Shift+draw)', () => {
  it('snaps to the dominant axis preserving sign', () => {
    expect(squarePointFromDrag({ x: 0, y: 0 }, { x: 30, y: 10 })).toEqual({ x: 30, y: 30 });
    expect(squarePointFromDrag({ x: 0, y: 0 }, { x: -30, y: 10 })).toEqual({ x: -30, y: 30 });
  });
});

describe('projectPointToAngle (line holding-angle)', () => {
  it('projects the drag onto the held angle ray (horizontal)', () => {
    const p = projectPointToAngle({ x: 0, y: 0 }, { x: 10, y: 5 }, 0);
    expect(p.x).toBeCloseTo(10, 5);
    expect(p.y).toBeCloseTo(0, 5);
  });
});
