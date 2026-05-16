// shutdownDialog — open/close state for the session-shutdown confirm modal.
//
// Why a store? Two trigger surfaces converge here:
//   - SessionMenu's "Session shutdown" dropdown item (existing)
//   - Cmd+Shift+Q global shortcut (G26 P1, ADR-0017 §D6)
//
// SessionMenu owns the actual ShutdownModal mount + sessionName binding;
// this store is a thin open/close signal so the shortcut handler (which
// has no DOM access) can drive it.

class ShutdownDialogStore {
  open = $state(false);

  show(): void {
    this.open = true;
  }

  close(): void {
    this.open = false;
  }

  toggle(): void {
    this.open = !this.open;
  }
}

export const shutdownDialog = new ShutdownDialogStore();
