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

/** Default TTL for the gesture-backed Cmd+C fallback buffer (D7), in ms. */
export const OSC52_FALLBACK_TTL_MS = 10_000;

// ── D7: gesture-backed Cmd+C fallback buffer ─────────────────────────────
//
// A mouse-mode TUI (claude) cannot create a native xterm selection, so a plain
// Cmd+C finds `term.getSelection()` empty and copies nothing. The TUI's only
// text channel is OSC 52, but the immediate `navigator.clipboard.writeText`
// from the async OSC handler can be blocked by some browsers (no transient
// activation). So we bridge: the OSC 52 handler remembers the last gated write
// here, and the Cmd+C keydown (a real user gesture) drains it.
//
// Security (ADR-0049 D7): `rememberOsc52` is wired into the handler ONLY on the
// post-gate path (consent ON + secure context). If consent is OFF the buffer is
// never filled, so the fallback can never fire. The buffered text is the
// already-decoded, size-capped payload (D2/D6 still hold upstream).
//
// This buffer is module-scoped state — deliberately NOT importing any store or
// DOM so osc52.ts stays node-testable. The clock is injected (`nowMs`) by the
// caller (`performance.now()` in the browser, a fixed value in tests).

interface Osc52Buffer {
  text: string;
  at: number;
}

let osc52Buffer: Osc52Buffer | null = null;

/**
 * Remember the last successfully-gated OSC 52 write for the gesture-backed
 * fallback (D7). Call this from the handler write path ONLY after the dual gate
 * (D3) has passed, so the buffer never holds text the user did not consent to.
 *
 * `nowMs` is injected (the caller reads the clock) to keep this module free of
 * any clock/DOM dependency and node-testable.
 */
export function rememberOsc52(text: string, nowMs: number): void {
  osc52Buffer = { text, at: nowMs };
}

/**
 * Return the buffered OSC 52 text if it is fresh (within `ttlMs` of `nowMs`),
 * else `null`. One-shot: a successful take CLEARS the buffer so the same payload
 * is not re-copied on a later, unrelated Cmd+C. An expired buffer is also
 * cleared (it can never become valid again). `nowMs` is injected by the caller.
 */
export function takeRecentOsc52(
  ttlMs: number = OSC52_FALLBACK_TTL_MS,
  nowMs: number = 0,
): string | null {
  const buf = osc52Buffer;
  if (buf === null) return null;
  if (nowMs - buf.at > ttlMs) {
    // Stale — drop it so we do not keep checking a buffer that can never pass.
    osc52Buffer = null;
    return null;
  }
  osc52Buffer = null; // one-shot: consume on successful take.
  return buf.text;
}

/**
 * Like {@link takeRecentOsc52} but NON-draining: return the buffered OSC 52 text
 * if fresh (within `ttlMs` of `nowMs`), else `null`, WITHOUT clearing the buffer
 * or mutating any state.
 *
 * Used by the find shortcut (Cmd/Ctrl+F, ADR-0052 D2) to route a mouse-mode TUI
 * highlight into the search. Reading the highlight for search must NOT consume
 * the one-shot buffer a following Cmd+C copy relies on (ADR-0049 D7), so find
 * peeks while copy takes. A stale buffer is left in place (read-only) — the next
 * `takeRecentOsc52` clears it. `nowMs` is injected, same as `takeRecentOsc52`.
 */
export function peekRecentOsc52(
  ttlMs: number = OSC52_FALLBACK_TTL_MS,
  nowMs: number = 0,
): string | null {
  const buf = osc52Buffer;
  if (buf === null) return null;
  if (nowMs - buf.at > ttlMs) return null; // stale — but leave it (peek is read-only).
  return buf.text;
}

/** Test-only: clear the fallback buffer so cases don't bleed into each other. */
export function __resetOsc52Buffer(): void {
  osc52Buffer = null;
}

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
  /**
   * Monotonic-ish clock for the D7 fallback buffer timestamp. Injected so the
   * module stays free of clock/DOM imports and node-testable. Defaults to a
   * constant `0` (sufficient when the gesture-backed fallback is unused, e.g.
   * pure handler unit tests); the real caller passes `() => performance.now()`.
   */
  now?: () => number;
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
  const now = deps.now ?? (() => 0);
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

    // Belt: immediate write (auto-copy on drag). Suspenders (D7): remember the
    // gated text so a following Cmd+C gesture can copy it if the immediate
    // async write was blocked. Buffer is populated ONLY past the gate, so a
    // consent-off session never fills it and the fallback never fires.
    rememberOsc52(text, now());
    deps.writeClipboard(text);
    return true;
  };
}
