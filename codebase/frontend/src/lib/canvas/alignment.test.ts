import { describe, expect, it } from 'vitest';
import { alignBoxes, distributeBoxes, type AlignBox } from './alignment';

const box = (id: string, x: number, y: number, w: number, h: number): AlignBox => ({
  id,
  x,
  y,
  w,
  h,
});

describe('alignment box helpers', () => {
  it('aligns arbitrary boxes by their union bounds', () => {
    const moves = alignBoxes(
      [
        box('a', 10, 20, 40, 20),
        box('b', 80, 10, 20, 60),
      ],
      'right',
    );

    expect(moves.get('a')).toEqual({ dx: 50, dy: 0 });
    expect(moves.has('b')).toBe(false);
  });

  it('distributes arbitrary boxes by center while keeping extremes fixed', () => {
    const moves = distributeBoxes(
      [
        box('a', 0, 0, 10, 10),
        box('b', 20, 0, 10, 10),
        box('c', 60, 0, 10, 10),
      ],
      'horizontal',
    );

    expect(moves.get('b')).toEqual({ dx: 10, dy: 0 });
    expect(moves.has('a')).toBe(false);
    expect(moves.has('c')).toBe(false);
  });

  it('keeps locked boxes in the reference bounds but does not move them', () => {
    const moves = alignBoxes(
      [
        { ...box('a', 0, 0, 10, 10), locked: true },
        box('b', 40, 0, 10, 10),
      ],
      'left',
    );

    expect(moves.has('a')).toBe(false);
    expect(moves.get('b')).toEqual({ dx: -40, dy: 0 });
  });
});
