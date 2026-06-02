import { describe, expect, it } from 'vitest';

import { CODE_HIGHLIGHT_MAX_BYTES, highlightLines } from './codeHighlight';

describe('codeHighlight', () => {
  it('skips files above the highlight byte cap', async () => {
    await expect(highlightLines('x'.repeat(CODE_HIGHLIGHT_MAX_BYTES + 1), 'typescript')).resolves.toBeNull();
  });

  it('skips plain text languages', async () => {
    await expect(highlightLines('plain text', 'text')).resolves.toBeNull();
  });

  it('returns null for unsupported languages', async () => {
    await expect(highlightLines('value', 'not-a-real-language')).resolves.toBeNull();
  });

  it('returns structured tokens for supported code', async () => {
    const lines = await highlightLines('const value = 1;\n', 'typescript', 'light');
    expect(lines).not.toBeNull();
    expect(lines?.length).toBe(2);
    expect(lines?.[0]?.map((token) => token.content).join('')).toBe('const value = 1;');
    expect(lines?.[0]?.some((token) => token.color.startsWith('#'))).toBe(true);
  });
});
