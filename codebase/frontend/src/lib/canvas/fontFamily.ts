import type { FontFamily } from '$lib/types/canvas';

export function fontFamilyVar(family: FontFamily | undefined): string {
  switch (family) {
    case 'serif':
      return 'var(--font-serif)';
    case 'mono':
      return 'var(--font-mono)';
    default:
      return 'var(--font-sans)';
  }
}
