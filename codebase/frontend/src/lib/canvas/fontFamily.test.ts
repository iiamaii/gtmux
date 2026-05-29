import { describe, it, expect } from 'vitest';
import { fontFamilyVar } from '$lib/canvas/fontFamily';

// ADR-0041 — font_family enum → CSS token var (sans/serif/mono).

describe('fontFamilyVar', () => {
  it('maps serif', () => {
    expect(fontFamilyVar('serif')).toBe('var(--font-serif)');
  });
  it('maps mono', () => {
    expect(fontFamilyVar('mono')).toBe('var(--font-mono)');
  });
  it('maps sans', () => {
    expect(fontFamilyVar('sans')).toBe('var(--font-sans)');
  });
  it('defaults to sans when undefined', () => {
    expect(fontFamilyVar(undefined)).toBe('var(--font-sans)');
  });
});
