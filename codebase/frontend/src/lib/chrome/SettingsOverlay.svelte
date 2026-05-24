<script lang="ts">
  /**
   * SettingsOverlay — full-screen settings surface (FE-8 / G19).
   *
   * 정본:
   * - frontend-handover-v2 FE-8 + G19 (full-screen overlay + auto-save)
   * - ADR-0017 amend ④ (2026-05-16 Settings overlay shape)
   *
   * Layout: left-side nav (sections) + right-side section content.
   * Behaviour: Esc / [×] close. Auto-save — each control persists on
   * change, no [Save] button. BE-dependent sections render a clear
   * "BE endpoint pending" placeholder until those endpoints ship.
   *
   * This round ships Theme (G27 mode picker) + Shortcuts (read-only
   * registry table). Storage / Auth / Behavior / Debug are placeholders
   * pointing at the pending BE work.
   */

  import { themeStore, type ThemeMode } from '$lib/stores/theme.svelte';
  import { settingsDialog, type SettingsSection } from '$lib/stores/settingsDialog.svelte';
  import { settingsStore } from '$lib/stores/settings.svelte';
  import { shortcutRegistry, type ShortcutDescriptor } from '$lib/keyboard/shortcutRegistry.svelte';
  import { UnauthorizedError } from '$lib/http/sessions';
  import { toastStore } from '$lib/ui/toast-store.svelte';

  const open = $derived(settingsDialog.open);
  const section = $derived(settingsDialog.section);

  /* ── Section nav ─────────────────────────────────────────────────── */

  const SECTIONS: { id: SettingsSection; label: string; ready: boolean }[] = [
    { id: 'theme', label: 'Theme', ready: true },
    { id: 'shortcuts', label: 'Shortcuts', ready: true },
    { id: 'storage', label: 'Storage', ready: false },
    { id: 'auth', label: 'Auth', ready: false },
    { id: 'behavior', label: 'Behavior', ready: true },
    { id: 'debug', label: 'Debug', ready: false },
  ];

  /* ── Theme section ───────────────────────────────────────────────── */

  /**
   * ADR-0017 amend ④ D2 의 Auto-save 정책 정합 — change 즉시 persist + modal
   * 유지. chrome theme (token swap) 은 setMode 한 번에 즉시 반영. xterm 은
   * XtermHost 의 live theme effect 가 repaint 하며 terminal buffer 를 보존한다.
   */
  function setMode(mode: ThemeMode): void {
    themeStore.setMode(mode);
  }

  /* ── Behavior section ───────────────────────────────────────────── */

  /**
   * `auto_kill_terminal_on_panel_close` 토글 — ADR-0021 G25.1.b.
   * PATCH 실패 시 사용자에게 surface — 다음 close 가 default (modal 띄움) 로
   * fallback 되므로 silent 보다 toast 가 안전.
   */
  async function setAutoKill(next: boolean): Promise<void> {
    try {
      await settingsStore.setBehavior({ auto_kill_terminal_on_panel_close: next });
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Setting save failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  /** 0077 follow-up — session switch 완료 시 full page reload toggle. */
  async function setReloadOnSwitch(next: boolean): Promise<void> {
    try {
      await settingsStore.setBehavior({ reload_on_session_switch: next });
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Setting save failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  /* ── Shortcuts section ───────────────────────────────────────────── */

  const isMac = (() => {
    if (typeof navigator === 'undefined') return false;
    return /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent);
  })();

  /** Format a shortcut as a user-facing string like `⌘⇧L` or `Ctrl+Shift+L`. */
  function formatShortcut(d: ShortcutDescriptor): string {
    const parts: string[] = [];
    if (isMac) {
      if (d.ctrl) parts.push('⌃');
      if (d.alt) parts.push('⌥');
      if (d.shift) parts.push('⇧');
      if (d.meta) parts.push('⌘');
    } else {
      if (d.ctrl) parts.push('Ctrl');
      if (d.alt) parts.push('Alt');
      if (d.shift) parts.push('Shift');
      if (d.meta) parts.push('Win');
    }
    parts.push(d.key.length === 1 ? d.key.toUpperCase() : d.key);
    return isMac ? parts.join('') : parts.join('+');
  }

  /** Group descriptors by category. Show ⌘+Ctrl-pair entries (e.g. our
   *  mac/win duplicates for the same chrome action) only once per
   *  description by collapsing on description. */
  const grouped = $derived.by(() => {
    const list = shortcutRegistry.list();
    const map = new Map<string, ShortcutDescriptor[]>();
    const seen = new Set<string>();
    for (const d of list) {
      const desc = d.description ?? '';
      // Mac/Win pair collapse: on macOS we keep the meta variant; on
      // non-mac we keep the ctrl variant. Falls through if neither
      // modifier matches (e.g. plain keys like `[`).
      const isMeta = Boolean(d.meta);
      const isCtrl = Boolean(d.ctrl);
      if (desc && (isMeta || isCtrl)) {
        const key = `${desc}@${d.shift ? 'S' : ''}${d.alt ? 'A' : ''}${d.key}`;
        if (seen.has(key)) continue;
        // For pair-collision, prefer the platform-native modifier.
        const pair = list.find(
          (other) =>
            other !== d &&
            (other.description ?? '') === desc &&
            (other.key.toLowerCase() === d.key.toLowerCase()) &&
            Boolean(other.shift) === Boolean(d.shift) &&
            Boolean(other.alt) === Boolean(d.alt) &&
            Boolean(other.meta) !== isMeta,
        );
        if (pair) {
          const preferred = isMac ? (isMeta ? d : pair) : isMeta ? pair : d;
          if (preferred !== d) continue;
          seen.add(key);
        }
      }
      const cat = d.category ?? 'Misc';
      const bucket = map.get(cat);
      if (bucket) bucket.push(d);
      else map.set(cat, [d]);
    }
    return Array.from(map.entries()).sort((a, b) => a[0].localeCompare(b[0]));
  });

  /* ── Close ───────────────────────────────────────────────────────── */

  function close(): void {
    settingsDialog.close();
  }

  function onWindowKey(e: KeyboardEvent): void {
    if (!open) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  }

  $effect(() => {
    if (typeof window === 'undefined') return;
    if (!open) return;
    window.addEventListener('keydown', onWindowKey);
    return () => window.removeEventListener('keydown', onWindowKey);
  });
</script>

{#if open}
  <div
    class="settings-backdrop"
    role="presentation"
    onclick={close}
    onkeydown={() => {}}
  >
    <div
      class="settings-overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby="settings-title"
      tabindex="-1"
      onclick={(e: MouseEvent) => e.stopPropagation()}
      onkeydown={() => {}}
    >
      <header class="settings-head">
        <h2 id="settings-title">Settings</h2>
        <button type="button" class="close" aria-label="Close settings" onclick={close}>×</button>
      </header>

      <div class="settings-body">
        <nav class="section-nav" aria-label="Settings sections">
          {#each SECTIONS as s (s.id)}
            <button
              type="button"
              class="nav-btn"
              class:active={section === s.id}
              class:disabled={!s.ready}
              onclick={() => settingsDialog.setSection(s.id)}
            >
              <span class="nav-label">{s.label}</span>
              {#if !s.ready}
                <span class="nav-badge">soon</span>
              {/if}
            </button>
          {/each}
        </nav>

        <section class="section-pane" aria-live="polite">
          {#if section === 'theme'}
            <h3 class="section-head">Theme</h3>
            <p class="section-hint">
              Choose the appearance — <em>System</em> follows your OS preference.
              Changes apply immediately.
            </p>
            <div class="radio-group" role="radiogroup" aria-label="Theme mode">
              {#each ['system', 'light', 'dark'] as mode (mode)}
                {@const isActive = themeStore.mode === mode}
                <label class="radio-card" class:active={isActive}>
                  <input
                    type="radio"
                    name="theme-mode"
                    value={mode}
                    checked={isActive}
                    onchange={() => setMode(mode as ThemeMode)}
                  />
                  <span class="radio-title">
                    {mode === 'system' ? 'System' : mode === 'light' ? 'Light' : 'Dark'}
                  </span>
                  <span class="radio-sub">
                    {mode === 'system'
                      ? `Current: ${themeStore.resolved}`
                      : mode === 'light'
                        ? 'Bright workspace'
                        : 'Default for terminal-heavy work'}
                  </span>
                </label>
              {/each}
            </div>
          {:else if section === 'shortcuts'}
            <h3 class="section-head">Shortcuts</h3>
            <p class="section-hint">
              Read-only matrix from the registered handlers. Customization
              lands in a future revision.
            </p>
            {#if grouped.length === 0}
              <p class="placeholder">No shortcuts registered yet.</p>
            {:else}
              {#each grouped as [category, items] (category)}
                <div class="shortcut-group">
                  <h4 class="group-head">{category}</h4>
                  <table class="shortcut-table">
                    <tbody>
                      {#each items as d (d.description ?? d.key)}
                        <tr>
                          <td class="desc">{d.description ?? '(no description)'}</td>
                          <td class="combo mono">{formatShortcut(d)}</td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </div>
              {/each}
            {/if}
          {:else if section === 'storage'}
            <h3 class="section-head">Storage</h3>
            <p class="placeholder">
              Workspace path · file_path allowlist editor.
              <br />
              Session import/export is available from the session menu.
              Waiting on BE: <code>/api/file-path/*</code>.
            </p>
          {:else if section === 'auth'}
            <h3 class="section-head">Auth</h3>
            <p class="placeholder">
              Token rotate · password change.
              <br />
              Waiting on BE: <code>/auth/rotate</code>, <code>/auth/set-password</code>.
            </p>
          {:else if section === 'behavior'}
            <h3 class="section-head">Behavior</h3>
            <p class="section-hint">
              Per-action defaults. Settings persist for the lifetime of the
              server process.
            </p>
            <label class="toggle-row">
              <input
                type="checkbox"
                checked={settingsStore.behavior.auto_kill_terminal_on_panel_close}
                onchange={(e) => void setAutoKill((e.currentTarget as HTMLInputElement).checked)}
              />
              <span class="toggle-text">
                <span class="toggle-title">Auto-kill terminal on panel close</span>
                <span class="toggle-sub">
                  Skip the confirm dialog and SIGTERM the terminal whenever a panel
                  is closed. Mirror panels in other sessions will go dangling.
                </span>
              </span>
            </label>
            <label class="toggle-row">
              <input
                type="checkbox"
                checked={settingsStore.behavior.reload_on_session_switch}
                onchange={(e) => void setReloadOnSwitch((e.currentTarget as HTMLInputElement).checked)}
              />
              <span class="toggle-text">
                <span class="toggle-title">Reload page on session switch</span>
                <span class="toggle-sub">
                  When switching from one session to another, do a full page
                  reload after the new layout loads. Resets caches, WS state,
                  and re-runs the attach pipeline — useful for clearing any
                  divergence between FE and BE. First attach and cancel paths
                  are unaffected.
                </span>
              </span>
            </label>
          {:else if section === 'debug'}
            <h3 class="section-head">Debug</h3>
            <p class="placeholder">
              Server pid · build sha · log path.
              <br />
              Waiting on BE: <code>GET /api/settings</code>.
            </p>
          {/if}
        </section>
      </div>
    </div>
  </div>
{/if}

<style>
  .settings-backdrop {
    position: fixed;
    inset: 0;
    background: transparent;
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    z-index: var(--z-modal);
    display: grid;
    place-items: center;
  }

  .settings-overlay {
    width: min(880px, 92vw);
    height: min(640px, 86vh);
    background: var(--color-surface);
    color: var(--color-fg);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .settings-head {
    display: flex;
    align-items: center;
    gap: var(--space-8);
    padding: var(--space-12) var(--space-16) var(--space-8);
    border-bottom: 1px solid var(--color-border);
  }

  .settings-head h2 {
    flex: 1 1 auto;
    margin: 0;
    font-size: var(--text-xl);
    font-weight: var(--weight-medium);
  }

  .close {
    width: 28px;
    height: 28px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-fg-muted);
    font-size: 20px;
    line-height: 1;
    cursor: pointer;
  }

  .close:hover {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  .settings-body {
    display: grid;
    grid-template-columns: 200px 1fr;
    flex: 1 1 auto;
    min-height: 0;
  }

  .section-nav {
    border-right: 1px solid var(--color-border);
    padding: var(--space-8) var(--space-6);
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
  }

  .nav-btn {
    display: flex;
    align-items: center;
    gap: var(--space-6);
    padding: var(--space-6) var(--space-10);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .nav-btn:hover {
    background: var(--color-glass-1);
  }

  .nav-btn.active {
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    color: var(--color-accent);
  }

  .nav-btn.disabled {
    color: var(--color-fg-muted);
  }

  .nav-label {
    flex: 1 1 auto;
  }

  .nav-badge {
    padding: 0 6px;
    border-radius: var(--radius-pill);
    background: var(--color-surface-2);
    color: var(--color-fg-subtle);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    letter-spacing: 0.4px;
  }

  .section-pane {
    overflow-y: auto;
    padding: var(--space-16);
    min-height: 0;
  }

  .section-head {
    margin: 0 0 var(--space-8);
    font-size: var(--text-lg);
    font-weight: var(--weight-medium);
  }

  .section-hint {
    margin: 0 0 var(--space-16);
    color: var(--color-fg-muted);
    font-size: var(--text-md);
  }

  .placeholder {
    margin: var(--space-12) 0;
    padding: var(--space-12);
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
    color: var(--color-fg-muted);
    font-size: var(--text-md);
    line-height: var(--leading-normal);
  }

  .placeholder code {
    font-family: var(--font-mono);
    font-size: var(--text-base);
    padding: 1px 4px;
    border-radius: 3px;
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  /* ── Theme · radio cards ───────────────────────────────────────── */
  .radio-group {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--space-8);
  }

  .radio-card {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-10) var(--space-12);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition:
      border-color var(--motion-fast) var(--motion-easing),
      background var(--motion-fast) var(--motion-easing);
  }

  .radio-card:hover {
    background: var(--color-glass-1);
  }

  .radio-card.active {
    border-color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 8%, transparent);
  }

  .radio-card input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .radio-title {
    font-weight: var(--weight-medium);
    color: var(--color-fg);
  }

  .radio-sub {
    color: var(--color-fg-muted);
    font-size: var(--text-base);
  }

  /* ── Behavior toggle ─────────────────────────────────────────── */
  .toggle-row {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-10);
    align-items: flex-start;
    padding: var(--space-10) var(--space-12);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .toggle-row:hover {
    background: var(--color-glass-1);
  }

  .toggle-row input[type='checkbox'] {
    margin-top: 3px;
    accent-color: var(--color-accent);
  }

  .toggle-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .toggle-title {
    font-weight: var(--weight-medium);
    color: var(--color-fg);
  }

  .toggle-sub {
    color: var(--color-fg-muted);
    font-size: var(--text-base);
    line-height: var(--leading-normal);
  }

  /* ── Shortcuts ───────────────────────────────────────────────── */
  .shortcut-group {
    margin-bottom: var(--space-16);
  }

  .group-head {
    margin: 0 0 var(--space-6);
    font-size: var(--text-md);
    font-family: var(--font-mono);
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--color-fg-muted);
    font-weight: var(--weight-regular);
  }

  .shortcut-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-md);
  }

  .shortcut-table td {
    padding: var(--space-4) var(--space-8);
    border-bottom: 1px solid var(--color-border);
  }

  .shortcut-table tr:last-child td {
    border-bottom: 0;
  }

  .shortcut-table .desc {
    color: var(--color-fg);
  }

  .shortcut-table .combo {
    text-align: right;
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    white-space: nowrap;
  }

  .mono {
    font-family: var(--font-mono);
  }
</style>
