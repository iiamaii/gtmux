import { describe, expect, it } from 'vitest';
import { mapDisplayHitToBodyOffset } from './noteCaret';

describe('mapDisplayHitToBodyOffset', () => {
  it('maps an offset within a single text node', () => {
    expect(mapDisplayHitToBodyOffset([20], 0, 7, 20)).toBe(7);
  });

  it('maps node start and node end', () => {
    expect(mapDisplayHitToBodyOffset([20], 0, 0, 20)).toBe(0);
    expect(mapDisplayHitToBodyOffset([20], 0, 20, 20)).toBe(20);
  });

  it('sums preceding text node lengths in the multi-node case', () => {
    expect(mapDisplayHitToBodyOffset([5, 3, 10], 2, 4, 18)).toBe(12);
    expect(mapDisplayHitToBodyOffset([5, 3, 10], 1, 0, 18)).toBe(5);
  });

  it('clamps to end when the hit is outside the text (hitIndex -1)', () => {
    expect(mapDisplayHitToBodyOffset([20], -1, 0, 20)).toBe(20);
    expect(mapDisplayHitToBodyOffset([], -1, 0, 0)).toBe(0);
  });

  it('clamps to end when hitIndex is out of range', () => {
    expect(mapDisplayHitToBodyOffset([5, 3], 2, 1, 8)).toBe(8);
  });

  it('clamps hitOffset into the hit node length', () => {
    expect(mapDisplayHitToBodyOffset([5, 3], 1, 99, 8)).toBe(8);
    expect(mapDisplayHitToBodyOffset([5, 3], 1, -4, 8)).toBe(5);
  });

  it('clamps the summed offset to bodyLength', () => {
    // Display longer than body should never happen (identity precondition),
    // but the mapping stays safe if it does.
    expect(mapDisplayHitToBodyOffset([10, 10], 1, 10, 15)).toBe(15);
  });

  it('handles empty node list (empty body / placeholder)', () => {
    expect(mapDisplayHitToBodyOffset([], 0, 0, 0)).toBe(0);
  });
});
