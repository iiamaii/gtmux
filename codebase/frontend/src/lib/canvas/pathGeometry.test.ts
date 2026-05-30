import { describe, expect, it } from 'vitest';
import type { CanvasItem, PathItem } from '$lib/types/canvas';
import {
  anchorPoint,
  autoRoutePath,
  bestAnchorPair,
  buildPathDFromPoints,
  connectPathEndpoint,
  computePathBBox,
  degradeDeletedEndpoint,
  detachConnectedEndpoints,
  editPathGeometry,
  expandedPathPoints,
  moveWaypoints,
  pathPointChain,
  updateConnectedFallbacks,
} from './pathGeometry';

function rect(id: string, x: number, y: number, w = 100, h = 80): CanvasItem {
  return {
    id,
    parent_id: null,
    x,
    y,
    w,
    h,
    z: 0,
    visibility: 'visible',
    locked: false,
    minimized: false,
    type: 'rect',
    stroke: '#111',
    fill: '#fff',
    stroke_width: 2,
  };
}

function path(overrides: Partial<PathItem> = {}): PathItem {
  return {
    id: '99999999-9999-4999-8999-999999999999',
    parent_id: null,
    x: 0,
    y: 0,
    w: 1,
    h: 1,
    z: 0,
    visibility: 'visible',
    locked: false,
    minimized: false,
    type: 'path',
    from: { kind: 'free', point: { x: 0, y: 0 } },
    to: { kind: 'free', point: { x: 100, y: 100 } },
    routing: 'orthogonal',
    head_from: 'none',
    head_to: 'arrow',
    stroke: '#111',
    stroke_width: 2,
    ...overrides,
  };
}

describe('pathGeometry', () => {
  it('resolves anchor points on box-like items', () => {
    const item = rect('a', 10, 20, 100, 80);
    expect(anchorPoint(item, 'N')).toEqual({ x: 60, y: 20 });
    expect(anchorPoint(item, 'E')).toEqual({ x: 110, y: 60 });
    expect(anchorPoint(item, 'center')).toEqual({ x: 60, y: 60 });
  });

  it('chooses horizontal, vertical, and diagonal anchor pairs', () => {
    expect(bestAnchorPair(rect('a', 0, 0), rect('b', 300, 10))).toEqual({
      from: 'E',
      to: 'W',
    });
    expect(bestAnchorPair(rect('a', 0, 0), rect('b', 10, 300))).toEqual({
      from: 'S',
      to: 'N',
    });
    expect(bestAnchorPair(rect('a', 0, 0), rect('b', 180, 180))).toEqual({
      from: 'SE',
      to: 'NW',
    });
  });

  it('updates connected endpoint fallback points from current target geometry', () => {
    const a = rect('a', 0, 0);
    const b = rect('b', 300, 0);
    const itemMap = new Map([a, b].map((it) => [it.id, it] as const));
    const p = path({
      from: {
        kind: 'connected',
        item_id: 'a',
        anchor: 'E',
        fallback_point: { x: 0, y: 0 },
      },
      to: {
        kind: 'connected',
        item_id: 'b',
        anchor: 'W',
        fallback_point: { x: 0, y: 0 },
      },
    });
    const next = updateConnectedFallbacks(p, itemMap);
    expect(next.from).toEqual({
      kind: 'connected',
      item_id: 'a',
      anchor: 'E',
      fallback_point: { x: 100, y: 40 },
    });
    expect(next.to).toEqual({
      kind: 'connected',
      item_id: 'b',
      anchor: 'W',
      fallback_point: { x: 300, y: 40 },
    });
  });

  it('resolves connected endpoint offsets relative to anchors', () => {
    const a = rect('a', 0, 0);
    const itemMap = new Map([[a.id, a] as const]);
    const p = path({
      from: {
        kind: 'connected',
        item_id: 'a',
        anchor: 'E',
        offset: { x: 12, y: -6 },
        fallback_point: { x: 0, y: 0 },
      },
      to: { kind: 'free', point: { x: 160, y: 40 } },
    });
    const next = updateConnectedFallbacks(p, itemMap);
    expect(pathPointChain(next, itemMap)[0]).toEqual({ x: 112, y: 34 });
    expect(next.from).toEqual({
      kind: 'connected',
      item_id: 'a',
      anchor: 'E',
      offset: { x: 12, y: -6 },
      fallback_point: { x: 112, y: 34 },
    });
  });

  it('updates offset fallback and bbox cache when a target moves or resizes', () => {
    const a = rect('a', 20, 30, 100, 80);
    const moved = rect('a', 50, 60, 140, 100);
    const p = path({
      from: {
        kind: 'connected',
        item_id: 'a',
        anchor: 'SE',
        offset: { x: 10, y: -12 },
        fallback_point: { x: 0, y: 0 },
      },
      to: { kind: 'free', point: { x: 260, y: 200 } },
    });

    const initial = computePathBBox(
      updateConnectedFallbacks(p, new Map([[a.id, a] as const])),
      new Map([[a.id, a] as const]),
    );
    const next = updateConnectedFallbacks(p, new Map([[moved.id, moved] as const]));
    const nextBox = computePathBBox(next, new Map([[moved.id, moved] as const]));

    expect(next.from).toEqual({
      kind: 'connected',
      item_id: 'a',
      anchor: 'SE',
      offset: { x: 10, y: -12 },
      fallback_point: { x: 200, y: 148 },
    });
    expect(nextBox.x).not.toBe(initial.x);
    expect(pathPointChain(next, new Map([[moved.id, moved] as const]))[0]).toEqual({
      x: 200,
      y: 148,
    });
  });

  it('degrades deleted connected targets to free fallback endpoints', () => {
    const p = path({
      from: {
        kind: 'connected',
        item_id: 'gone',
        anchor: 'E',
        fallback_point: { x: 22, y: 33 },
      },
    });
    expect(degradeDeletedEndpoint(p, new Set(['gone'])).from).toEqual({
      kind: 'free',
      point: { x: 22, y: 33 },
    });
  });

  it('degrades only endpoints whose connected target was deleted', () => {
    const p = path({
      from: {
        kind: 'connected',
        item_id: 'gone',
        anchor: 'E',
        offset: { x: 4, y: 5 },
        fallback_point: { x: 22, y: 33 },
      },
      to: {
        kind: 'connected',
        item_id: 'kept',
        anchor: 'W',
        offset: { x: -3, y: 2 },
        fallback_point: { x: 80, y: 40 },
      },
    });
    const next = degradeDeletedEndpoint(p, new Set(['gone']));
    expect(next.from).toEqual({ kind: 'free', point: { x: 22, y: 33 } });
    expect(next.to).toEqual({
      kind: 'connected',
      item_id: 'kept',
      anchor: 'W',
      offset: { x: -3, y: 2 },
      fallback_point: { x: 80, y: 40 },
    });
  });

  it('detaches connected endpoints before geometry edits', () => {
    const a = rect('a', 0, 0);
    const p = path({
      from: {
        kind: 'connected',
        item_id: 'a',
        anchor: 'E',
        fallback_point: { x: 0, y: 0 },
      },
    });
    const next = detachConnectedEndpoints(p, new Map([[a.id, a]]));
    expect(next.from).toEqual({ kind: 'free', point: { x: 100, y: 40 } });
  });

  it('detaches offset endpoints to their resolved coordinates before width scaling', () => {
    const a = rect('a', 0, 0, 100, 80);
    const p = path({
      from: {
        kind: 'connected',
        item_id: 'a',
        anchor: 'E',
        offset: { x: 12, y: -6 },
        fallback_point: { x: 0, y: 0 },
      },
      to: { kind: 'free', point: { x: 212, y: 34 } },
      waypoints: [{ id: 'mid', x: 162, y: 34 }],
    });
    const itemMap = new Map([[a.id, a] as const]);
    const current = computePathBBox(detachConnectedEndpoints(p, itemMap), itemMap);
    const next = editPathGeometry(p, 'w', current.w + 100, itemMap);

    expect(next.from).toEqual({ kind: 'free', point: { x: 112, y: 34 } });
    expect(next.to.kind === 'free' ? Math.round(next.to.point.x) : null).toBe(312);
    expect(next.waypoints?.[0]).toEqual({ id: 'mid', x: 212, y: 34 });
  });

  it('connects a dragged endpoint to the nearest target anchor', () => {
    const target = rect('target', 100, 100, 80, 60);
    const p = path({
      from: { kind: 'free', point: { x: 0, y: 0 } },
      to: { kind: 'free', point: { x: 20, y: 20 } },
    });
    const itemMap = new Map([[target.id, target] as const]);
    const next = connectPathEndpoint(p, 'to', target, { x: 175, y: 130 }, itemMap);
    expect(next?.to).toEqual({
      kind: 'connected',
      item_id: 'target',
      anchor: 'E',
      fallback_point: { x: 180, y: 130 },
    });
  });

  it('rejects endpoint connection when it would self-loop to the same target', () => {
    const target = rect('target', 100, 100, 80, 60);
    const p = path({
      from: {
        kind: 'connected',
        item_id: 'target',
        anchor: 'W',
        fallback_point: { x: 100, y: 130 },
      },
      to: { kind: 'free', point: { x: 20, y: 20 } },
    });
    expect(connectPathEndpoint(p, 'to', target, { x: 175, y: 130 }, new Map())).toBeNull();
  });

  it('edits cached path geometry by detaching and moving concrete points', () => {
    const a = rect('a', 0, 0);
    const p = path({
      from: {
        kind: 'connected',
        item_id: 'a',
        anchor: 'E',
        fallback_point: { x: 0, y: 0 },
      },
      to: { kind: 'free', point: { x: 200, y: 40 } },
    });
    const itemMap = new Map([[a.id, a] as const]);
    const current = computePathBBox(detachConnectedEndpoints(p, itemMap), itemMap);
    const moved = editPathGeometry(p, 'x', current.x + 25, itemMap);
    expect(moved.from).toEqual({ kind: 'free', point: { x: 125, y: 40 } });
    expect(moved.to).toEqual({ kind: 'free', point: { x: 225, y: 40 } });
    expect(Math.round(moved.x)).toBe(Math.round(current.x + 25));
  });

  it('edits cached path width by scaling free endpoints and waypoints', () => {
    const p = path({
      from: { kind: 'free', point: { x: 10, y: 20 } },
      to: { kind: 'free', point: { x: 110, y: 20 } },
      waypoints: [{ id: 'mid', x: 60, y: 20 }],
      stroke_width: 2,
    });
    const current = computePathBBox(p, new Map());
    const next = editPathGeometry(p, 'w', current.w + 100, new Map());
    expect(next.from).toEqual({ kind: 'free', point: { x: 10, y: 20 } });
    expect(next.to.kind === 'free' ? Math.round(next.to.point.x) : null).toBe(210);
    expect(next.waypoints?.[0]?.x).toBe(110);
    expect(Math.round(next.w)).toBe(Math.round(current.w + 100));
  });

  it('builds point chains and bboxes with orthogonal routing expansion', () => {
    const p = path({
      from: { kind: 'free', point: { x: 10, y: 10 } },
      to: { kind: 'free', point: { x: 90, y: 70 } },
      waypoints: [{ id: 'w1', x: 40, y: 20 }],
    });
    expect(pathPointChain(p, new Map())).toEqual([
      { x: 10, y: 10 },
      { x: 40, y: 20 },
      { x: 90, y: 70 },
    ]);
    const box = computePathBBox(p, new Map());
    expect(box.x).toBeLessThan(10);
    expect(box.y).toBeLessThan(10);
    expect(box.w).toBeGreaterThan(80);
    expect(box.h).toBeGreaterThan(60);
  });

  it('builds smooth routing with continuous waypoint tangents', () => {
    const d = buildPathDFromPoints(
      [
        { x: 0, y: 0 },
        { x: 50, y: 100 },
        { x: 100, y: 0 },
      ],
      'bezier',
    );
    const nums = d.match(/-?\d+(?:\.\d+)?/g)?.map(Number) ?? [];
    expect(nums.length).toBe(14);
    const waypoint = { x: nums[6]!, y: nums[7]! };
    const incoming = {
      x: waypoint.x - nums[4]!,
      y: waypoint.y - nums[5]!,
    };
    const outgoing = {
      x: nums[8]! - waypoint.x,
      y: nums[9]! - waypoint.y,
    };
    expect(incoming.x).toBeCloseTo(outgoing.x);
    expect(incoming.y).toBeCloseTo(outgoing.y);
  });

  it('moves selected waypoints only', () => {
    const p = path({
      waypoints: [
        { id: 'a', x: 10, y: 10 },
        { id: 'b', x: 20, y: 20 },
      ],
    });
    expect(moveWaypoints(p, new Set(['b']), { x: 5, y: -2 }).waypoints).toEqual([
      { id: 'a', x: 10, y: 10 },
      { id: 'b', x: 25, y: 18 },
    ]);
  });

  it('auto-routes connected orthogonal paths around intervening components', () => {
    const a = rect('a', 0, 0);
    const b = rect('b', 300, 0);
    const blocker = rect('blocker', 170, -20, 70, 120);
    const itemMap = new Map([a, b, blocker].map((it) => [it.id, it] as const));
    const p = path({
      from: {
        kind: 'connected',
        item_id: 'a',
        anchor: 'E',
        fallback_point: { x: 100, y: 40 },
      },
      to: {
        kind: 'connected',
        item_id: 'b',
        anchor: 'W',
        fallback_point: { x: 300, y: 40 },
      },
    });
    const next = autoRoutePath(p, itemMap);
    expect(next.waypoints?.length).toBeGreaterThan(0);
    const points = expandedPathPoints(pathPointChain(next, itemMap), next.routing);
    const crossesBlocker = points.some((point, index) => {
      const prev = points[index - 1];
      if (prev === undefined) return false;
      if (prev.y === point.y) {
        return (
          point.y > blocker.y &&
          point.y < blocker.y + blocker.h &&
          Math.min(prev.x, point.x) < blocker.x + blocker.w &&
          Math.max(prev.x, point.x) > blocker.x
        );
      }
      if (prev.x === point.x) {
        return (
          point.x > blocker.x &&
          point.x < blocker.x + blocker.w &&
          Math.min(prev.y, point.y) < blocker.y + blocker.h &&
          Math.max(prev.y, point.y) > blocker.y
        );
      }
      return true;
    });
    expect(crossesBlocker).toBe(false);
  });
});
