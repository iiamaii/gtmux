// MuxStore — backend-mirror state. Post-ADR-0013 (Stage B) the surface
// shrinks to *Pane lifecycle only* — Window / Session / layout-string
// mirroring are gone (no tmux backend). Each entry tracks just the
// `dead` flag so the Sidebar can render a zombie panel marker.
//
// 정본:
// - `docs/adr/0013-pty-direct-no-tmux.md` D10 (NOTIFY_MIRROR kinds:
//   `pane-spawned`, `pane-died`, `layout-changed`, `server-ready`).
// - `docs/adr/0014-process-supervisor.md` D2/D9 (Pane = 1 PTY + 1 child).
// - `docs/reports/0026-stage-b-carry-forward.md` §2.5 (envelope mapping).
//
// Pre-Stage-B this file held `windows: SvelteMap<...>`, `session`, and
// methods for `window-add` / `window-renamed` / `window-close` /
// `session-changed` / `layout-change` / `pane-mode-changed`. All gone
// — that vocabulary belonged to tmux control-mode and is permanently
// retired (see git log of this file pre-Stage-B for the historical
// record).

import { SvelteMap } from 'svelte/reactivity';

/** Per-pane mirror. Key in [`MuxStore.panes`] is the PaneId integer. */
export interface MirroredPane {
  /** `true` after a `pane-died` NOTIFY_MIRROR — Sidebar paints a zombie
   *  marker; entry is *not* deleted so historical references stay
   *  resolvable. */
  dead: boolean;
}

class MuxStore {
  // SvelteMap for entry-level reactivity (R8 §F3 pattern).
  panes = $state(new SvelteMap<number, MirroredPane>());

  /**
   * Idempotent. Called on `PANE_OUT` first-sight (so a pane that emits
   * before the matching `pane-spawned` NOTIFY arrives still lands in
   * the store) and on the explicit `pane-spawned` notification.
   */
  addPane(paneId: number): void {
    if (this.panes.has(paneId)) return;
    this.panes.set(paneId, { dead: false });
  }

  /**
   * Mark a pane as dead. Entry is preserved (not deleted) so any UI
   * surface that holds a stale paneId can still resolve it as a
   * zombie. The Sidebar renders dead panes in a muted style; the
   * canvas Panel close button stays usable to dispose of the visual
   * panel itself.
   */
  killPane(paneId: number): void {
    const cur = this.panes.get(paneId);
    this.panes.set(paneId, { dead: true, ...(cur ?? {}) });
  }
}

export const muxStore = new MuxStore();
