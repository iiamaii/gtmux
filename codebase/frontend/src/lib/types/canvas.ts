// Canvas Item types — schema v2 (ADR-0018).
//
// 정본:
// - ADR-0018 D1 (unified items[] discriminated union)
// - ADR-0018 D3 (common field matrix)
// - ADR-0018 D4 (type-specific payload)
// - ADR-0018 D5 (v1 → v2 hard cutover, schema_version: 2)
// - ADR-0024 D3 (Group은 z field 없음 — Group type은 lib/types/group.ts)
// - plan-0007 §14 + frontend-handover §6 Stage 1.1
//
// 어휘 (CONTEXT.md):
// - Canvas Item = items[] 의 element (10 type discriminated union)
// - Panel = `type: "terminal"` 인 Canvas Item (어휘 호환)
// - Terminal = backend execution unit (ADR-0021); schema 안 reference 만
//
// ⚠️ 2026-05-15 G20 amend: `maximized` 는 schema field 가 아닌 FE-only ephemeral
//    state (한 시점 1 panel, 다음 attach 시 자동 unmaximize). 본 module 의 type
//    에는 포함하지 않는다.

import type { Group } from './group';

/** Terminal panel 최소화 시 header 32px + selected border 1.5px × 2. */
export const MINIMIZED_TERMINAL_PANEL_HEIGHT = 35;

/* ────────────────────────────────────────────────────────────────────────── */
/* Common field (ADR-0018 D3)                                                 */
/* ────────────────────────────────────────────────────────────────────────── */

/** schema v2 의 visibility — string enum. ADR-0018 D1 예시 JSON 정합. */
export type Visibility = 'visible' | 'hidden';
export type TextAlign = 'left' | 'center' | 'right';
export type TextVerticalAlign = 'top' | 'middle' | 'bottom';

/**
 * ADR-0018 D4 amend ① (batch-5, 2026-05-20) — rect/ellipse/line 의 stroke
 * dash pattern. snake_case wire (BE schema.rs `FigureStrokeDash` 와 정합).
 * connector 의 `StrokeDash` 와는 의미·default 가 달라 별 enum.
 */
export type FigureStrokeDash = 'solid' | 'dash' | 'dot' | 'dash_dot';

/**
 * ADR-0018 D4 amend ② (batch-5, 2026-05-20) — text 의 font weight 3-bucket.
 * Grill #6: register 의 100~900 numeric 은 P1 로 미루고 3 variant 채택.
 */
export type FontWeight = 'light' | 'normal' | 'bold';

/** 모든 Canvas Item 공통 field. type-specific payload 는 각 variant 가 amend. */
export interface ItemCommon {
  /** UUID. `type: "terminal"` 인 경우 backend Terminal.id 와 동일 (ADR-0018 D2). */
  id: string;
  /** 부모 Group id 또는 Canvas 루트 직속이면 null. */
  parent_id: string | null;
  /** SvelteFlow coordinate (음수 허용). */
  x: number;
  y: number;
  w: number;
  h: number;
  /**
   * z-index. flat global z 공간 (group sibling 의 자식들과 직접 비교 가능,
   * ADR-0024 D3). 신규 item z = max(z) + 1 (ADR-0018 D7).
   * Tree drag reorder 는 z 영향 X — z mutation 은 ADR-0024 D2 의 4 액션
   * (Bring/Send to front/back, Bring/Send forward/backward) 으로만.
   */
  z: number;
  visibility: Visibility;
  locked: boolean;
  /** 사용자 자유 라벨 (optional). */
  label?: string;
  /** 사용자 자유 메모 (optional, multiline). */
  description?: string;
  /** header bar 만 표시. 영속 (G20 amend 후에도 minimize 는 schema field). */
  minimized: boolean;
}

/* ────────────────────────────────────────────────────────────────────────── */
/* Type-specific variants (ADR-0018 D4)                                       */
/* ────────────────────────────────────────────────────────────────────────── */

/** `type: "terminal"` — backend Terminal 의 visual representation. payload 없음. */
export interface TerminalItem extends ItemCommon {
  type: 'terminal';
}

export interface TextItem extends ItemCommon {
  type: 'text';
  text: string;
  font_size: number;
  /** Horizontal text alignment inside the text box. Defaults to center for new items. */
  text_align?: TextAlign;
  /** Vertical text alignment inside the text box. Defaults to middle for new items. */
  text_vertical_align?: TextVerticalAlign;
  /** CSS color string (e.g. `"#333"` or `"rgba(0,0,0,0.6)"`). */
  color: string;
  /** ADR-0018 D4 amend ② (batch-5) — font weight. default `"normal"`. */
  font_weight?: FontWeight;
  /** ADR-0018 D4 amend ② (batch-5) — italic toggle. default false. */
  italic?: boolean;
  /** ADR-0018 D4 amend ② (batch-5) — underline toggle. composes with strikethrough. */
  underline?: boolean;
  /** ADR-0018 D4 amend ② (batch-5) — strikethrough toggle. composes with underline. */
  strikethrough?: boolean;
}

export interface NoteItem extends ItemCommon {
  type: 'note';
  title: string;
  body: string;
  color: string;
}

export interface RectItem extends ItemCommon {
  type: 'rect';
  stroke: string;
  fill: string;
  /** ADR-0018 D4 amend ① — stroke width. BE-side validation 1≤≤32. */
  stroke_width: number;
  /**
   * ADR-0018 D4 amend ① (batch-5) — fill on/off (≠ alpha). `false` 면
   * SVG fill 도 `none`, hit-test 도 fill area 제외. default `true`.
   */
  fill_enabled?: boolean;
  /**
   * ADR-0018 D4 amend ① (batch-5) — stroke on/off. `false` 면 border 시각
   * + hit-target band 모두 제거. default `true`.
   */
  stroke_enabled?: boolean;
  /**
   * ADR-0018 D4 amend ① (batch-5, Grill #5) — rect 의 corner round 토글.
   * `true` 면 FE 가 자동 radius `clamp(min(w,h)*0.15, 4, 16)` 계산.
   * BE schema 는 boolean 만 저장. default `false`.
   */
  corner_rounded?: boolean;
  /** ADR-0018 D4 amend ① (batch-5) — stroke dash pattern. undefined=solid. */
  stroke_dash?: FigureStrokeDash;
}

export interface EllipseItem extends ItemCommon {
  type: 'ellipse';
  stroke: string;
  fill: string;
  /** ADR-0018 D4 amend ① — stroke width. BE-side validation 1≤≤32. */
  stroke_width: number;
  /** ADR-0018 D4 amend ① (batch-5) — see RectItem.fill_enabled. */
  fill_enabled?: boolean;
  /** ADR-0018 D4 amend ① (batch-5) — see RectItem.stroke_enabled. */
  stroke_enabled?: boolean;
  /** ADR-0018 D4 amend ① (batch-5) — stroke dash pattern. undefined=solid. */
  stroke_dash?: FigureStrokeDash;
}

export interface LineItem extends ItemCommon {
  type: 'line';
  stroke: string;
  /** ADR-0018 D4 amend ① — stroke width. BE-side validation 1≤≤32. */
  stroke_width: number;
  /** 끝 점 좌표 — (x, y) 가 시작, (x2, y2) 가 끝. canvas 절대 좌표. */
  x2: number;
  y2: number;
  /** ADR-0018 D4 amend ① (batch-5) — stroke dash pattern. undefined=solid. */
  stroke_dash?: FigureStrokeDash;
}

export interface Point {
  x: number;
  y: number;
}

export interface FreeDrawItem extends ItemCommon {
  type: 'free_draw';
  stroke: string;
  stroke_width: number;
  /** P2+ 의 point cap (ADR-0018 D8 — 기본 5000). */
  points: Point[];
}

export interface ImageItem extends ItemCommon {
  type: 'image';
  /** sha256 hash → `/api/assets/<sha256>`. */
  asset_id: string;
  mime: string;
  original_w?: number;
  original_h?: number;
}

export interface DocumentItem extends ItemCommon {
  type: 'document';
  /**
   * ADR-0018 D4 amend ② (2026-05-17, BE schema.rs ship) — 두 mode 상호
   * 배타. (a) asset-based: `asset_id` set + `mime`/`file_name`/`size_bytes`.
   * (b) inline-stored: `content` set (UTF-8 markdown, cap 64 KB) +
   * `file_name` (display 용). 두 mode 모두 file_name + mime + size_bytes
   * 는 BE struct 의 required field — inline mode 에서도 placeholder 값
   * (mime="", size_bytes=content.length) 필요.
   */
  asset_id?: string;
  mime: string;
  file_name: string;
  size_bytes: number;
  /** Inline-stored mode 의 UTF-8 markdown content (cap 64 KB). */
  content?: string;
}

export interface FilePathItem extends ItemCommon {
  type: 'file_path';
  /** UTF-8 path string. OS-level open 은 ADR-0023 의 confirm + allowlist 흐름. */
  path: string;
  kind?: 'directory' | 'file';
}

/** Discriminated union — `type` field 로 narrow. */
export type CanvasItem =
  | TerminalItem
  | TextItem
  | NoteItem
  | RectItem
  | EllipseItem
  | LineItem
  | FreeDrawItem
  | ImageItem
  | DocumentItem
  | FilePathItem;

/** `CanvasItem['type']` 의 모든 값 — UI registry / Toolbar 등록에 사용. */
export type CanvasItemType = CanvasItem['type'];

/* ────────────────────────────────────────────────────────────────────────── */
/* Layout envelope (ADR-0018 D1 + D5)                                         */
/* ────────────────────────────────────────────────────────────────────────── */

export interface Viewport {
  x: number;
  y: number;
  /** SvelteFlow scale — 일반적으로 0.1 ~ 4.0. */
  zoom: number;
}

/**
 * Session layout file envelope. ADR-0018 D1.
 *
 * `schema_version: 2` 만 valid — v1 (`groups[] + panels[]`) 는 boot 시 hard
 * cutover migrate 됨 (ADR-0018 D5). v2 reader 는 v1 file 을 절대 직접 받지 않음.
 */
export interface CanvasLayout {
  schema_version: 2;
  groups: Group[];
  items: CanvasItem[];
  viewport: Viewport;
}

/* ────────────────────────────────────────────────────────────────────────── */
/* Type guards (narrowing helper)                                             */
/* ────────────────────────────────────────────────────────────────────────── */

export const isTerminal = (it: CanvasItem): it is TerminalItem => it.type === 'terminal';
export const isText = (it: CanvasItem): it is TextItem => it.type === 'text';
export const isNote = (it: CanvasItem): it is NoteItem => it.type === 'note';
export const isRect = (it: CanvasItem): it is RectItem => it.type === 'rect';
export const isEllipse = (it: CanvasItem): it is EllipseItem => it.type === 'ellipse';
export const isLine = (it: CanvasItem): it is LineItem => it.type === 'line';
export const isFreeDraw = (it: CanvasItem): it is FreeDrawItem => it.type === 'free_draw';
export const isImage = (it: CanvasItem): it is ImageItem => it.type === 'image';
export const isDocument = (it: CanvasItem): it is DocumentItem => it.type === 'document';
export const isFilePath = (it: CanvasItem): it is FilePathItem => it.type === 'file_path';
