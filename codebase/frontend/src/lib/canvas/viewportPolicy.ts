// ADR-0005 D7 prop-lock: minZoom/maxZoom implementation source of truth.
// Provisional restore 2026-07-28 (0.25/2 → 0.05/3); see ADR-0005 changelog.
export const VIEWPORT_MIN_ZOOM = 0.05;
export const VIEWPORT_MAX_ZOOM = 3;
export const VIEWPORT_ZOOM_STEP = 0.1;

export function clampViewportZoom(zoom: number): number {
  return Math.min(VIEWPORT_MAX_ZOOM, Math.max(VIEWPORT_MIN_ZOOM, zoom));
}
