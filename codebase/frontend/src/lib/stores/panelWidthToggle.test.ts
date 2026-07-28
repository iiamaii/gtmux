import { describe, it, expect } from 'vitest';
import { resolveWidthToggle } from '$lib/stores/panelWidthToggle';

// ADR-0017 amend 2026-07-28 ㉓ (재지정) — resize-handle double-click width
// minimize ↔ content-fit toggle.
const LEFT_MIN = 230;
const LEFT_MAX = 520;
const RIGHT_MIN = 240;
const RIGHT_MAX = 560;
const FLOOR = 268;

describe('resolveWidthToggle', () => {
  it('minimizes to MIN when wider than MIN (content-fit ignored)', () => {
    expect(resolveWidthToggle(400, LEFT_MIN, 9999, FLOOR, LEFT_MAX)).toBe(LEFT_MIN);
  });

  it('expands to the content-fit width when at MIN', () => {
    expect(resolveWidthToggle(LEFT_MIN, LEFT_MIN, 410, FLOOR, LEFT_MAX)).toBe(410);
  });

  it('floors a narrow content-fit to 268 (restore never looks like a no-op)', () => {
    expect(resolveWidthToggle(LEFT_MIN, LEFT_MIN, 200, FLOOR, LEFT_MAX)).toBe(FLOOR);
    // A content-fit exactly at MIN still floors up to 268.
    expect(resolveWidthToggle(LEFT_MIN, LEFT_MIN, LEFT_MIN, FLOOR, LEFT_MAX)).toBe(FLOOR);
  });

  it('clamps an over-wide content-fit to the panel MAX', () => {
    expect(resolveWidthToggle(LEFT_MIN, LEFT_MIN, 800, FLOOR, LEFT_MAX)).toBe(LEFT_MAX);
  });

  it('rounds fractional content-fit widths', () => {
    expect(resolveWidthToggle(LEFT_MIN, LEFT_MIN, 333.6, FLOOR, LEFT_MAX)).toBe(334);
  });

  it('round-trips: minimize then content-fit restore', () => {
    const down = resolveWidthToggle(420, LEFT_MIN, 0, FLOOR, LEFT_MAX);
    expect(down).toBe(LEFT_MIN);
    const up = resolveWidthToggle(LEFT_MIN, LEFT_MIN, 420, FLOOR, LEFT_MAX);
    expect(up).toBe(420);
  });

  it('works for the right panel MIN (240) / MAX (560)', () => {
    expect(resolveWidthToggle(500, RIGHT_MIN, 0, FLOOR, RIGHT_MAX)).toBe(RIGHT_MIN);
    expect(resolveWidthToggle(RIGHT_MIN, RIGHT_MIN, 500, FLOOR, RIGHT_MAX)).toBe(500);
    expect(resolveWidthToggle(RIGHT_MIN, RIGHT_MIN, 700, FLOOR, RIGHT_MAX)).toBe(RIGHT_MAX);
  });
});
