<script lang="ts">
  /**
   * CodeEditArea — IDE-style plain-textarea editor (ADR-0057 D1).
   *
   * ADR-0057 R1 stands: NO CodeMirror. The editable surface is still a real
   * `<textarea>` (owns caret / selection / native input / undo). The IDE look
   * is an *overlay* architecture that matches the read-mode CodeViewer's visual
   * vocabulary:
   *
   *   - a sticky line-number gutter (same `--code-viewer-*` metrics as
   *     CodeViewer), scroll-synced to the content;
   *   - a Shiki-highlighted `<pre>` (reuses `highlightLines`, same 512 KB cap +
   *     plain fallback) rendered UNDER a transparent-text textarea. The token
   *     DOM is real elements (never `{@html}` — CLAUDE.md §4 / ADR-0037 D7.3);
   *   - exact metric parity (font / size / line-height / tab-size / padding,
   *     `wrap="off"` + overlay `white-space: pre`) so the caret sits on the
   *     tokens. Scroll is mirrored textarea → overlay + gutter.
   *
   * Re-highlight is debounced ~150ms on input; the plain text is visible
   * instantly through the fallback line render (progressive, like CodeViewer).
   * Above the Shiki cap `highlightLines` returns null → gutter + plain mono
   * (no colors), never a freeze.
   */

  import { untrack } from 'svelte';
  import { highlightLines, type HlLine } from '$lib/canvas/codeHighlight';
  import { themeStore } from '$lib/stores/theme.svelte';
  import { debounce } from '$lib/common/debounce';

  let {
    value = $bindable(''),
    lang = 'text',
    ariaLabel = 'Code editor',
  }: {
    value: string;
    lang?: string;
    ariaLabel?: string;
  } = $props();

  let textareaEl = $state<HTMLTextAreaElement | null>(null);
  let overlayEl = $state<HTMLPreElement | null>(null);
  let gutterInnerEl = $state<HTMLDivElement | null>(null);
  let highlighted = $state<HlLine[] | null>(null);

  const lines = $derived(value.split('\n'));
  const cleanLang = $derived(lang.trim().toLowerCase() || 'text');

  // Debounced (re)highlight (ADR-0057 D1 — ~150ms, progressive). The plain
  // fallback render keeps text visible instantly while this settles.
  const scheduleHighlight = debounce((raw: string, nextLang: string, theme: 'light' | 'dark') => {
    void (async () => {
      const next = await highlightLines(raw, nextLang, theme);
      // Guard against a stale async resolve landing after the text moved on.
      if (untrack(() => value) === raw && cleanLang === nextLang) highlighted = next;
    })();
  }, 150);

  $effect(() => {
    const raw = value;
    const nextLang = cleanLang;
    const theme = themeStore.resolved;
    // Drop stale colors immediately so we never paint the previous file's
    // tokens over new text; the fallback line render covers the gap.
    highlighted = null;
    scheduleHighlight(raw, nextLang, theme);
  });

  $effect(() => () => scheduleHighlight.cancel());

  /** Mirror textarea scroll onto the overlay + gutter (both are non-scrolling). */
  function syncScroll(): void {
    const ta = textareaEl;
    if (ta === null) return;
    if (overlayEl !== null) {
      overlayEl.style.transform = `translate(${-ta.scrollLeft}px, ${-ta.scrollTop}px)`;
    }
    if (gutterInnerEl !== null) {
      gutterInnerEl.style.transform = `translateY(${-ta.scrollTop}px)`;
    }
  }

  // Keep the overlay aligned when content growth changes scroll extents (e.g.
  // typing past the viewport) without a scroll event firing.
  $effect(() => {
    void lines.length;
    void tickScrollSync();
  });

  function tickScrollSync(): void {
    // rAF so the DOM has laid out the new line before we read scroll offsets.
    requestAnimationFrame(syncScroll);
  }

  /**
   * Tab inserts a literal tab (tab-size:4, matching read mode) instead of
   * moving focus; `execCommand('insertText')` keeps the native undo stack,
   * with a `setRangeText` fallback. Shift+Tab is a no-op (v1).
   */
  function onKeydown(e: KeyboardEvent): void {
    if (e.key !== 'Tab') return;
    e.preventDefault();
    if (e.shiftKey) return; // v1 — no dedent.
    const ta = textareaEl;
    if (ta === null) return;
    const inserted = document.execCommand('insertText', false, '\t');
    if (!inserted) {
      const start = ta.selectionStart;
      const end = ta.selectionEnd;
      ta.setRangeText('\t', start, end, 'end');
      value = ta.value;
    }
  }

  // ── Imperative undo/redo (ADR-0057 D1 amend 2026-07-27) ──
  // Drives the textarea's NATIVE undo stack. Native typing and the Tab
  // `execCommand('insertText')` path both keep that stack intact (the
  // `bind:value` write-back skips because `el.value` already equals `value`),
  // so `execCommand('undo'/'redo')` — deprecated but still functional for form
  // controls in Chromium/WebKit — replays exactly the user's keystrokes and
  // fires `input`, keeping `value` (the parent's `draft`) in sync.
  //
  // The textarea must hold focus for `execCommand` to target it; the edit-bar
  // buttons preventDefault their mousedown to avoid stealing focus, and we
  // re-focus here defensively. The DOM exposes no reliable introspection of the
  // native stack (`queryCommandEnabled` is unreliable/deprecated), so no
  // canUndo/canRedo — the edit-bar buttons stay enabled while editing.
  export function undo(): void {
    const ta = textareaEl;
    if (ta === null) return;
    ta.focus();
    document.execCommand('undo');
  }

  export function redo(): void {
    const ta = textareaEl;
    if (ta === null) return;
    ta.focus();
    document.execCommand('redo');
  }
</script>

<div class="cea">
  <div class="cea-gutter" aria-hidden="true">
    <div bind:this={gutterInnerEl} class="cea-gutter-inner">
      {#each lines as _line, i (i)}
        <div class="cea-gutter-num">{i + 1}</div>
      {/each}
    </div>
  </div>
  <div class="cea-main">
    <!-- ADR-0057/0058 amend (2026-07-24) — each visual line is wrapped in an
         inline `.cv-line[data-line]` > `[data-code]` pair mirroring CodeViewer,
         so the DocumentFindController's line/col path (rangeForLineMatch) can
         address draft matches on this overlay. The wrappers are INLINE spans and
         the literal `\n` between lines is preserved, so text flow (and thus the
         textarea caret alignment) is byte-for-byte unchanged. -->
    <pre bind:this={overlayEl} class="cea-overlay" aria-hidden="true"><code
      >{#each lines as line, i (i)}{@const toks = highlighted?.[i]}<span class="cv-line" data-line={i + 1}><span data-code>{#if toks !== undefined && toks.length > 0}{#each toks as token, ti (ti)}<span
              style:color={token.color}>{token.content}</span
            >{/each}{:else}{line}{/if}</span></span>{#if i < lines.length - 1}{'\n'}{/if}{/each}</code
    ></pre>
    <textarea
      bind:this={textareaEl}
      bind:value
      class="cea-textarea"
      spellcheck="false"
      autocomplete="off"
      autocapitalize="off"
      wrap="off"
      aria-label={ariaLabel}
      onscroll={syncScroll}
      onkeydown={onKeydown}
    ></textarea>
  </div>
</div>

<style>
  /* Metrics mirror the read-mode CodeViewer via the same `--code-viewer-*`
     vars (defined on `.preview-surface`), so edit and read modes line up
     glyph-for-glyph. Local fallbacks match CodeViewer's own defaults. */
  .cea {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    overflow: hidden;
    background: var(--code-viewer-bg, var(--color-surface));
    /* Shared metrics — every text surface below reads these. */
    --cea-font-size: var(--code-viewer-font-size, 10.5px);
    --cea-line-height: var(--code-viewer-line-height, 1.6);
    --cea-pad-y: var(--space-8);
    --cea-pad-x: var(--space-8);
    --cea-tab-size: 4;
  }

  .cea-gutter {
    flex: 0 0 var(--code-viewer-gutter-width, 28px);
    overflow: hidden;
    box-sizing: content-box;
    padding: var(--cea-pad-y) 0;
    background: var(--code-viewer-bg, var(--color-surface));
    font-family: var(--font-mono);
    font-size: var(--cea-font-size);
    line-height: var(--cea-line-height);
    user-select: none;
  }

  .cea-gutter-inner {
    /* translated by -scrollTop to track the textarea. */
    will-change: transform;
  }

  .cea-gutter-num {
    /* No padding-right: the number is right-aligned flush to the 28px track
       edge, exactly like read-mode CodeViewer's `.cv-gutter` (which sits in a
       28px grid column with no padding). The 8px gap to the code then comes
       from `.cea-overlay`/`.cea-textarea` padding-left (--cea-pad-x), mirroring
       CodeViewer's grid `gap`. A padding-right here pulled the digits 8px left
       of the read-mode position (visible number shift on Viewer↔Edit toggle). */
    color: var(--color-fg-subtle);
    text-align: right;
    white-space: pre;
  }

  .cea-main {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
  }

  /* Both the overlay and the textarea MUST share identical box + text metrics
     so the caret lands on the highlighted tokens. */
  .cea-overlay,
  .cea-textarea {
    position: absolute;
    inset: 0;
    margin: 0;
    box-sizing: border-box;
    padding: var(--cea-pad-y) var(--cea-pad-x);
    border: 0;
    font-family: var(--font-mono);
    font-size: var(--cea-font-size);
    line-height: var(--cea-line-height);
    tab-size: var(--cea-tab-size);
    -moz-tab-size: var(--cea-tab-size);
    white-space: pre;
    overflow-wrap: normal;
    word-break: normal;
  }

  .cea-overlay {
    /* `overflow: visible` (not hidden): the overlay is translated by
       -scrollTop/-scrollLeft to mirror the textarea. If it clipped its OWN box
       first, content below/right of the box would be clipped away BEFORE the
       transform could reveal it (scrolled lines render blank). The parent
       `.cea-main` (overflow: hidden) does the viewport clip instead, so
       translated content scrolls into view correctly. Fixes scrolled-overlay
       blanking observed 2026-07-24 (also unblanks find scroll-to-match). */
    overflow: visible;
    pointer-events: none;
    color: var(--color-fg);
    background: transparent;
    will-change: transform;
  }

  .cea-overlay code {
    font: inherit;
    white-space: pre;
  }

  .cea-textarea {
    overflow: auto;
    resize: none;
    outline: none;
    /* Transparent glyphs — the overlay paints the (highlighted) text; the
       textarea keeps caret + selection. */
    color: transparent;
    caret-color: var(--color-fg);
    background: transparent;
    scrollbar-width: thin;
  }

  .cea-textarea::selection {
    /* Selection paints on the textarea layer (overlay text is transparent). */
    background: var(--color-selection, rgba(120, 160, 255, 0.35));
    /* Selected glyphs must stay invisible too. Browsers force a visible
       foreground on selected text, overriding the base `color: transparent`;
       that made the textarea's own (metric-divergent) glyphs paint on top of
       the overlay tokens → doubled/ghosted text under the highlight. Pin both
       the logical color and WebKit's text-fill so only the overlay paints. */
    color: transparent;
    -webkit-text-fill-color: transparent;
  }
</style>
