// Pure resolution for the panel resize-handle double-click width toggle
// (ADR-0017 amend 2026-07-28 ㉓, 재지정). A double-click on a panel's
// `.resize-handle` toggles the panel width between MIN (content-only,
// fold-independent) and a **content-fit** width — the width that shows all
// currently rendered panel content without horizontal ellipsis/clipping —
// mirroring the IDE splitter convention.
//
// The resolver is kept pure + framework-free so it can be unit-tested in
// isolation. The DOM measurement that produces `contentFitWidth` lives in the
// components (they own the DOM); see `measurePanelContentFitWidth` below.

/**
 * Resolve one double-click width toggle.
 *
 * - current > min → minimize: collapse to `min`.
 * - current == min → expand to the measured content-fit width, clamped to
 *   `[restoreFloor, max]`. The floor guards against a no-op-looking restore when
 *   content is narrow; the ceiling is the panel's clamp MAX.
 *
 * Pure: `(current, min, contentFitWidth, restoreFloor, max) -> width`.
 */
export function resolveWidthToggle(
  current: number,
  min: number,
  contentFitWidth: number,
  restoreFloor: number,
  max: number,
): number {
  if (current > min) return min;
  return Math.min(max, Math.max(restoreFloor, Math.round(contentFitWidth)));
}

/**
 * Measure the content-fit width for a panel — the width at which all currently
 * rendered content is fully visible without horizontal ellipsis/clipping.
 *
 * Nested tree/list rows ellipsize their labels (`min-width: 0` + `white-space:
 * nowrap` + `text-overflow: ellipsis`), so the scroll container's own
 * `scrollWidth` never exceeds its `clientWidth` — the overflow is hidden
 * *inside* each leaf span. A plain container-scrollWidth read therefore reveals
 * nothing. Instead we scan every descendant for horizontal clipping
 * (`scrollWidth > clientWidth`) and take the largest overflow delta; adding it
 * to the panel's current width un-clips the worst offender (and, transitively,
 * every less-clipped element), preserving the panel's existing horizontal chrome
 * inset (padding/indentation are already baked into each element's clientWidth).
 *
 * Works identically for both panels since it is content-agnostic: it keys off
 * the CSS overflow the ellipsis produces, not off any panel-specific selector.
 */
export function measurePanelContentFitWidth(
  panelEl: HTMLElement,
  currentWidth: number,
): number {
  let maxOverflow = 0;
  for (const el of panelEl.querySelectorAll<HTMLElement>('*')) {
    const overflow = el.scrollWidth - el.clientWidth;
    if (overflow > maxOverflow) maxOverflow = overflow;
  }
  // +1 absorbs sub-pixel rounding of scrollWidth/clientWidth when clipped.
  return currentWidth + (maxOverflow > 0 ? maxOverflow + 1 : 0);
}
