<script lang="ts">
  /**
   * Shared canvas/chrome glyph — icon system unification 2026-07-27
   * (ADR-0016 정합).
   *
   * Single source of truth for every header / toolbar / modal *button* glyph
   * across the canvas node components (PanelNode, DocumentNode, SnippetsNode,
   * NoteNode, ImageNode, FilePathNode, CanvasCloseButton) and the chrome
   * surfaces (FilePreviewView, MaximizedItemModal, FindBar).
   *
   * Mechanism (why inline SVG, not lucide-svelte components):
   *   - The canvas render path deliberately avoids the lucide-svelte *components*
   *     because of the documented strict-build conflict (Toolbar2 §7 handover);
   *     the established idiom is direct inline SVG. This component keeps that
   *     idiom — zero new dependency, zero bundle delta, no build-conflict risk —
   *     while giving same-function buttons a byte-identical glyph everywhere.
   *   - Paths are lucide's canonical 24×24 icon geometry drawn inline, so the
   *     glyphs read as "lucide" (per the unification brief) without importing
   *     lucide components into the canvas tree.
   *
   * Standard metrics (the ONE standard — NoteNode-anchored rev, user feedback
   * 2026-07-27): every glyph shares viewBox 0 0 24 24, fill none, stroke
   * currentColor, stroke-width 2, round caps + joins, aria-hidden. Only the
   * rendered `size` (px) varies, by surface tier:
   *   - CANVAS tier (all six node headers — terminal / note / document /
   *     image / file-path / snippets; buttons AND type-identity glyphs)
   *     -> size 12 (default), button box 20×20, inter-button gap 1px.
   *   - CHROME tier (FilePreviewView toolbar, MaximizedItemModal, RightPanel
   *     inspector + rail, FindBar) -> size 13.
   * Type-identity glyphs share the button metrics so the header reads as one
   * system; 'note' = lucide scroll-text, anchored to the Toolbar2 note tool
   * (note glyph unification 2026-07-27 — the earlier simplified silhouette,
   * drawn because scroll-text is busy at 12px, was superseded by the user's
   * explicit toolbar-anchor directive).
   */

  type GlyphName =
    | 'close' // window/close X
    | 'minimize' // window-minimize (single low line) — keep current minimize
    | 'restore-min' // restore FROM minimized = square (lucide square)
    | 'maximize' // lucide maximize (corner brackets, outward)
    | 'restore-max' // lucide minimize (corner brackets, inward) — while maximized
    | 'search' // find magnifier
    | 'copy' // copy path / copy-mode (lucide copy)
    | 'download'
    | 'change' // change source (link-2)
    | 'book-open' // rendered / viewer mode
    | 'code' // source mode </>
    | 'pencil' // edit mode
    | 'trash' // delete mode
    | 'save'
    | 'undo'
    | 'redo'
    | 'chevron-up'
    | 'chevron-down'
    | 'plus'
    // Type-identity glyphs (header component icons)
    | 'terminal' // lucide square-terminal — PanelNode / modal terminal header
    | 'file' // lucide file — DocumentNode / FilePathNode(file) / modal doc header
    | 'folder' // lucide folder — FilePathNode(directory)
    | 'note' // lucide scroll-text (Toolbar2 anchor) — note tool / NoteNode / modal / layer tree
    | 'library' // lucide square-library — SnippetsNode header
    | 'image' // lucide image — ImageNode empty state
    | 'globe' // lucide globe — WebViewNode identity / web_view tool
    | 'reload' // lucide rotate-cw — web_view reload
    | 'external' // lucide external-link — open in browser
    // Inspector state glyphs (RightPanel inspect tab)
    | 'eye' // visible
    | 'eye-off' // hidden
    | 'lock' // locked
    | 'lock-open'; // unlocked

  interface Props {
    name: GlyphName;
    /** Rendered px (width == height). Defaults to the canvas-tier 12. */
    size?: number;
    class?: string;
  }

  const { name, size = 12, class: klass }: Props = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  class={klass}
>
  {#if name === 'close'}
    <path d="M18 6 6 18" /><path d="m6 6 12 12" />
  {:else if name === 'minimize'}
    <line x1="5" y1="18" x2="19" y2="18" />
  {:else if name === 'restore-min'}
    <rect x="4" y="4" width="16" height="16" rx="2" />
  {:else if name === 'maximize'}
    <path d="M8 3H5a2 2 0 0 0-2 2v3" />
    <path d="M21 8V5a2 2 0 0 0-2-2h-3" />
    <path d="M3 16v3a2 2 0 0 0 2 2h3" />
    <path d="M16 21h3a2 2 0 0 0 2-2v-3" />
  {:else if name === 'restore-max'}
    <path d="M8 3v3a2 2 0 0 1-2 2H3" />
    <path d="M21 8h-3a2 2 0 0 1-2-2V3" />
    <path d="M3 16h3a2 2 0 0 1 2 2v3" />
    <path d="M16 21v-3a2 2 0 0 1 2-2h3" />
  {:else if name === 'search'}
    <circle cx="11" cy="11" r="8" />
    <path d="m21 21-4.3-4.3" />
  {:else if name === 'copy'}
    <rect x="8" y="8" width="14" height="14" rx="2" ry="2" />
    <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
  {:else if name === 'download'}
    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
    <path d="m7 10 5 5 5-5" />
    <path d="M12 15V3" />
  {:else if name === 'change'}
    <path d="M9 17H7A5 5 0 0 1 7 7h2" />
    <path d="M15 7h2a5 5 0 1 1 0 10h-2" />
    <line x1="8" x2="16" y1="12" y2="12" />
  {:else if name === 'book-open'}
    <path d="M12 7v14" />
    <path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z" />
  {:else if name === 'code'}
    <path d="m16 18 6-6-6-6" />
    <path d="m8 6-6 6 6 6" />
  {:else if name === 'pencil'}
    <path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5z" />
    <path d="m15 5 4 4" />
  {:else if name === 'trash'}
    <path d="M3 6h18" />
    <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
    <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
    <line x1="10" x2="10" y1="11" y2="17" />
    <line x1="14" x2="14" y1="11" y2="17" />
  {:else if name === 'save'}
    <path d="M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" />
    <path d="M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7" />
    <path d="M7 3v4a1 1 0 0 0 1 1h7" />
  {:else if name === 'undo'}
    <path d="M9 14 4 9l5-5" />
    <path d="M4 9h11a5 5 0 0 1 0 10h-1" />
  {:else if name === 'redo'}
    <path d="m15 14 5-5-5-5" />
    <path d="M20 9H9a5 5 0 0 0 0 10h1" />
  {:else if name === 'chevron-up'}
    <path d="m18 15-6-6-6 6" />
  {:else if name === 'chevron-down'}
    <path d="m6 9 6 6 6-6" />
  {:else if name === 'plus'}
    <path d="M5 12h14" /><path d="M12 5v14" />
  {:else if name === 'terminal'}
    <rect width="18" height="18" x="3" y="3" rx="2" />
    <path d="m8 9 3 3-3 3" />
    <path d="M13 15h4" />
  {:else if name === 'file'}
    <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" />
    <path d="M14 2v4a2 2 0 0 0 2 2h4" />
  {:else if name === 'folder'}
    <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
  {:else if name === 'note'}
    <!-- lucide scroll-text — anchored to the Toolbar2 note-tool icon (note
         glyph unification 2026-07-27). Replaces the earlier simplified
         silhouette so toolbar / node header / modal / layer tree / minimized
         chip all share one drawing. -->
    <path d="M15 12h-5" />
    <path d="M15 8h-5" />
    <path d="M19 17V5a2 2 0 0 0-2-2H4" />
    <path d="M8 21h12a2 2 0 0 0 2-2v-1a1 1 0 0 0-1-1H11a1 1 0 0 0-1 1v1a2 2 0 1 1-4 0V5a2 2 0 1 0-4 0v2a1 1 0 0 0 1 1h3" />
  {:else if name === 'library'}
    <rect width="18" height="18" x="3" y="3" rx="2" />
    <path d="M7 7v10" />
    <path d="M11 7v10" />
    <path d="m15 7 2 10" />
  {:else if name === 'image'}
    <rect width="18" height="18" x="3" y="3" rx="2" ry="2" />
    <circle cx="9" cy="9" r="2" />
    <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" />
  {:else if name === 'globe'}
    <!-- lucide globe (24×24 canonical geometry) — web_view type identity. -->
    <circle cx="12" cy="12" r="10" />
    <path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20" />
    <path d="M2 12h20" />
  {:else if name === 'reload'}
    <!-- lucide rotate-cw — reload the live view. -->
    <path d="M21 12a9 9 0 1 1-2.64-6.36" />
    <path d="M21 3v6h-6" />
  {:else if name === 'external'}
    <!-- lucide external-link — open in a new browser tab. -->
    <path d="M15 3h6v6" />
    <path d="M10 14 21 3" />
    <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
  {:else if name === 'eye'}
    <path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0" />
    <circle cx="12" cy="12" r="3" />
  {:else if name === 'eye-off'}
    <path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49" />
    <path d="M14.084 14.158a3 3 0 0 1-4.242-4.242" />
    <path d="M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143" />
    <path d="m2 2 20 20" />
  {:else if name === 'lock'}
    <rect width="18" height="11" x="3" y="11" rx="2" ry="2" />
    <path d="M7 11V7a5 5 0 0 1 10 0v4" />
  {:else if name === 'lock-open'}
    <rect width="18" height="11" x="3" y="11" rx="2" ry="2" />
    <path d="M7 11V7a5 5 0 0 1 9.9-1" />
  {/if}
</svg>
