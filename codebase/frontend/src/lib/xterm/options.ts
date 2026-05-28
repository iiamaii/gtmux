// xterm.js 옵션 디폴트 — R2 F6 SECURE_XTERM_OPTIONS.
// OSC 52 비활성, link handler non-http 금지, scrollback 500.
import type { ITerminalOptions } from '@xterm/xterm';

export const SECURE_XTERM_OPTIONS: ITerminalOptions = {
  scrollback: 500,
  fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
  fontSize: 13,
  cursorBlink: true,
  allowProposedApi: true,
  // xterm's FitAddon subtracts overviewRuler.width from the fitted column
  // area, and xterm also uses it as the vertical scrollbar width. Keep this
  // explicit so the rightmost glyphs do not render under the scrollbar.
  overviewRuler: { width: 18 },
  // 보안 옵션은 P0 구현 시 R2 F6 따라 채움.
};
