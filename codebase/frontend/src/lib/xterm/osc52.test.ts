import { describe, expect, it, vi } from 'vitest';
import { decodeOsc52Base64, makeOsc52Handler, OSC52_MAX_BYTES, runOnce } from './osc52';

// Standard base64 of a UTF-8 string. `btoa` is latin1-only, so encode to UTF-8
// bytes first and feed each byte as a code unit. Avoids node-only `Buffer` so
// the file type-checks under the app (DOM-lib) tsconfig used by `pnpm check`.
function b64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

interface Harness {
  handler: (data: string) => boolean;
  writes: string[];
  hints: number;
  allow: { value: boolean };
}

function harness(opts?: { allow?: boolean; maxBytes?: number }): Harness {
  const writes: string[] = [];
  const allow = { value: opts?.allow ?? true };
  let hints = 0;
  const handler = makeOsc52Handler({
    allowWrite: () => allow.value,
    writeClipboard: (t) => writes.push(t),
    hint: () => {
      hints += 1;
    },
    maxBytes: opts?.maxBytes,
  });
  // hints is captured by closure; expose via getter on the object.
  return {
    handler,
    writes,
    allow,
    get hints() {
      return hints;
    },
  } as Harness;
}

describe('decodeOsc52Base64', () => {
  it('decodes valid base64 to UTF-8 text', () => {
    expect(decodeOsc52Base64(b64('hello'))).toBe('hello');
    expect(decodeOsc52Base64(b64('héllo 한글 🚀'))).toBe('héllo 한글 🚀');
  });

  it('returns null for malformed base64', () => {
    // Spaces and `!` are outside the base64 alphabet → atob/Buffer reject.
    expect(decodeOsc52Base64('not valid base64!!!')).toBeNull();
    expect(decodeOsc52Base64('@@@@')).toBeNull();
  });

  it('returns null for empty payload', () => {
    expect(decodeOsc52Base64('')).toBeNull();
  });

  it('enforces the decoded-byte size cap', () => {
    const small = b64('x'.repeat(8));
    expect(decodeOsc52Base64(small, 16)).toBe('x'.repeat(8));
    const tooBig = b64('x'.repeat(64));
    expect(decodeOsc52Base64(tooBig, 16)).toBeNull();
  });

  it('default cap is 64 KB', () => {
    expect(OSC52_MAX_BYTES).toBe(64 * 1024);
    const atCap = b64('y'.repeat(OSC52_MAX_BYTES));
    expect(decodeOsc52Base64(atCap)).toBe('y'.repeat(OSC52_MAX_BYTES));
    const overCap = b64('y'.repeat(OSC52_MAX_BYTES + 1));
    expect(decodeOsc52Base64(overCap)).toBeNull();
  });
});

describe('makeOsc52Handler — write path (gate ON, secure)', () => {
  it('decodes base64 and calls the clipboard write', () => {
    const h = harness({ allow: true });
    const ret = h.handler(`c;${b64('hello')}`);
    expect(ret).toBe(true);
    expect(h.writes).toEqual(['hello']);
    expect(h.hints).toBe(0);
  });

  it('handles different selection targets (p, s)', () => {
    const h = harness({ allow: true });
    h.handler(`p;${b64('primary')}`);
    h.handler(`s;${b64('select')}`);
    expect(h.writes).toEqual(['primary', 'select']);
  });
});

describe('makeOsc52Handler — read/query forbidden (D2)', () => {
  it('swallows a `?` query with no write and no response', () => {
    const h = harness({ allow: true });
    const ret = h.handler('c;?');
    // Consumed (no fallthrough) but absolutely no clipboard side effect and no
    // hint (a query is not a gate failure — it is simply never honored).
    expect(ret).toBe(true);
    expect(h.writes).toEqual([]);
    expect(h.hints).toBe(0);
  });

  it('does not honor `?` even when the gate is open', () => {
    const h = harness({ allow: true });
    h.handler('p;?');
    expect(h.writes).toEqual([]);
  });
});

describe('makeOsc52Handler — gate closed (D3)', () => {
  it('does not write and shows a hint when allowWrite is false', () => {
    const h = harness({ allow: false });
    const ret = h.handler(`c;${b64('secret')}`);
    expect(ret).toBe(true);
    expect(h.writes).toEqual([]);
    expect(h.hints).toBe(1);
  });

  it('re-checks the gate per call (toggle on → write resumes)', () => {
    const h = harness({ allow: false });
    h.handler(`c;${b64('first')}`);
    expect(h.writes).toEqual([]);
    h.allow.value = true;
    h.handler(`c;${b64('second')}`);
    expect(h.writes).toEqual(['second']);
  });
});

describe('makeOsc52Handler — size cap (D6)', () => {
  it('ignores an oversized decoded payload, no write', () => {
    const h = harness({ allow: true, maxBytes: 16 });
    const ret = h.handler(`c;${b64('x'.repeat(64))}`);
    expect(ret).toBe(true);
    expect(h.writes).toEqual([]);
  });
});

describe('makeOsc52Handler — malformed input', () => {
  it('swallows a payload without a `;` separator', () => {
    const h = harness({ allow: true });
    const ret = h.handler('no-separator');
    expect(ret).toBe(true);
    expect(h.writes).toEqual([]);
    expect(h.hints).toBe(0);
  });

  it('swallows malformed base64 (gate open) without writing', () => {
    const h = harness({ allow: true });
    const ret = h.handler('c;@@@not-base64@@@');
    expect(ret).toBe(true);
    expect(h.writes).toEqual([]);
  });

  it('never throws for arbitrary garbage', () => {
    const h = harness({ allow: true });
    expect(() => h.handler('')).not.toThrow();
    expect(() => h.handler(';')).not.toThrow();
    expect(() => h.handler(';;;;')).not.toThrow();
  });
});

describe('runOnce', () => {
  it('invokes the wrapped fn at most once', () => {
    const fn = vi.fn();
    const once = runOnce(fn);
    once();
    once();
    once();
    expect(fn).toHaveBeenCalledTimes(1);
  });
});

describe('makeOsc52Handler — gate is queried lazily', () => {
  it('does not call allowWrite for a `?` query (no gate side effect)', () => {
    const allowWrite = vi.fn(() => true);
    const handler = makeOsc52Handler({
      allowWrite,
      writeClipboard: () => {},
      hint: () => {},
    });
    handler('c;?');
    expect(allowWrite).not.toHaveBeenCalled();
  });
});
