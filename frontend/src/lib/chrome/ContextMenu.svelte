<script lang="ts">
  /**
   * ContextMenu — right-click menu (plan 0005 Stage F, ADR-0017 §D2).
   *
   * Pattern (ref §10):
   *   - Canvas's `oncontextmenu` / `onnodecontextmenu` calls `openAt(x, y, paneId)`
   *   - Menu opens at the event coordinates, clamped to viewport bounds
   *   - Click outside or Esc → close
   *   - Item activation closes the menu and dispatches the action
   *
   * Item set (v0):
   *   - Copy pane_id (clipboard, falls back to selectAll if clipboard API
   *     unavailable)
   *   - Close pane (CTRL kill-pane — disabled when paneId missing)
   *   - (separator)
   *   - Hide / Lock — placeholders, future Stage G/E wire
   *
   * The menu is rendered absolutely-positioned within `+page.svelte`'s
   * workspace so coordinates are viewport-relative. It mounts at the top
   * of the workspace stack so the rail/sidebar/canvas don't intercept.
   */

  import { getContext } from 'svelte';
  import { sendCtrl } from '$lib/ws/ctrl-registry';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import type { WsClient } from '$lib/ws/client';

  interface WsClientHolder {
    current: WsClient | null;
  }
  const wsClientHolder = getContext<WsClientHolder>('wsClient');

  let open = $state(false);
  let pos = $state<{ x: number; y: number }>({ x: 0, y: 0 });
  let paneIdStr = $state<string | null>(null);
  let panelIdStr = $state<string | null>(null);
  let menuEl: HTMLDivElement | undefined = $state();

  /** External trigger — Canvas passes the raw MouseEvent + (optional)
   *  panel + pane identifiers. */
  export function openAt(args: {
    clientX: number;
    clientY: number;
    paneId?: string | null;
    panelId?: string | null;
  }): void {
    paneIdStr = args.paneId ?? null;
    panelIdStr = args.panelId ?? null;
    open = true;
    // Initial position; clamped after the menu lays out (next tick).
    pos = { x: args.clientX, y: args.clientY };
    queueMicrotask(clampPos);
  }

  function close(): void {
    open = false;
  }

  function clampPos(): void {
    if (!menuEl) return;
    const rect = menuEl.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let nx = pos.x;
    let ny = pos.y;
    if (nx + rect.width > vw) nx = Math.max(0, vw - rect.width - 4);
    if (ny + rect.height > vh) ny = Math.max(0, vh - rect.height - 4);
    pos = { x: nx, y: ny };
  }

  function onWindowMousedown(e: MouseEvent): void {
    if (!open || !menuEl) return;
    if (!menuEl.contains(e.target as Node)) close();
  }

  function onWindowKey(e: KeyboardEvent): void {
    if (open && e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  }

  $effect(() => {
    if (typeof window === 'undefined') return;
    window.addEventListener('mousedown', onWindowMousedown);
    window.addEventListener('keydown', onWindowKey);
    return () => {
      window.removeEventListener('mousedown', onWindowMousedown);
      window.removeEventListener('keydown', onWindowKey);
    };
  });

  async function onCopyPaneId(): Promise<void> {
    if (!paneIdStr) return;
    try {
      await navigator.clipboard.writeText(paneIdStr);
      toastStore.show({ message: `Copied ${paneIdStr} to clipboard`, tone: 'success' });
    } catch (e) {
      toastStore.show({ message: `Clipboard failed: ${(e as Error).message ?? e}`, tone: 'error' });
    }
    close();
  }

  async function onClosePane(): Promise<void> {
    const client = wsClientHolder?.current;
    if (!client) {
      toastStore.show({ message: 'WebSocket not ready', tone: 'error' });
      close();
      return;
    }
    if (!paneIdStr || !paneIdStr.startsWith('%')) {
      toastStore.show({ message: 'No pane selected to close', tone: 'warning' });
      close();
      return;
    }
    const numeric = paneIdStr.slice(1);
    try {
      const { response } = sendCtrl(client, 'kill-pane', [numeric], { timeoutMs: 5_000 });
      const r = await response;
      if (!r.ok) {
        toastStore.show({ message: `kill-pane: ${r.code ?? '?'} ${r.error ?? ''}`, tone: 'error' });
      }
    } catch (e) {
      toastStore.show({ message: `kill-pane request failed: ${(e as Error).message ?? e}`, tone: 'error' });
    }
    close();
  }

  function onHide(): void {
    toastStore.show({ message: 'Hide — not yet wired (Stage G/E)', tone: 'info' });
    close();
  }

  function onLock(): void {
    toastStore.show({ message: 'Lock — not yet wired (Stage G/E)', tone: 'info' });
    close();
  }
</script>

{#if open}
  <div
    bind:this={menuEl}
    class="ctx-menu"
    role="menu"
    style="left: {pos.x}px; top: {pos.y}px;"
  >
    <div class="ctx-section">Pane</div>
    <button
      type="button"
      class="ctx-item"
      onclick={onCopyPaneId}
      disabled={!paneIdStr}
    >
      <span class="label">Copy pane_id</span>
      <span class="kbd mono">{paneIdStr ?? '—'}</span>
    </button>
    <button
      type="button"
      class="ctx-item danger"
      onclick={onClosePane}
      disabled={!paneIdStr}
    >
      <span class="label">Close pane</span>
      <span class="kbd mono">⌫</span>
    </button>

    <div class="ctx-sep"></div>

    <div class="ctx-section">View (P1+)</div>
    <button type="button" class="ctx-item" onclick={onHide} disabled={!panelIdStr}>
      <span class="label">Hide / Show</span>
      <span class="kbd mono">⌘\</span>
    </button>
    <button type="button" class="ctx-item" onclick={onLock} disabled={!panelIdStr}>
      <span class="label">Lock / Unlock</span>
      <span class="kbd mono">⌘L</span>
    </button>
  </div>
{/if}

<style>
  .ctx-menu {
    position: fixed;
    min-width: 220px;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-lg);
    padding: var(--space-6) 0;
    z-index: var(--z-context-menu);
    color: var(--color-fg);
    font-size: var(--text-md);
    user-select: none;
    animation: ctx-in var(--motion-fast) var(--motion-easing);
  }

  .ctx-section {
    padding: var(--space-4) var(--space-14);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--color-fg-muted);
  }

  .ctx-sep {
    height: 1px;
    background: var(--color-border);
    margin: var(--space-4) 0;
  }

  .ctx-item {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    width: 100%;
    padding: var(--space-6) var(--space-14);
    background: transparent;
    border: 0;
    color: inherit;
    text-align: left;
    cursor: pointer;
    font-family: inherit;
    font-size: inherit;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .ctx-item:hover:not(:disabled) {
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }

  .ctx-item:hover:not(:disabled) .kbd {
    color: rgba(255, 255, 255, 0.85);
  }

  .ctx-item:disabled {
    color: var(--color-fg-subtle);
    cursor: not-allowed;
  }

  .ctx-item.danger:not(:disabled) {
    color: var(--color-danger);
  }

  .ctx-item.danger:hover:not(:disabled) {
    background: var(--color-danger);
    color: white;
  }

  .kbd {
    color: var(--color-fg-muted);
    font-size: var(--text-base);
    letter-spacing: 0.4px;
  }

  .kbd.mono {
    font-family: var(--font-mono);
  }

  @keyframes ctx-in {
    from {
      opacity: 0;
      transform: translateY(-2px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
</style>
