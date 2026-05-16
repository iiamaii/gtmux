<script lang="ts">
  /**
   * PaneInfoPanel — 268px floating right panel (plan 0005 Stage E,
   * ADR-0017 §D2).
   *
   * v0 read-only — shows the *first* M-selected Panel's properties:
   *   - pane_id (mono)
   *   - label (or — fallback)
   *   - position / size (x, y, w, h)
   *   - z-index
   *   - locked / visibility / minimized booleans
   *   - dead status (from muxStore)
   *
   * Editable controls (rename / lock toggle / visibility toggle) land
   * in a later phase. The empty state ("No selection") matches Figma's
   * Design tab when nothing is selected on the canvas.
   */

  import { onMount } from 'svelte';
  import { muxStore } from '$lib/stores/mux.svelte';
  import { ensureMutationOk, sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { mutateLayout, UnauthorizedError } from '$lib/http/sessions';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import {
    alignItems,
    distributeItems,
    type AlignMode,
    type DistributeMode,
  } from '$lib/canvas/alignment';
  import ColorPicker from '$lib/ui/ColorPicker.svelte';
  import InspectorField from './InspectorField.svelte';
  import type {
    CanvasItem,
    LineItem,
    NoteItem,
    TextAlign,
    TextItem,
    TextVerticalAlign,
    Visibility,
  } from '$lib/types/canvas';

  // ADR-0027 D2 — multi-select aware. selectedIds = M 의 array snapshot.
  // selectedPanelId = first id (display 의 single-item fallback path 용).
  const selectedIds = $derived.by((): string[] => Array.from(sessionStore.M));
  const selectedPanelId = $derived.by((): string | null =>
    selectedIds.length === 0 ? null : (selectedIds[0] as string),
  );
  const selectionCount = $derived(selectedIds.length);

  // 다중 선택의 type 동질성 — 모두 동일 type 이면 그 type, 아니면 null (mixed).
  const selectedItems = $derived.by((): CanvasItem[] => {
    const out: CanvasItem[] = [];
    for (const id of selectedIds) {
      const it = sessionStore.items.get(id);
      if (it !== undefined) out.push(it);
    }
    return out;
  });
  const commonType = $derived.by((): string | null => {
    const first = selectedItems[0];
    if (first === undefined) return null;
    const t = first.type;
    for (const it of selectedItems) {
      if (it.type !== t) return null;
    }
    return t;
  });
  const isMultiMixed = $derived(selectionCount > 1 && commonType === null);
  const isMultiHomogeneous = $derived(selectionCount > 1 && commonType !== null);

  /**
   * ADR-0027 D3 — Common numeric / string field 의 mixed-aware reader.
   * 모든 selected item 의 `key` 값이 동일 하면 그 값, 아니면 sentinel 'Mixed'.
   * Empty selection 은 null.
   */
  function commonField<K extends keyof CanvasItem>(key: K): CanvasItem[K] | 'Mixed' | null {
    const head = selectedItems[0];
    if (head === undefined) return null;
    const first = head[key];
    for (const it of selectedItems) {
      if (it[key] !== first) return 'Mixed';
    }
    return first;
  }

  /** Common field 를 display string 으로 — numeric / boolean / Mixed 처리. */
  function commonNumStr(key: keyof CanvasItem, fallback = '—'): string {
    const v = commonField(key);
    if (v === null) return fallback;
    if (v === 'Mixed') return 'Mixed';
    if (typeof v === 'number') return String(Math.round(v));
    return fallback;
  }
  function commonStrOr(key: keyof CanvasItem, fallback = '—'): string {
    const v = commonField(key);
    if (v === null) return fallback;
    if (v === 'Mixed') return 'Mixed';
    if (typeof v === 'string' && v.length > 0) return v;
    return fallback;
  }

  /**
   * Note 의 `title` 은 의미상 Common label 과 동일 — Inspector 의 label field 가
   * note 의 title 을 read/write 한다. mixed 판정도 type-별 source 를 정규화 후 비교.
   */
  function noteAwareLabel(it: CanvasItem): string {
    return it.type === 'note' ? it.title : (it.label ?? '');
  }
  function commonNoteAwareLabel(): string | 'Mixed' | null {
    if (selectedItems.length === 0) return null;
    const first = selectedItems[0];
    if (first === undefined) return null;
    const firstVal = noteAwareLabel(first);
    for (const it of selectedItems) {
      if (noteAwareLabel(it) !== firstVal) return 'Mixed';
    }
    return firstVal;
  }

  const selectedPanel = $derived.by((): Record<string, unknown> | null => {
    if (selectedPanelId === null) return null;
    const it = sessionStore.items.get(selectedPanelId);
    if (!it) return null;
    return {
      id: it.id,
      type: it.type,
      pane_id: it.type === 'terminal' ? it.id : null,
      x: it.x,
      y: it.y,
      w: it.w,
      h: it.h,
      z: it.z,
      visibility: it.visibility === 'visible',
      locked: it.locked,
      minimized: it.minimized,
      label: it.label ?? null,
    };
  });

  const paneNumeric = $derived.by(() => {
    if (!selectedPanel) return null;
    const pid = selectedPanel['pane_id'];
    if (typeof pid !== 'string' || pid[0] !== '%') return null;
    const n = Number.parseInt(pid.slice(1), 10);
    return Number.isNaN(n) ? null : n;
  });

  const isDead = $derived.by(() => {
    if (paneNumeric === null) return false;
    return muxStore.panes.get(paneNumeric)?.dead === true;
  });

  // Pool 폴링 구독 (mount 동안 유지).
  onMount(() => terminalPool.subscribe());

  /**
   * Selected panel 의 terminal item pool lookup — selected id 가 terminal type
   * 이면 그 UUID 의 pool entry 표시.
   */
  const terminalPoolEntry = $derived.by(() => {
    if (selectedPanelId === null) return null;
    const it = sessionStore.items.get(selectedPanelId);
    if (it?.type !== 'terminal') return null;
    return terminalPool.byId(selectedPanelId);
  });

  const sessionItem = $derived.by(() => {
    if (selectedPanelId === null) return null;
    return sessionStore.items.get(selectedPanelId) ?? null;
  });

  function numOr(value: unknown, fallback: string): string {
    if (typeof value === 'number') return String(Math.round(value));
    return fallback;
  }

  function boolStr(value: unknown): string {
    if (typeof value !== 'boolean') return '—';
    return value ? 'true' : 'false';
  }

  function strOr(value: unknown, fallback: string): string {
    if (typeof value === 'string' && value.length > 0) return value;
    return fallback;
  }

  const isSelectedTerminal = $derived(sessionItem?.type === 'terminal');

  /* ── Common field broadcast (ADR-0027 D1/D3/D6) ──
   * Geometry / label / state 는 한 mutateLayout PUT 으로 selected item 전체에
   * broadcast. locked item 은 geometry mutation 에서 제외 (alignment 와 동일
   * 정책 — D7). label / state 는 lock 과 무관 broadcast.
   */
  type CommonNumKey = 'x' | 'y' | 'w' | 'h' | 'z';
  type CommonBoolKey = 'visible' | 'locked' | 'minimized';

  async function broadcastMutation(
    abortMessage: string,
    transform: (it: CanvasItem) => CanvasItem,
  ): Promise<void> {
    const active = sessionStore.active;
    if (active === null) return;
    if (selectedItems.length === 0) return;
    if (!(await ensureMutationOk(abortMessage))) return;
    const ids = new Set(selectedIds);
    try {
      const { layout } = await mutateLayout(active.name, (cur) => ({
        ...cur,
        items: cur.items.map((it) => (ids.has(it.id) ? transform(it) : it)),
      }));
      sessionStore.loadLayout(layout);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Inspector edit failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  async function applyCommonNum(key: CommonNumKey, value: number): Promise<void> {
    await broadcastMutation('Edit aborted — session reconnect failed.', (it) => {
      // locked item 은 geometry 변경 skip (D7 정합). z 는 lock 과 무관 — UI 의
      // ordering 만 영향이라 lock 된 item 도 z 갱신 OK.
      if (it.locked && key !== 'z') return it;
      return { ...it, [key]: value } as CanvasItem;
    });
  }

  async function applyCommonLabel(next: string): Promise<void> {
    // Note 는 title 이 label 의미 — type-aware broadcast (ADR-0027 D1/D6 변형).
    await broadcastMutation('Edit aborted — session reconnect failed.', (it) => {
      if (it.type === 'note') {
        return { ...it, title: next } as CanvasItem;
      }
      return { ...it, label: next } as CanvasItem;
    });
  }

  async function applyCommonBool(key: CommonBoolKey, next: boolean): Promise<void> {
    await broadcastMutation('Edit aborted — session reconnect failed.', (it) => {
      if (key === 'visible') {
        const v: Visibility = next ? 'visible' : 'hidden';
        return { ...it, visibility: v } as CanvasItem;
      }
      if (key === 'minimized') {
        return applyMinimizeGeom(it, next);
      }
      return { ...it, [key]: next } as CanvasItem;
    });
  }

  // PanelNode / NoteNode 의 onMinimizeClick 와 동일 패턴 — schema geom 변경 +
  // sessionStore.restoredItemGeoms 백업. 둘 다 같은 backup map 을 공유하므로
  // node-side button 과 inspector button 어느 쪽에서 토글해도 일관.
  // - terminal / note: h = 32 (header-strip), w 유지
  // 그 외 type (rect/ellipse/line/text/free_draw/file_path/image/document) 은
  // minimize 시각 정의 없음 — inspector 의 minimize 버튼이 selectedTypes 에
  // terminal/note 가 하나도 없으면 hide. (selectionSupportsMinimize)
  function applyMinimizeGeom(it: CanvasItem, next: boolean): CanvasItem {
    if (it.minimized === next) return it;
    if (it.type !== 'terminal' && it.type !== 'note') {
      return { ...it, minimized: next } as CanvasItem;
    }
    const NOTE_RESTORE_H = 96;
    const PANEL_RESTORE_H = 220;
    const MIN_STRIP_H = 32;
    if (next === true) {
      sessionStore.backupItemGeom(it.id, { x: it.x, y: it.y, w: it.w, h: it.h });
      return { ...it, minimized: true, h: MIN_STRIP_H } as CanvasItem;
    }
    const backup = sessionStore.getRestoredGeom(it.id);
    sessionStore.clearRestoredGeom(it.id);
    const defaultH = it.type === 'note' ? NOTE_RESTORE_H : PANEL_RESTORE_H;
    const h = backup?.h ?? defaultH;
    return { ...it, minimized: false, h } as CanvasItem;
  }

  // selectedItems 중 minimize 지원 (terminal / note) 가 하나라도 있는지.
  // figure 만 선택된 경우 inspector 의 minimize 버튼 숨김.
  const selectionSupportsMinimize = $derived.by(() =>
    selectedItems.some((it) => it.type === 'terminal' || it.type === 'note'),
  );

  function focusFirstSelected(): void {
    const id = selectedIds[0];
    if (id === undefined) return;
    sessionStore.zoomToItem(id);
  }

  async function applyLineEndpoint(field: 'x2' | 'y2', value: number): Promise<void> {
    await broadcastMutation('Edit aborted — session reconnect failed.', (it) => {
      if (it.type !== 'line' || it.locked) return it;
      return { ...(it as LineItem), [field]: value } as LineItem;
    });
  }

  async function applyNoteColor(hex: string): Promise<void> {
    await broadcastMutation('Color change aborted — session reconnect failed.', (it) => {
      if (it.type !== 'note') return it;
      return { ...(it as NoteItem), color: hex } as NoteItem;
    });
  }

  /** Multi-select 의 boolean 동질성 — 모두 같으면 그 값, 아니면 null (mixed). */
  function commonBool(reader: (it: CanvasItem) => boolean): boolean | null {
    if (selectedItems.length === 0) return null;
    const first = reader(selectedItems[0] as CanvasItem);
    for (const it of selectedItems) {
      if (reader(it) !== first) return null;
    }
    return first;
  }

  const visibleState = $derived.by(() => commonBool((it) => it.visibility === 'visible'));
  const lockedState = $derived.by(() => commonBool((it) => it.locked));
  const minimizedState = $derived.by(() => commonBool((it) => it.minimized));

  /* ── Multi-select node alignment (ADR-0027 D4~D8, plan-0010 Task 5) ──
   * Selection BBox 기준의 6 align + 2 distribute. M.size ≥ 2 일 때 button
   * row 표시, distribute 는 ≥ 3 일 때만. 한 mutateLayout PUT 으로 broadcast
   * (D6 batch contract). locked item 은 새 position 갱신 skip (D7).
   */
  async function applyAlignMutation(
    moves: Map<string, { x: number; y: number; x2?: number; y2?: number }>,
    abortMessage: string,
  ): Promise<void> {
    if (moves.size === 0) return;
    const active = sessionStore.active;
    if (active === null) return;
    if (!(await ensureMutationOk(abortMessage))) return;
    try {
      const { layout } = await mutateLayout(active.name, (cur) => ({
        ...cur,
        items: cur.items.map((it) => {
          const m = moves.get(it.id);
          if (m === undefined) return it;
          if (it.type === 'line' && m.x2 !== undefined && m.y2 !== undefined) {
            return { ...it, x: m.x, y: m.y, x2: m.x2, y2: m.y2 } as CanvasItem;
          }
          return { ...it, x: m.x, y: m.y } as CanvasItem;
        }),
      }));
      sessionStore.loadLayout(layout);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Align failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  async function onAlign(mode: AlignMode): Promise<void> {
    const moves = alignItems(selectedItems, mode);
    await applyAlignMutation(moves, 'Align aborted — session reconnect failed.');
  }

  async function onDistribute(mode: DistributeMode): Promise<void> {
    const moves = distributeItems(selectedItems, mode);
    await applyAlignMutation(moves, 'Distribute aborted — session reconnect failed.');
  }

  /* ── Shape fill / stroke editor (ADR-0027 D3, plan-0010 Task 3) ──
   * rect / ellipse / line 의 fill / stroke 를 ColorPicker 로 편집. multi
   * 동일 type 일 때 broadcast (D6 batch). value 가 mixed 면 placeholder.
   */
  async function applyShapeColor(
    field: 'fill' | 'stroke',
    hex: string,
  ): Promise<void> {
    const active = sessionStore.active;
    if (active === null) return;
    if (selectedItems.length === 0) return;
    if (!(await ensureMutationOk('Color change aborted — session reconnect failed.'))) return;
    const ids = new Set(selectedIds);
    try {
      const { layout } = await mutateLayout(active.name, (cur) => ({
        ...cur,
        items: cur.items.map((it) => {
          if (!ids.has(it.id)) return it;
          if (it.type !== 'rect' && it.type !== 'ellipse' && it.type !== 'line') return it;
          // line 에는 fill 이 없음 — 무시.
          if (field === 'fill' && it.type === 'line') return it;
          return { ...it, [field]: hex } as CanvasItem;
        }),
      }));
      sessionStore.loadLayout(layout);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Color change failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  /* ── Text alignment — Figma-style segmented control ──────────────
   * Inspector 가 text item 의 alignment 를 직접 mutate. 옛
   * ToolbarSubbar/TextNode 에 분산되어 있던 로직을 본 곳으로 단일화. */

  async function applyTextAlign(
    target: TextItem,
    next: TextAlign,
  ): Promise<void> {
    if (next === (target.text_align ?? 'center')) return;
    const active = sessionStore.active;
    if (active === null) return;
    try {
      const { layout } = await mutateLayout(active.name, (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === target.id && it.type === 'text'
            ? ({ ...it, text_align: next } as TextItem)
            : it,
        ),
      }));
      sessionStore.loadLayout(layout);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Text align failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  async function applyTextVerticalAlign(
    target: TextItem,
    next: TextVerticalAlign,
  ): Promise<void> {
    if (next === (target.text_vertical_align ?? 'middle')) return;
    const active = sessionStore.active;
    if (active === null) return;
    try {
      const { layout } = await mutateLayout(active.name, (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === target.id && it.type === 'text'
            ? ({ ...it, text_vertical_align: next } as TextItem)
            : it,
        ),
      }));
      sessionStore.loadLayout(layout);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Text vertical align failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }
</script>

<div class="item-info-view" aria-label="Item info">
  <div class="pane-info-body">
    {#if selectedPanel === null}
      <div class="empty">
        <p>No selection</p>
        <p class="hint">Click a panel on the canvas to inspect.</p>
      </div>
    {:else}
      {#if selectionCount > 1}
        <!-- ADR-0027 D2 — multi-select header (count + type homogeneity). -->
        <div class="multi-header">
          <span class="multi-count mono">{selectionCount}</span>
          <span class="multi-label">
            {#if isMultiMixed}
              items selected · <span class="muted">multiple types</span>
            {:else}
              {commonType}s selected
            {/if}
          </span>
        </div>
      {/if}
      <section class="prop-section">
        <div class="prop-head"><h4>Identity</h4></div>
        <div class="prop-row full">
          <div class="display-row" class:mixed={isMultiMixed}>
            <span class="k">type</span>
            <span class="display-val mono">
              {#if selectionCount === 1}
                {strOr(selectedPanel['type'], 'panel')}
              {:else if isMultiMixed}
                Mixed
              {:else}
                {commonType}
              {/if}
            </span>
          </div>
        </div>
        {#if selectionCount === 1 && isSelectedTerminal}
          <div class="prop-row full">
            <div class="display-row">
              <span class="k">term</span>
              <span class="display-val mono">{strOr(selectedPanel['pane_id'], '—')}</span>
            </div>
          </div>
        {/if}
        <div class="prop-row full">
          <InspectorField
            k="label"
            value={(() => {
              const v = commonNoteAwareLabel();
              return typeof v === 'string' ? v : '';
            })()}
            mixed={commonNoteAwareLabel() === 'Mixed'}
            placeholder="—"
            ariaLabel="Label"
            oncommit={(next) => void applyCommonLabel(next)}
          />
        </div>
        {#if selectionCount === 1}
          <div class="prop-row full">
            <div class="display-row">
              <span class="k">id</span>
              <span class="display-val mono" title={selectedPanel.id as string}>{selectedPanel.id}</span>
            </div>
          </div>
        {/if}
      </section>

      <section class="prop-section">
        <div class="prop-head"><h4>Geometry</h4></div>
        <div class="prop-row">
          <InspectorField
            type="number"
            k="X"
            value={(() => { const v = commonField('x'); return typeof v === 'number' ? String(Math.round(v)) : '0'; })()}
            mixed={commonField('x') === 'Mixed'}
            ariaLabel="x"
            oncommit={(s) => void applyCommonNum('x', Number(s))}
          />
          <InspectorField
            type="number"
            k="Y"
            value={(() => { const v = commonField('y'); return typeof v === 'number' ? String(Math.round(v)) : '0'; })()}
            mixed={commonField('y') === 'Mixed'}
            ariaLabel="y"
            oncommit={(s) => void applyCommonNum('y', Number(s))}
          />
        </div>
        <div class="prop-row">
          <InspectorField
            type="number"
            k="W"
            value={(() => { const v = commonField('w'); return typeof v === 'number' ? String(Math.round(v)) : ''; })()}
            mixed={commonField('w') === 'Mixed'}
            ariaLabel="w"
            oncommit={(s) => void applyCommonNum('w', Number(s))}
          />
          <InspectorField
            type="number"
            k="H"
            value={(() => { const v = commonField('h'); return typeof v === 'number' ? String(Math.round(v)) : ''; })()}
            mixed={commonField('h') === 'Mixed'}
            ariaLabel="h"
            oncommit={(s) => void applyCommonNum('h', Number(s))}
          />
        </div>
        <div class="prop-row full">
          <InspectorField
            type="number"
            k="Z"
            value={(() => { const v = commonField('z'); return typeof v === 'number' ? String(Math.round(v)) : '0'; })()}
            mixed={commonField('z') === 'Mixed'}
            ariaLabel="z-index"
            oncommit={(s) => void applyCommonNum('z', Number(s))}
          />
        </div>
        {#if selectionCount >= 2}
          <!-- ADR-0027 D4/D9 — alignment row. Distribute 는 N≥3. -->
          <div class="align-row" role="group" aria-label="Alignment">
            <div class="align-group" aria-label="Align horizontal">
              <button type="button" class="align-btn" title="Align left" aria-label="Align left" onclick={() => onAlign('left')}>
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><line x1="2" y1="2" x2="2" y2="14"/><rect x="3.5" y="4" width="9" height="3"/><rect x="3.5" y="9" width="5" height="3"/></svg>
              </button>
              <button type="button" class="align-btn" title="Align center horizontally" aria-label="Align center horizontally" onclick={() => onAlign('center-x')}>
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><line x1="8" y1="2" x2="8" y2="14"/><rect x="3.5" y="4" width="9" height="3"/><rect x="5.5" y="9" width="5" height="3"/></svg>
              </button>
              <button type="button" class="align-btn" title="Align right" aria-label="Align right" onclick={() => onAlign('right')}>
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><line x1="14" y1="2" x2="14" y2="14"/><rect x="3.5" y="4" width="9" height="3"/><rect x="7.5" y="9" width="5" height="3"/></svg>
              </button>
            </div>
            <div class="align-group" aria-label="Align vertical">
              <button type="button" class="align-btn" title="Align top" aria-label="Align top" onclick={() => onAlign('top')}>
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><line x1="2" y1="2" x2="14" y2="2"/><rect x="4" y="3.5" width="3" height="9"/><rect x="9" y="3.5" width="3" height="5"/></svg>
              </button>
              <button type="button" class="align-btn" title="Align center vertically" aria-label="Align center vertically" onclick={() => onAlign('center-y')}>
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><line x1="2" y1="8" x2="14" y2="8"/><rect x="4" y="3.5" width="3" height="9"/><rect x="9" y="5.5" width="3" height="5"/></svg>
              </button>
              <button type="button" class="align-btn" title="Align bottom" aria-label="Align bottom" onclick={() => onAlign('bottom')}>
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><line x1="2" y1="14" x2="14" y2="14"/><rect x="4" y="3.5" width="3" height="9"/><rect x="9" y="7.5" width="3" height="5"/></svg>
              </button>
            </div>
            {#if selectionCount >= 3}
              <div class="align-group" aria-label="Distribute">
                <button type="button" class="align-btn" title="Distribute horizontally" aria-label="Distribute horizontally" onclick={() => onDistribute('horizontal')}>
                  <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><rect x="1" y="4" width="3" height="8"/><rect x="6.5" y="4" width="3" height="8"/><rect x="12" y="4" width="3" height="8"/></svg>
                </button>
                <button type="button" class="align-btn" title="Distribute vertically" aria-label="Distribute vertically" onclick={() => onDistribute('vertical')}>
                  <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><rect x="4" y="1" width="8" height="3"/><rect x="4" y="6.5" width="8" height="3"/><rect x="4" y="12" width="8" height="3"/></svg>
                </button>
              </div>
            {/if}
          </div>
        {/if}
      </section>

      {#if selectionCount === 1 && isSelectedTerminal && (terminalPoolEntry !== null || sessionItem !== null)}
        <section class="prop-section">
          <div class="prop-head"><h4>Terminal · Pool</h4></div>
          {#if terminalPoolEntry !== null}
            <div class="prop-row full">
              <div class="display-row">
                <span class="k">attach</span>
                <span class="display-val mono">×{terminalPoolEntry.attach_count}</span>
              </div>
            </div>
            {#if terminalPoolEntry.attached_sessions.length > 0}
              <div class="prop-row full">
                <div class="display-row wrap">
                  <span class="k">sess</span>
                  <span class="display-val">
                    {#each terminalPoolEntry.attached_sessions as s, i (s)}
                      {#if i > 0}, {/if}<span
                        class="session-chip"
                        class:current={sessionStore.active?.name === s}
                      >{s}</span>
                    {/each}
                  </span>
                </div>
              </div>
            {/if}
            <div class="prop-row full">
              <div class="display-row">
                <span class="k">alive</span>
                <span class="display-val mono" class:dead={!terminalPoolEntry.alive}>
                  {terminalPoolEntry.alive ? 'live' : 'dangling'}
                </span>
              </div>
            </div>
          {:else if sessionItem !== null && sessionItem.type === 'terminal'}
            <div class="prop-row full">
              <div class="display-row">
                <span class="k">pool</span>
                <span class="display-val"><span class="warn">missing</span></span>
              </div>
            </div>
          {/if}
        </section>
      {/if}

      {#if selectionCount === 1 && sessionItem !== null && (sessionItem.type === 'rect' || sessionItem.type === 'ellipse' || sessionItem.type === 'line' || sessionItem.type === 'text' || sessionItem.type === 'note' || sessionItem.type === 'file_path')}
        <section class="prop-section">
          <div class="prop-head"><h4>Item Payload</h4></div>
          {#if sessionItem.type === 'rect' || sessionItem.type === 'ellipse'}
            <div class="prop-row full">
              <div class="display-row picker">
                <span class="k">stroke</span>
                <ColorPicker
                  value={sessionItem.stroke}
                  allowAlpha={true}
                  oncommit={(hex) => void applyShapeColor('stroke', hex)}
                />
              </div>
            </div>
            <div class="prop-row full">
              <div class="display-row picker">
                <span class="k">fill</span>
                <ColorPicker
                  value={sessionItem.fill}
                  allowAlpha={true}
                  allowTransparent={true}
                  oncommit={(hex) => void applyShapeColor('fill', hex)}
                />
              </div>
            </div>
          {:else if sessionItem.type === 'line'}
            {@const line = sessionItem}
            <div class="prop-row">
              <InspectorField
                type="number"
                k="X2"
                value={String(Math.round(line.x2))}
                mixed={false}
                ariaLabel="x2"
                oncommit={(s) => void applyLineEndpoint('x2', Number(s))}
              />
              <InspectorField
                type="number"
                k="Y2"
                value={String(Math.round(line.y2))}
                mixed={false}
                ariaLabel="y2"
                oncommit={(s) => void applyLineEndpoint('y2', Number(s))}
              />
            </div>
            <div class="prop-row full">
              <div class="display-row picker">
                <span class="k">stroke</span>
                <ColorPicker
                  value={line.stroke}
                  allowAlpha={true}
                  oncommit={(hex) => void applyShapeColor('stroke', hex)}
                />
              </div>
            </div>
          {:else if sessionItem.type === 'text'}
            {@const txt = sessionItem}
            {@const h = txt.text_align ?? 'center'}
            {@const v = txt.text_vertical_align ?? 'middle'}
            <div class="prop-row full">
              <div class="display-row">
                <span class="k">chars</span>
                <span class="display-val mono">{txt.text.length}</span>
              </div>
            </div>
            <div class="prop-row full">
              <div class="display-row picker">
                <span class="k">align</span>
                <div class="align-group" role="group" aria-label="Horizontal alignment">
                  <button
                    type="button"
                    class="align-btn"
                    class:active={h === 'left'}
                    aria-pressed={h === 'left'}
                    title="Align left"
                    aria-label="Align left"
                    onclick={() => void applyTextAlign(txt, 'left')}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
                      <line x1="4" y1="6" x2="20" y2="6"/>
                      <line x1="4" y1="12" x2="14" y2="12"/>
                      <line x1="4" y1="18" x2="18" y2="18"/>
                    </svg>
                  </button>
                  <button
                    type="button"
                    class="align-btn"
                    class:active={h === 'center'}
                    aria-pressed={h === 'center'}
                    title="Align center"
                    aria-label="Align center"
                    onclick={() => void applyTextAlign(txt, 'center')}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
                      <line x1="4" y1="6" x2="20" y2="6"/>
                      <line x1="7" y1="12" x2="17" y2="12"/>
                      <line x1="5" y1="18" x2="19" y2="18"/>
                    </svg>
                  </button>
                  <button
                    type="button"
                    class="align-btn"
                    class:active={h === 'right'}
                    aria-pressed={h === 'right'}
                    title="Align right"
                    aria-label="Align right"
                    onclick={() => void applyTextAlign(txt, 'right')}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
                      <line x1="4" y1="6" x2="20" y2="6"/>
                      <line x1="10" y1="12" x2="20" y2="12"/>
                      <line x1="6" y1="18" x2="20" y2="18"/>
                    </svg>
                  </button>
                </div>
              </div>
            </div>
            <div class="prop-row full">
              <div class="display-row picker">
                <span class="k">v-align</span>
                <div class="align-group" role="group" aria-label="Vertical alignment">
                  <button
                    type="button"
                    class="align-btn"
                    class:active={v === 'top'}
                    aria-pressed={v === 'top'}
                    title="Align top"
                    aria-label="Align top"
                    onclick={() => void applyTextVerticalAlign(txt, 'top')}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
                      <line x1="5" y1="5" x2="19" y2="5"/>
                      <line x1="8" y1="10" x2="16" y2="10"/>
                      <line x1="10" y1="15" x2="14" y2="15"/>
                    </svg>
                  </button>
                  <button
                    type="button"
                    class="align-btn"
                    class:active={v === 'middle'}
                    aria-pressed={v === 'middle'}
                    title="Align middle"
                    aria-label="Align middle"
                    onclick={() => void applyTextVerticalAlign(txt, 'middle')}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
                      <line x1="6" y1="7" x2="18" y2="7"/>
                      <line x1="4" y1="12" x2="20" y2="12"/>
                      <line x1="6" y1="17" x2="18" y2="17"/>
                    </svg>
                  </button>
                  <button
                    type="button"
                    class="align-btn"
                    class:active={v === 'bottom'}
                    aria-pressed={v === 'bottom'}
                    title="Align bottom"
                    aria-label="Align bottom"
                    onclick={() => void applyTextVerticalAlign(txt, 'bottom')}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
                      <line x1="10" y1="9" x2="14" y2="9"/>
                      <line x1="8" y1="14" x2="16" y2="14"/>
                      <line x1="5" y1="19" x2="19" y2="19"/>
                    </svg>
                  </button>
                </div>
              </div>
            </div>
          {:else if sessionItem.type === 'note'}
            <div class="prop-row full">
              <div class="display-row picker">
                <span class="k">color</span>
                <ColorPicker
                  value={sessionItem.color}
                  oncommit={(hex) => void applyNoteColor(hex)}
                />
              </div>
            </div>
          {:else if sessionItem.type === 'file_path'}
            <div class="prop-row full">
              <div class="display-row">
                <span class="k">path</span>
                <span class="display-val mono" title={sessionItem.path}>{strOr(sessionItem.path, '—')}</span>
              </div>
            </div>
          {/if}
        </section>
      {/if}

      <section class="prop-section">
        <div class="prop-head"><h4>State</h4></div>
        <div class="state-row" role="group" aria-label="Item state">
          <button
            type="button"
            class="state-btn"
            class:active={visibleState === true}
            class:mixed={visibleState === null}
            aria-pressed={visibleState === true}
            aria-label={visibleState === true ? 'Hide' : 'Show'}
            title={visibleState === null ? 'Visibility · Mixed' : visibleState ? 'Visible (click to hide)' : 'Hidden (click to show)'}
            onclick={() => void applyCommonBool('visible', !(visibleState ?? false))}
          >
            {#if visibleState === false}
              <!-- eye-off -->
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94"/>
                <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19"/>
                <path d="M14.12 14.12a3 3 0 1 1-4.24-4.24"/>
                <line x1="1" y1="1" x2="23" y2="23"/>
              </svg>
            {:else}
              <!-- eye -->
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8S1 12 1 12z"/>
                <circle cx="12" cy="12" r="3"/>
              </svg>
            {/if}
            {#if visibleState === null}<span class="dash" aria-hidden="true"></span>{/if}
          </button>

          <button
            type="button"
            class="state-btn"
            class:active={lockedState === true}
            class:mixed={lockedState === null}
            aria-pressed={lockedState === true}
            aria-label={lockedState === true ? 'Unlock' : 'Lock'}
            title={lockedState === null ? 'Lock · Mixed' : lockedState ? 'Locked (click to unlock)' : 'Unlocked (click to lock)'}
            onclick={() => void applyCommonBool('locked', !(lockedState ?? false))}
          >
            {#if lockedState === true}
              <!-- lock closed -->
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <rect x="4" y="11" width="16" height="10" rx="2"/>
                <path d="M8 11V8a4 4 0 1 1 8 0v3"/>
              </svg>
            {:else}
              <!-- lock open -->
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <rect x="4" y="11" width="16" height="10" rx="2"/>
                <path d="M8 11V8a4 4 0 0 1 7.5-2"/>
              </svg>
            {/if}
            {#if lockedState === null}<span class="dash" aria-hidden="true"></span>{/if}
          </button>

          {#if selectionSupportsMinimize}
            <button
              type="button"
              class="state-btn"
              class:active={minimizedState === true}
              class:mixed={minimizedState === null}
              aria-pressed={minimizedState === true}
              aria-label={minimizedState === true ? 'Restore' : 'Minimize'}
              title={minimizedState === null ? 'Minimized · Mixed' : minimizedState ? 'Minimized (click to restore)' : 'Visible (click to minimize)'}
              onclick={() => void applyCommonBool('minimized', !(minimizedState ?? false))}
            >
              {#if minimizedState === true}
                <!-- restore -->
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <rect x="4" y="4" width="16" height="16" rx="1.5"/>
                </svg>
              {:else}
                <!-- minimize (underscore) -->
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <line x1="5" y1="18" x2="19" y2="18"/>
                </svg>
              {/if}
              {#if minimizedState === null}<span class="dash" aria-hidden="true"></span>{/if}
            </button>
          {/if}

          <button
            type="button"
            class="state-btn action-only"
            aria-label="Focus item"
            title={selectionCount > 1 ? 'Focus first selected — zoom viewport' : 'Focus — zoom viewport to item'}
            disabled={selectionCount === 0}
            onclick={focusFirstSelected}
          >
            <!-- target / focus reticle (LayerTreeView 의 focus icon 정합) -->
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <circle cx="12" cy="12" r="9" />
              <circle cx="12" cy="12" r="3" fill="currentColor" />
            </svg>
          </button>
        </div>
        {#if selectionCount === 1 && isSelectedTerminal}
          <div class="prop-row full">
            <div class="display-row">
              <span class="k">alive</span>
              <span class="display-val mono" class:dead={isDead}>
                {isDead ? 'dead' : 'live'}
              </span>
            </div>
          </div>
        {/if}
      </section>
    {/if}
  </div>
</div>

<style>
  /* Embedded view — host (RightPanel) owns outer chrome + tabs + fold. */
  .item-info-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .pane-info-body {
    flex: 1 1 auto;
    overflow-y: auto;
    padding: var(--space-6) 0;
  }

  .empty {
    padding: var(--space-12);
    color: var(--color-fg-muted);
  }

  .empty p {
    margin: 0 0 var(--space-4);
    font-size: var(--text-md);
  }

  .empty .hint {
    font-size: var(--text-base);
    color: var(--color-fg-subtle);
  }

  /* ADR-0027 D2 — multi-select header. */
  .multi-header {
    display: flex;
    align-items: baseline;
    gap: var(--space-6);
    padding: var(--space-8) var(--space-12);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-2);
    font-size: var(--text-md);
  }

  .multi-count {
    font-weight: 540;
    color: var(--color-fg);
  }

  .multi-label {
    color: var(--color-fg-muted);
  }

  .multi-label .muted {
    color: var(--color-fg-subtle);
  }

  /* ref/frontend-design/index-v2.html — `.prop-section / .prop-row / .input`
   * 정합. Inspector input 의 시각 (height/bg/font/hover) 은 InspectorField
   * 컴포넌트 안에서 캡슐화. 본 파일은 row layout (1fr 1fr / full) 만 관리. */

  .prop-section {
    padding: var(--space-8) var(--space-12) var(--space-12);
    border-bottom: 1px solid var(--color-border);
  }

  .prop-section:last-child {
    border-bottom: 0;
  }

  .prop-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-8);
  }

  .prop-head h4 {
    margin: 0;
    font-family: var(--font-mono);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    font-weight: var(--weight-regular);
    color: var(--color-fg-muted);
  }

  .prop-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
    margin-bottom: 6px;
    min-width: 0;
  }

  .prop-row.full {
    grid-template-columns: 1fr;
  }

  /* Read-only display row — InspectorField 의 시각 (.inspector-input) 과 정합. */
  .display-row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    min-width: 0;
    height: 28px;
    padding: 0 8px;
    box-sizing: border-box;
    background: var(--color-surface-2);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.2px;
    color: var(--color-fg);
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .display-row:hover {
    background: var(--color-glass-1);
  }

  .display-row.picker {
    /* ColorPicker / align-group 등 inline control 을 안에 두는 row — 컨트롤이
     * 자체 surface 를 가져 height 가 늘어날 수 있음. */
    height: auto;
    min-height: 28px;
    padding: 4px 8px;
    flex-wrap: wrap;
  }

  /* Fixed-width label — 모든 row 의 value 시작 x 정렬 (color box / button
   * group 의 가로 위치 일치). 가장 긴 label "v-align" (7자) 기준 + buffer. */
  .display-row .k {
    flex: 0 0 56px;
    width: 56px;
    color: var(--color-fg-muted);
    text-transform: uppercase;
    font-size: 10px;
    letter-spacing: 0.4px;
  }

  /* .picker row 의 control (ColorPicker / align-group) 이 row 너비 full
   * 채우도록 — RightPanel resize 와 정합. */
  .display-row.picker > .align-group,
  .display-row.picker > :global(.color-picker) {
    flex: 1 1 auto;
    min-width: 0;
  }

  .display-row.picker > :global(.color-picker .hex-input) {
    flex: 1 1 auto;
    min-width: 0;
    width: auto;
  }

  /* Read-only value — editable InspectorField (color-fg) 와 색 차별. */
  .display-row .display-val {
    flex: 1 1 auto;
    min-width: 0;
    color: var(--color-fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .display-row .display-val.mono {
    font-family: var(--font-mono);
  }

  .display-row.wrap {
    height: auto;
    min-height: 28px;
    padding: 4px 8px;
  }

  .display-row.wrap .display-val {
    white-space: normal;
  }

  .display-row .display-val.dead {
    color: var(--color-warning);
  }

  .session-chip {
    display: inline-block;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    padding: 1px 6px;
    margin-right: 2px;
    border-radius: var(--radius-pill);
    background: var(--color-surface-2);
    color: var(--color-fg-muted);
    border: 1px solid var(--color-border);
  }

  .session-chip.current {
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    color: var(--color-accent);
    border-color: color-mix(in srgb, var(--color-accent) 30%, transparent);
  }

  .warn {
    color: var(--color-warning);
    font-weight: var(--weight-medium);
  }

  /* ADR-0027 D9 — multi-select alignment row (Geometry section 안). row 가
   * full width, 안의 align-group 들이 균등 분포, 각 button 도 균등 flex. */
  .prop-section > .align-row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-6);
    margin-top: var(--space-4);
  }

  .prop-section > .align-row > .align-group {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
  }

  .prop-section > .align-row > .align-group > .align-btn {
    flex: 1 1 0;
    width: auto;
    min-width: 0;
  }

  .align-group {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    padding: 2px;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
  }

  /* picker row 의 align-group 은 row 너비 채움 + button 들 균등 분포. */
  .display-row.picker > .align-group {
    display: flex;
  }

  .display-row.picker > .align-group > .align-btn {
    flex: 1 1 0;
    width: auto;
    min-width: 0;
  }

  .align-btn {
    width: 26px;
    height: 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 0;
    border-radius: 3px;
    color: var(--color-fg-muted);
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .align-btn:hover {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .align-btn:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .align-btn.active {
    background: var(--color-surface);
    color: var(--color-fg);
    box-shadow: var(--shadow-sm);
  }

  /* Figma-style state icon row (visibility / lock / minimize) — panel full width. */
  .state-row {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    margin-bottom: var(--space-6);
    width: 100%;
  }

  .state-btn {
    position: relative;
    flex: 1 1 0;
    min-width: 0;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 0;
    border-radius: 3px;
    color: var(--color-fg-subtle);
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .state-btn:hover {
    background: var(--color-glass-1);
    color: var(--color-fg-muted);
  }

  .state-btn.active {
    color: var(--color-fg);
  }

  .state-btn:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 1px;
  }

  /* action-only: focus 같은 trigger 버튼 — toggle state 없음. disabled 시 dim. */
  .state-btn.action-only:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }
  .state-btn.action-only:disabled:hover {
    background: transparent;
    color: var(--color-fg-subtle);
  }

  /* Mixed: dash overlay across the icon (indeterminate marker — ADR-0027 D3). */
  .state-btn.mixed {
    color: var(--color-fg-subtle);
  }

  .state-btn .dash {
    position: absolute;
    inset: 50% 4px auto 4px;
    height: 2px;
    background: var(--color-fg-muted);
    border-radius: 1px;
    transform: translateY(-50%);
    pointer-events: none;
  }
</style>
