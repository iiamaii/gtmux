// chromeShortcuts — chrome-level keyboard shortcuts (G26 P1 partial).
//
// 정본:
// - frontend-handover-v2 §3.3 G26 P1 매트릭스
// - ADR-0017 §D6 (Stage C chrome shortcuts) — partial wire here.
//
// Wired in this module:
//   Cmd+Shift+L  → toggle LeftPanel  (Layers/Terminals)
//   Cmd+Shift+I  → toggle RightPanel (Inspect)
//
// Deferred (need their own anchor components to land first):
//   Cmd+N        → new terminal      (Toolbar2 terminal-tool wire)
//   Cmd+Shift+Q  → ShutdownModal     (SessionMenu)
//   Cmd+,        → Settings overlay  (Slice C)

import { chromeStore } from '$lib/stores/chrome.svelte';
import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { settingsDialog } from '$lib/stores/settingsDialog.svelte';
import { shutdownDialog } from '$lib/stores/shutdownDialog.svelte';
import { toolStore } from '$lib/stores/toolStore.svelte';
import { shortcutRegistry } from './shortcutRegistry.svelte';

export function bindChromeShortcuts(): () => void {
  const unsubs: Array<() => void> = [];

  // Cmd+N / Ctrl+N → arm the Terminal tool. Subsequent canvas click
  // spawns the terminal at the click position (matches the Toolbar2
  // [Terminal] tool behaviour). Doesn't spawn outright because the
  // user hasn't picked a location yet — picking is part of the gesture.
  unsubs.push(
    shortcutRegistry.register({
      actionId: 'canvas.new_terminal',
      key: 'n',
      meta: true,
      customizable: true,
      description: 'New terminal (click canvas to place)',
      category: 'Canvas',
      handler: () => {
        // Toolbar2 의 12 도구가 no-session 시 disabled — keyboard 도 같은
        // 정책으로 일관성 유지 (canvas 가 자체적으로 active===null 시 spawn
        // 무시하지만, 도구 highlight 만 켜진 confusing UX 차단).
        if (sessionStore.active === null) return true;
        toolStore.set('terminal');
        return true;
      },
    }),
  );
  unsubs.push(
    shortcutRegistry.register({
      actionId: 'canvas.new_terminal',
      key: 'n',
      ctrl: true,
      customizable: true,
      description: 'New terminal (Win/Linux)',
      category: 'Canvas',
      handler: () => {
        if (sessionStore.active === null) return true;
        toolStore.set('terminal');
        return true;
      },
    }),
  );

  // Cmd+Shift+Q → Session shutdown confirm modal (ADR-0017 §D6).
  unsubs.push(
    shortcutRegistry.register({
      actionId: 'chrome.shutdown_session',
      key: 'q',
      meta: true,
      shift: true,
      customizable: true,
      description: 'Session shutdown…',
      category: 'Chrome',
      handler: () => {
        // SessionMenu 의 Session shutdown 항목이 no-session 시 disabled —
        // keyboard 도 같은 정책.
        if (sessionStore.active === null) return true;
        shutdownDialog.show();
        return true;
      },
    }),
  );
  unsubs.push(
    shortcutRegistry.register({
      actionId: 'chrome.shutdown_session',
      key: 'q',
      ctrl: true,
      shift: true,
      customizable: true,
      description: 'Session shutdown (Win/Linux)',
      category: 'Chrome',
      handler: () => {
        if (sessionStore.active === null) return true;
        shutdownDialog.show();
        return true;
      },
    }),
  );

  // Cmd+, / Ctrl+, → Settings overlay (macOS / Win-Linux convention).
  unsubs.push(
    shortcutRegistry.register({
      actionId: 'chrome.open_settings',
      key: ',',
      meta: true,
      customizable: true,
      description: 'Open Settings',
      category: 'Chrome',
      handler: () => {
        settingsDialog.toggle();
        return true;
      },
    }),
  );
  unsubs.push(
    shortcutRegistry.register({
      actionId: 'chrome.open_settings',
      key: ',',
      ctrl: true,
      customizable: true,
      description: 'Open Settings (Win/Linux)',
      category: 'Chrome',
      handler: () => {
        settingsDialog.toggle();
        return true;
      },
    }),
  );

  unsubs.push(
    shortcutRegistry.register({
      actionId: 'chrome.toggle_left_panel',
      key: 'l',
      meta: true,
      shift: true,
      customizable: true,
      description: 'Toggle Layers/Terminals panel',
      category: 'Chrome',
      handler: () => {
        chromeStore.toggleSidebar();
        return true;
      },
    }),
  );
  unsubs.push(
    shortcutRegistry.register({
      actionId: 'chrome.toggle_left_panel',
      key: 'l',
      ctrl: true,
      shift: true,
      customizable: true,
      description: 'Toggle Layers/Terminals panel (Win/Linux)',
      category: 'Chrome',
      handler: () => {
        chromeStore.toggleSidebar();
        return true;
      },
    }),
  );
  unsubs.push(
    shortcutRegistry.register({
      actionId: 'chrome.toggle_right_panel',
      key: 'i',
      meta: true,
      shift: true,
      customizable: true,
      description: 'Toggle Inspect panel',
      category: 'Chrome',
      handler: () => {
        chromeStore.togglePaneInfo();
        return true;
      },
    }),
  );
  unsubs.push(
    shortcutRegistry.register({
      actionId: 'chrome.toggle_right_panel',
      key: 'i',
      ctrl: true,
      shift: true,
      customizable: true,
      description: 'Toggle Inspect panel (Win/Linux)',
      category: 'Chrome',
      handler: () => {
        chromeStore.togglePaneInfo();
        return true;
      },
    }),
  );

  return () => {
    for (const fn of unsubs) fn();
  };
}
