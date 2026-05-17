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
} from '$lib/types/canvas';
import { sessionStore } from '$lib/stores/sessionStore.svelte';

/** 신규 item 의 기본 좌표 + 크기 default. ADR-0018 D7 의 z 정책은 commit 시점에 결정. */
const DEFAULT_TERMINAL_SIZE = { w: 480, h: 320 } as const;
// Text default — font 16 × line-height 1.4 ≈ 22.4 line-box. h 56 이면 inner
// content 영역 48px 안에서 line-box 가 약 47% 차지 → vertical align 의 top /
// middle / bottom 차이가 ±12.5px 만큼 가시. 너무 작으면 (예 h<40) 정렬 차이가
// 거의 안 보여 fit-perception 가짐.
const DEFAULT_TEXT_SIZE = { w: 160, h: 56 } as const;
const DEFAULT_NOTE_SIZE = { w: 300, h: 96 } as const;
// 시안 §03 — fp-main(icon 24 + padding 11+10 = 45) + fp-foot(content 14 +
// padding 6+7 = 27) + border 2 = ~74. NodeResizer minHeight 와 정합 — 셋
// (factory default / NodeResizer minHeight / onResizeEnd clamp) 가 동일해야
// resize 시 layout shift 없음.
const DEFAULT_FILE_PATH_SIZE = { w: 320, h: 80 } as const;
const DEFAULT_SHAPE_SIZE = { w: 200, h: 140 } as const;
const DEFAULT_LINE_SIZE = { w: 240, h: 80 } as const;
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
  return crypto.randomUUID();
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
  if (type === 'rect') {
    return {
      ...common,
      type: 'rect',
      stroke: 'var(--color-fg)',
      fill: 'transparent',
      stroke_width: 2,
    };
  }
  return {
    ...common,
    type: 'ellipse',
    stroke: 'var(--color-fg)',
    fill: 'transparent',
    stroke_width: 2,
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
  return committed;
}
