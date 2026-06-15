// OSC 52 terminal clipboard write — ADR-0049.
//
// Write-only handler for the OSC 52 escape sequence (`ESC ] 52 ; Pc ; Pd BEL`).
// A mouse-mode TUI such as `claude` cannot create a native xterm selection, so
// it writes its internal selection to the clipboard via OSC 52. gtmux did not
// handle OSC 52 (security default `xterm.osc52.clipboard_write=false`), so the
// write was dropped. This module implements the *opt-in* write path.
//
// Hard security rules (ADR-0049):
//  - D2: write-only. An OSC 52 *read/query* (`Pd === '?'`) is swallowed and we
//        NEVER emit a response frame — that would be a clipboard-exfiltration
//        vector. The handler simply returns true (consumed) and does nothing.
//  - D3: dual gate — the clipboard write happens only when BOTH the consent
//        setting is on AND the page is a secure context. The gate is injected
//        (see `Osc52Deps.allowWrite`) so this module stays free of store/DOM
//        imports and is unit-testable in a node environment.
//  - D6: a decoded-text size cap (default 64 KB) blocks oversized payloads.
//
// The handler always returns `true` (sequence consumed) so xterm never falls
// back to any built-in behaviour, and never throws.

/** Default decoded-text size cap (D6). Payloads above this are ignored. */
export const OSC52_MAX_BYTES = 64 * 1024;

/**
 * Wrap `fn` so it runs at most once per process. Used for the gate-closed hint
 * (T3): the toast must appear once *per session*, not once per XtermHost — a
 * Svelte non-module `<script>` flag would reset per component instance, and the
 * same pane can be mirrored across multiple instances (ADR-0021 D1).
 */
export function runOnce(fn: () => void): () => void {
  let done = false;
  return () => {
    if (done) return;
    done = true;
    fn();
  };
}

export interface Osc52Deps {
  /** D3 dual gate — true only when consent setting AND secure context hold. */
  allowWrite: () => boolean;
  /** Performs the actual clipboard write (e.g. copyTextToSystemClipboard). */
  writeClipboard: (text: string) => void;
  /** One-time hint when the gate is closed (non-secure / disabled). */
  hint: () => void;
  /** Override the size cap (defaults to OSC52_MAX_BYTES). For tests. */
  maxBytes?: number;
}

/**
 * Base64-decode an OSC 52 `Pd` payload into UTF-8 text, enforcing the size cap.
 *
 * Returns `null` on malformed base64 or when the decoded byte length exceeds
 * `maxBytes` (caller swallows the sequence in both cases). The cap is applied to
 * the decoded *byte* length (UTF-8), matching the D6 intent of bounding the
 * payload that lands on the clipboard.
 */
export function decodeOsc52Base64(pd: string, maxBytes: number = OSC52_MAX_BYTES): string | null {
  // Empty payload — nothing to write, treat as a no-op (not an error).
  if (pd.length === 0) return null;
  // Cheap lower-bound pre-check: every 4 base64 chars decode to at least 2 bytes
  // (3 chars → 2 bytes is the worst case with padding), so the decoded size is
  // at least floor(len/4)*2. If even that lower bound exceeds the cap, reject
  // before allocating. Conservative — never rejects a payload at/below the cap;
  // the exact-length cap check below is authoritative.
  if (Math.floor(pd.length / 4) * 2 > maxBytes) {
    return null;
  }
  let bytes: Uint8Array;
  try {
    bytes = base64ToBytes(pd);
  } catch {
    return null; // malformed base64 → swallow
  }
  if (bytes.length > maxBytes) return null;
  try {
    return new TextDecoder('utf-8', { fatal: false }).decode(bytes);
  } catch {
    return null;
  }
}

/**
 * Decode standard base64 to bytes. Throws on malformed input.
 *
 * Uses the `atob` global, which is present in every target runtime (browser,
 * jsdom, and node ≥ 16 where vitest runs). `atob` throws `InvalidCharacterError`
 * on characters outside the base64 alphabet, which the caller turns into a
 * swallowed sequence.
 */
function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64); // throws on invalid characters
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) out[i] = binary.charCodeAt(i);
  return out;
}

/**
 * Build the OSC 52 handler callback for `term.parser.registerOscHandler(52, …)`.
 *
 * Pure factory: all side-effects (gate read, clipboard write, hint) are injected
 * via `deps`, so the returned function can be unit-tested without xterm/DOM.
 *
 * The `data` argument is the OSC payload *after* the `52;` identifier, i.e.
 * `"Pc;Pd"` where `Pc` is the selection target (`c`/`p`/`s`/…) and `Pd` is the
 * base64 text or `?` for a read/query.
 */
export function makeOsc52Handler(deps: Osc52Deps): (data: string) => boolean {
  const maxBytes = deps.maxBytes ?? OSC52_MAX_BYTES;
  return (data: string): boolean => {
    const sep = data.indexOf(';');
    if (sep < 0) return true; // malformed (no Pc;Pd split) → swallow
    const pd = data.slice(sep + 1);

    // D2: read/query is forbidden. Swallow without emitting any response.
    if (pd === '?') return true;

    // D3: dual gate. Closed → swallow + one-time hint, never throw.
    if (!deps.allowWrite()) {
      deps.hint();
      return true;
    }

    const text = decodeOsc52Base64(pd, maxBytes);
    if (text === null) return true; // malformed / oversized → swallow (D6)

    deps.writeClipboard(text);
    return true;
  };
}
