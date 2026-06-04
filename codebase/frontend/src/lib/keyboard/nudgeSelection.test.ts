import { describe, expect, it } from 'vitest';
import type { CanvasItem } from '$lib/types/canvas';
import type { Group } from '$lib/types/group';
import { materializeNudgeItemIds } from './nudgeSelection';

function rect(id: string, parentId: string | null, locked = false): CanvasItem {
  return {
    id,
    parent_id: parentId,
    x: 0,
    y: 0,
    w: 100,
    h: 80,
    z: 0,
    visibility: 'visible',
    locked,
    minimized: false,
    type: 'rect',
    stroke: '#111111',
    fill: '#ffffff',
    stroke_width: 2,
  };
}

function group(
  id: string,
  parentId: string | null,
  order: number,
  locked = false,
): Group {
  return {
    id,
    parent_id: parentId,
    label: null,
    color: null,
    visibility: 'visible',
    locked,
    order,
  };
}

describe('materializeNudgeItemIds', () => {
  it('expands selected groups to descendant items and deduplicates direct item selections', () => {
    const groups = new Map<string, Group>([
      ['g-a', group('g-a', null, 0)],
      ['g-b', group('g-b', 'g-a', 1)],
    ]);
    const items = new Map<string, CanvasItem>([
      ['a', rect('a', 'g-a')],
      ['b', rect('b', 'g-b')],
      ['root', rect('root', null)],
    ]);

    expect(materializeNudgeItemIds(['g-a', 'b', 'root'], items, groups)).toEqual([
      'a',
      'b',
      'root',
    ]);
  });

  it('excludes self-locked items and items under locked groups', () => {
    const groups = new Map<string, Group>([
      ['g-a', group('g-a', null, 0, true)],
      ['g-b', group('g-b', null, 1)],
    ]);
    const items = new Map<string, CanvasItem>([
      ['inside-locked-group', rect('inside-locked-group', 'g-a')],
      ['self-locked', rect('self-locked', 'g-b', true)],
      ['movable', rect('movable', 'g-b')],
    ]);

    expect(materializeNudgeItemIds(['g-a', 'g-b'], items, groups)).toEqual([
      'movable',
    ]);
  });

  it('keeps plain item selections movable when mixed with group selections', () => {
    const groups = new Map<string, Group>([['g-a', group('g-a', null, 0)]]);
    const items = new Map<string, CanvasItem>([
      ['inside', rect('inside', 'g-a')],
      ['outside', rect('outside', null)],
    ]);

    expect(materializeNudgeItemIds(['outside', 'g-a'], items, groups)).toEqual([
      'outside',
      'inside',
    ]);
  });
});
