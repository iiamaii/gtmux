// documentViewStateSaver — durable persist of one document's `view_state`.
//
// 정본: ADR-0056 D4 (저장 사이클 — debounce + no-op 회피 + history 제외).
//
// 저장 경로는 `mutateLayout`(GET→merge→PUT with If-Match, etag rebase 1회)를
// 직접 쓴다 — sessionStore.applyMutation 을 우회하므로 **history(ADR-0028)에
// 쌓이지 않는다**(view_state 저장은 semantic 변경이 아니라 undo 오염 금지).
// viewport debounce flush(#flushViewport)와 동일 노선.
//
// self-PUT 의 0x80 LAYOUT_CHANGED 에코는 putLayout 이 note 한 etag 를
// LayoutRefetchGate 가 비교해 무시한다(ADR-0053 D7) — 여기서 별도 처리 없음.

import { mutateLayout } from '$lib/http/sessions';
import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { layoutEtag } from '$lib/stores/layoutEtag';
import { webpageHeaders } from '$lib/session/webpageId';
import type { DocViewAnchor } from '$lib/canvas/documentAnchor';
import type { CanvasItem, CanvasLayout, DocumentViewState } from '$lib/types/canvas';

/** ADR-0056 D4 — scroll idle debounce. viewport 의 500ms 와 별도 상수. */
export const DOCUMENT_VIEW_STATE_DEBOUNCE_MS = 700;

interface PendingSave {
  timer: ReturnType<typeof setTimeout>;
  sessionName: string;
  viewState: DocumentViewState | undefined;
}

const pending = new Map<string, PendingSave>();
const lastSaved = new Map<string, DocumentViewState | undefined>();

/** Assemble a `view_state` from the live sources, or `undefined` when both are
 *  at default (mode rendered + no anchor) so the field is cleared. */
export function buildDocumentViewState(
  mode: 'rendered' | 'source',
  anchor: DocViewAnchor | null,
): DocumentViewState | undefined {
  const vs: DocumentViewState = {};
  if (mode !== 'rendered') vs.mode = mode;
  if (anchor !== null) vs.anchor = anchor;
  if (vs.mode === undefined && vs.anchor === undefined) return undefined;
  return vs;
}

/**
 * ADR-0056 D4 no-op 회피: 직전 저장과 mode 동일 + anchor index 동일 +
 * |frac 차| < 0.05 이면 동일로 간주(PUT 생략). applyMutation/mutateLayout 은
 * 항상 전체-layout PUT 이라 값 비교 없인 스크롤이 mutation 폭주를 만든다.
 */
export function viewStateEqualish(
  a: DocumentViewState | undefined,
  b: DocumentViewState | undefined,
): boolean {
  const am = a?.mode ?? 'rendered';
  const bm = b?.mode ?? 'rendered';
  if (am !== bm) return false;
  const aa = a?.anchor;
  const ba = b?.anchor;
  if (aa === undefined && ba === undefined) return true;
  if (aa === undefined || ba === undefined) return false;
  if (aa.kind !== ba.kind) return false;
  if (aa.index !== ba.index) return false;
  return Math.abs(aa.frac - ba.frac) < 0.05;
}

function mergeViewState(
  layout: CanvasLayout,
  itemId: string,
  viewState: DocumentViewState | undefined,
): CanvasLayout {
  return {
    ...layout,
    items: layout.items.map((it: CanvasItem) =>
      it.id === itemId && it.type === 'document'
        ? ({ ...it, view_state: viewState } as CanvasItem)
        : it,
    ),
  };
}

async function commit(
  itemId: string,
  sessionName: string,
  viewState: DocumentViewState | undefined,
): Promise<void> {
  // history-exempt by construction — mutateLayout never touches historyStore.
  await mutateLayout(sessionName, (cur) => mergeViewState(cur, itemId, viewState));
  lastSaved.set(itemId, viewState);
}

let pagehideRegistered = false;
function ensurePagehideListener(): void {
  if (pagehideRegistered || typeof window === 'undefined') return;
  pagehideRegistered = true;
  window.addEventListener('pagehide', flushDocumentViewStateOnPagehide);
}

/** ADR-0056 D4 (i) — scroll idle: debounced durable save (700ms). */
export function scheduleDocumentViewStateSave(
  itemId: string,
  viewState: DocumentViewState | undefined,
): void {
  const active = sessionStore.active;
  if (active === null) return;
  ensurePagehideListener();
  const existing = pending.get(itemId);
  if (existing !== undefined) clearTimeout(existing.timer);
  const sessionName = active.name;
  const timer = setTimeout(() => {
    pending.delete(itemId);
    if (viewStateEqualish(viewState, lastSaved.get(itemId))) return;
    void commit(itemId, sessionName, viewState).catch((err) => {
      console.debug('[gtmux] document view_state persist failed', err);
    });
  }, DOCUMENT_VIEW_STATE_DEBOUNCE_MS);
  pending.set(itemId, { timer, sessionName, viewState });
}

/** ADR-0056 D4 (ii) / D3 — immediate durable save: maximize 열기·닫기 전환,
 *  rendered/source 토글 커밋. debounce 대기 없이 즉시 PUT(history 제외). */
export async function flushDocumentViewStateSave(
  itemId: string,
  viewState: DocumentViewState | undefined,
): Promise<void> {
  const existing = pending.get(itemId);
  if (existing !== undefined) {
    clearTimeout(existing.timer);
    pending.delete(itemId);
  }
  const active = sessionStore.active;
  if (active === null) return;
  if (viewStateEqualish(viewState, lastSaved.get(itemId))) return;
  try {
    await commit(itemId, active.name, viewState);
  } catch (err) {
    console.debug('[gtmux] document view_state flush failed', err);
  }
}

/**
 * Drop all persist bookkeeping for one item (document delete). Cancels any
 * pending debounce timer and prunes both module-level maps so dead item ids do
 * not leak. No PUT — the item is gone. Caller: sessionStore.applyDeletion.
 */
export function discardDocumentViewStateSave(itemId: string): void {
  const existing = pending.get(itemId);
  if (existing !== undefined) {
    clearTimeout(existing.timer);
    pending.delete(itemId);
  }
  lastSaved.delete(itemId);
}

/**
 * Full reset (session teardown). Cancels every pending timer and clears both
 * maps so a session switch does not carry stale saver state. Caller:
 * sessionStore.clear.
 */
export function resetDocumentViewStateSaver(): void {
  for (const save of pending.values()) clearTimeout(save.timer);
  pending.clear();
  lastSaved.clear();
}

/**
 * ADR-0056 D4 (iii) — pagehide best-effort flush. unload 중에는 GET→PUT 을 할
 * 수 없어 마지막으로 확인한 etag 로 낙관적 CAS 하는 단일 keepalive PUT 을 쏜다.
 * 실패는 수용한다(다음 reload 의 idle save 가 정본). etag 가 어긋났으면 412 로
 * drop 될 뿐 — layout 을 덮어쓰지 않는다.
 */
export function flushDocumentViewStateOnPagehide(): void {
  const active = sessionStore.active;
  const etag = layoutEtag.hex;
  if (active === null || etag === null || pending.size === 0) return;
  let layout: CanvasLayout = sessionStore.layoutSnapshot();
  let dirty = false;
  for (const [itemId, save] of pending) {
    if (save.sessionName !== active.name) continue;
    if (viewStateEqualish(save.viewState, lastSaved.get(itemId))) continue;
    layout = mergeViewState(layout, itemId, save.viewState);
    dirty = true;
  }
  if (!dirty) return;
  try {
    void fetch(`/api/sessions/${encodeURIComponent(active.name)}/layout`, {
      method: 'PUT',
      headers: {
        'Content-Type': 'application/json',
        'If-Match': `"${etag}"`,
        ...webpageHeaders(),
      },
      credentials: 'include',
      body: JSON.stringify(layout),
      keepalive: true,
    });
  } catch {
    // best-effort — unload 경합/네트워크 실패는 수용 (D4 iii).
  }
}
