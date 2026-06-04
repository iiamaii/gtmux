export const VIEWPORT_MIN_ZOOM = 0.25;
export const VIEWPORT_MAX_ZOOM = 2;
export const VIEWPORT_ZOOM_STEP = 0.1;

export function clampViewportZoom(zoom: number): number {
  return Math.min(VIEWPORT_MAX_ZOOM, Math.max(VIEWPORT_MIN_ZOOM, zoom));
}
