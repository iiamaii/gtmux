// shortcutOverrides — FE-local custom shortcut bindings (ADR-0017 D6 amend ⑬).
//
// Persistence is browser-local on purpose. Shortcut preferences depend on
// keyboard layout, platform, and browser, so the first pass avoids server
// settings until a dedicated settings schema exists.

export interface ShortcutBinding {
  key: string;
  meta?: boolean;
  ctrl?: boolean;
  alt?: boolean;
  shift?: boolean;
}

interface ShortcutOverridePayload {
  version: 1;
  overrides: Record<string, ShortcutBinding[]>;
  disabled?: string[];
}

const STORAGE_KEY = 'gtmux-shortcut-overrides:v1';

function bool(v: unknown): boolean {
  return v === true;
}

export function normalizeShortcutBinding(binding: ShortcutBinding): ShortcutBinding {
  const key = binding.key.length === 1 ? binding.key.toLowerCase() : binding.key;
  return {
    key,
    meta: bool(binding.meta),
    ctrl: bool(binding.ctrl),
    alt: bool(binding.alt),
    shift: bool(binding.shift),
  };
}

export function bindingKey(binding: ShortcutBinding): string {
  const b = normalizeShortcutBinding(binding);
  return `${b.meta ? 'M' : '-'}${b.ctrl ? 'C' : '-'}${b.alt ? 'A' : '-'}${b.shift ? 'S' : '-'}:${b.key}`;
}

function isValidBinding(value: unknown): value is ShortcutBinding {
  if (typeof value !== 'object' || value === null) return false;
  const v = value as Partial<ShortcutBinding>;
  if (typeof v.key !== 'string' || v.key.length === 0) return false;
  for (const k of ['meta', 'ctrl', 'alt', 'shift'] as const) {
    if (v[k] !== undefined && typeof v[k] !== 'boolean') return false;
  }
  return true;
}

function loadOverrides(): Record<string, ShortcutBinding[]> {
  if (typeof localStorage === 'undefined') return {};
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) return {};
    const parsed = JSON.parse(raw) as Partial<ShortcutOverridePayload>;
    if (parsed.version !== 1 || typeof parsed.overrides !== 'object' || parsed.overrides === null) {
      return {};
    }
    const out: Record<string, ShortcutBinding[]> = {};
    for (const [actionId, bindings] of Object.entries(parsed.overrides)) {
      if (!Array.isArray(bindings)) continue;
      const valid = bindings.filter(isValidBinding).map(normalizeShortcutBinding);
      if (valid.length > 0) out[actionId] = valid;
    }
    return out;
  } catch {
    return {};
  }
}

class ShortcutOverridesStore {
  overrides = $state<Record<string, ShortcutBinding[]>>(loadOverrides());
  revision = $state(0);
  enabled = $state(true);

  get(actionId: string): ShortcutBinding[] | null {
    if (!this.enabled) return null;
    const bindings = this.overrides[actionId];
    return bindings && bindings.length > 0 ? bindings : null;
  }

  isOverridden(actionId: string): boolean {
    return this.get(actionId) !== null;
  }

  set(actionId: string, binding: ShortcutBinding): void {
    this.overrides = {
      ...this.overrides,
      [actionId]: [normalizeShortcutBinding(binding)],
    };
    this.#persist();
  }

  reset(actionId: string): void {
    if (!(actionId in this.overrides)) return;
    const next = { ...this.overrides };
    delete next[actionId];
    this.overrides = next;
    this.#persist();
  }

  resetAll(): void {
    this.overrides = {};
    this.#persist();
  }

  #persist(): void {
    this.revision += 1;
    if (typeof localStorage === 'undefined') return;
    const payload: ShortcutOverridePayload = {
      version: 1,
      overrides: this.overrides,
      disabled: [],
    };
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(payload));
    } catch {
      // Private browsing / quota failures must not break shortcut dispatch.
    }
  }
}

export const shortcutOverrides = new ShortcutOverridesStore();
