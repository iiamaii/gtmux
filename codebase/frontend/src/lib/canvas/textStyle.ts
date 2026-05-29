import type { FontWeight } from '$lib/types/canvas';

export function fontWeightCss(weight: FontWeight | undefined): 300 | 400 | 700 {
  if (weight === 'light') return 300;
  if (weight === 'bold') return 700;
  return 400;
}

export function textDecorationCss(style: {
  underline?: boolean;
  strikethrough?: boolean;
}): string {
  const parts: string[] = [];
  if (style.underline === true) parts.push('underline');
  if (style.strikethrough === true) parts.push('line-through');
  return parts.length === 0 ? 'none' : parts.join(' ');
}
