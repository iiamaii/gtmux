// WS binary envelope 타입 정의. ADR-0002 + grill D14 0x80–0x8F web-domain 슬롯.
// 실제 디코더는 src/lib/ws/decode.ts.

export type Envelope =
  | { kind: 'pane_out'; paneId: number; bytes: Uint8Array }
  | { kind: 'layout_changed'; etag: Uint8Array }
  | { kind: 'm_changed'; panelIds: number[] }
  | { kind: 'i_changed'; paneId: number | null }
  | { kind: 'viewport_changed'; x: number; y: number; zoom: number }
  | { kind: 'focus_mode_changed'; enabled: boolean; targetPanelId: number | null };

export const OPCODE = {
  // tmux-domain (0x01–0x0F) — frontend는 PANE_OUT만 사용.
  PANE_OUT: 0x02,
  // web-domain (0x80–0x8F) — MT-3 broadcast.
  LAYOUT_CHANGED: 0x80,
  M_CHANGED: 0x81,
  I_CHANGED: 0x82,
  VIEWPORT_CHANGED: 0x83,
  FOCUS_MODE_CHANGED: 0x84,
} as const;
