// strokeDash — FigureStrokeDash → SVG `stroke-dasharray` 매핑 helper.
//
// 정본: ADR-0018 D4 amend ① (batch-5, 2026-05-20) — rect/ellipse/line 의
// `stroke_dash` 필드는 4 variant (solid / dash / dot / dash_dot). SVG 의
// `stroke-dasharray` 는 length array — stroke_width 에 비례해 결정해야 두께가
// 바뀌어도 시각 비율이 유지된다.
//
// 패턴 (snake_case wire 값 → SVG dasharray):
//   solid    → "none"     (dashed 미적용, continuous stroke)
//   dash     → `${w*4} ${w*2}`             (긴 dash + 짧은 gap)
//   dot      → `${w} ${w*2}`               (짧은 dot + 두 배 gap — round cap 과 결합 시 원형 dot)
//   dash_dot → `${w*4} ${w*2} ${w} ${w*2}` (dash + gap + dot + gap)
//
// 사용처: ShapeNode (rect/ellipse), LineNode. 두 곳 모두 같은 helper.

import type { FigureStrokeDash } from '$lib/types/canvas';

/**
 * SVG `stroke-dasharray` 값 계산. `undefined` 또는 `"solid"` 는 `"none"` 반환.
 *
 * @param dash - `FigureStrokeDash` enum 값 (undefined = solid)
 * @param strokeWidth - 현재 stroke 두께 (px). 음수/0 일 때도 안전한 default 사용.
 */
export function strokeDashArray(
  dash: FigureStrokeDash | undefined,
  strokeWidth: number,
): string {
  if (dash === undefined || dash === 'solid') return 'none';
  const w = strokeWidth > 0 ? strokeWidth : 1;
  switch (dash) {
    case 'dash':
      return `${w * 4} ${w * 2}`;
    case 'dot':
      return `${w} ${w * 2}`;
    case 'dash_dot':
      return `${w * 4} ${w * 2} ${w} ${w * 2}`;
  }
}
