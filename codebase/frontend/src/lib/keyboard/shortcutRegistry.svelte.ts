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

import {
  bindingKey,
  normalizeShortcutBinding,
  shortcutOverrides,
  type ShortcutBinding,
} from '$lib/stores/shortcutOverrides.svelte';

const LETTER_KEYS = /^[a-z]$/;

export type { ShortcutBinding };

export interface ShortcutDescriptor extends ShortcutBinding {
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

export interface ShortcutAction {
  actionId: string;
  description: string;
  category: string;
  customizable: boolean;
  protectedReason?: string;
  defaultBindings: ShortcutBinding[];
  activeBindings: ShortcutBinding[];
  overridden: boolean;
}

export interface ShortcutConflict {
  kind: 'reserved' | 'action';
  actionId?: string;
  description: string;
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

function isMacPlatform(): boolean {
  if (typeof navigator === 'undefined') return false;
  return /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent);
}

function descriptorBinding(d: ShortcutDescriptor): ShortcutBinding {
  return normalizeShortcutBinding({
    key: d.key,
    meta: d.meta,
    ctrl: d.ctrl,
    alt: d.alt,
    shift: d.shift,
  });
}

function eventMatchesBinding(binding: ShortcutBinding, e: KeyboardEvent): boolean {
  const normalized = normalizeShortcutBinding(binding);
  if (normalized.meta !== e.metaKey) return false;
  if (normalized.ctrl !== e.ctrlKey) return false;
  if (normalized.alt !== e.altKey) return false;
  if (normalized.shift !== e.shiftKey) return false;
  // Letters: case-insensitive.
  const expected = LETTER_KEYS.test(normalized.key.toLowerCase())
    ? normalized.key.toLowerCase()
    : normalized.key;
  const actual = LETTER_KEYS.test(e.key.toLowerCase()) ? e.key.toLowerCase() : e.key;
  return expected === actual;
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
  revision = $state(0);

  /** Register a shortcut. Returns an unregister callback. */
  register(d: ShortcutDescriptor): () => void {
    this.#handlers.add(d);
    this.revision += 1;
    this.#ensureAttached();
    return () => {
      this.#handlers.delete(d);
      this.revision += 1;
    };
  }

  /** Snapshot of currently-registered descriptors — for the future
   *  Settings · Shortcuts read-only list. */
  list(): ShortcutDescriptor[] {
    void this.revision;
    return Array.from(this.#handlers);
  }

  listActions(): ShortcutAction[] {
    void this.revision;
    void shortcutOverrides.revision;
    return Array.from(this.#groupedActions().values())
      .map((descriptors) => this.#toAction(descriptors))
      .sort((a, b) => {
        const cat = a.category.localeCompare(b.category);
        return cat !== 0 ? cat : a.description.localeCompare(b.description);
      });
  }

  conflictFor(actionId: string, binding: ShortcutBinding): ShortcutConflict | null {
    const normalized = normalizeShortcutBinding(binding);
    const reserved = reservedReason(normalized);
    if (reserved !== null) return { kind: 'reserved', description: reserved };
    const wanted = bindingKey(normalized);
    for (const action of this.listActions()) {
      if (action.actionId === actionId) continue;
      for (const active of action.activeBindings) {
        if (bindingKey(active) === wanted) {
          return {
            kind: 'action',
            actionId: action.actionId,
            description: action.description,
          };
        }
      }
    }
    return null;
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
    void shortcutOverrides.revision;
    const actions = Array.from(this.#groupedActions().values());
    const editableActive = isEditableFocused();
    const xtermActive = isXtermFocused();

    for (const descriptors of actions) {
      const d = descriptors[0];
      if (d === undefined) continue;
      const activeBindings = this.#activeBindings(descriptors);
      if (!activeBindings.some((binding) => eventMatchesBinding(binding, event))) continue;
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
    this.revision += 1;
  }

  #groupedActions(): Map<string, ShortcutDescriptor[]> {
    const map = new Map<string, ShortcutDescriptor[]>();
    for (const d of this.#handlers) {
      const bucket = map.get(d.actionId);
      if (bucket) bucket.push(d);
      else map.set(d.actionId, [d]);
    }
    return map;
  }

  #toAction(descriptors: ShortcutDescriptor[]): ShortcutAction {
    const first = descriptors[0]!;
    const customizable = first.customizable !== false;
    return {
      actionId: first.actionId,
      description: first.description ?? first.actionId,
      category: first.category ?? 'Misc',
      customizable,
      protectedReason: first.protectedReason,
      defaultBindings: this.#defaultBindings(descriptors),
      activeBindings: this.#activeBindings(descriptors),
      overridden: shortcutOverrides.isOverridden(first.actionId),
    };
  }

  #activeBindings(descriptors: ShortcutDescriptor[]): ShortcutBinding[] {
    const first = descriptors[0];
    if (first !== undefined && first.customizable !== false) {
      const override = shortcutOverrides.get(first.actionId);
      if (override !== null) return override;
    }
    return this.#defaultBindings(descriptors);
  }

  #defaultBindings(descriptors: ShortcutDescriptor[]): ShortcutBinding[] {
    const isMac = isMacPlatform();
    const hasMetaCtrlPair = descriptors.some((d) => d.meta) && descriptors.some((d) => d.ctrl);
    const filtered = hasMetaCtrlPair
      ? descriptors.filter((d) => (isMac ? d.meta === true : d.ctrl === true))
      : descriptors;
    const unique = new Map<string, ShortcutBinding>();
    for (const d of filtered) {
      const binding = descriptorBinding(d);
      unique.set(bindingKey(binding), binding);
    }
    return Array.from(unique.values());
  }
}

export const shortcutRegistry = new ShortcutRegistry();

function reservedReason(binding: ShortcutBinding): string | null {
  const b = normalizeShortcutBinding(binding);
  if (b.key === 'Escape') return 'Esc is reserved for cancel and shell input routing.';
  if (b.key === 'Enter') return 'Enter is reserved for inline edit commit.';
  if (b.key === 'Backspace' || b.key === 'Delete') return 'Delete and Backspace are reserved destructive keys.';
  if (b.key === 'Tab') return 'Tab is reserved for browser focus navigation.';
  if (b.key === ' ') return 'Space is reserved for hold-to-pan.';
  const mod = b.meta || b.ctrl;
  if (mod && ['s', 'p', 'w', 'r', 'f'].includes(b.key.toLowerCase())) {
    return 'This browser or OS standard shortcut is reserved.';
  }
  return null;
}
