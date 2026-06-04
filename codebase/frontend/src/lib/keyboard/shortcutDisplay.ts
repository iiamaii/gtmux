import type { ShortcutAction, ShortcutBinding } from './shortcutRegistry.svelte';

const KEY_LABELS: Record<string, string> = {
  ' ': 'Space',
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
  Escape: 'Esc',
  Delete: 'Del',
  Backspace: 'Backspace',
  Enter: 'Enter',
  Tab: 'Tab',
};

export function isMacPlatform(): boolean {
  if (typeof navigator === 'undefined') return false;
  return /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent);
}

export function primaryModifierBinding(
  key: string,
  options: { shift?: boolean; alt?: boolean } = {},
): ShortcutBinding {
  const isMac = isMacPlatform();
  return {
    key,
    meta: isMac,
    ctrl: !isMac,
    alt: options.alt,
    shift: options.shift,
  };
}

export function formatShortcutBinding(binding: ShortcutBinding | undefined): string {
  if (binding === undefined) return 'Unassigned';
  const key = keyLabel(binding.key);
  const parts: string[] = [];
  if (isMacPlatform()) {
    if (binding.ctrl) parts.push('⌃');
    if (binding.alt) parts.push('⌥');
    if (binding.shift) parts.push('⇧');
    if (binding.meta) parts.push('⌘');
    parts.push(key);
    return parts.join('');
  }
  if (binding.ctrl) parts.push('Ctrl');
  if (binding.alt) parts.push('Alt');
  if (binding.shift) parts.push('Shift');
  if (binding.meta) parts.push('Win');
  parts.push(key);
  return parts.join('+');
}

export function shortcutForAction(
  actions: readonly ShortcutAction[],
  actionId: string,
): string {
  const action = actions.find((candidate) => candidate.actionId === actionId);
  const binding = action?.activeBindings[0];
  return binding === undefined ? '' : formatShortcutBinding(binding);
}

export function labelWithShortcut(label: string, shortcut: string): string {
  return shortcut.length > 0 ? `${label} · ${shortcut}` : label;
}

function keyLabel(key: string): string {
  const mapped = KEY_LABELS[key];
  if (mapped !== undefined) return mapped;
  return key.length === 1 ? key.toUpperCase() : key;
}
