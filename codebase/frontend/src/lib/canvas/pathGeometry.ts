import type {
  Anchor,
  CanvasItem,
  PathEndpoint,
  PathItem,
  PathRouting,
  PathWaypoint,
  Point,
} from '$lib/types/canvas';
import { generateUuidV4 } from '$lib/uuid';

export const PATH_HIT_PADDING = 12;
const ORTHOGONAL_DOMINANCE = 1.6;
const ANCHORS: readonly Anchor[] = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW', 'center'];

export type ConnectableItem = Extract<
  CanvasItem,
  | { type: 'terminal' }
  | { type: 'text' }
  | { type: 'note' }
  | { type: 'rect' }
  | { type: 'ellipse' }
  | { type: 'image' }
  | { type: 'document' }
  | { type: 'file_path' }
  | { type: 'snippets' }
>;

export function isConnectableItem(item: CanvasItem): item is ConnectableItem {
  return (
    item.type === 'terminal' ||
    item.type === 'text' ||
    item.type === 'note' ||
    item.type === 'rect' ||
    item.type === 'ellipse' ||
    item.type === 'image' ||
    item.type === 'document' ||
    item.type === 'file_path' ||
    item.type === 'snippets'
  );
}

export function connectableTargetAtPoint(
  point: Point,
  itemMap: ReadonlyMap<string, CanvasItem>,
  options: {
    margin: number;
    excludeId?: string | null;
    excludeSecondId?: string | null;
  },
): ConnectableItem | null {
  let topmost: ConnectableItem | null = null;
  for (const item of itemMap.values()) {
    if (options.excludeId != null && item.id === options.excludeId) continue;
    if (options.excludeSecondId != null && item.id === options.excludeSecondId) continue;
    if (!isConnectableItem(item) || item.visibility !== 'visible') continue;
    const inside =
      point.x >= item.x - options.margin &&
      point.x <= item.x + item.w + options.margin &&
      point.y >= item.y - options.margin &&
      point.y <= item.y + item.h + options.margin;
    if (!inside) continue;
    if (topmost === null || item.z >= topmost.z) topmost = item;
  }
  return topmost;
}

export function anchorPoint(item: CanvasItem, anchor: Anchor): Point {
  const x1 = item.x;
  const y1 = item.y;
  const x2 = item.x + item.w;
  const y2 = item.y + item.h;
  const cx = item.x + item.w / 2;
  const cy = item.y + item.h / 2;
  switch (anchor) {
    case 'N':
      return { x: cx, y: y1 };
    case 'NE':
      return { x: x2, y: y1 };
    case 'E':
      return { x: x2, y: cy };
    case 'SE':
      return { x: x2, y: y2 };
    case 'S':
      return { x: cx, y: y2 };
    case 'SW':
      return { x: x1, y: y2 };
    case 'W':
      return { x: x1, y: cy };
    case 'NW':
      return { x: x1, y: y1 };
    case 'center':
      return { x: cx, y: cy };
  }
}

export function offsetPoint(point: Point, offset?: Point | null): Point {
  if (offset == null) return point;
  return { x: point.x + offset.x, y: point.y + offset.y };
}

export function connectedEndpointPoint(
  item: CanvasItem,
  anchor: Anchor,
  offset?: Point | null,
): Point {
  return offsetPoint(anchorPoint(item, anchor), offset);
}

export function bestAnchorPair(
  fromItem: CanvasItem,
  toItem: CanvasItem,
): { from: Anchor; to: Anchor } {
  const fromCenter = anchorPoint(fromItem, 'center');
  const toCenter = anchorPoint(toItem, 'center');
  const dx = toCenter.x - fromCenter.x;
  const dy = toCenter.y - fromCenter.y;
  const ax = Math.abs(dx);
  const ay = Math.abs(dy);
  if (ax > ay * ORTHOGONAL_DOMINANCE) {
    return dx >= 0 ? { from: 'E', to: 'W' } : { from: 'W', to: 'E' };
  }
  if (ay > ax * ORTHOGONAL_DOMINANCE) {
    return dy >= 0 ? { from: 'S', to: 'N' } : { from: 'N', to: 'S' };
  }
  if (dx >= 0 && dy >= 0) return { from: 'SE', to: 'NW' };
  if (dx >= 0 && dy < 0) return { from: 'NE', to: 'SW' };
  if (dx < 0 && dy >= 0) return { from: 'SW', to: 'NE' };
  return { from: 'NW', to: 'SE' };
}

export function nearestAnchor(item: CanvasItem, point: Point): Anchor {
  let best = ANCHORS[0]!;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (const anchor of ANCHORS) {
    const anchorPos = anchorPoint(item, anchor);
    const distance = Math.hypot(anchorPos.x - point.x, anchorPos.y - point.y);
    if (distance < bestDistance) {
      best = anchor;
      bestDistance = distance;
    }
  }
  return best;
}

export function resolveEndpoint(
  endpoint: PathEndpoint,
  itemMap: ReadonlyMap<string, CanvasItem>,
): Point {
  if (endpoint.kind === 'free') return endpoint.point;
  const item = itemMap.get(endpoint.item_id);
  if (item === undefined || !isConnectableItem(item)) return endpoint.fallback_point;
  return connectedEndpointPoint(item, endpoint.anchor, endpoint.offset);
}

export function pathPointChain(
  path: PathItem,
  itemMap: ReadonlyMap<string, CanvasItem>,
): Point[] {
  return [
    resolveEndpoint(path.from, itemMap),
    ...(path.waypoints ?? []).map((p) => ({ x: p.x, y: p.y })),
    resolveEndpoint(path.to, itemMap),
  ];
}

export function expandedPathPoints(points: readonly Point[], routing: PathRouting): Point[] {
  if (points.length <= 1 || routing !== 'orthogonal') return [...points];
  const out: Point[] = [];
  for (let i = 0; i < points.length - 1; i += 1) {
    const a = points[i]!;
    const b = points[i + 1]!;
    if (i === 0) out.push(a);
    if (a.x !== b.x && a.y !== b.y) out.push({ x: b.x, y: a.y });
    out.push(b);
  }
  return out;
}

function bezierSegments(points: readonly Point[]): {
  c1: Point;
  c2: Point;
  to: Point;
}[] {
  const segments: {
    c1: Point;
    c2: Point;
    to: Point;
  }[] = [];
  for (let i = 0; i < points.length - 1; i += 1) {
    const p0 = points[i - 1] ?? points[i]!;
    const p1 = points[i]!;
    const p2 = points[i + 1]!;
    const p3 = points[i + 2] ?? p2;
    segments.push({
      c1: {
        x: p1.x + (p2.x - p0.x) / 6,
        y: p1.y + (p2.y - p0.y) / 6,
      },
      c2: {
        x: p2.x - (p3.x - p1.x) / 6,
        y: p2.y - (p3.y - p1.y) / 6,
      },
      to: p2,
    });
  }
  return segments;
}

function bboxPoints(points: readonly Point[], routing: PathRouting): Point[] {
  if (routing !== 'bezier') return expandedPathPoints(points, routing);
  const first = points[0];
  if (first === undefined) return [];
  return [
    first,
    ...bezierSegments(points).flatMap((segment) => [
      segment.c1,
      segment.c2,
      segment.to,
    ]),
  ];
}

export function buildPathD(
  path: PathItem,
  itemMap: ReadonlyMap<string, CanvasItem>,
): string {
  const points = pathPointChain(path, itemMap);
  return buildPathDFromPoints(points, path.routing);
}

export function buildPathDFromPoints(
  points: readonly Point[],
  routing: PathRouting,
): string {
  const first = points[0];
  if (first === undefined) return '';
  if (points.length === 1) return `M ${first.x} ${first.y}`;
  if (routing === 'bezier') {
    let d = `M ${first.x} ${first.y}`;
    for (const segment of bezierSegments(points)) {
      d += ` C ${segment.c1.x} ${segment.c1.y} ${segment.c2.x} ${segment.c2.y} ${segment.to.x} ${segment.to.y}`;
    }
    return d;
  }
  const routed = expandedPathPoints(points, routing);
  return routed
    .map((p, i) => `${i === 0 ? 'M' : 'L'} ${p.x} ${p.y}`)
    .join(' ');
}

export function computePathBBox(
  path: PathItem,
  itemMap: ReadonlyMap<string, CanvasItem>,
): { x: number; y: number; w: number; h: number } {
  const points = bboxPoints(pathPointChain(path, itemMap), path.routing);
  const first = points[0] ?? { x: path.x, y: path.y };
  let minX = first.x;
  let minY = first.y;
  let maxX = first.x;
  let maxY = first.y;
  for (const p of points) {
    minX = Math.min(minX, p.x);
    minY = Math.min(minY, p.y);
    maxX = Math.max(maxX, p.x);
    maxY = Math.max(maxY, p.y);
  }
  const pad = Math.max(PATH_HIT_PADDING, path.stroke_width / 2 + PATH_HIT_PADDING);
  return {
    x: minX - pad,
    y: minY - pad,
    w: Math.max(maxX - minX, 1) + pad * 2,
    h: Math.max(maxY - minY, 1) + pad * 2,
  };
}

function pathPointBounds(
  path: PathItem,
  itemMap: ReadonlyMap<string, CanvasItem>,
): { minX: number; minY: number; maxX: number; maxY: number } {
  const points = expandedPathPoints(pathPointChain(path, itemMap), path.routing);
  const first = points[0] ?? { x: path.x, y: path.y };
  let minX = first.x;
  let minY = first.y;
  let maxX = first.x;
  let maxY = first.y;
  for (const p of points) {
    minX = Math.min(minX, p.x);
    minY = Math.min(minY, p.y);
    maxX = Math.max(maxX, p.x);
    maxY = Math.max(maxY, p.y);
  }
  return { minX, minY, maxX, maxY };
}

export function hasConnectedEndpoint(path: PathItem): boolean {
  return path.from.kind === 'connected' || path.to.kind === 'connected';
}

export function isPathConnectedToAny(
  path: PathItem,
  itemIds: ReadonlySet<string>,
): boolean {
  return (
    (path.from.kind === 'connected' && itemIds.has(path.from.item_id)) ||
    (path.to.kind === 'connected' && itemIds.has(path.to.item_id))
  );
}

export function updateConnectedFallbacks(
  path: PathItem,
  itemMap: ReadonlyMap<string, CanvasItem>,
): PathItem {
  const update = (endpoint: PathEndpoint): PathEndpoint => {
    if (endpoint.kind !== 'connected') return endpoint;
    const item = itemMap.get(endpoint.item_id);
    if (item === undefined || !isConnectableItem(item)) return endpoint;
    return {
      ...endpoint,
      fallback_point: connectedEndpointPoint(item, endpoint.anchor, endpoint.offset),
    };
  };
  return {
    ...path,
    from: update(path.from),
    to: update(path.to),
  };
}

export function updatePathBBoxCache(
  path: PathItem,
  itemMap: ReadonlyMap<string, CanvasItem>,
): PathItem {
  const withFallbacks = updateConnectedFallbacks(path, itemMap);
  const box = computePathBBox(withFallbacks, itemMap);
  return { ...withFallbacks, ...box };
}

const AUTO_ROUTE_MARGIN = 32;
const BEND_PENALTY = 8;

type RouteRect = { x: number; y: number; w: number; h: number };
type RouteDir = 'h' | 'v' | 'none';

function uniqueSorted(values: Iterable<number>): number[] {
  return [...new Set([...values].map((value) => Math.round(value * 1000) / 1000))]
    .sort((a, b) => a - b);
}

function endpointTargetIds(path: PathItem): ReadonlySet<string> {
  const ids = new Set<string>();
  if (path.from.kind === 'connected') ids.add(path.from.item_id);
  if (path.to.kind === 'connected') ids.add(path.to.item_id);
  return ids;
}

function routeObstacles(
  path: PathItem,
  itemMap: ReadonlyMap<string, CanvasItem>,
): RouteRect[] {
  const endpointIds = endpointTargetIds(path);
  const obstacles: RouteRect[] = [];
  for (const item of itemMap.values()) {
    if (endpointIds.has(item.id)) continue;
    if (!isConnectableItem(item)) continue;
    if (item.visibility !== 'visible') continue;
    obstacles.push({
      x: item.x - AUTO_ROUTE_MARGIN,
      y: item.y - AUTO_ROUTE_MARGIN,
      w: item.w + AUTO_ROUTE_MARGIN * 2,
      h: item.h + AUTO_ROUTE_MARGIN * 2,
    });
  }
  return obstacles;
}

function axisSegmentIntersectsRect(a: Point, b: Point, rect: RouteRect): boolean {
  if (a.y === b.y) {
    const y = a.y;
    if (y <= rect.y || y >= rect.y + rect.h) return false;
    const minX = Math.min(a.x, b.x);
    const maxX = Math.max(a.x, b.x);
    return minX < rect.x + rect.w && maxX > rect.x;
  }
  if (a.x === b.x) {
    const x = a.x;
    if (x <= rect.x || x >= rect.x + rect.w) return false;
    const minY = Math.min(a.y, b.y);
    const maxY = Math.max(a.y, b.y);
    return minY < rect.y + rect.h && maxY > rect.y;
  }
  return true;
}

function isRouteSegmentClear(a: Point, b: Point, obstacles: readonly RouteRect[]): boolean {
  return obstacles.every((rect) => !axisSegmentIntersectsRect(a, b, rect));
}

function compactRoute(points: readonly Point[]): Point[] {
  const deduped: Point[] = [];
  for (const point of points) {
    const prev = deduped[deduped.length - 1];
    if (prev !== undefined && prev.x === point.x && prev.y === point.y) continue;
    deduped.push(point);
  }
  const compacted: Point[] = [];
  for (const point of deduped) {
    const a = compacted[compacted.length - 2];
    const b = compacted[compacted.length - 1];
    if (
      a !== undefined &&
      b !== undefined &&
      ((a.x === b.x && b.x === point.x) || (a.y === b.y && b.y === point.y))
    ) {
      compacted[compacted.length - 1] = point;
    } else {
      compacted.push(point);
    }
  }
  return compacted;
}

function findOrthogonalRoute(
  start: Point,
  end: Point,
  obstacles: readonly RouteRect[],
): Point[] | null {
  const xs = uniqueSorted([
    start.x,
    end.x,
    ...obstacles.flatMap((rect) => [rect.x, rect.x + rect.w]),
  ]);
  const ys = uniqueSorted([
    start.y,
    end.y,
    ...obstacles.flatMap((rect) => [rect.y, rect.y + rect.h]),
  ]);
  const keyOf = (x: number, y: number): string => `${x},${y}`;
  const pointOf = (key: string): Point => {
    const [x, y] = key.split(',').map(Number);
    return { x: x ?? 0, y: y ?? 0 };
  };
  const startKey = keyOf(start.x, start.y);
  const endKey = keyOf(end.x, end.y);
  const dist = new Map<string, number>([[`${startKey}|none`, 0]]);
  const prev = new Map<string, string>();
  const queue: { key: string; dir: RouteDir; cost: number }[] = [
    { key: startKey, dir: 'none', cost: 0 },
  ];

  const push = (
    fromState: string,
    from: Point,
    to: Point,
    nextDir: Exclude<RouteDir, 'none'>,
    currentDir: RouteDir,
    currentCost: number,
  ): void => {
    if (!isRouteSegmentClear(from, to, obstacles)) return;
    const nextKey = keyOf(to.x, to.y);
    const bend = currentDir !== 'none' && currentDir !== nextDir ? BEND_PENALTY : 0;
    const nextCost = currentCost + Math.hypot(to.x - from.x, to.y - from.y) + bend;
    const nextState = `${nextKey}|${nextDir}`;
    if (nextCost >= (dist.get(nextState) ?? Number.POSITIVE_INFINITY)) return;
    dist.set(nextState, nextCost);
    prev.set(nextState, fromState);
    queue.push({ key: nextKey, dir: nextDir, cost: nextCost });
  };

  while (queue.length > 0) {
    queue.sort((a, b) => a.cost - b.cost);
    const current = queue.shift()!;
    const stateKey = `${current.key}|${current.dir}`;
    if (current.cost !== dist.get(stateKey)) continue;
    if (current.key === endKey) {
      const routeKeys: string[] = [stateKey];
      let cursor = stateKey;
      while (prev.has(cursor)) {
        cursor = prev.get(cursor)!;
        routeKeys.push(cursor);
      }
      routeKeys.reverse();
      return compactRoute(routeKeys.map((key) => pointOf(key.split('|')[0]!)));
    }
    const point = pointOf(current.key);
    const xi = xs.indexOf(point.x);
    const yi = ys.indexOf(point.y);
    if (xi > 0) push(stateKey, point, { x: xs[xi - 1]!, y: point.y }, 'h', current.dir, current.cost);
    if (xi < xs.length - 1) push(stateKey, point, { x: xs[xi + 1]!, y: point.y }, 'h', current.dir, current.cost);
    if (yi > 0) push(stateKey, point, { x: point.x, y: ys[yi - 1]! }, 'v', current.dir, current.cost);
    if (yi < ys.length - 1) push(stateKey, point, { x: point.x, y: ys[yi + 1]! }, 'v', current.dir, current.cost);
  }
  return null;
}

export function autoRoutePath(
  path: PathItem,
  itemMap: ReadonlyMap<string, CanvasItem>,
): PathItem {
  if (path.routing !== 'orthogonal') return updatePathBBoxCache(path, itemMap);
  if (path.from.kind !== 'connected' || path.to.kind !== 'connected') {
    return updatePathBBoxCache(path, itemMap);
  }
  const start = resolveEndpoint(path.from, itemMap);
  const end = resolveEndpoint(path.to, itemMap);
  const obstacles = routeObstacles(path, itemMap);
  const route = findOrthogonalRoute(start, end, obstacles);
  if (route === null) return updatePathBBoxCache(path, itemMap);
  const compacted = compactRoute(route);
  const waypoints = compacted.slice(1, -1).map((point) => ({
    id: generateUuidV4(),
    x: point.x,
    y: point.y,
  }));
  return updatePathBBoxCache(
    { ...path, waypoints: waypoints.length === 0 ? undefined : waypoints },
    itemMap,
  );
}

export function connectPathEndpoint(
  path: PathItem,
  endpointId: 'from' | 'to',
  target: CanvasItem,
  point: Point,
  itemMap: ReadonlyMap<string, CanvasItem>,
): PathItem | null {
  if (!isConnectableItem(target)) return null;
  const other = endpointId === 'from' ? path.to : path.from;
  if (other.kind === 'connected' && other.item_id === target.id) return null;
  const anchor = nearestAnchor(target, point);
  const endpoint: PathEndpoint = {
    kind: 'connected',
    item_id: target.id,
    anchor,
    fallback_point: connectedEndpointPoint(target, anchor),
  };
  const next = endpointId === 'from'
    ? { ...path, from: endpoint }
    : { ...path, to: endpoint };
  return autoRoutePath(next, itemMap);
}

export function detachConnectedEndpoints(
  path: PathItem,
  itemMap: ReadonlyMap<string, CanvasItem>,
): PathItem {
  const detach = (endpoint: PathEndpoint): PathEndpoint =>
    endpoint.kind === 'connected'
      ? { kind: 'free', point: resolveEndpoint(endpoint, itemMap) }
      : endpoint;
  const next = { ...path, from: detach(path.from), to: detach(path.to) };
  return updatePathBBoxCache(next, itemMap);
}

export function degradeDeletedEndpoint(
  path: PathItem,
  deletedIds: ReadonlySet<string>,
): PathItem {
  const degrade = (endpoint: PathEndpoint): PathEndpoint =>
    endpoint.kind === 'connected' && deletedIds.has(endpoint.item_id)
      ? { kind: 'free', point: endpoint.fallback_point }
      : endpoint;
  return { ...path, from: degrade(path.from), to: degrade(path.to) };
}

export function translatePath(path: PathItem, dx: number, dy: number): PathItem {
  const moveEndpoint = (endpoint: PathEndpoint): PathEndpoint =>
    endpoint.kind === 'free'
      ? { kind: 'free', point: { x: endpoint.point.x + dx, y: endpoint.point.y + dy } }
      : endpoint;
  const next: PathItem = {
    ...path,
    from: moveEndpoint(path.from),
    to: moveEndpoint(path.to),
    waypoints: path.waypoints?.map((p) => ({ ...p, x: p.x + dx, y: p.y + dy })),
  };
  return {
    ...next,
    x: path.x + dx,
    y: path.y + dy,
  };
}

export function editPathGeometry(
  path: PathItem,
  key: 'x' | 'y' | 'w' | 'h',
  value: number,
  itemMap: ReadonlyMap<string, CanvasItem>,
): PathItem {
  const detached = detachConnectedEndpoints(path, itemMap);
  const box = computePathBBox(detached, itemMap);
  if (key === 'x' || key === 'y') {
    const moved = translatePath(
      detached,
      key === 'x' ? value - box.x : 0,
      key === 'y' ? value - box.y : 0,
    );
    return updatePathBBoxCache(moved, itemMap);
  }

  const bounds = pathPointBounds(detached, itemMap);
  const pad = Math.max(PATH_HIT_PADDING, detached.stroke_width / 2 + PATH_HIT_PADDING);
  const oldInnerW = Math.max(bounds.maxX - bounds.minX, 1);
  const oldInnerH = Math.max(bounds.maxY - bounds.minY, 1);
  const targetInnerW = key === 'w'
    ? Math.max(1, value - pad * 2)
    : oldInnerW;
  const targetInnerH = key === 'h'
    ? Math.max(1, value - pad * 2)
    : oldInnerH;
  const sx = targetInnerW / oldInnerW;
  const sy = targetInnerH / oldInnerH;
  const scalePoint = (point: Point): Point => ({
    x: bounds.minX + (point.x - bounds.minX) * sx,
    y: bounds.minY + (point.y - bounds.minY) * sy,
  });
  const scaleEndpoint = (endpoint: PathEndpoint): PathEndpoint =>
    endpoint.kind === 'free'
      ? { kind: 'free', point: scalePoint(endpoint.point) }
      : endpoint;
  const scaled: PathItem = {
    ...detached,
    from: scaleEndpoint(detached.from),
    to: scaleEndpoint(detached.to),
    waypoints: detached.waypoints?.map((p) => {
      const point = scalePoint(p);
      return { ...p, x: point.x, y: point.y };
    }),
  };
  return updatePathBBoxCache(scaled, itemMap);
}

export function insertWaypointAtSegment(
  path: PathItem,
  segmentIndex: number,
  point: Point,
): PathItem {
  const waypoints = [...(path.waypoints ?? [])];
  const index = Math.max(0, Math.min(waypoints.length, segmentIndex));
  waypoints.splice(index, 0, { id: generateUuidV4(), x: point.x, y: point.y });
  return { ...path, waypoints };
}

function projectPointToSegment(point: Point, a: Point, b: Point): Point {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const lenSq = dx * dx + dy * dy;
  if (lenSq === 0) return a;
  const t = Math.max(0, Math.min(1, ((point.x - a.x) * dx + (point.y - a.y) * dy) / lenSq));
  return { x: a.x + dx * t, y: a.y + dy * t };
}

function expandedPairSegments(a: Point, b: Point, routing: PathRouting): [Point, Point][] {
  if (routing === 'orthogonal' && a.x !== b.x && a.y !== b.y) {
    const elbow = { x: b.x, y: a.y };
    return [[a, elbow], [elbow, b]];
  }
  return [[a, b]];
}

export function insertWaypointNearPoint(
  path: PathItem,
  point: Point,
  itemMap: ReadonlyMap<string, CanvasItem>,
): PathItem {
  const chain = pathPointChain(path, itemMap);
  let bestIndex = 0;
  let bestPoint = point;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (let i = 0; i < chain.length - 1; i += 1) {
    const a = chain[i]!;
    const b = chain[i + 1]!;
    for (const [segA, segB] of expandedPairSegments(a, b, path.routing)) {
      const projected = projectPointToSegment(point, segA, segB);
      const distance = Math.hypot(projected.x - point.x, projected.y - point.y);
      if (distance < bestDistance) {
        bestDistance = distance;
        bestIndex = i;
        bestPoint = projected;
      }
    }
  }
  return insertWaypointAtSegment(path, bestIndex, bestPoint);
}

export function removeWaypoints(
  path: PathItem,
  waypointIds: ReadonlySet<string>,
): PathItem {
  const waypoints = (path.waypoints ?? []).filter((p) => !waypointIds.has(p.id));
  return { ...path, waypoints };
}

export function moveWaypoints(
  path: PathItem,
  waypointIds: ReadonlySet<string>,
  delta: Point,
  _routing: PathRouting = path.routing,
): PathItem {
  if (waypointIds.size === 0) return path;
  const waypoints: PathWaypoint[] = (path.waypoints ?? []).map((p) =>
    waypointIds.has(p.id) ? { ...p, x: p.x + delta.x, y: p.y + delta.y } : p,
  );
  return { ...path, waypoints };
}
