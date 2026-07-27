<script lang="ts">
  // FindBar — shared in-document find bar (ADR-0058 D1). Floating top-right
  // overlay of a searchable surface; borrows the LeftPanel footer-search
  // styling vocabulary (ADR-0052 D2 — --color-border box, :focus-within
  // accent, mono input, clear ×) and adds the IDE extensions: n/M counter,
  // prev/next, Enter = next / Shift+Enter = prev, Esc = close (escRouter p1).
  //
  // The host owns the find state (DocumentFindController); this component is
  // presentation + input debounce only.

  import { debounce } from './debounce';
  import { escRouter } from './escRouter.svelte';
  import { FIND_MAX_MATCHES } from './textFind';
  import CanvasGlyph from '$lib/canvas/CanvasGlyph.svelte';

  let {
    matchCount,
    currentIndex,
    capped = false,
    initialQuery = '',
    onQueryChange,
    onNavigate,
    onClose,
  }: {
    matchCount: number;
    /** 0-based index of the current match (ignored when matchCount is 0). */
    currentIndex: number;
    /** True when matching stopped at FIND_MAX_MATCHES → total shows "5000+". */
    capped?: boolean;
    /** Query restored on reopen (host keeps it across close, ADR-0058 D1). */
    initialQuery?: string;
    onQueryChange: (query: string) => void;
    onNavigate: (dir: 1 | -1) => void;
    onClose: () => void;
  } = $props();

  let inputEl = $state<HTMLInputElement | null>(null);
  // Deliberately captures only the mount-time value — the input is the owner
  // of the text afterwards (host pushes changes via prefill()).
  // svelte-ignore state_referenced_locally
  let value = $state(initialQuery);

  // 150ms trailing debounce (ADR-0058 D1, shared debounce.ts default).
  const debouncedChange = debounce((query: string) => onQueryChange(query), 150);

  function commitNow(): void {
    debouncedChange.cancel();
    onQueryChange(value);
  }

  /** Focus (and optionally select-all) the input — shortcut re-invoke path. */
  export function focusInput(opts?: { selectAll?: boolean }): void {
    inputEl?.focus();
    if (opts?.selectAll === true) inputEl?.select();
  }

  /** Set the query from a DOM selection (ADR-0058 D5 branch 2 prefill). */
  export function prefill(text: string): void {
    value = text;
    commitNow();
  }

  const totalLabel = $derived(capped ? `${FIND_MAX_MATCHES}+` : String(matchCount));
  const currentLabel = $derived(matchCount === 0 ? 0 : currentIndex + 1);
  const noMatch = $derived(value.length > 0 && matchCount === 0);

  function onInputKeydown(e: KeyboardEvent): void {
    if (e.key !== 'Enter' || e.isComposing) return;
    e.preventDefault();
    e.stopPropagation();
    // Flush a pending debounce so navigation uses what the user sees.
    commitNow();
    onNavigate(e.shiftKey ? -1 : 1);
  }

  function clearQuery(): void {
    value = '';
    commitNow();
    inputEl?.focus();
  }

  // Esc = close while open — inline-edit tier so closing the find wins over
  // unmaximize / drill-out (ADR-0058 D1). Registered only while mounted; the
  // host mounts the bar only while open.
  $effect(() => {
    return escRouter.register({
      priority: 1,
      handler: () => {
        onClose();
        return true;
      },
    });
  });

  // No autofocus on mount: focus is an EXPLICIT-open concern only (every host's
  // openFindSurface calls focusInput() after tick). Focusing on mount would
  // steal focus on a maximize/restore transition remount, where the bar must
  // survive silently (ADR-0058 D1 override 2026-07-23 — survive maximize/restore).

  $effect(() => {
    return () => debouncedChange.cancel();
  });
</script>

<!-- `nodrag` + pointer/dblclick containment: on the canvas the bar must not
     start a node drag or trigger the document body's dblclick-to-edit. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="find-bar nodrag"
  role="search"
  aria-label="Find in document"
  onpointerdown={(e: PointerEvent) => e.stopPropagation()}
  onmousedown={(e: MouseEvent) => e.stopPropagation()}
  ondblclick={(e: MouseEvent) => e.stopPropagation()}
  onclick={(e: MouseEvent) => e.stopPropagation()}
>
  <span class="find-bar-icon" aria-hidden="true">
    <CanvasGlyph name="search" size={13} />
  </span>
  <input
    bind:this={inputEl}
    bind:value
    class="find-bar-input"
    class:no-match={noMatch}
    type="search"
    placeholder="Find"
    aria-label="Find in document"
    oninput={() => debouncedChange(value)}
    onkeydown={onInputKeydown}
  />
  {#if value !== ''}
    <button
      type="button"
      class="find-bar-btn"
      title="Clear"
      aria-label="Clear find query"
      onclick={clearQuery}
    >
      <CanvasGlyph name="close" size={13} />
    </button>
  {/if}
  <span class="find-bar-count" class:no-match={noMatch} aria-live="polite">
    {currentLabel}/{totalLabel}
  </span>
  <button
    type="button"
    class="find-bar-btn"
    title="Previous match (Shift+Enter)"
    aria-label="Previous match"
    disabled={matchCount === 0}
    onclick={() => onNavigate(-1)}
  >
    <CanvasGlyph name="chevron-up" size={13} />
  </button>
  <button
    type="button"
    class="find-bar-btn"
    title="Next match (Enter)"
    aria-label="Next match"
    disabled={matchCount === 0}
    onclick={() => onNavigate(1)}
  >
    <CanvasGlyph name="chevron-down" size={13} />
  </button>
  <button
    type="button"
    class="find-bar-btn"
    title="Close (Esc)"
    aria-label="Close find"
    onclick={onClose}
  >
    <CanvasGlyph name="close" size={13} />
  </button>
</div>

<style>
  /* Floating overlay, top-right of the host surface (host provides
   * position: relative). Styling mirrors .footer-search (LeftPanel) with a
   * surface fill + shadow since it floats over content. */
  .find-bar {
    position: absolute;
    top: var(--space-8);
    right: var(--space-12);
    left: auto;
    z-index: 5;
    /* Absolutely positioned + right-anchored: the box shrink-wraps its content
       and grows LEFT. On a narrow surface (right-panel preview, small canvas
       document node) that overflows the left edge and gets clipped by an
       overflow:hidden ancestor (.document-node / .max-body). Cap the width to
       the wrapper minus the right inset + a small left gap so the bar always
       fits fully; the input (min-width:0) and counter then shrink to fit
       (ADR-0058 D1 — user report 2026-07-23, "preview search가 panel 크기에
       의해 잘려서 나와"). */
    max-width: calc(100% - var(--space-12) - var(--space-8));
    display: flex;
    align-items: center;
    gap: var(--space-6);
    box-sizing: border-box;
    padding: 0 var(--space-6);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    box-shadow: var(--shadow-md);
    transition: border-color var(--motion-fast) var(--motion-easing);
  }

  .find-bar:hover {
    border-color: var(--color-border-strong);
  }

  .find-bar:focus-within {
    border-color: var(--color-accent);
  }

  .find-bar-icon {
    flex: 0 0 auto;
    display: inline-flex;
    color: var(--color-fg-muted);
  }

  .find-bar-input {
    /* Higher shrink factor than the counter (flex-shrink 3 vs 1) so at narrow
       widths the input gives up space FIRST and the n/M count stays readable;
       the counter still shrinks before the always-visible buttons (ADR-0058 D1
       clipping fix). */
    flex: 1 3 auto;
    width: 132px;
    min-width: 0;
    margin: 0;
    padding: var(--space-6) 0;
    border: 0;
    background: transparent;
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: var(--text-base);
    line-height: 1.2;
  }

  .find-bar-input::placeholder {
    color: var(--color-fg-muted);
  }

  .find-bar-input:focus {
    outline: none;
  }

  /* 0/0 warning tone (ADR-0058 D1). */
  .find-bar-input.no-match {
    color: var(--color-danger);
  }

  /* Strip the native search "clear" affordance — we render our own. */
  .find-bar-input::-webkit-search-decoration,
  .find-bar-input::-webkit-search-cancel-button {
    -webkit-appearance: none;
    appearance: none;
  }

  .find-bar-count {
    /* Shrinks (and truncates) before the prev/next/close buttons, which stay
       flex:0 0 auto — at the narrowest widths the counter clips but navigation
       stays reachable (ADR-0058 D1 clipping fix). */
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    letter-spacing: 0.2px;
    white-space: nowrap;
    user-select: none;
  }

  .find-bar-count.no-match {
    color: var(--color-danger);
  }

  .find-bar-btn {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px; /* chrome-tier compact box, 13px glyphs (icon unification 2026-07-27) */
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-fg-muted);
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .find-bar-btn:hover:not(:disabled) {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .find-bar-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .find-bar-btn:focus-visible {
    outline: 1px dashed var(--color-accent);
    outline-offset: 1px;
  }
</style>
