// Envelope dispatcher — R8 §F4 메인 스레드 단일 dispatcher.
// PANE_OUT → registered handler, web-domain → store 갱신.

type PaneOutHandler = (buf: Uint8Array, cb: () => void) => void;

const handlers = new Map<string, PaneOutHandler>();

export function registerPaneOut(paneId: string, handler: PaneOutHandler): void {
  handlers.set(paneId, handler);
}

export function unregisterPaneOut(paneId: string): void {
  handlers.delete(paneId);
}
