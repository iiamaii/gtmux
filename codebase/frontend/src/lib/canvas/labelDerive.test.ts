import { describe, it, expect } from 'vitest';
import { deriveLabel, effectiveLabelAuto, shouldDeriveLabel } from '$lib/canvas/labelDerive';

// ADR-0040 D9 — text↔label one-shot derive (fresh + paste 통일).

describe('deriveLabel', () => {
  it('takes the first line only', () => {
    expect(deriveLabel('Hello\nWorld')).toBe('Hello');
  });
  it('trims surrounding whitespace', () => {
    expect(deriveLabel('   spaced   ')).toBe('spaced');
  });
  it('returns empty for empty text', () => {
    expect(deriveLabel('')).toBe('');
  });
  it('caps at 4000 chars', () => {
    expect(deriveLabel('a'.repeat(5000))).toHaveLength(4000);
  });
});

describe('effectiveLabelAuto', () => {
  it('absent flag + empty label => derive (legacy rule)', () => {
    expect(effectiveLabelAuto(undefined, '')).toBe(true);
  });
  it('absent flag + non-empty label => no derive (legacy rule)', () => {
    expect(effectiveLabelAuto(undefined, 'Greeting')).toBe(false);
  });
  it('explicit true overrides non-empty label (paste case)', () => {
    expect(effectiveLabelAuto(true, 'copied label')).toBe(true);
  });
  it('explicit false pins label even when empty', () => {
    expect(effectiveLabelAuto(false, '')).toBe(false);
  });
});

describe('shouldDeriveLabel', () => {
  it('fresh empty-label item derives on first non-empty text', () => {
    expect(shouldDeriveLabel(undefined, '', 'New text')).toBe(true);
  });
  it('pasted item (label_auto=true) re-derives on next text change', () => {
    expect(shouldDeriveLabel(true, 'copied', 'Changed text')).toBe(true);
  });
  it('user-pinned label (label_auto=false) never re-derives', () => {
    expect(shouldDeriveLabel(false, 'pinned', 'Changed text')).toBe(false);
  });
  it('empty next text never derives', () => {
    expect(shouldDeriveLabel(undefined, '', '')).toBe(false);
  });
});
