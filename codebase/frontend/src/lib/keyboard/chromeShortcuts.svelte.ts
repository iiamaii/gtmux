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
// Terminal tool shortcut lives in toolShortcuts. Cmd/Ctrl+N is browser
// new-window and must remain reserved.

import { chromeStore } from '$lib/stores/chrome.svelte';
import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { settingsDialog } from '$lib/stores/settingsDialog.svelte';
import { shutdownDialog } from '$lib/stores/shutdownDialog.svelte';
import { shortcutRegistry } from './shortcutRegistry.svelte';

export function bindChromeShortcuts(): () => void {
  const unsubs: Array<() => void> = [];

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
