// shortcutRegistry — global keydown dispatcher for non-Esc shortcuts.
//
// 정본:
// - frontend-handover-v2 G26 (Hybrid registry + xterm focus)
// - ADR-0017 §D6 (Stage C chrome shortcuts) — partial wire here, the
//   rest land alongside Settings overlay (Slice C)
// - ADR-0024 D2 (z 4 actions — [, ], Shift+[, Shift+])
//
// Why a registry rather than scattered `addEventListener`?
//   - Single source of truth for "what shortcuts exist" — debug surface
//     (future Settings · Shortcuts section reads from here).
//   - Centralized guard logic (editable / xterm focus) — every consumer
//     would duplicate it otherwise.
//   - Symmetric with `escRouter` for the global Esc chain.
//
// Esc is intentionally NOT routed here — `escRouter` has its own
// priority chain (inline-edit > modal > unmaximize > tool > select)
// that doesn't map cleanly onto a flat keycombo table.
//
// Usage:
//   onMount(() => shortcutRegistry.register({
//     actionId: 'canvas.new_terminal',
//     key: 'n',
//     meta: true,
//     handler: () => { spawnTerminal(); return true; },
//   }));

const LETTER_KEYS = /^[a-z]$/;

export interface ShortcutDescriptor {
  /**
   * Stable command identity for future Settings shortcut overrides.
   * Multiple platform variants (Cmd vs Ctrl) share the same actionId.
   */
  actionId: string;
  /** event.key value to match. Letters are case-insensitive (compare
   *  via .toLowerCase()). Symbols are matched literally — e.g. `[`,
   *  `{`, `,`. */
  key: string;
  /** Meta (Cmd on macOS, ⊞ on Windows). Default `false`. */
  meta?: boolean;
  /** Ctrl. Default `false`. */
  ctrl?: boolean;
  /** Alt / Option. Default `false`. */
  alt?: boolean;
  /** Shift. Default `false`. */
  shift?: boolean;
  /** Handler — return `true` to consume the event (stopPropagation +
   *  preventDefault). Return `false` to let the chain continue. */
  handler: (event: KeyboardEvent) => boolean;
  /** Allow firing while an editable element is focused
   *  (input/textarea/select/contenteditable). Default: `true` if any
   *  modifier (meta/ctrl/alt) is required, `false` for plain keys. */
  allowInEditable?: boolean;
  /** Allow firing while xterm.js has focus. Default matches
   *  `allowInEditable`. */
  allowInXterm?: boolean;
  /** Optional description — surfaces in future Settings · Shortcuts. */
  description?: string;
  /** Optional category — `Canvas`, `Selection`, `Z`, `Chrome`, … */
  category?: string;
  /** Whether the first custom-shortcut pass may expose this action. */
  customizable?: boolean;
  /** Human-readable reason when customizable is false. */
  protectedReason?: string;
}

function isEditableFocused(): boolean {
  if (typeof document === 'undefined') return false;
  const el = document.activeElement as HTMLElement | null;
  if (el === null) return false;
  const tag = el.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true;
  if (el.isContentEditable) return true;
  return false;
}

function isXtermFocused(): boolean {
  if (typeof document === 'undefined') return false;
  const el = document.activeElement as HTMLElement | null;
  if (el === null) return false;
  return el.classList.contains('xterm-helper-textarea');
}

function eventMatches(d: ShortcutDescriptor, e: KeyboardEvent): boolean {
  if ((d.meta ?? false) !== e.metaKey) return false;
  if ((d.ctrl ?? false) !== e.ctrlKey) return false;
  if ((d.alt ?? false) !== e.altKey) return false;
  if ((d.shift ?? false) !== e.shiftKey) return false;
  // Letters: case-insensitive.
  const a = LETTER_KEYS.test(d.key.toLowerCase()) ? d.key.toLowerCase() : d.key;
  const b = LETTER_KEYS.test(e.key.toLowerCase()) ? e.key.toLowerCase() : e.key;
  return a === b;
}

function defaultAllowInEditable(d: ShortcutDescriptor): boolean {
  if (d.allowInEditable !== undefined) return d.allowInEditable;
  return Boolean(d.meta || d.ctrl || d.alt);
}

function defaultAllowInXterm(d: ShortcutDescriptor): boolean {
  if (d.allowInXterm !== undefined) return d.allowInXterm;
  return defaultAllowInEditable(d);
}

class ShortcutRegistry {
  #handlers = new Set<ShortcutDescriptor>();
  #attached = false;

  /** Register a shortcut. Returns an unregister callback. */
  register(d: ShortcutDescriptor): () => void {
    this.#handlers.add(d);
    this.#ensureAttached();
    return () => {
      this.#handlers.delete(d);
    };
  }

  /** Snapshot of currently-registered descriptors — for the future
   *  Settings · Shortcuts read-only list. */
  list(): ShortcutDescriptor[] {
    return Array.from(this.#handlers);
  }

  #ensureAttached(): void {
    if (this.#attached) return;
    if (typeof window === 'undefined') return;
    this.#attached = true;
    // bubble phase — let xterm/IME handle the key first when applicable.
    window.addEventListener('keydown', this.#onkeydown);
  }

  #onkeydown = (event: KeyboardEvent): void => {
    if (event.isComposing) return;

    // Snapshot — handler may unregister itself during dispatch.
    const handlers = Array.from(this.#handlers);
    const editableActive = isEditableFocused();
    const xtermActive = isXtermFocused();

    for (const d of handlers) {
      if (!eventMatches(d, event)) continue;
      if (editableActive && !defaultAllowInEditable(d)) continue;
      if (xtermActive && !defaultAllowInXterm(d)) continue;
      const consumed = d.handler(event);
      if (consumed) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
    }
  };

  /** Test/dev only. */
  _reset(): void {
    this.#handlers.clear();
  }
}

export const shortcutRegistry = new ShortcutRegistry();
