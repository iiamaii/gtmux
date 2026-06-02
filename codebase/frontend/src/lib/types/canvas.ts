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
import type { components } from './api';

/**
 * BE `schema.rs` 에서 생성된 OpenAPI component 스키마 (ADR-0042 / plan-0017 Phase 3).
 * leaf enum 류는 생성 타입을 단일 진실로 alias 해 BE↔FE enum drift 를 컴파일타임에
 * 잡는다. 복합 variant interface 는 아직 수기 정의 — openapi-typescript 가 serde
 * `skip_serializing_if` 의 `Option<T>` 를 (wire 상 *부재* 인데) `T | null` 로 렌더해
 * 통째 alias 시 부정확한 `| null` 이 번지기 때문. 전체 마이그레이션은 plan-0017 의
 * deferred 항목 (golden fixture 적합성 테스트로 구조 drift 는 가드).
 */
type Schemas = components['schemas'];

/** Terminal panel 최소화 시 header 32px + selected border 1.5px × 2. */
export const MINIMIZED_TERMINAL_PANEL_HEIGHT = 35;

/* ────────────────────────────────────────────────────────────────────────── */
/* Common field (ADR-0018 D3)                                                 */
/* ────────────────────────────────────────────────────────────────────────── */

// ⬇ leaf enum 6종은 생성 스키마(Schemas)에서 alias — BE enum 변경이 즉시 FE 에
//    반영되고, 빠진 case 는 switch 망라성 에러로 drift 포착 (ADR-0042 / Phase 3).

/** schema v2 의 visibility — string enum. BE `schema.rs::Visibility` 정본. */
export type Visibility = Schemas['Visibility'];
export type TextAlign = Schemas['TextAlign'];
export type TextVerticalAlign = Schemas['TextVerticalAlign'];
/** ADR-0041 — system-stack font family. BE `schema.rs::FontFamily` 정본. */
export type FontFamily = Schemas['FontFamily'];

/**
 * ADR-0018 D4 amend ① — rect/ellipse/line 의 stroke dash pattern (snake_case
 * wire). connector 의 `StrokeDash` 와는 의미·default 가 달라 별 enum.
 */
export type FigureStrokeDash = Schemas['FigureStrokeDash'];
export type Anchor = Schemas['Anchor'];
export type Head = Schemas['Head'];

/**
 * ADR-0018 D4 amend ② — text 의 font weight 3-bucket (Grill #6: 100~900
 * numeric 은 P1, 3 variant 채택).
 */
export type FontWeight = Schemas['FontWeight'];

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
  /** ADR-0041 — system-stack font family. default `"sans"`. */
  font_family?: FontFamily;
  /** ADR-0040 D9 — one-shot text→label derive flag. */
  label_auto?: boolean;
  /** ADR-0040 — optional box style for text items. defaults off. */
  stroke?: string;
  fill?: string;
  stroke_width?: number;
  fill_enabled?: boolean;
  stroke_enabled?: boolean;
  corner_rounded?: boolean;
  stroke_dash?: FigureStrokeDash;
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
  /** ADR-0040 — embedded figure text. */
  text?: string;
  font_size?: number;
  color?: string;
  text_align?: TextAlign;
  text_vertical_align?: TextVerticalAlign;
  font_weight?: FontWeight;
  italic?: boolean;
  underline?: boolean;
  strikethrough?: boolean;
  font_family?: FontFamily;
  /** ADR-0040 D9 — one-shot text→label derive flag. */
  label_auto?: boolean;
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
  /** ADR-0040 — embedded figure text. */
  text?: string;
  font_size?: number;
  color?: string;
  text_align?: TextAlign;
  text_vertical_align?: TextVerticalAlign;
  font_weight?: FontWeight;
  italic?: boolean;
  underline?: boolean;
  strikethrough?: boolean;
  font_family?: FontFamily;
  /** ADR-0040 D9 — one-shot text→label derive flag. */
  label_auto?: boolean;
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
  /** ADR-0043 D2 — optional marker at the start endpoint. */
  head_from?: Head;
  /** ADR-0043 D2 — optional marker at the end endpoint. */
  head_to?: Head;
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
  /** Workspace(B)-relative path. New source model from ADR-0047. */
  path?: string;
  /** sha256 hash → `/api/assets/<sha256>`; legacy read-only fallback. */
  asset_id?: string;
  mime?: string;
  original_w?: number;
  original_h?: number;
}

export interface DocumentItem extends ItemCommon {
  type: 'document';
  /**
   * ADR-0018 D4 amend ② (2026-05-17, BE schema.rs ship) — 두 mode 상호
   * 배타. (a) workspace-file: `path` set. (b) legacy asset-based:
   * `asset_id` set + `mime`/`file_name`/`size_bytes`. (c) inline-stored:
   * `content` set (UTF-8 markdown, cap 64 KB) +
   * `file_name` (display 용). 두 mode 모두 file_name + mime + size_bytes
   * 는 BE struct 의 required field — inline mode 에서도 placeholder 값
   * (mime="", size_bytes=content.length) 필요.
   */
  path?: string;
  asset_id?: string;
  mime?: string;
  file_name?: string;
  size_bytes?: number;
  /** Inline-stored mode 의 UTF-8 markdown content (cap 64 KB). */
  content?: string;
}

export interface FilePathItem extends ItemCommon {
  type: 'file_path';
  /** UTF-8 path string. OS-level open 은 ADR-0023 의 confirm + allowlist 흐름. */
  path: string;
  kind?: 'directory' | 'file';
}

/** ADR-0038 — 1 snippet = 1 (key, body) pair. */
export interface SnippetEntry {
  /** UUID v4 lowercase 36-char. List 안에서 unique — list reconciliation +
   *  reorder + edit 의 stable id. BE schema 의 SnippetEntry::id 정합. */
  id: string;
  /** Badge display label. Save 시 trim 후 비어 있으면 reject (FE + BE 이중). */
  key: string;
  /** Clipboard 복사 대상. multiline 허용. 64 KB cap (`SNIPPET_BODY_MAX_BYTES`). */
  body: string;
}

/** ADR-0038 — Snippet collection canvas item. */
export interface SnippetsItem extends ItemCommon {
  type: 'snippets';
  /** 0..1000 entries. 순서 = 사용자가 본 badge 순서. drag-reorder 는 P1+. */
  entries: SnippetEntry[];
}

export type PathRouting = Schemas['Routing'];

export type PathEndpoint =
  | {
      kind: 'free';
      point: Point;
    }
  | {
      kind: 'connected';
      item_id: string;
      anchor: Anchor;
      /** Optional delta from the resolved anchor point. Missing means {0,0}. */
      offset?: Point | null;
      fallback_point: Point;
    };

export interface PathWaypoint {
  id: string;
  x: number;
  y: number;
}

export interface PathItem extends ItemCommon {
  type: 'path';
  from: PathEndpoint;
  to: PathEndpoint;
  routing: PathRouting;
  waypoints?: PathWaypoint[];
  head_from: Head;
  head_to: Head;
  stroke: string;
  stroke_width: number;
  stroke_dash?: FigureStrokeDash;
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
  | FilePathItem
  | SnippetsItem
  | PathItem;

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
  /** Effective Project Workspace(B) persisted on the session record. */
  workspace_root?: string | null;
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
export const isSnippets = (it: CanvasItem): it is SnippetsItem => it.type === 'snippets';
export const isPath = (it: CanvasItem): it is PathItem => it.type === 'path';
