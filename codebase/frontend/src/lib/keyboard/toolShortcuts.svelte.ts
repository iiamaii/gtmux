// toolShortcuts — Toolbar tool / cursor mode keyboard shortcuts.
//
// 정본:
// - ADR-0017 D6 amend ⑫ — tool actions participate in custom shortcuts.
// - plan-0007 §14.20.5 — single-key shortcuts skip editable / xterm focus.

import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { toolStore, type ToolId } from '$lib/stores/toolStore.svelte';
import { shortcutRegistry, type ShortcutDescriptor } from './shortcutRegistry.svelte';

const TOOL_BINDINGS: Array<{
  actionId: string;
  key: string;
  shift?: boolean;
  tool: ToolId;
  description: string;
}> = [
  { actionId: 'tool.select', key: 'v', tool: 'select', description: 'Select tool' },
  { actionId: 'tool.hand', key: 'h', tool: 'hand', description: 'Hand tool' },
  { actionId: 'tool.text', key: 't', tool: 'text', description: 'Text tool' },
  { actionId: 'tool.rect', key: 'r', tool: 'rect', description: 'Rectangle tool' },
  { actionId: 'tool.ellipse', key: 'o', tool: 'ellipse', description: 'Ellipse tool' },
  { actionId: 'tool.line', key: 'l', tool: 'line', description: 'Line tool' },
  { actionId: 'tool.path', key: 'l', shift: true, tool: 'path', description: 'Path tool' },
  { actionId: 'tool.free_draw', key: 'p', tool: 'free_draw', description: 'Free draw tool' },
  { actionId: 'tool.note', key: 'n', tool: 'note', description: 'Note tool' },
  { actionId: 'tool.snippets', key: 's', tool: 'snippets', description: 'Snippets tool' },
  { actionId: 'tool.document', key: 'd', tool: 'document', description: 'Document tool' },
  { actionId: 'tool.image', key: 'i', tool: 'image', description: 'Image tool' },
  { actionId: 'tool.file_path', key: 'f', tool: 'file_path', description: 'File path tool' },
];

function setTool(tool: ToolId): boolean {
  if (tool !== 'select' && tool !== 'hand' && sessionStore.active === null) return true;
  toolStore.set(tool);
  return true;
}

export function bindToolShortcuts(): () => void {
  const unsubs: Array<() => void> = [];
  for (const t of TOOL_BINDINGS) {
    const descriptor: ShortcutDescriptor = {
      actionId: t.actionId,
      key: t.key,
      shift: t.shift,
      category: 'Tool',
      customizable: true,
      description: t.description,
      allowInEditable: false,
      allowInXterm: false,
      handler: () => setTool(t.tool),
    };
    unsubs.push(shortcutRegistry.register(descriptor));
  }
  return () => {
    for (const fn of unsubs) fn();
  };
}
