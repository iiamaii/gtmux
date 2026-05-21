// CanvasItem factory — toolStore.current 별 fresh item 생성 + layout append helper.
//
// 정본:
// - ADR-0018 D4 (type-specific payload)
// - ADR-0018 D7 (신규 item z = max(z) + 1)
// - 0033 §8.2 (Stage 5 creation gestures + non-terminal Node renderers)
//
// Batch 1 범위: text / note / file_path (point-spawn). rect/ellipse/line 의
// drag-create 는 Batch 2 에서 추가 (좌표 인자만 확장).
//
// 사용 패턴:
//   const item = createCanvasItem('text', { x, y });
//   await commitNewItem(sessionName, item);

import type {
  CanvasItem,
  CanvasItemType,
  TextItem,
  NoteItem,
  FilePathItem,
  RectItem,
  EllipseItem,
  LineItem,
  TerminalItem,
  ImageItem,
  DocumentItem,
  FreeDrawItem,
  Point,
} from '$lib/types/canvas';
import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { generateUuidV4 } from '$lib/uuid';

/** 신규 item 의 기본 좌표 + 크기 default. ADR-0018 D7 의 z 정책은 commit 시점에 결정. */
export const DEFAULT_TERMINAL_SIZE = { w: 480, h: 320 } as const;
// Text default — font 16 × line-height 1.4 ≈ 22.4 line-box. h 56 이면 inner
// content 영역 48px 안에서 line-box 가 약 47% 차지 → vertical align 의 top /
// middle / bottom 차이가 ±12.5px 만큼 가시. 너무 작으면 (예 h<40) 정렬 차이가
// 거의 안 보여 fit-perception 가짐.
const DEFAULT_TEXT_SIZE = { w: 160, h: 56 } as const;
export const DEFAULT_NOTE_SIZE = { w: 300, h: 96 } as const;
// 시안 §03 — fp-main(icon 24 + padding 11+10 = 45) + fp-foot(content 14 +
// padding 6+7 = 27) + border 2 = ~74. NodeResizer minHeight 와 정합 — 셋
// (factory default / NodeResizer minHeight / onResizeEnd clamp) 가 동일해야
// resize 시 layout shift 없음.
export const DEFAULT_FILE_PATH_SIZE = { w: 320, h: 80 } as const;
const DEFAULT_SHAPE_SIZE = { w: 200, h: 140 } as const;
const DEFAULT_LINE_SIZE = { w: 240, h: 80 } as const;
/** ImageNode placeholder (BE asset endpoint 미land — ADR-0018 D4 의 P2+). */
export const DEFAULT_IMAGE_SIZE = { w: 320, h: 240 } as const;
/** DocumentNode — 시안 §02 inline-stored mode (ADR-0018 D4 amend ②). */
export const DEFAULT_DOCUMENT_SIZE = { w: 360, h: 280 } as const;
export const LINE_MIN_LENGTH = 5;
export const LINE_HIT_PADDING = 8;

export function lineBoxFromEndpoints(
  p1: { x: number; y: number },
  p2: { x: number; y: number },
): { x: number; y: number; w: number; h: number } {
  const minX = Math.min(p1.x, p2.x);
  const minY = Math.min(p1.y, p2.y);
  const dx = Math.abs(p2.x - p1.x);
  const dy = Math.abs(p2.y - p1.y);
  return {
    x: minX - LINE_HIT_PADDING,
    y: minY - LINE_HIT_PADDING,
    w: Math.max(dx, 1) + LINE_HIT_PADDING * 2,
    h: Math.max(dy, 1) + LINE_HIT_PADDING * 2,
  };
}

/** UUID v4 (browser-native crypto). 모든 신규 item.id 에 사용. */
function freshId(): string {
  return generateUuidV4();
}

/**
 * Multi-session terminal item — fresh UUID + click 위치. mutateLayout 으로 append
 * 후 `attachConfirm` 이 unmatched UUID 를 spawn. 0x88 TERMINAL_SPAWNED 가 도착하면
 * terminalPool.bindPaneId → XtermHost mount.
 *
 * BE 의 `POST /api/sessions/:name/terminals` (P2 endpoint) 가 ship 되면 이 emulation
 * 은 endpoint 직접 호출로 대체. wire 는 같은 결과 (MOUNT_CASCADE + spawn).
 */
export function createTerminalItem(pos: { x: number; y: number }): TerminalItem {
  return {
    id: freshId(),
    parent_id: null,
    x: pos.x,
    y: pos.y,
    z: 0,
    visibility: 'visible',
    locked: false,
    minimized: false,
    type: 'terminal',
    w: DEFAULT_TERMINAL_SIZE.w,
    h: DEFAULT_TERMINAL_SIZE.h,
  };
}

/**
 * 점 좌표로 spawn 되는 도구 (Batch 1) 의 fresh item 생성.
 *
 * 매트릭스:
 *   - text       → 짧은 placeholder, 256px × 64px
 *   - note       → 빈 title/body, 280px × 160px, accent 약한 색조
 *   - file_path  → 빈 path, 320px × 48px
 *
 * z 는 commit 시점에 `max(z) + 1` 로 결정 — 본 함수는 0 으로 두고 caller 가
 * (commitNewItem 안에서) 재계산.
 */
export function createCanvasItem(
  type: Extract<CanvasItemType, 'text' | 'note' | 'file_path'>,
  pos: { x: number; y: number },
): TextItem | NoteItem | FilePathItem {
  const common = {
    id: freshId(),
    parent_id: null,
    x: pos.x,
    y: pos.y,
    z: 0,
    visibility: 'visible' as const,
    locked: false,
    minimized: false,
  };
  switch (type) {
    case 'text':
      return {
        ...common,
        type: 'text',
        w: DEFAULT_TEXT_SIZE.w,
        h: DEFAULT_TEXT_SIZE.h,
        text: '',
        font_size: 16,
        text_align: 'center',
        text_vertical_align: 'middle',
        color: 'var(--color-fg)',
      };
    case 'note':
      return {
        ...common,
        type: 'note',
        w: DEFAULT_NOTE_SIZE.w,
        h: DEFAULT_NOTE_SIZE.h,
        title: '',
        body: '',
        color: 'var(--color-accent)',
      };
    case 'file_path':
      return {
        ...common,
        type: 'file_path',
        w: DEFAULT_FILE_PATH_SIZE.w,
        h: DEFAULT_FILE_PATH_SIZE.h,
        path: '',
      };
  }
}

/**
 * Image item — placeholder spawn (ADR-0018 D4 P2+, asset endpoint 미land).
 *
 * 현 단계: asset_id = '' (empty placeholder). 사용자가 canvas click 으로 빈
 * ImageNode 생성 → 추후 BE asset endpoint ship 시 inline file picker → upload
 * → asset_id wire.
 */
export function createImageItem(pos: { x: number; y: number }): ImageItem {
  return {
    id: freshId(),
    parent_id: null,
    x: pos.x,
    y: pos.y,
    w: DEFAULT_IMAGE_SIZE.w,
    h: DEFAULT_IMAGE_SIZE.h,
    z: 0,
    visibility: 'visible',
    locked: false,
    minimized: false,
    type: 'image',
    asset_id: '',
    mime: '',
  };
}

/**
 * Document item — inline-stored mode placeholder (ADR-0018 D4 amend ②).
 *
 * 현 단계: content = '' (빈 markdown) + file_name = 'document'. BE schema 의
 * inline-stored validation 정합 (asset_id 없음, content 있음, file_name set).
 * 사용자가 더블 클릭으로 content / file_name inline edit 진입 — Slice-A2 FE
 * wire 후속에서 InlineEdit + ColorPicker 등.
 */
export function createDocumentItem(pos: { x: number; y: number }): DocumentItem {
  const content = '';
  return {
    id: freshId(),
    parent_id: null,
    x: pos.x,
    y: pos.y,
    w: DEFAULT_DOCUMENT_SIZE.w,
    h: DEFAULT_DOCUMENT_SIZE.h,
    z: 0,
    visibility: 'visible',
    locked: false,
    minimized: false,
    label: 'document',
    type: 'document',
    // BE schema.rs `Item::Document` 는 mime/size_bytes 가 required. inline-
    // stored mode 에서 placeholder: mime="" (text/markdown 미지정), size_bytes=
    // content.byteLength. asset_id 는 omit (None → inline mode 분기).
    mime: '',
    file_name: 'document',
    size_bytes: new TextEncoder().encode(content).length,
    content,
  };
}

/**
 * Drag bounds 로 spawn 되는 사각형/원형 도구 (Batch 2).
 *
 * 사용 위치: Canvas 의 drag-to-create gesture. bounds.w/h <= 0 (= 사용자가 클릭만)
 * 면 default 크기 폴백.
 */
export function createShapeItem(
  type: 'rect' | 'ellipse',
  bounds: { x: number; y: number; w: number; h: number },
): RectItem | EllipseItem {
  const width = bounds.w > 0 ? Math.max(bounds.w, 20) : DEFAULT_SHAPE_SIZE.w;
  const height = bounds.h > 0 ? Math.max(bounds.h, 20) : DEFAULT_SHAPE_SIZE.h;
  const common = {
    id: freshId(),
    parent_id: null,
    x: bounds.x,
    y: bounds.y,
    w: width,
    h: height,
    z: 0,
    visibility: 'visible' as const,
    locked: false,
    minimized: false,
  };
  // ADR-0018 D4 amend ① factory default (batch-5 Grill #3):
  //   fill_enabled=false (outline-only 시각, sketching 도구에 자연) +
  //   stroke_enabled=true + fill 색은 회색 `#D9D9D9` 로 stored — 사용자가
  //   Inspector 에서 fill toggle ON 시 즉시 회색 채움 (no color picker 추가
  //   step). 옛 default `fill: "transparent"` 는 alpha 0 의 의미였으나, 본
  //   batch 후 *enabled boolean* 이 of-off 의 진짜 source — `transparent`
  //   string 은 옛 record 호환용 의미만 유지 (D4 표 주석).
  const SHAPE_FACTORY_DEFAULT_FILL = '#D9D9D9';
  if (type === 'rect') {
    return {
      ...common,
      type: 'rect',
      stroke: 'var(--color-fg)',
      fill: SHAPE_FACTORY_DEFAULT_FILL,
      stroke_width: 2,
      fill_enabled: false,
      stroke_enabled: true,
    };
  }
  return {
    ...common,
    type: 'ellipse',
    stroke: 'var(--color-fg)',
    fill: SHAPE_FACTORY_DEFAULT_FILL,
    stroke_width: 2,
    fill_enabled: false,
    stroke_enabled: true,
  };
}

/**
 * Free draw stroke — drag-to-stroke gesture 의 결과. `points` 는 flow-coord
 * sequence. bounding box 를 잡고 schema 의 (x, y, w, h) 로 보관.
 *
 * Caller (Canvas.svelte 의 onCanvasPointerUp) 책임:
 *  - `points.length >= 2` 일 때만 호출 (단일 click 은 stroke 의미 없음).
 *  - point cap (ADR-0018 D4, 5000) 은 수집 단계에서 이미 enforce.
 */
const FREE_DRAW_PADDING = 8;
export function createFreeDrawItem(points: Point[]): FreeDrawItem {
  const first = points[0];
  if (first === undefined) {
    throw new Error('createFreeDrawItem: empty points (caller must guard length >= 1)');
  }
  let minX = first.x, minY = first.y, maxX = first.x, maxY = first.y;
  for (const p of points) {
    if (p.x < minX) minX = p.x;
    if (p.y < minY) minY = p.y;
    if (p.x > maxX) maxX = p.x;
    if (p.y > maxY) maxY = p.y;
  }
  return {
    id: freshId(),
    parent_id: null,
    x: minX - FREE_DRAW_PADDING,
    y: minY - FREE_DRAW_PADDING,
    w: Math.max(maxX - minX, 1) + FREE_DRAW_PADDING * 2,
    h: Math.max(maxY - minY, 1) + FREE_DRAW_PADDING * 2,
    z: 0,
    visibility: 'visible',
    locked: false,
    minimized: false,
    type: 'free_draw',
    stroke: 'var(--color-fg)',
    stroke_width: 2,
    points,
  };
}

/**
 * Endpoints 로 spawn 되는 line 도구. Schema 정합: `(x,y)` = 시작, `(x2,y2)` = 끝 —
 * 둘 다 canvas 절대 좌표 (canvas.ts LineItem 의 주석). 4 방향 (TL→BR, BR→TL,
 * TR→BL, BL→TR) 모두 보존.
 *
 * NB: SvelteFlow Node.position 은 *bounding box top-left* 가 필요 — itemToNode 가
 * `min(x, x2), min(y, y2)` 로 계산. 본 factory 는 schema 값만 채움.
 *
 * w/h 는 |dx|, |dy| — 0 이면 default 폴백 (사용자가 점 클릭만 했을 때 hit-box).
 */
export function createLineItem(
  p1: { x: number; y: number },
  p2: { x: number; y: number },
): LineItem {
  const distance = Math.hypot(p2.x - p1.x, p2.y - p1.y);
  const end =
    distance >= LINE_MIN_LENGTH
      ? p2
      : { x: p1.x + DEFAULT_LINE_SIZE.w, y: p1.y + DEFAULT_LINE_SIZE.h };
  const box = lineBoxFromEndpoints(p1, end);
  return {
    id: freshId(),
    parent_id: null,
    x: p1.x,
    y: p1.y,
    w: box.w,
    h: box.h,
    z: 0,
    visibility: 'visible' as const,
    locked: false,
    minimized: false,
    type: 'line',
    stroke: 'var(--color-fg)',
    stroke_width: 2,
    x2: end.x,
    y2: end.y,
  };
}

/**
 * 신규 item 을 active session 의 layout 에 commit.
 *
 * - z 는 commit 시점에 `max(z) + 1` 로 재계산 (ADR-0018 D7).
 * - `mutateLayout` 의 412 rebase 가 race-safe — 두 user 가 동시에 click 해도
 *   각자 다른 z 로 안전히 append.
 * - 성공 후 `sessionStore.loadLayout` + `sessionStore.setM([id])` 으로 신규
 *   item 을 선택 상태로 — 사용자가 즉시 InlineEdit / Z 단축키 사용 가능.
 *
 * 반환: 실제 commit 된 item (z 재계산 반영). active session 없으면 null.
 */
export async function commitNewItem(item: CanvasItem): Promise<CanvasItem | null> {
  if (sessionStore.active === null) return null;
  let committed: CanvasItem = item;
  const result = await sessionStore.applyMutation(
    (cur) => {
      const maxZ = cur.items.reduce((m, it) => (it.z > m ? it.z : m), 0);
      committed = { ...item, z: maxZ + 1 };
      return { ...cur, items: [...cur.items, committed] };
    },
    {
      abortMessage: 'Item creation aborted — session reconnect failed.',
      failMessage: 'Item creation failed',
    },
  );
  if (!result.ok) return null;
  sessionStore.setM([committed.id]);
  // R7 (batch-5) — text item spawn 직후 auto-edit 진입 signal. TextNode 의
  // mount $effect 가 본 flag 읽고 editing=true 설정 + flag clear.
  if (committed.type === 'text') {
    sessionStore.justSpawnedTextId = committed.id;
  }
  return committed;
}
