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
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { filePicker } from '$lib/stores/filePicker.svelte';
  import { pickLocalFile } from '$lib/files/localFilePicker';
  import { uploadAsset, AssetUploadUnavailableError } from '$lib/http/assets';
  import { UnauthorizedError } from '$lib/http/sessions';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import {
    alignItems,
    distributeItems,
    type AlignMode,
    type DistributeMode,
  } from '$lib/canvas/alignment';
  import ColorPicker from '$lib/ui/ColorPicker.svelte';
  import InspectorField from './InspectorField.svelte';
  import {
    MINIMIZED_TERMINAL_PANEL_HEIGHT,
    type CanvasItem,
    type LineItem,
    type NoteItem,
    type TextAlign,
    type TextItem,
    type TextVerticalAlign,
    type Visibility,
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
  const IMAGE_ACCEPT = 'image/png,image/jpeg,image/gif,image/webp,image/svg+xml';
  const DOCUMENT_ACCEPT = '.md,.txt,.json,.html,.css,.js,.ts,.tsx,.jsx,.pdf,text/*,application/json,application/pdf';

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
    if (selectedItems.length === 0) return;
    const ids = new Set(selectedIds);
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it) => (ids.has(it.id) ? transform(it) : it)),
      }),
      {
        abortMessage,
        failMessage: 'Inspector edit failed',
      },
    );
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
  // - terminal (PanelNode): h = 32 (header-strip), w 유지
  // - note    (NoteNode):   w = h = 32 (chip), 사각형 — node-side 와 정합
  // 그 외 type (rect/ellipse/line/text/free_draw/file_path/image) 은
  // minimize 시각 정의 없음 — inspector 의 minimize 버튼이 selectedTypes 에
  // terminal/note 가 하나도 없으면 hide. (selectionSupportsMinimize)
  function applyMinimizeGeom(it: CanvasItem, next: boolean): CanvasItem {
    if (it.minimized === next) return it;
    if (it.type !== 'terminal' && it.type !== 'note' && it.type !== 'document') {
      return { ...it, minimized: next } as CanvasItem;
    }
    // node-side 상수 정합:
    //   NoteNode.svelte:  MIN_CHIP=32, RESTORE_DEFAULT_W=240, RESTORE_DEFAULT_H=96
    //   PanelNode.svelte: MIN_HEADER_H=34, RESTORE_DEFAULT_H=220
    // 0077 follow-up — PANEL_STRIP_H 32 → 34. .panel 의 box-sizing: border-box
    // + 1px top/bottom border 합산 (header 32px + border 2px = outer 34px).
    // 옛 32px 는 header bottom 2px 가 panel boundary 밖 overflow → ring 시각
    // 가려짐. 상세 PanelNode.svelte 의 MIN_HEADER_H 주석 참조.
    const NOTE_CHIP = 32;
    const NOTE_RESTORE_W = 240;
    const NOTE_RESTORE_H = 96;
    const PANEL_STRIP_H = MINIMIZED_TERMINAL_PANEL_HEIGHT;
    const PANEL_RESTORE_H = 220;
    const DOC_STRIP_H = 30;
    const DOC_RESTORE_W = 360;
    const DOC_RESTORE_H = 220;
    if (next === true) {
      sessionStore.backupItemGeom(it.id, { x: it.x, y: it.y, w: it.w, h: it.h });
      if (it.type === 'note') {
        return { ...it, minimized: true, w: NOTE_CHIP, h: NOTE_CHIP } as CanvasItem;
      }
      if (it.type === 'document') {
        return { ...it, minimized: true, h: DOC_STRIP_H } as CanvasItem;
      }
      return { ...it, minimized: true, h: PANEL_STRIP_H } as CanvasItem;
    }
    const backup = sessionStore.getRestoredGeom(it.id);
    sessionStore.clearRestoredGeom(it.id);
    if (it.type === 'note') {
      // Note chip 은 정사각형이라 w 도 함께 복원해야 한다.
      const w = backup?.w ?? NOTE_RESTORE_W;
      const h = backup?.h ?? NOTE_RESTORE_H;
      return { ...it, minimized: false, w, h } as CanvasItem;
    }
    if (it.type === 'document') {
      const w = backup?.w ?? DOC_RESTORE_W;
      const h = backup?.h ?? DOC_RESTORE_H;
      return { ...it, minimized: false, w, h } as CanvasItem;
    }
    const h = backup?.h ?? PANEL_RESTORE_H;
    return { ...it, minimized: false, h } as CanvasItem;
  }

  // selectedItems 중 minimize 지원 (terminal / note) 가 하나라도 있는지.
  // figure 만 선택된 경우 inspector 의 minimize 버튼 숨김.
  const selectionSupportsMinimize = $derived.by(() =>
    selectedItems.some((it) => it.type === 'terminal' || it.type === 'note' || it.type === 'document'),
  );

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

  function onAssetChangeError(err: unknown, kind: 'image' | 'document'): void {
    if (err instanceof UnauthorizedError) {
      window.location.href = '/auth';
      return;
    }
    toastStore.show({
      message: err instanceof AssetUploadUnavailableError
        ? 'Asset upload API is not available yet.'
        : `${kind === 'image' ? 'Image' : 'Document'} change failed: ${err instanceof Error ? err.message : String(err)}`,
      tone: 'error',
      durationMs: 6_000,
    });
  }

  function changeFilePathFromInspector(): void {
    const item = sessionItem;
    if (item?.type !== 'file_path' || item.locked) return;
    filePicker.openFor('', (path) => {
      void sessionStore.applyMutation(
        (cur) => ({
          ...cur,
          items: cur.items.map((it: CanvasItem) =>
            it.id === item.id && it.type === 'file_path'
              ? ({ ...it, path, kind: 'file' } as CanvasItem)
              : it,
          ),
        }),
        {
          abortMessage: 'File path change aborted — session reconnect failed.',
          failMessage: 'File path change failed',
        },
      );
    });
  }

  async function changeImageFromInspector(): Promise<void> {
    const item = sessionItem;
    if (item?.type !== 'image' || item.locked) return;
    const file = await pickLocalFile({ accept: IMAGE_ACCEPT });
    if (file === null) return;
    try {
      const uploaded = await uploadAsset(file, 'image');
      await sessionStore.applyMutation(
        (cur) => ({
          ...cur,
          items: cur.items.map((it: CanvasItem) =>
            it.id === item.id && it.type === 'image'
              ? ({
                  ...it,
                  label: uploaded.file_name,
                  asset_id: uploaded.asset_id,
                  mime: uploaded.mime,
                  original_w: uploaded.original_w,
                  original_h: uploaded.original_h,
                } as CanvasItem)
              : it,
          ),
        }),
        {
          abortMessage: 'Image change aborted — session reconnect failed.',
          failMessage: 'Image change failed',
        },
      );
    } catch (err) {
      onAssetChangeError(err, 'image');
    }
  }

  async function changeDocumentFromInspector(): Promise<void> {
    const item = sessionItem;
    if (item?.type !== 'document' || item.locked) return;
    const file = await pickLocalFile({ accept: DOCUMENT_ACCEPT });
    if (file === null) return;
    try {
      const uploaded = await uploadAsset(file, 'document');
      await sessionStore.applyMutation(
        (cur) => ({
          ...cur,
          items: cur.items.map((it: CanvasItem) =>
            it.id === item.id && it.type === 'document'
              ? ({
                  ...it,
                  asset_id: uploaded.asset_id,
                  file_name: uploaded.file_name,
                  mime: uploaded.mime,
                  size_bytes: uploaded.size_bytes,
                  content: undefined,
                } as CanvasItem)
              : it,
          ),
        }),
        {
          abortMessage: 'Document change aborted — session reconnect failed.',
          failMessage: 'Document change failed',
        },
      );
    } catch (err) {
      onAssetChangeError(err, 'document');
    }
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
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it) => {
          const m = moves.get(it.id);
          if (m === undefined) return it;
          if (it.type === 'line' && m.x2 !== undefined && m.y2 !== undefined) {
            return { ...it, x: m.x, y: m.y, x2: m.x2, y2: m.y2 } as CanvasItem;
          }
          return { ...it, x: m.x, y: m.y } as CanvasItem;
        }),
      }),
      { abortMessage, failMessage: 'Align failed' },
    );
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
    if (selectedItems.length === 0) return;
    const ids = new Set(selectedIds);
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it) => {
          if (!ids.has(it.id)) return it;
          if (it.type !== 'rect' && it.type !== 'ellipse' && it.type !== 'line') return it;
          // line 에는 fill 이 없음 — 무시.
          if (field === 'fill' && it.type === 'line') return it;
          return { ...it, [field]: hex } as CanvasItem;
        }),
      }),
      {
        abortMessage: 'Color change aborted — session reconnect failed.',
        failMessage: 'Color change failed',
      },
    );
  }

  /* ── Text alignment — Figma-style segmented control ──────────────
   * Inspector 가 text item 의 alignment 를 직접 mutate. 옛
   * ToolbarSubbar/TextNode 에 분산되어 있던 로직을 본 곳으로 단일화. */

  async function applyTextAlign(
    target: TextItem,
    next: TextAlign,
  ): Promise<void> {
    if (next === (target.text_align ?? 'center')) return;
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === target.id && it.type === 'text'
            ? ({ ...it, text_align: next } as TextItem)
            : it,
        ),
      }),
      { failMessage: 'Text align failed' },
    );
  }

  async function applyTextVerticalAlign(
    target: TextItem,
    next: TextVerticalAlign,
  ): Promise<void> {
    if (next === (target.text_vertical_align ?? 'middle')) return;
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === target.id && it.type === 'text'
            ? ({ ...it, text_vertical_align: next } as TextItem)
            : it,
        ),
      }),
      { failMessage: 'Text vertical align failed' },
    );
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
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect width="9" height="6" x="6" y="14" rx="2"/><rect width="16" height="6" x="6" y="4" rx="2"/><path d="M2 2v20"/></svg>
              </button>
              <button type="button" class="align-btn" title="Align center horizontally" aria-label="Align center horizontally" onclick={() => onAlign('center-x')}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 2v20"/><path d="M8 10H4a2 2 0 0 1-2-2V6c0-1.1.9-2 2-2h4"/><path d="M16 10h4a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2h-4"/><path d="M8 20H7a2 2 0 0 1-2-2v-2c0-1.1.9-2 2-2h1"/><path d="M16 14h1a2 2 0 0 1 2 2v2a2 2 0 0 1-2 2h-1"/></svg>
              </button>
              <button type="button" class="align-btn" title="Align right" aria-label="Align right" onclick={() => onAlign('right')}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect width="16" height="6" x="2" y="4" rx="2"/><rect width="9" height="6" x="9" y="14" rx="2"/><path d="M22 22V2"/></svg>
              </button>
            </div>
            <div class="align-group" aria-label="Align vertical">
              <button type="button" class="align-btn" title="Align top" aria-label="Align top" onclick={() => onAlign('top')}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect width="6" height="16" x="4" y="6" rx="2"/><rect width="6" height="9" x="14" y="6" rx="2"/><path d="M22 2H2"/></svg>
              </button>
              <button type="button" class="align-btn" title="Align center vertically" aria-label="Align center vertically" onclick={() => onAlign('center-y')}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2 12h20"/><path d="M10 16v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-4"/><path d="M10 8V4a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v4"/><path d="M20 16v1a2 2 0 0 1-2 2h-2a2 2 0 0 1-2-2v-1"/><path d="M14 8V7c0-1.1.9-2 2-2h2a2 2 0 0 1 2 2v1"/></svg>
              </button>
              <button type="button" class="align-btn" title="Align bottom" aria-label="Align bottom" onclick={() => onAlign('bottom')}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect width="6" height="16" x="4" y="2" rx="2"/><rect width="6" height="9" x="14" y="9" rx="2"/><path d="M22 22H2"/></svg>
              </button>
            </div>
            {#if selectionCount >= 3}
              <div class="align-group" aria-label="Distribute">
                <button type="button" class="align-btn" title="Distribute horizontally" aria-label="Distribute horizontally" onclick={() => onDistribute('horizontal')}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect width="6" height="14" x="4" y="5" rx="2"/><rect width="6" height="10" x="14" y="7" rx="2"/><path d="M17 22v-5"/><path d="M17 7V2"/><path d="M7 22v-3"/><path d="M7 5V2"/></svg>
                </button>
                <button type="button" class="align-btn" title="Distribute vertically" aria-label="Distribute vertically" onclick={() => onDistribute('vertical')}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M22 17h-3"/><path d="M22 7h-5"/><path d="M5 17H2"/><path d="M7 7H2"/><rect x="5" y="14" width="14" height="6" rx="2"/><rect x="7" y="4" width="10" height="6" rx="2"/></svg>
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
            {@const onCanvas = selectedPanelId !== null && sessionStore.items.has(selectedPanelId)}
            {@const desynced = terminalPoolEntry.attach_count === 0 && onCanvas}
            {@const liveCount = terminalPoolEntry.live_attached_sessions.length}
            {@const inactiveRefs = terminalPoolEntry.attached_sessions.filter((s) => !terminalPoolEntry.live_attached_sessions.includes(s))}
            <div class="prop-row full">
              <div class="display-row">
                <span class="k">attach</span>
                <span class="display-val mono" class:warn={desynced}>
                  ×{liveCount}{inactiveRefs.length > 0 ? ` (+${inactiveRefs.length} inactive)` : ''}{desynced ? '  (desync)' : ''}
                </span>
              </div>
            </div>
            <!-- sess row — live attach + inactive file ref 분리 표시 (0077 follow-up). -->
            <div class="prop-row full">
              <div class="display-row wrap">
                <span class="k">sess</span>
                <span class="display-val">
                  {#if terminalPoolEntry.attached_sessions.length === 0}
                    <span class="muted-hint">— pool only (no panel)</span>
                  {:else}
                    {#each terminalPoolEntry.live_attached_sessions as s, i (s)}
                      {#if i > 0}, {/if}<span
                        class="session-chip"
                        class:current={sessionStore.active?.name === s}
                      >{s}</span>
                    {/each}
                    {#if inactiveRefs.length > 0}
                      {#if liveCount > 0}, {/if}{#each inactiveRefs as s, i (s)}
                        {#if i > 0}, {/if}<span
                          class="session-chip inactive"
                          title="Inactive — file reference only (session detached)"
                        >{s}</span>
                      {/each}
                    {/if}
                  {/if}
                </span>
              </div>
            </div>
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

      {#if selectionCount === 1 && sessionItem !== null && (sessionItem.type === 'rect' || sessionItem.type === 'ellipse' || sessionItem.type === 'line' || sessionItem.type === 'text' || sessionItem.type === 'note' || sessionItem.type === 'file_path' || sessionItem.type === 'image' || sessionItem.type === 'document')}
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
                <button
                  type="button"
                  class="inline-action"
                  title="Change path"
                  aria-label="Change path"
                  disabled={sessionItem.locked}
                  onclick={changeFilePathFromInspector}
                >
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
                    <path d="M9 17H7A5 5 0 0 1 7 7h2"/>
                    <path d="M15 7h2a5 5 0 1 1 0 10h-2"/>
                    <line x1="8" x2="16" y1="12" y2="12"/>
                  </svg>
                </button>
              </div>
            </div>
          {:else if sessionItem.type === 'image'}
            <div class="prop-row full">
              <div class="display-row">
                <span class="k">file</span>
                <span class="display-val mono" title={sessionItem.label ?? sessionItem.asset_id}>{strOr(sessionItem.label ?? sessionItem.asset_id, '—')}</span>
                <button
                  type="button"
                  class="inline-action"
                  title="Change image"
                  aria-label="Change image"
                  disabled={sessionItem.locked}
                  onclick={() => void changeImageFromInspector()}
                >
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
                    <path d="M9 17H7A5 5 0 0 1 7 7h2"/>
                    <path d="M15 7h2a5 5 0 1 1 0 10h-2"/>
                    <line x1="8" x2="16" y1="12" y2="12"/>
                  </svg>
                </button>
              </div>
            </div>
          {:else if sessionItem.type === 'document'}
            <div class="prop-row full">
              <div class="display-row">
                <span class="k">file</span>
                <span class="display-val mono" title={sessionItem.file_name}>{strOr(sessionItem.file_name, '—')}</span>
                <button
                  type="button"
                  class="inline-action"
                  title="Change document"
                  aria-label="Change document"
                  disabled={sessionItem.locked}
                  onclick={() => void changeDocumentFromInspector()}
                >
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
                    <path d="M9 17H7A5 5 0 0 1 7 7h2"/>
                    <path d="M15 7h2a5 5 0 1 1 0 10h-2"/>
                    <line x1="8" x2="16" y1="12" y2="12"/>
                  </svg>
                </button>
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
                <!-- restore — PanelNode header 의 restore 아이콘과 정합 (두 줄). -->
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <line x1="5" y1="12" x2="19" y2="12"/>
                  <line x1="5" y1="18" x2="19" y2="18"/>
                </svg>
              {:else}
                <!-- minimize (underscore) — bottom line 만. restore 의 두 줄 중
                     아래 라인과 y 일치 (전환 시 위 라인만 들고/내림). -->
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <line x1="5" y1="18" x2="19" y2="18"/>
                </svg>
              {/if}
              {#if minimizedState === null}<span class="dash" aria-hidden="true"></span>{/if}
            </button>
          {/if}

          <!-- Focus 는 ViewportCtrl 의 focus 버튼으로 이동. -->
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

  .inline-action {
    width: 22px;
    height: 22px;
    flex: 0 0 22px;
    display: inline-grid;
    place-items: center;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .inline-action:hover:not(:disabled) {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .inline-action:disabled {
    opacity: 0.45;
    cursor: not-allowed;
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

  /* Inactive — file reference 만 있고 attach lock 안 보유. dashed muted. */
  .session-chip.inactive {
    color: var(--color-fg-subtle);
    border-style: dashed;
    font-style: italic;
  }

  .warn {
    color: var(--color-warning);
    font-weight: var(--weight-medium);
  }

  .muted-hint {
    color: var(--color-fg-subtle);
    font-style: italic;
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
