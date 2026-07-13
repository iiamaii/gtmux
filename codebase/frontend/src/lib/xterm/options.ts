// xterm.js 옵션 디폴트 — R2 F6 SECURE_XTERM_OPTIONS.
// OSC 52 write = 게이트(secure context + 동의 setting) 시에만, read 금지 (ADR-0049).
// link handler non-http 금지, scrollback 500.
import type { ITerminalOptions } from '@xterm/xterm';

export const SECURE_XTERM_OPTIONS: ITerminalOptions = {
  scrollback: 500,
  fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
  fontSize: 13,
  cursorBlink: true,
  allowProposedApi: true,
  // NOTE (ADR-0004 D8, 2026-07-12): do NOT set `overviewRuler` here. Setting its
  // width renders a `z-index:8` overlay canvas (`.xterm-decoration-overview-ruler`,
  // right:0) OVER the text grid — it covered the rightmost column. gtmux registers
  // no decorations, so the ruler is pure overlay. Without it, FitAddon reserves its
  // default 14px for the native `.xterm-viewport` scrollbar (pinned to 8px in
  // XtermHost's CSS) — enough clearance, and no on-top overlay over the glyphs.
  // 보안 옵션은 P0 구현 시 R2 F6 따라 채움.
};
