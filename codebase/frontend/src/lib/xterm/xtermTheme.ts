// xtermTheme — produce an xterm.js `ITheme` object that matches the
// chrome theme (G27, ADR-0017 amend ④ follow-up).
//
// 정본:
// - frontend-handover-v2 §3.4 G27 (xterm theme hot reload)
// - ADR-0016 (token palette)
//
// Strategy:
// - foreground / background / cursor / selection 은 design token (chrome
//   palette) 와 시각 정합. 본 함수는 *resolved* theme ('light' | 'dark')
//   에 따라 다른 색상을 반환.
// - ANSI 16색은 terminal 표준에 가까운 well-tested 팔레트 (Tango +
//   Solarized 변형). chrome 의 design token 과는 별도 — 사용자가 보는
//   tmux/vim/htop 의 색이 plausible 해야 함.
//
// Usage:
//   import { xtermTheme } from '$lib/xterm/xtermTheme';
//   term.options.theme = xtermTheme(themeStore.resolved);

import type { ITheme } from '@xterm/xterm';

import type { Theme } from '$lib/stores/theme.svelte';

/** Common ANSI palette — dark variant (Tango-ish, well-contrasted on dark bg). */
const ANSI_DARK = {
  black: '#000000',
  red: '#cc241d',
  green: '#98971a',
  yellow: '#d79921',
  blue: '#458588',
  magenta: '#b16286',
  cyan: '#689d6a',
  white: '#a89984',
  brightBlack: '#928374',
  brightRed: '#fb4934',
  brightGreen: '#b8bb26',
  brightYellow: '#fabd2f',
  brightBlue: '#83a598',
  brightMagenta: '#d3869b',
  brightCyan: '#8ec07c',
  brightWhite: '#ebdbb2',
} as const;

/** Common ANSI palette — light variant (Solarized-light inspired, readable on white bg). */
const ANSI_LIGHT = {
  black: '#073642',
  red: '#dc322f',
  green: '#859900',
  yellow: '#b58900',
  blue: '#268bd2',
  magenta: '#d33682',
  cyan: '#2aa198',
  white: '#eee8d5',
  brightBlack: '#586e75',
  brightRed: '#cb4b16',
  brightGreen: '#586e75',
  brightYellow: '#657b83',
  brightBlue: '#839496',
  brightMagenta: '#6c71c4',
  brightCyan: '#93a1a1',
  brightWhite: '#fdf6e3',
} as const;

const DARK: ITheme = {
  background: '#1a1a1a', // matches --canvas-bg in tokens.css :root.dark
  foreground: '#ebdbb2',
  cursor: '#ebdbb2',
  cursorAccent: '#1a1a1a',
  selectionBackground: 'rgba(235, 219, 178, 0.30)',
  selectionForeground: undefined,
  scrollbarSliderBackground: 'rgba(235, 219, 178, 0.16)',
  scrollbarSliderHoverBackground: 'rgba(235, 219, 178, 0.28)',
  scrollbarSliderActiveBackground: 'rgba(235, 219, 178, 0.36)',
  overviewRulerBorder: 'rgba(235, 219, 178, 0.10)',
  ...ANSI_DARK,
};

const LIGHT: ITheme = {
  background: '#fdf6e3', // soft cream, eyes-friendly on white chrome
  foreground: '#073642',
  cursor: '#073642',
  cursorAccent: '#fdf6e3',
  selectionBackground: 'rgba(7, 54, 66, 0.20)',
  selectionForeground: undefined,
  scrollbarSliderBackground: 'rgba(7, 54, 66, 0.14)',
  scrollbarSliderHoverBackground: 'rgba(7, 54, 66, 0.24)',
  scrollbarSliderActiveBackground: 'rgba(7, 54, 66, 0.32)',
  overviewRulerBorder: 'rgba(7, 54, 66, 0.10)',
  ...ANSI_LIGHT,
};

/** Return the xterm theme object matching the given resolved chrome theme. */
export function xtermTheme(resolved: Theme): ITheme {
  return resolved === 'dark' ? DARK : LIGHT;
}
