import { describe, it, expect } from 'vitest';
import {
  resizeEventShiftKey,
  constrainResizeAspect,
  constrainResizeAspectIfShift,
  scheduleLiveAspectResize,
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
  it('preserves a non-square shape aspect for rect/ellipse resize', () => {
    const out = constrainResizeAspect(
      { x: 0, y: 0, width: 360, height: 180 },
      { x: 0, y: 0, w: 240, h: 120 },
      240 / 120,
      20,
      20,
    );
    expect(out.width / out.height).toBeCloseTo(2, 5);
    expect(out.width).not.toBe(out.height);
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

describe('projectPointToAngle (line holding-angle)', () => {
  it('projects the drag onto the held angle ray (horizontal)', () => {
    const p = projectPointToAngle({ x: 0, y: 0 }, { x: 10, y: 5 }, 0);
    expect(p.x).toBeCloseTo(10, 5);
    expect(p.y).toBeCloseTo(0, 5);
  });
});
