export interface ResizeParams {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface CurrentBounds {
  x: number;
  y: number;
  w: number;
  h: number;
}

export function resizeEventShiftKey(event: unknown): boolean {
  const direct = event as { shiftKey?: unknown } | null;
  if (direct !== null && direct?.shiftKey === true) return true;
  const sourced = event as { sourceEvent?: { shiftKey?: unknown } } | null;
  return sourced?.sourceEvent?.shiftKey === true;
}

export function constrainResizeAspect(
  params: ResizeParams,
  current: CurrentBounds,
  aspect: number,
  minWidth: number,
  minHeight: number,
): ResizeParams {
  if (!Number.isFinite(aspect) || aspect <= 0) return params;

  const rawW = Math.max(minWidth, params.width);
  const rawH = Math.max(minHeight, params.height);
  const widthDelta = Math.abs(rawW - current.w);
  const heightDelta = Math.abs(rawH - current.h);

  let width = rawW;
  let height = rawH;
  if (widthDelta >= heightDelta) {
    height = width / aspect;
    if (height < minHeight) {
      height = minHeight;
      width = height * aspect;
    }
  } else {
    width = height * aspect;
    if (width < minWidth) {
      width = minWidth;
      height = width / aspect;
    }
  }

  const affectsX = Math.abs(params.x - current.x) > 0.001;
  const affectsY = Math.abs(params.y - current.y) > 0.001;
  const right = current.x + current.w;
  const bottom = current.y + current.h;

  return {
    x: affectsX ? right - width : params.x,
    y: affectsY ? bottom - height : params.y,
    width,
    height,
  };
}

export function constrainResizeAspectIfShift(
  event: unknown,
  params: ResizeParams,
  current: CurrentBounds,
  aspect: number,
  minWidth: number,
  minHeight: number,
): ResizeParams {
  return resizeEventShiftKey(event)
    ? constrainResizeAspect(params, current, aspect, minWidth, minHeight)
    : params;
}

export function scheduleLiveAspectResize(
  event: unknown,
  params: ResizeParams,
  current: CurrentBounds,
  aspect: number,
  minWidth: number,
  minHeight: number,
  apply: (next: ResizeParams) => void,
): void {
  if (!resizeEventShiftKey(event)) return;
  const constrained = constrainResizeAspect(params, current, aspect, minWidth, minHeight);
  queueMicrotask(() => apply(constrained));
}

export function scheduleLiveSquareResize(
  event: unknown,
  params: ResizeParams,
  current: CurrentBounds,
  minSize: number,
  apply: (next: ResizeParams) => void,
): void {
  if (!resizeEventShiftKey(event)) return;
  const constrained = constrainResizeSquare(params, current, minSize);
  queueMicrotask(() => apply(constrained));
}

export function constrainResizeSquare(
  params: ResizeParams,
  current: CurrentBounds,
  minSize: number,
): ResizeParams {
  const next = constrainResizeAspect(params, current, 1, minSize, minSize);
  const size = Math.max(minSize, next.width, next.height);
  return constrainResizeAspect(
    { ...next, width: size, height: size },
    current,
    1,
    minSize,
    minSize,
  );
}

export function squarePointFromDrag(
  start: { x: number; y: number },
  current: { x: number; y: number },
): { x: number; y: number } {
  const dx = current.x - start.x;
  const dy = current.y - start.y;
  const size = Math.max(Math.abs(dx), Math.abs(dy));
  return {
    x: start.x + Math.sign(dx || 1) * size,
    y: start.y + Math.sign(dy || 1) * size,
  };
}

export function projectPointToAngle(
  start: { x: number; y: number },
  current: { x: number; y: number },
  angle: number,
): { x: number; y: number } {
  const ux = Math.cos(angle);
  const uy = Math.sin(angle);
  const dx = current.x - start.x;
  const dy = current.y - start.y;
  const distance = dx * ux + dy * uy;
  return {
    x: start.x + ux * distance,
    y: start.y + uy * distance,
  };
}
