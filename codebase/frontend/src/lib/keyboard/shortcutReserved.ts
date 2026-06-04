import type { ShortcutBinding } from '$lib/stores/shortcutOverrides.svelte';

export function reservedReasonForBinding(binding: ShortcutBinding): string | null {
  const b = normalizeBinding(binding);
  if (b.key === 'Escape') return 'Esc is reserved for cancel and shell input routing.';
  if (b.key === 'Enter') return 'Enter is reserved for inline edit commit.';
  if (b.key === 'Backspace' || b.key === 'Delete') return 'Delete and Backspace are reserved destructive keys.';
  if (b.key === 'Tab') return 'Tab is reserved for browser focus navigation.';
  if (b.key === ' ') return 'Space is reserved for hold-to-pan.';
  const mod = b.meta || b.ctrl;
  if (mod && ['s', 'p', 'w', 'r', 'f', 'n'].includes(b.key.toLowerCase())) {
    return 'This browser or OS standard shortcut is reserved.';
  }
  return null;
}

function normalizeBinding(binding: ShortcutBinding): Required<ShortcutBinding> {
  return {
    key: binding.key.length === 1 ? binding.key.toLowerCase() : binding.key,
    meta: binding.meta === true,
    ctrl: binding.ctrl === true,
    alt: binding.alt === true,
    shift: binding.shift === true,
  };
}
