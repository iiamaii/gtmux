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
  import { changeTerminalDialog } from '$lib/stores/changeTerminalDialog.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { patchTerminalLabel } from '$lib/http/terminals';
  import { filePicker } from '$lib/stores/filePicker.svelte';
  import { pickLocalFile } from '$lib/files/localFilePicker';
  import { uploadAsset, AssetUploadUnavailableError } from '$lib/http/assets';
  import { UnauthorizedError } from '$lib/http/sessions';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { snippetEditPanel } from '$lib/stores/snippetEditPanel.svelte';
  import {
    ancestorChain,
    descendantGroups,
    descendantItems,
    directParentGroupId,
    effectiveLocked,
    effectiveVisibility,
    inheritedColor,
    inheritedLabel,
    type Group,
  } from '$lib/types/group';
  import {
    alignItems,
    distributeItems,
    type AlignMode,
    type DistributeMode,
  } from '$lib/canvas/alignment';
  import ColorPicker from '$lib/ui/ColorPicker.svelte';
  import Toggle from '$lib/ui/Toggle.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import DashSegments from './DashSegments.svelte';
  import InspectorField from './InspectorField.svelte';
  import {
    MINIMIZED_TERMINAL_PANEL_HEIGHT,
    type CanvasItem,
    type FigureStrokeDash,
    type FontFamily,
    type LineItem,
    type NoteItem,
    type RectItem,
    type EllipseItem,
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

  /* ── Group selection (ADR-0010 D19 + plan-0012 §3.6 F.2) ────────────
   *
   * M.size === 1 + sole element 가 group id 이면 group inspector view 진입.
   * mixed M (group + item) 은 본 view 안 진입 — item-level Common section
   * 그대로. createGroup 직후 post-M = {newGroupId} → 자연스럽게 본 view 표시.
   */
  const groupSelection = $derived.by((): Group | null => {
    if (selectionCount !== 1) return null;
    const sole = selectedIds[0];
    if (sole === undefined || !sessionStore.isGroupId(sole)) return null;
    return sessionStore.groups.get(sole) ?? null;
  });

  /** 자손 통계 — direct child item 수 / 전체 자손 item 수. */
  const groupDescendantStats = $derived.by((): { direct: number; total: number } => {
    if (groupSelection === null) return { direct: 0, total: 0 };
    const gid = groupSelection.id;
    const groupsArr = Array.from(sessionStore.groups.values());
    const itemsArr = Array.from(sessionStore.items.values());
    const direct =
      itemsArr.filter((it) => it.parent_id === gid).length +
      groupsArr.filter((g) => g.parent_id === gid).length;
    const total = descendantItems(gid, groupsArr, itemsArr).length;
    return { direct, total };
  });

  const groupZIndexDisplay = $derived.by((): string => {
    if (groupSelection === null) return '—';
    const descendants = descendantItems(
      groupSelection.id,
      Array.from(sessionStore.groups.values()),
      Array.from(sessionStore.items.values()),
    );
    if (descendants.length === 0) return '—';
    let minZ = Number.POSITIVE_INFINITY;
    let maxZ = Number.NEGATIVE_INFINITY;
    for (const it of descendants) {
      if (it.z < minZ) minZ = it.z;
      if (it.z > maxZ) maxZ = it.z;
    }
    if (!Number.isFinite(minZ) || !Number.isFinite(maxZ)) return '—';
    return minZ === maxZ ? String(minZ) : `${minZ}–${maxZ}`;
  });

  /** Effective visibility / locked — ancestor 전파 결과. self 가 false 이고 effective
   *  true 이면 ancestor 가 source. UI 의 disabled tooltip 에 사용. */
  const groupEffective = $derived.by(() => {
    if (groupSelection === null) {
      return { visible: true, locked: false, inheritedHidden: false, inheritedLocked: false };
    }
    const g = groupSelection;
    const groupsMap = sessionStore.groups;
    const effVisible = effectiveVisibility(g.visibility, g.parent_id, groupsMap);
    const effLocked = effectiveLocked(g.locked, g.parent_id, groupsMap);
    // self 가 visible 이지만 effective hidden = ancestor 가 source.
    const inheritedHidden = g.visibility === 'visible' && !effVisible;
    const inheritedLocked = !g.locked && effLocked;
    return { visible: effVisible, locked: effLocked, inheritedHidden, inheritedLocked };
  });

  /** Inherit hint — self.label/color 가 null 일 때 ancestor 의 효과적 값. */
  const groupInheritedLabel = $derived.by((): string | null => {
    if (groupSelection === null) return null;
    if (groupSelection.label !== null) return null;
    return inheritedLabel(null, groupSelection.parent_id, sessionStore.groups);
  });
  const groupInheritedColor = $derived.by((): string | null => {
    if (groupSelection === null) return null;
    if (groupSelection.color !== null) return null;
    return inheritedColor(null, groupSelection.parent_id, sessionStore.groups);
  });

  /* ── Drill-state breadcrumb (ADR-0010 D22.7 + plan-0013 §3.7 H.6) ──
   *
   * M = leaf item / nested group 일 때 Inspector 상단에 ancestor chain 표시.
   * 각 segment = drill-out 버튼 (setM([segment.id])).
   * Root level item / root level group 은 breadcrumb 미표시.
   */
  const drillBreadcrumb = $derived.by((): Group[] => {
    if (selectionCount !== 1) return [];
    const sole = selectedIds[0];
    if (sole === undefined) return [];
    return ancestorChain(sole, sessionStore.items, sessionStore.groups);
  });

  function onBreadcrumbSegmentClick(groupId: string, event: MouseEvent): void {
    event.currentTarget instanceof HTMLElement && event.currentTarget.blur();
    sessionStore.setM([groupId]);
    sessionStore.setDrillRoot(
      directParentGroupId(groupId, sessionStore.items, sessionStore.groups),
    );
  }

  function selectedDisplayLabel(): string {
    if (groupSelection !== null) {
      return groupSelection.label ?? 'Untitled group';
    }
    if (selectedPanelId === null) return '';
    const it = sessionStore.items.get(selectedPanelId);
    return it !== undefined ? displayLabel(it) : '';
  }

  /** Group entity mutation — sessionStore.applyMutation 직접 호출 (groups[] 갱신).
   *  optimisticMutation 은 items 전용이라 사용 불가. */
  async function applyGroupMutation(
    transform: (g: Group) => Group,
    failMessage: string,
  ): Promise<void> {
    if (groupSelection === null) return;
    const gid = groupSelection.id;
    // Optimistic update.
    const cur = sessionStore.groups.get(gid);
    if (cur === undefined) return;
    const next = transform(cur);
    sessionStore.groups.set(gid, next);
    await sessionStore.applyMutation(
      (layout) => ({
        ...layout,
        groups: layout.groups.map((g) => (g.id === gid ? transform(g) : g)),
      }),
      {
        abortMessage: 'Group edit aborted — session reconnect failed.',
        failMessage,
      },
    );
  }

  async function applyGroupLabel(next: string): Promise<void> {
    const trimmed = next.trim();
    await applyGroupMutation(
      (g) => ({ ...g, label: trimmed.length === 0 ? null : trimmed }),
      'Group label edit failed',
    );
  }

  async function applyGroupColor(hex: string | null): Promise<void> {
    await applyGroupMutation((g) => ({ ...g, color: hex }), 'Group color edit failed');
  }

  async function applyGroupVisibility(visible: boolean): Promise<void> {
    await applyGroupMutation(
      (g) => ({ ...g, visibility: visible ? 'visible' : 'hidden' }),
      'Group visibility edit failed',
    );
  }

  async function applyGroupLocked(locked: boolean): Promise<void> {
    await applyGroupMutation((g) => ({ ...g, locked }), 'Group lock edit failed');
  }

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

  type TextStylableItem = TextItem | RectItem | EllipseItem;

  function isTextStylable(it: CanvasItem): it is TextStylableItem {
    return it.type === 'text' || it.type === 'rect' || it.type === 'ellipse';
  }

  function fontLabel(family: FontFamily | undefined): string {
    if (family === 'serif') return 'Serif';
    if (family === 'mono') return 'Mono';
    return 'Sans';
  }
  const FONT_FAMILIES: FontFamily[] = ['sans', 'serif', 'mono'];
  let figureTextSettingsOpen = $state(false);

  function fileStem(fileName: string): string {
    const base = fileName.trim().split('/').pop() ?? fileName.trim();
    const dot = base.lastIndexOf('.');
    if (dot <= 0) return base;
    return base.slice(0, dot);
  }

  /**
   * Common label 은 surface 별 표시 title 과 같은 값을 읽는다.
   * - terminal: terminalPool label 이 server-wide source 이므로 우선.
   * - note: title 이 canvas header title.
   * - document: label 이 canvas header title, 없으면 filename stem.
   */
  function displayLabel(it: CanvasItem): string {
    if (it.type === 'terminal') {
      return terminalPool.byId(it.id)?.label?.trim() || it.label || '';
    }
    if (it.type === 'note') return it.title;
    if (it.type === 'document') return it.label?.trim() || fileStem(it.file_name);
    return it.label ?? '';
  }
  function commonDisplayLabel(): string | 'Mixed' | null {
    if (selectedItems.length === 0) return null;
    const first = selectedItems[0];
    if (first === undefined) return null;
    const firstVal = displayLabel(first);
    for (const it of selectedItems) {
      if (displayLabel(it) !== firstVal) return 'Mixed';
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

  function supportsMinimize(it: CanvasItem): boolean {
    return (
      it.type === 'terminal' ||
      it.type === 'note' ||
      it.type === 'document' ||
      it.type === 'snippets'
    );
  }

  function supportsMaximize(it: CanvasItem): boolean {
    return it.type === 'terminal' || it.type === 'note' || it.type === 'document';
  }

  async function broadcastMutation(
    abortMessage: string,
    transform: (it: CanvasItem) => CanvasItem,
  ): Promise<void> {
    if (selectedItems.length === 0) return;
    const ids = new Set(selectedIds);
    await sessionStore.optimisticMutation(
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
    const terminalIds = selectedItems
      .filter((it) => it.type === 'terminal')
      .map((it) => it.id);
    await broadcastMutation('Edit aborted — session reconnect failed.', (it) => {
      if (it.type === 'note') {
        return { ...it, title: next } as CanvasItem;
      }
      if (it.type === 'text' || it.type === 'rect' || it.type === 'ellipse') {
        return { ...it, label: next, label_auto: false } as CanvasItem;
      }
      return { ...it, label: next } as CanvasItem;
    });
    if (terminalIds.length === 0) return;
    try {
      await Promise.all(terminalIds.map((id) => patchTerminalLabel(id, next)));
      await terminalPool.refresh();
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Terminal label sync failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  async function applyCommonBool(key: CommonBoolKey, next: boolean): Promise<void> {
    if (key === 'minimized') {
      await applyCommonMinimized(next);
      return;
    }
    await broadcastMutation('Edit aborted — session reconnect failed.', (it) => {
      if (key === 'visible') {
        const v: Visibility = next ? 'visible' : 'hidden';
        return { ...it, visibility: v } as CanvasItem;
      }
      return { ...it, [key]: next } as CanvasItem;
    });
  }

  async function applyCommonMinimized(next: boolean): Promise<void> {
    if (selectedItems.length === 0) return;
    const ids = new Set(selectedIds);
    const restoreGeoms = new Map<string, { x: number; y: number; w: number; h: number } | null>();
    const restoringIds: string[] = [];

    for (const it of selectedItems) {
      if (!supportsMinimize(it) || it.minimized === next) continue;
      if (next) {
        sessionStore.backupItemGeom(it.id, { x: it.x, y: it.y, w: it.w, h: it.h });
      } else {
        restoreGeoms.set(it.id, sessionStore.getRestoredGeom(it.id));
        restoringIds.push(it.id);
      }
    }

    const result = await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it) =>
          ids.has(it.id) ? applyMinimizeGeom(it, next, restoreGeoms) : it,
        ),
      }),
      {
        abortMessage: 'Edit aborted — session reconnect failed.',
        failMessage: 'Inspector edit failed',
      },
    );

    if (result.ok && !next) {
      for (const id of restoringIds) sessionStore.clearRestoredGeom(id);
    }
  }

  // PanelNode / NoteNode 의 onMinimizeClick 와 동일 패턴 — schema geom 변경.
  // backup/clear 는 applyCommonMinimized 에서 transform 밖에서 처리한다.
  // optimisticMutation 이 transform 을 두 번 실행하므로 본 함수는 순수해야 한다.
  // - terminal (PanelNode): h = 32 (header-strip), w 유지
  // - note    (NoteNode):   w = h = 32 (chip), 사각형 — node-side 와 정합
  // 그 외 type (rect/ellipse/line/text/free_draw/file_path/image) 은
  // minimize 시각 정의 없음 — inspector 의 minimize 버튼이 selectedTypes 에
  // terminal/note 가 하나도 없으면 hide. (selectionSupportsMinimize)
  function applyMinimizeGeom(
    it: CanvasItem,
    next: boolean,
    restoreGeoms: ReadonlyMap<string, { x: number; y: number; w: number; h: number } | null>,
  ): CanvasItem {
    if (it.minimized === next) return it;
    if (!supportsMinimize(it)) return it;
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
    const DOC_STRIP_H = 35;
    const DOC_RESTORE_W = 360;
    const DOC_RESTORE_H = 220;
    // SnippetsNode.svelte 와 정합 (head-only mode 의 collapsed h + fallback
    // restore geom). 정상 restore 는 restoreGeoms 의 기존 사용자 크기를 우선한다.
    const SNIP_STRIP_H = 35;
    const SNIP_RESTORE_W = 320;
    const SNIP_RESTORE_H = 150;
    if (next === true) {
      if (it.type === 'note') {
        return { ...it, minimized: true, w: NOTE_CHIP, h: NOTE_CHIP } as CanvasItem;
      }
      if (it.type === 'document') {
        return { ...it, minimized: true, h: DOC_STRIP_H } as CanvasItem;
      }
      if (it.type === 'snippets') {
        return { ...it, minimized: true, h: SNIP_STRIP_H } as CanvasItem;
      }
      return { ...it, minimized: true, h: PANEL_STRIP_H } as CanvasItem;
    }
    const backup = restoreGeoms.get(it.id) ?? null;
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
    if (it.type === 'snippets') {
      const w = backup?.w ?? SNIP_RESTORE_W;
      const h = backup?.h ?? SNIP_RESTORE_H;
      return { ...it, minimized: false, w, h } as CanvasItem;
    }
    const h = backup?.h ?? PANEL_RESTORE_H;
    return { ...it, minimized: false, h } as CanvasItem;
  }

  // selectedItems 중 minimize 지원 (terminal / note) 가 하나라도 있는지.
  // figure 만 선택된 경우 inspector 의 minimize 버튼 숨김.
  const minimizableSelectedItems = $derived.by(() => selectedItems.filter(supportsMinimize));
  const selectionSupportsMinimize = $derived(minimizableSelectedItems.length > 0);
  const singleMaximizableItem = $derived.by((): CanvasItem | null => {
    if (selectionCount !== 1) return null;
    const it = selectedItems[0];
    return it !== undefined && supportsMaximize(it) ? it : null;
  });
  const selectionSupportsMaximize = $derived(singleMaximizableItem !== null);
  const maximizedState = $derived.by((): boolean => {
    if (singleMaximizableItem === null) return false;
    return sessionStore.maximizedItemId === singleMaximizableItem.id;
  });

  function toggleSelectedMaximize(): void {
    if (singleMaximizableItem === null) return;
    sessionStore.toggleMaximize(singleMaximizableItem.id);
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
      void sessionStore.optimisticMutation(
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
      await sessionStore.optimisticMutation(
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
      await sessionStore.optimisticMutation(
        (cur) => ({
          ...cur,
          items: cur.items.map((it: CanvasItem) =>
            it.id === item.id && it.type === 'document'
              ? ({
                  ...it,
                  asset_id: uploaded.asset_id,
                  label: uploaded.file_name.replace(/\.[^/.]+$/, ''),
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
  function commonBoolIn(items: readonly CanvasItem[], reader: (it: CanvasItem) => boolean): boolean | null {
    if (items.length === 0) return null;
    const first = reader(items[0] as CanvasItem);
    for (const it of items) {
      if (reader(it) !== first) return null;
    }
    return first;
  }

  const visibleState = $derived.by(() => commonBoolIn(selectedItems, (it) => it.visibility === 'visible'));
  const lockedState = $derived.by(() => commonBoolIn(selectedItems, (it) => it.locked));
  const minimizedState = $derived.by(() => commonBoolIn(minimizableSelectedItems, (it) => it.minimized));

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
    await sessionStore.optimisticMutation(
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
    await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it) => {
          if (!ids.has(it.id)) return it;
          if (it.locked) return it;
          if (it.type !== 'rect' && it.type !== 'ellipse' && it.type !== 'line' && it.type !== 'text') return it;
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

  /* ── Shape boolean toggle (batch-5 R1+R2) ──
   * fill_enabled / stroke_enabled / corner_rounded 의 toggle. multi 시
   * 같은 type 의 selected item 에 broadcast. corner_rounded 는 rect 만 적용.
   */
  async function applyShapeBoolean(
    field: 'fill_enabled' | 'stroke_enabled' | 'corner_rounded',
    next: boolean,
  ): Promise<void> {
    if (selectedItems.length === 0) return;
    const ids = new Set(selectedIds);
    await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it) => {
          if (!ids.has(it.id)) return it;
          if (it.locked) return it;
          if (field === 'corner_rounded') {
            if (it.type !== 'rect' && it.type !== 'text') return it;
            return { ...it, corner_rounded: next } as CanvasItem;
          }
          if (it.type !== 'rect' && it.type !== 'ellipse' && it.type !== 'text') return it;
          return { ...it, [field]: next } as CanvasItem;
        }),
      }),
      {
        abortMessage: 'Toggle aborted — session reconnect failed.',
        failMessage: 'Toggle failed',
      },
    );
  }

  /* ── Shape stroke_width / stroke_dash editor (batch-5 R2) ──
   * 두 field 모두 rect/ellipse/line 공통. stroke_width 는 BE 의 1..=32 cap
   * (ValidationError::StrokeWidthOutOfRange) 와 정합 — 본 함수도 clamp.
   */
  async function applyShapeStrokeWidth(width: number): Promise<void> {
    if (selectedItems.length === 0) return;
    const clamped = Math.max(1, Math.min(32, Math.round(width)));
    const ids = new Set(selectedIds);
    await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it) => {
          if (!ids.has(it.id)) return it;
          if (it.locked) return it;
          if (it.type !== 'rect' && it.type !== 'ellipse' && it.type !== 'line' && it.type !== 'text') return it;
          return { ...it, stroke_width: clamped } as CanvasItem;
        }),
      }),
      {
        abortMessage: 'Stroke width aborted — session reconnect failed.',
        failMessage: 'Stroke width failed',
      },
    );
  }

  async function applyShapeDash(dash: FigureStrokeDash | undefined): Promise<void> {
    if (selectedItems.length === 0) return;
    const ids = new Set(selectedIds);
    await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it) => {
          if (!ids.has(it.id)) return it;
          if (it.locked) return it;
          if (it.type !== 'rect' && it.type !== 'ellipse' && it.type !== 'line' && it.type !== 'text') return it;
          // undefined = solid → field 제거 (옵셔널 의미 보존)
          const next = { ...it } as CanvasItem & { stroke_dash?: FigureStrokeDash };
          if (dash === undefined || dash === 'solid') {
            delete next.stroke_dash;
          } else {
            next.stroke_dash = dash;
          }
          return next;
        }),
      }),
      {
        abortMessage: 'Dash change aborted — session reconnect failed.',
        failMessage: 'Dash change failed',
      },
    );
  }

  /* ── Text alignment — Figma-style segmented control ──────────────
   * Inspector 가 text item 의 alignment 를 직접 mutate. 옛
   * ToolbarSubbar/TextNode 에 분산되어 있던 로직을 본 곳으로 단일화. */

  async function applyTextAlign(next: TextAlign): Promise<void> {
    const ids = new Set(selectedIds);
    if (ids.size === 0) return;
    await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) => {
          if (!ids.has(it.id) || !isTextStylable(it) || it.locked) return it;
          if (next === (it.text_align ?? 'center')) return it;
          return { ...it, text_align: next } as CanvasItem;
        }),
      }),
      { failMessage: 'Text align failed' },
    );
  }

  async function applyTextVerticalAlign(next: TextVerticalAlign): Promise<void> {
    const ids = new Set(selectedIds);
    if (ids.size === 0) return;
    await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) => {
          if (!ids.has(it.id) || !isTextStylable(it) || it.locked) return it;
          if (next === (it.text_vertical_align ?? 'middle')) return it;
          return { ...it, text_vertical_align: next } as CanvasItem;
        }),
      }),
      { failMessage: 'Text vertical align failed' },
    );
  }

  /* ── Text font style — font_weight / italic / underline / strikethrough /
   *    font_size / color (batch-5 R3) ──
   * 모두 selected text item 의 broadcast (multi-aware).
   */
  async function applyTextFontWeight(next: 'light' | 'normal' | 'bold'): Promise<void> {
    const ids = new Set(selectedIds);
    if (ids.size === 0) return;
    await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          ids.has(it.id) && isTextStylable(it)
            && !it.locked
            ? ({ ...it, font_weight: next } as CanvasItem)
            : it,
        ),
      }),
      { failMessage: 'Font weight failed' },
    );
  }

  async function applyTextBoolean(
    field: 'italic' | 'underline' | 'strikethrough',
    next: boolean,
  ): Promise<void> {
    const ids = new Set(selectedIds);
    if (ids.size === 0) return;
    await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          ids.has(it.id) && isTextStylable(it)
            && !it.locked
            ? ({ ...it, [field]: next } as CanvasItem)
            : it,
        ),
      }),
      { failMessage: 'Text style failed' },
    );
  }

  async function applyTextFontSize(size: number): Promise<void> {
    const ids = new Set(selectedIds);
    if (ids.size === 0) return;
    // BE 의 TextFontSizeOutOfRange 와 정합 — 8..=96.
    const clamped = Math.max(8, Math.min(96, Math.round(size)));
    await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          ids.has(it.id) && isTextStylable(it)
            && !it.locked
            ? ({ ...it, font_size: clamped } as CanvasItem)
            : it,
        ),
      }),
      { failMessage: 'Font size failed' },
    );
  }

  async function applyTextColor(hex: string): Promise<void> {
    const ids = new Set(selectedIds);
    if (ids.size === 0) return;
    await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          ids.has(it.id) && isTextStylable(it)
            && !it.locked
            ? ({ ...it, color: hex } as CanvasItem)
            : it,
        ),
      }),
      { failMessage: 'Text color failed' },
    );
  }

  async function applyTextFontFamily(next: FontFamily): Promise<void> {
    const ids = new Set(selectedIds);
    if (ids.size === 0) return;
    await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          ids.has(it.id) && isTextStylable(it) && !it.locked
            ? ({ ...it, font_family: next } as CanvasItem)
            : it,
        ),
      }),
      { failMessage: 'Font family failed' },
    );
  }
</script>

<div class="item-info-view" aria-label="Item info">
  <div class="pane-info-body">
    {#if drillBreadcrumb.length > 0}
      <!-- ADR-0010 D22.7 + design handover §8.2.2 — drill-state ancestor breadcrumb.
           Each segment = drill-out button (setM([segment.id])).
           Last segment = current selection label (display only, not a button). -->
      <nav class="drill-breadcrumb" aria-label="Drill hierarchy">
        {#each drillBreadcrumb as seg, i (seg.id)}
          <button
            type="button"
            class="breadcrumb-seg"
            title={`Drill out to ${seg.label ?? seg.id.slice(0, 8)}`}
            onclick={(e: MouseEvent) => onBreadcrumbSegmentClick(seg.id, e)}
          >
            {seg.label ?? seg.id.slice(0, 8)}
          </button>
          <span class="breadcrumb-sep" aria-hidden="true">›</span>
        {/each}
        <span class="breadcrumb-current">{selectedDisplayLabel()}</span>
      </nav>
    {/if}

    {#if groupSelection !== null}
      <!-- ADR-0010 D19 + plan-0012 §3.6 F.2 — Group-specific Inspector view.
           Common section (geometry / z) 숨김 — group 은 frame 없음 (G-hybrid). -->
      <div class="multi-header">
        <span class="multi-label">Group · <span class="muted">{groupDescendantStats.total} items</span></span>
      </div>

      <section class="prop-section">
        <div class="prop-head"><h4>Identity</h4></div>
        <div class="prop-row full">
          <InspectorField
            k="label"
            value={groupSelection.label ?? ''}
            mixed={false}
            placeholder={groupInheritedLabel ?? 'Untitled group'}
            ariaLabel="Group label"
            live={true}
            oncommit={(next) => void applyGroupLabel(next)}
          />
        </div>
        <div class="prop-row full">
          <div class="display-row">
            <span class="k">id</span>
            <span class="display-val mono" title={groupSelection.id}>{groupSelection.id}</span>
          </div>
        </div>
        <div class="prop-row full">
          <div class="display-row">
            <span class="k">Z-INDEX</span>
            <span class="display-val mono">{groupZIndexDisplay}</span>
          </div>
        </div>
      </section>

      <section class="prop-section">
        <div class="prop-head"><h4>Color</h4></div>
        <div class="prop-row full">
          <ColorPicker
            value={groupSelection.color ?? groupInheritedColor ?? null}
            live={true}
            oncommit={(hex) => void applyGroupColor(hex)}
          />
        </div>
        {#if groupSelection.color !== null}
          <div class="prop-row full">
            <button
              type="button"
              class="inline-action"
              onclick={() => void applyGroupColor(null)}
              title="Inherit color from parent"
            >
              Reset to inherit
            </button>
          </div>
        {:else if groupInheritedColor !== null}
          <div class="prop-row full">
            <span class="hint mono">Inherited</span>
          </div>
        {/if}
      </section>

      <section class="prop-section">
        <div class="prop-head"><h4>State</h4></div>
        <div class="state-row" role="group" aria-label="Group state">
          <button
            type="button"
            class="state-btn"
            class:active={groupSelection.visibility === 'visible'}
            disabled={groupEffective.inheritedHidden}
            aria-pressed={groupSelection.visibility === 'visible'}
            aria-label={groupSelection.visibility === 'visible' ? 'Hide group' : 'Show group'}
            title={groupEffective.inheritedHidden
              ? 'Hidden by ancestor — cannot show until ancestor visible'
              : groupSelection.visibility === 'visible'
                ? 'Visible (click to hide)'
                : 'Hidden (click to show)'}
            onclick={() => void applyGroupVisibility(groupSelection.visibility !== 'visible')}
          >
            {#if groupSelection.visibility === 'visible'}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8S1 12 1 12z"/>
                <circle cx="12" cy="12" r="3"/>
              </svg>
            {:else}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94"/>
                <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19"/>
                <path d="M14.12 14.12a3 3 0 1 1-4.24-4.24"/>
                <line x1="1" y1="1" x2="23" y2="23"/>
              </svg>
            {/if}
          </button>

          <button
            type="button"
            class="state-btn"
            class:active={groupSelection.locked}
            disabled={groupEffective.inheritedLocked}
            aria-pressed={groupSelection.locked}
            aria-label={groupSelection.locked ? 'Unlock group' : 'Lock group'}
            title={groupEffective.inheritedLocked
              ? 'Locked by ancestor — cannot unlock until ancestor unlocked'
              : groupSelection.locked
                ? 'Locked (click to unlock)'
                : 'Unlocked (click to lock)'}
            onclick={() => void applyGroupLocked(!groupSelection.locked)}
          >
            {#if groupSelection.locked}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <rect x="4" y="11" width="16" height="10" rx="2"/>
                <path d="M8 11V8a4 4 0 1 1 8 0v3"/>
              </svg>
            {:else}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <rect x="4" y="11" width="16" height="10" rx="2"/>
                <path d="M8 11V8a4 4 0 0 1 7.5-2"/>
              </svg>
            {/if}
          </button>
        </div>
      </section>

      <section class="prop-section">
        <div class="prop-head"><h4>Contents</h4></div>
        <div class="prop-row full">
          <div class="display-row">
            <span class="k">direct</span>
            <span class="display-val mono">{groupDescendantStats.direct}</span>
          </div>
        </div>
        <div class="prop-row full">
          <div class="display-row">
            <span class="k">total</span>
            <span class="display-val mono">{groupDescendantStats.total}</span>
          </div>
        </div>
      </section>

    {:else if selectedPanel === null}
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
              const v = commonDisplayLabel();
              return typeof v === 'string' ? v : '';
            })()}
            mixed={commonDisplayLabel() === 'Mixed'}
            placeholder="—"
            ariaLabel="Label"
            live={true}
            oncommit={(next) => void applyCommonLabel(next)}
          />
        </div>
        {#if selectionCount === 1}
          <div class="prop-row full">
            <div class="display-row">
              <span class="k">id</span>
              <span class="display-val mono" title={selectedPanel.id as string}>{selectedPanel.id}</span>
              {#if isSelectedTerminal}
                <!-- Change terminal — header button 과 동일 entry (사용자 요구
                     2026-05-21). DocumentNode/FilePathNode 의 inline-action
                     패턴 정합. -->
                <button
                  type="button"
                  class="inline-action"
                  title="Change terminal"
                  aria-label="Change terminal"
                  disabled={(selectedPanel as { locked?: boolean }).locked === true}
                  onclick={() => changeTerminalDialog.show(selectedPanel.id as string)}
                >
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
                    <path d="M9 17H7A5 5 0 0 1 7 7h2"/>
                    <path d="M15 7h2a5 5 0 1 1 0 10h-2"/>
                    <line x1="8" x2="16" y1="12" y2="12"/>
                  </svg>
                </button>
              {/if}
            </div>
          </div>
          <div class="prop-row full">
            <div class="display-row">
              <span class="k">Z-INDEX</span>
              <span class="display-val mono">{numOr(selectedPanel['z'], '—')}</span>
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
            live={true}
            oncommit={(s) => void applyCommonNum('x', Number(s))}
          />
          <InspectorField
            type="number"
            k="Y"
            value={(() => { const v = commonField('y'); return typeof v === 'number' ? String(Math.round(v)) : '0'; })()}
            mixed={commonField('y') === 'Mixed'}
            ariaLabel="y"
            live={true}
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
            live={true}
            oncommit={(s) => void applyCommonNum('w', Number(s))}
          />
          <InspectorField
            type="number"
            k="H"
            value={(() => { const v = commonField('h'); return typeof v === 'number' ? String(Math.round(v)) : ''; })()}
            mixed={commonField('h') === 'Mixed'}
            ariaLabel="h"
            live={true}
            oncommit={(s) => void applyCommonNum('h', Number(s))}
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

      {#if sessionItem !== null && ((selectionCount === 1 && (sessionItem.type === 'rect' || sessionItem.type === 'ellipse' || sessionItem.type === 'line' || sessionItem.type === 'text' || sessionItem.type === 'note' || sessionItem.type === 'file_path' || sessionItem.type === 'image' || sessionItem.type === 'document' || sessionItem.type === 'snippets')) || (isMultiHomogeneous && (commonType === 'rect' || commonType === 'ellipse' || commonType === 'text')))}
        <section class="prop-section">
          <div class="prop-head"><h4>Item Payload</h4></div>
          {#if sessionItem.type === 'rect' || sessionItem.type === 'ellipse'}
            {@const shape = sessionItem}
            {@const fillOn = shape.fill_enabled !== false}
            {@const strokeOn = shape.stroke_enabled !== false}
            {@const shapeText = shape.text ?? ''}
            {@const shapeH = shape.text_align ?? 'center'}
            {@const shapeV = shape.text_vertical_align ?? 'middle'}
            {@const shapeFw = shape.font_weight ?? 'normal'}
            {@const shapeFamily = shape.font_family ?? 'sans'}
            {@const shapeItalic = shape.italic === true}
            {@const shapeUnderline = shape.underline === true}
            {@const shapeStrike = shape.strikethrough === true}
            <!-- Inspector design 규칙 (사용자 정의): fill/stroke/rounded 가
                 border-style group. header (label + toggle 우측) ↔ body 가
                 1px divider 로 분리. background/border 색은 state-row 와 통일
                 (var(--color-surface-2) + var(--color-border)). color picker
                 는 자체 라벨 ("C") 보유라 외부 label 무. -->

            <!-- FILL group -->
            <div class="fig-group" class:is-on={fillOn}>
              <div class="fig-group-head">
                <span class="k">fill</span>
                <span class="fig-spacer"></span>
                <Toggle
                  checked={fillOn}
                  disabled={shape.locked}
                  ariaLabel="Toggle fill"
                  onchange={(next) => void applyShapeBoolean('fill_enabled', next)}
                />
              </div>
              {#if fillOn}
                <div class="fig-group-body">
                  <ColorPicker
                    value={shape.fill}
                    live={true}
                    allowAlpha={true}
                    allowTransparent={true}
                    disabled={shape.locked}
                    oncommit={(hex) => void applyShapeColor('fill', hex)}
                  />
                </div>
              {/if}
            </div>

            <!-- STROKE group -->
            <div class="fig-group" class:is-on={strokeOn}>
              <div class="fig-group-head">
                <span class="k">stroke</span>
                <span class="fig-spacer"></span>
                <Toggle
                  checked={strokeOn}
                  disabled={shape.locked}
                  ariaLabel="Toggle stroke"
                  onchange={(next) => void applyShapeBoolean('stroke_enabled', next)}
                />
              </div>
              {#if strokeOn}
                <div class="fig-group-body">
                  <ColorPicker
                    value={shape.stroke}
                    live={true}
                    allowAlpha={true}
                    disabled={shape.locked}
                    oncommit={(hex) => void applyShapeColor('stroke', hex)}
                  />
                  <InspectorField
                    type="number"
                    k="width"
                    value={String(shape.stroke_width)}
                    mixed={false}
                    ariaLabel="Stroke width"
                    disabled={shape.locked}
                    live={true}
                    oncommit={(s) => void applyShapeStrokeWidth(Number(s))}
                  />
                  <DashSegments
                    value={shape.stroke_dash ?? 'solid'}
                    disabled={shape.locked}
                    onpick={(next) => void applyShapeDash(next)}
                  />
                </div>
              {/if}
            </div>

            <!-- ROUNDED group (rect only, single toggle no body) -->
            {#if shape.type === 'rect'}
              {@const rounded = shape.corner_rounded === true}
              <div class="fig-group" class:is-on={rounded}>
                <div class="fig-group-head">
                  <span class="k">rounded</span>
                  <span class="fig-spacer"></span>
                  <Toggle
                    checked={rounded}
                    disabled={shape.locked}
                    ariaLabel="Toggle rounded corners"
                    onchange={(next) => void applyShapeBoolean('corner_rounded', next)}
                  />
                </div>
              </div>
            {/if}
            <div class="fig-group" class:is-on={figureTextSettingsOpen}>
              <button
                type="button"
                class="fig-group-head fig-toggle-head"
                aria-expanded={figureTextSettingsOpen}
                aria-label={figureTextSettingsOpen ? 'Collapse text settings' : 'Expand text settings'}
                onclick={() => (figureTextSettingsOpen = !figureTextSettingsOpen)}
              >
                <span class="k">text</span>
                <span class="fig-spacer"></span>
                <span class="fig-caret" aria-hidden="true">{figureTextSettingsOpen ? '▾' : '◂'}</span>
              </button>
              {#if figureTextSettingsOpen}
                <div class="fig-group-body">
                <div class="font-dropdown">
                  <Dropdown placement="bottom-start">
                    {#snippet trigger({ toggle })}
                      <button
                        type="button"
                        class="font-trigger"
                        disabled={shape.locked || shapeText.length === 0}
                        aria-label="Font family"
                        title="Font family"
                        onclick={toggle}
                      >
                        <span class="font-label">font</span>
                        <span class="font-value font-preview-{shapeFamily}">{fontLabel(shapeFamily)}</span>
                        <span class="font-caret" aria-hidden="true">▾</span>
                      </button>
                    {/snippet}
                    {#snippet menu({ close })}
                      {#each FONT_FAMILIES as f}
                        <button
                          type="button"
                          class="font-option font-preview-{f}"
                          disabled={shape.locked}
                          onclick={() => {
                            void applyTextFontFamily(f);
                            close();
                          }}
                        >
                          {fontLabel(f)}
                        </button>
                      {/each}
                    {/snippet}
                  </Dropdown>
                </div>
                <InspectorField
                  type="number"
                  k="size"
                  value={String(shape.font_size ?? 14)}
                  mixed={false}
                  ariaLabel="Font size"
                  disabled={shape.locked}
                  live={true}
                  oncommit={(s) => void applyTextFontSize(Number(s))}
                />
                <ColorPicker
                  value={shape.color ?? 'var(--color-fg)'}
                  live={true}
                  allowAlpha={true}
                  disabled={shape.locked}
                  oncommit={(hex) => void applyTextColor(hex)}
                />
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">weight</span>
                  <div class="segmented-control" role="group" aria-label="Font weight">
                    <button type="button" class="seg-btn weight-btn weight-light" class:active={shapeFw === 'light'} aria-pressed={shapeFw === 'light'} title="Light (300)" aria-label="Light weight" disabled={shape.locked} onclick={() => void applyTextFontWeight('light')}>L</button>
                    <button type="button" class="seg-btn weight-btn weight-normal" class:active={shapeFw === 'normal'} aria-pressed={shapeFw === 'normal'} title="Normal (400)" aria-label="Normal weight" disabled={shape.locked} onclick={() => void applyTextFontWeight('normal')}>N</button>
                    <button type="button" class="seg-btn weight-btn weight-bold" class:active={shapeFw === 'bold'} aria-pressed={shapeFw === 'bold'} title="Bold (700)" aria-label="Bold weight" disabled={shape.locked} onclick={() => void applyTextFontWeight('bold')}>B</button>
                  </div>
                </div>
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">style</span>
                  <div class="segmented-control multi" role="group" aria-label="Text style">
                    <button type="button" class="seg-btn style-btn style-italic" class:active={shapeItalic} aria-pressed={shapeItalic} title="Italic" aria-label="Toggle italic" disabled={shape.locked} onclick={() => void applyTextBoolean('italic', !shapeItalic)}>I</button>
                    <button type="button" class="seg-btn style-btn style-underline" class:active={shapeUnderline} aria-pressed={shapeUnderline} title="Underline" aria-label="Toggle underline" disabled={shape.locked} onclick={() => void applyTextBoolean('underline', !shapeUnderline)}>U</button>
                    <button type="button" class="seg-btn style-btn style-strike" class:active={shapeStrike} aria-pressed={shapeStrike} title="Strikethrough" aria-label="Toggle strikethrough" disabled={shape.locked} onclick={() => void applyTextBoolean('strikethrough', !shapeStrike)}>S</button>
                  </div>
                </div>
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">align</span>
                  <div class="segmented-control" role="group" aria-label="Horizontal alignment">
                    <button type="button" class="seg-btn" class:active={shapeH === 'left'} aria-pressed={shapeH === 'left'} title="Align left" aria-label="Align left" disabled={shape.locked} onclick={() => void applyTextAlign('left')}>L</button>
                    <button type="button" class="seg-btn" class:active={shapeH === 'center'} aria-pressed={shapeH === 'center'} title="Align center" aria-label="Align center" disabled={shape.locked} onclick={() => void applyTextAlign('center')}>C</button>
                    <button type="button" class="seg-btn" class:active={shapeH === 'right'} aria-pressed={shapeH === 'right'} title="Align right" aria-label="Align right" disabled={shape.locked} onclick={() => void applyTextAlign('right')}>R</button>
                  </div>
                </div>
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">v-align</span>
                  <div class="segmented-control" role="group" aria-label="Vertical alignment">
                    <button type="button" class="seg-btn" class:active={shapeV === 'top'} aria-pressed={shapeV === 'top'} title="Align top" aria-label="Align top" disabled={shape.locked} onclick={() => void applyTextVerticalAlign('top')}>T</button>
                    <button type="button" class="seg-btn" class:active={shapeV === 'middle'} aria-pressed={shapeV === 'middle'} title="Align middle" aria-label="Align middle" disabled={shape.locked} onclick={() => void applyTextVerticalAlign('middle')}>M</button>
                    <button type="button" class="seg-btn" class:active={shapeV === 'bottom'} aria-pressed={shapeV === 'bottom'} title="Align bottom" aria-label="Align bottom" disabled={shape.locked} onclick={() => void applyTextVerticalAlign('bottom')}>B</button>
                  </div>
                </div>
                </div>
              {/if}
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
                disabled={line.locked}
                live={true}
                oncommit={(s) => void applyLineEndpoint('x2', Number(s))}
              />
              <InspectorField
                type="number"
                k="Y2"
                value={String(Math.round(line.y2))}
                mixed={false}
                ariaLabel="y2"
                disabled={line.locked}
                live={true}
                oncommit={(s) => void applyLineEndpoint('y2', Number(s))}
              />
            </div>
            <!-- line stroke: figma-style group (no toggle, color + w + style 항상 노출) -->
            <div class="fig-group is-on">
              <div class="fig-group-head">
                <span class="k">stroke</span>
              </div>
              <div class="fig-group-body">
                <ColorPicker
                  value={line.stroke}
                  live={true}
                  allowAlpha={true}
                  disabled={line.locked}
                  oncommit={(hex) => void applyShapeColor('stroke', hex)}
                />
                <InspectorField
                  type="number"
                  k="width"
                  value={String(line.stroke_width)}
                  mixed={false}
                  ariaLabel="Stroke width"
                  disabled={line.locked}
                  live={true}
                  oncommit={(s) => void applyShapeStrokeWidth(Number(s))}
                />
                <DashSegments
                  value={line.stroke_dash ?? 'solid'}
                  disabled={line.locked}
                  onpick={(next) => void applyShapeDash(next)}
                />
              </div>
            </div>
          {:else if sessionItem.type === 'text'}
            {@const txt = sessionItem}
            {@const h = txt.text_align ?? 'center'}
            {@const v = txt.text_vertical_align ?? 'middle'}
            {@const fw = txt.font_weight ?? 'normal'}
            {@const family = txt.font_family ?? 'sans'}
            {@const isItalic = txt.italic === true}
            {@const isUnderline = txt.underline === true}
            {@const isStrike = txt.strikethrough === true}
            {@const textFillOn = txt.fill_enabled === true}
            {@const textStrokeOn = txt.stroke_enabled === true}
            <!-- chars meta (read-only) — group 밖, info row -->
            <div class="prop-row full">
              <div class="display-row">
                <span class="k">chars</span>
                <span class="display-val mono">{txt.text.length}</span>
              </div>
            </div>
            <!-- Figure 와 통일성 정합 — text controls 도 Figma-style fig-group
                 안에. text 는 on/off toggle 없음 → head 에 label 만, 항상 is-on. -->
            <div class="fig-group is-on">
              <div class="fig-group-head">
                <span class="k">text</span>
              </div>
              <div class="fig-group-body">
                <div class="font-dropdown">
                  <Dropdown placement="bottom-start">
                    {#snippet trigger({ toggle })}
                      <button
                        type="button"
                        class="font-trigger"
                        disabled={txt.locked}
                        aria-label="Font family"
                        title="Font family"
                        onclick={toggle}
                      >
                        <span class="font-label">font</span>
                        <span class="font-value font-preview-{family}">{fontLabel(family)}</span>
                        <span class="font-caret" aria-hidden="true">▾</span>
                      </button>
                    {/snippet}
                    {#snippet menu({ close })}
                      {#each FONT_FAMILIES as f}
                        <button
                          type="button"
                          class="font-option font-preview-{f}"
                          disabled={txt.locked}
                          onclick={() => {
                            void applyTextFontFamily(f);
                            close();
                          }}
                        >
                          {fontLabel(f)}
                        </button>
                      {/each}
                    {/snippet}
                  </Dropdown>
                </div>
                <InspectorField
                  type="number"
                  k="size"
                  value={String(txt.font_size)}
                  mixed={false}
                  ariaLabel="Font size"
                  disabled={txt.locked}
                  live={true}
                  oncommit={(s) => void applyTextFontSize(Number(s))}
                />
                <ColorPicker
                  value={txt.color}
                  live={true}
                  allowAlpha={true}
                  disabled={txt.locked}
                  oncommit={(hex) => void applyTextColor(hex)}
                />
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">weight</span>
                  <div class="segmented-control" role="group" aria-label="Font weight">
                    <button
                      type="button"
                      class="seg-btn weight-btn weight-light"
                      class:active={fw === 'light'}
                      aria-pressed={fw === 'light'}
                      title="Light (300)"
                      aria-label="Light weight"
                      disabled={txt.locked}
                      onclick={() => void applyTextFontWeight('light')}
                    >L</button>
                    <button
                      type="button"
                      class="seg-btn weight-btn weight-normal"
                      class:active={fw === 'normal'}
                      aria-pressed={fw === 'normal'}
                      title="Normal (400)"
                      aria-label="Normal weight"
                      disabled={txt.locked}
                      onclick={() => void applyTextFontWeight('normal')}
                    >N</button>
                    <button
                      type="button"
                      class="seg-btn weight-btn weight-bold"
                      class:active={fw === 'bold'}
                      aria-pressed={fw === 'bold'}
                      title="Bold (700)"
                      aria-label="Bold weight"
                      disabled={txt.locked}
                      onclick={() => void applyTextFontWeight('bold')}
                    >B</button>
                  </div>
                </div>
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">style</span>
                  <div class="segmented-control multi" role="group" aria-label="Text style">
                    <button
                      type="button"
                      class="seg-btn style-btn style-italic"
                      class:active={isItalic}
                      aria-pressed={isItalic}
                      title="Italic"
                      aria-label="Toggle italic"
                      disabled={txt.locked}
                      onclick={() => void applyTextBoolean('italic', !isItalic)}
                    >I</button>
                    <button
                      type="button"
                      class="seg-btn style-btn style-underline"
                      class:active={isUnderline}
                      aria-pressed={isUnderline}
                      title="Underline"
                      aria-label="Toggle underline"
                      disabled={txt.locked}
                      onclick={() => void applyTextBoolean('underline', !isUnderline)}
                    >U</button>
                    <button
                      type="button"
                      class="seg-btn style-btn style-strike"
                      class:active={isStrike}
                      aria-pressed={isStrike}
                      title="Strikethrough"
                      aria-label="Toggle strikethrough"
                      disabled={txt.locked}
                      onclick={() => void applyTextBoolean('strikethrough', !isStrike)}
                    >S</button>
                  </div>
                </div>
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">align</span>
                  <div class="segmented-control icon-segments" role="group" aria-label="Horizontal alignment">
                    <button
                      type="button"
                      class="seg-btn"
                      class:active={h === 'left'}
                      aria-pressed={h === 'left'}
                      title="Align left"
                      aria-label="Align left"
                      disabled={txt.locked}
                      onclick={() => void applyTextAlign('left')}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
                        <line x1="4" y1="6" x2="20" y2="6"/>
                        <line x1="4" y1="12" x2="14" y2="12"/>
                        <line x1="4" y1="18" x2="18" y2="18"/>
                      </svg>
                    </button>
                    <button
                      type="button"
                      class="seg-btn"
                      class:active={h === 'center'}
                      aria-pressed={h === 'center'}
                      title="Align center"
                      aria-label="Align center"
                      disabled={txt.locked}
                      onclick={() => void applyTextAlign('center')}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
                        <line x1="4" y1="6" x2="20" y2="6"/>
                        <line x1="7" y1="12" x2="17" y2="12"/>
                        <line x1="5" y1="18" x2="19" y2="18"/>
                      </svg>
                    </button>
                    <button
                      type="button"
                      class="seg-btn"
                      class:active={h === 'right'}
                      aria-pressed={h === 'right'}
                      title="Align right"
                      aria-label="Align right"
                      disabled={txt.locked}
                      onclick={() => void applyTextAlign('right')}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
                        <line x1="4" y1="6" x2="20" y2="6"/>
                        <line x1="10" y1="12" x2="20" y2="12"/>
                        <line x1="6" y1="18" x2="20" y2="18"/>
                      </svg>
                    </button>
                  </div>
                </div>
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">v-align</span>
                  <div class="segmented-control icon-segments" role="group" aria-label="Vertical alignment">
                    <button
                      type="button"
                      class="seg-btn"
                      class:active={v === 'top'}
                      aria-pressed={v === 'top'}
                      title="Align top"
                      aria-label="Align top"
                      disabled={txt.locked}
                      onclick={() => void applyTextVerticalAlign('top')}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
                        <line x1="5" y1="5" x2="19" y2="5"/>
                        <line x1="8" y1="10" x2="16" y2="10"/>
                        <line x1="10" y1="15" x2="14" y2="15"/>
                      </svg>
                    </button>
                    <button
                      type="button"
                      class="seg-btn"
                      class:active={v === 'middle'}
                      aria-pressed={v === 'middle'}
                      title="Align middle"
                      aria-label="Align middle"
                      disabled={txt.locked}
                      onclick={() => void applyTextVerticalAlign('middle')}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
                        <line x1="6" y1="7" x2="18" y2="7"/>
                        <line x1="4" y1="12" x2="20" y2="12"/>
                        <line x1="6" y1="17" x2="18" y2="17"/>
                      </svg>
                    </button>
                    <button
                      type="button"
                      class="seg-btn"
                      class:active={v === 'bottom'}
                      aria-pressed={v === 'bottom'}
                      title="Align bottom"
                      aria-label="Align bottom"
                      disabled={txt.locked}
                      onclick={() => void applyTextVerticalAlign('bottom')}
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
            </div>
            <div class="fig-group" class:is-on={textFillOn}>
              <div class="fig-group-head">
                <span class="k">fill</span>
                <span class="fig-spacer"></span>
                <Toggle
                  checked={textFillOn}
                  disabled={txt.locked}
                  ariaLabel="Toggle text box fill"
                  onchange={(next) => void applyShapeBoolean('fill_enabled', next)}
                />
              </div>
              {#if textFillOn}
                <div class="fig-group-body">
                  <ColorPicker
                    value={txt.fill ?? 'var(--color-surface)'}
                    live={true}
                    allowAlpha={true}
                    allowTransparent={true}
                    disabled={txt.locked}
                    oncommit={(hex) => void applyShapeColor('fill', hex)}
                  />
                </div>
              {/if}
            </div>
            <div class="fig-group" class:is-on={textStrokeOn}>
              <div class="fig-group-head">
                <span class="k">stroke</span>
                <span class="fig-spacer"></span>
                <Toggle
                  checked={textStrokeOn}
                  disabled={txt.locked}
                  ariaLabel="Toggle text box stroke"
                  onchange={(next) => void applyShapeBoolean('stroke_enabled', next)}
                />
              </div>
              {#if textStrokeOn}
                <div class="fig-group-body">
                  <ColorPicker
                    value={txt.stroke ?? 'var(--color-fg)'}
                    live={true}
                    allowAlpha={true}
                    disabled={txt.locked}
                    oncommit={(hex) => void applyShapeColor('stroke', hex)}
                  />
                  <InspectorField
                    type="number"
                    k="width"
                    value={String(txt.stroke_width ?? 2)}
                    mixed={false}
                    ariaLabel="Text box stroke width"
                    disabled={txt.locked}
                    live={true}
                    oncommit={(s) => void applyShapeStrokeWidth(Number(s))}
                  />
                  <DashSegments
                    value={txt.stroke_dash ?? 'solid'}
                    disabled={txt.locked}
                    onpick={(next) => void applyShapeDash(next)}
                  />
                </div>
              {/if}
            </div>
            <div class="fig-group" class:is-on={txt.corner_rounded === true}>
              <div class="fig-group-head">
                <span class="k">rounded</span>
                <span class="fig-spacer"></span>
                <Toggle
                  checked={txt.corner_rounded === true}
                  disabled={txt.locked}
                  ariaLabel="Toggle text box rounded corners"
                  onchange={(next) => void applyShapeBoolean('corner_rounded', next)}
                />
              </div>
            </div>
          {:else if sessionItem.type === 'note'}
            <!-- ColorPicker 자체 "C" 라벨 보유 → 외부 label 무. -->
            <div class="prop-row full">
              <ColorPicker
                value={sessionItem.color}
                live={true}
                oncommit={(hex) => void applyNoteColor(hex)}
              />
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
          {:else if sessionItem.type === 'snippets'}
            <!-- ADR-0038 — v8 design §12.2. Read-only display + add trigger.
                 Editing lives in SnippetsNode inline form (Alt 5 rejected).
                 locked toggle is owned by the standard State section below — no
                 duplicate here. -->
            {@const snipItem = sessionItem}
            {@const snipEntries = snipItem.entries}
            <div class="prop-row full">
              <div class="display-row">
                <span class="k">items</span>
                <span class="display-val mono">{snipEntries.length}</span>
                <span class="display-val mono muted" style="margin-left:4px;">/ 1000</span>
                <button
                  type="button"
                  class="inline-action"
                  title={snipItem.locked ? 'Locked — unlock to add' : 'Add a new snippet'}
                  aria-label="Add a new snippet"
                  disabled={snipItem.locked || snipEntries.length >= 1000}
                  onclick={(e: MouseEvent) => {
                    const target = e.currentTarget as HTMLButtonElement;
                    const r = target.getBoundingClientRect();
                    snippetEditPanel.openFor({
                      nodeId: snipItem.id,
                      entryId: null,
                      anchor: { x: r.left, y: r.top, width: r.width, height: r.height },
                      source: 'inspector',
                    });
                  }}
                >
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
                    <path d="M12 5v14M5 12h14"/>
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
            aria-label={visibleState === null ? 'Show all' : visibleState ? 'Hide' : 'Show'}
            title={visibleState === null ? 'Visibility mixed (click to show all)' : visibleState ? 'Visible (click to hide)' : 'Hidden (click to show)'}
            onclick={() => void applyCommonBool('visible', !(visibleState ?? false))}
          >
            {#if visibleState === null}
              <svg class="mixed-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <rect x="4" y="4" width="16" height="16" rx="3" stroke="currentColor" stroke-width="1.8"/>
                <path d="M8 12h8" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
              </svg>
            {:else if visibleState === false}
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
          </button>

          <button
            type="button"
            class="state-btn"
            class:active={lockedState === true}
            class:mixed={lockedState === null}
            aria-pressed={lockedState === true}
            aria-label={lockedState === null ? 'Lock all' : lockedState ? 'Unlock' : 'Lock'}
            title={lockedState === null ? 'Lock mixed (click to lock all)' : lockedState ? 'Locked (click to unlock)' : 'Unlocked (click to lock)'}
            onclick={() => void applyCommonBool('locked', !(lockedState ?? false))}
          >
            {#if lockedState === null}
              <svg class="mixed-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <rect x="4" y="4" width="16" height="16" rx="3" stroke="currentColor" stroke-width="1.8"/>
                <path d="M8 12h8" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
              </svg>
            {:else if lockedState === true}
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
          </button>

          {#if selectionSupportsMinimize}
            <button
              type="button"
              class="state-btn"
              class:active={minimizedState === true}
              class:mixed={minimizedState === null}
              aria-pressed={minimizedState === true}
              aria-label={minimizedState === null ? 'Minimize all supported items' : minimizedState ? 'Restore' : 'Minimize'}
              title={minimizedState === null ? 'Minimize mixed (click to minimize all supported items)' : minimizedState ? 'Minimized (click to restore)' : 'Normal (click to minimize)'}
              onclick={() => void applyCommonBool('minimized', !(minimizedState ?? false))}
            >
              {#if minimizedState === null}
                <svg class="mixed-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                  <rect x="4" y="4" width="16" height="16" rx="3" stroke="currentColor" stroke-width="1.8"/>
                  <path d="M8 12h8" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
                </svg>
              {:else if minimizedState === true}
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
            </button>
          {/if}

          {#if selectionSupportsMaximize}
            <button
              type="button"
              class="state-btn"
              class:active={maximizedState}
              aria-pressed={maximizedState}
              aria-label={maximizedState ? 'Restore' : 'Maximize'}
              title={maximizedState ? 'Maximized (click to restore)' : 'Normal (click to maximize)'}
              onclick={toggleSelectedMaximize}
            >
              {#if maximizedState}
                <svg width="14" height="14" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
                  <rect x="2" y="3.6" width="6.5" height="6.4" rx="0.5"/>
                  <path d="M4 3.6V2.4h6.5v6.4H9"/>
                </svg>
              {:else}
                <svg width="14" height="14" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
                  <rect x="2.5" y="2.5" width="7" height="7" rx="0.6"/>
                </svg>
              {/if}
            </button>
          {/if}

          <!-- Focus 는 ViewportCtrl 의 focus 버튼으로 이동. -->
        </div>
        <!-- alive row 폐기 (사용자 요구 2026-05-21) — Terminal·Pool section 의
             alive row 와 중복. State section 은 visibility/lock/minimize/maximize 만. -->
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

  /* 2-column numeric rows are narrow inside the right inspector. Keep the
     full 56px label column for full-width rows, but compact X/Y/W/H/X2/Y2
     labels so negative and large coordinate values retain readable space. */
  .prop-row:not(.full) > :global(.inspector-input) {
    --inspector-k-w: 18px;
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

  /* .display-row.picker / .shape-style-row CSS 폐기 (2026-05-21) — Inspector
     design 규칙 재편 후 ColorPicker 가 자체 라벨 + width full 처리, 외부
     wrapper 불요. fig-group 이 layout 책임. */

  /* ── Inspector design 규칙 (2026-05-21 사용자 정의) ──────────────
     - Group: border-style container (state-row 참조). border + surface-2
       background 통일 (header / body 색 차별 X). header ↔ body 는 1px
       divider 로만 구분.
     - Height 24px 통일 (모든 component 평균).
     - Width full.
   */
  .fig-group {
    display: flex;
    flex-direction: column;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    width: 100%;
    /* overflow visible — DashSegments dropdown 의 popover 가 group 아래로
       돌출되어야 함. head/body 가 transparent bg 라 corner clipping 무관. */
    overflow: visible;
  }
  .fig-group + .fig-group {
    margin-top: 4px;
  }
  /* head row — 24px 통일. label 좌측 .k 56px 고정, toggle 우측 fig-spacer.
     body expand 여부와 무관, 위치/높이 절대 변경 X (사용자 요구). */
  .fig-group-head {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 6px;
    box-sizing: border-box;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--color-fg);
  }

  .fig-toggle-head {
    width: 100%;
    border: 0;
    background: transparent;
    cursor: pointer;
    text-align: left;
  }

  .fig-toggle-head:hover:not(:disabled) {
    background: var(--color-glass-1);
  }

  .fig-group-head > .k {
    flex: 0 0 56px;
    width: 56px;
    color: var(--color-fg-muted);
    text-transform: uppercase;
    font-size: 10px;
    letter-spacing: 0.4px;
  }
  .fig-group-head > .fig-spacer {
    flex: 1 1 auto;
  }
  .fig-caret {
    flex: 0 0 14px;
    width: 14px;
    text-align: center;
    color: var(--color-fg-muted);
  }
  /* on 시 head 와 body 사이 hairline divider — group 시각 결속. */
  .fig-group.is-on .fig-group-head {
    border-bottom: 1px solid var(--color-border);
  }
  /* body — 같은 surface-2 bg 유지 (사용자 요구: 색 통일). 내부 component 들이
     각자 자체 border + bg 를 가져서 group 색에 겹쳐 자연 정합. */
  .fig-group-body {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 4px 6px;
  }
  /* body 안의 InspectorField / DashSegments / ColorPicker 가 width full. */
  .fig-group-body > :global(.inspector-input),
  .fig-group-body > :global(.color-picker),
  .fig-group-body > :global(.style-dropdown),
  .fig-group-body > .font-dropdown {
    width: 100%;
    min-width: 0;
  }
  /* 2-column body row (stroke 의 w + style) — 1:1 균등 분배, gap 4px. */
  /* .fig-body-pair (2-col stroke w + style) 폐기 — 사용자 요구로 width / style
     각자 full row 로 분리. style 의 SVG preview 가 narrow column 에서 overflow
     하던 문제 해소 + label "width" full text 복원. */

  .control-row {
    justify-content: flex-start;
    gap: 10px;
  }

  .control-label {
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.4px;
    color: var(--color-fg-muted);
    text-transform: uppercase;
    /* label-front 정합 (사용자 요구) — .k 라벨 (56px) 과 같은 fixed width 로
       picker row 의 label column 과 좌측 정렬 맞춤. */
    flex: 0 0 56px;
    width: 56px;
  }

  .segmented-control {
    display: inline-flex;
    align-items: center;
    flex: 0 0 auto;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  /* control-row 안의 segmented-control 은 row 의 남은 너비 전체 차지 (사용자
     요구: "button 그룹의 너비가 full"). 자식 seg-btn 도 flex:1 로 균등 분포. */
  .control-row > .segmented-control {
    flex: 1 1 auto;
    width: 100%;
  }
  .control-row > .segmented-control > .seg-btn {
    flex: 1 1 0;
    width: auto;
    min-width: 0;
  }

  .seg-btn {
    width: 28px;
    height: 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    border-right: 1px solid var(--color-border);
    background: var(--color-surface-2);
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.5px;
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .seg-btn:last-child {
    border-right: 0;
  }

  .seg-btn:hover:not(:disabled):not(.active) {
    background: var(--color-glass-1);
  }

  .seg-btn.active {
    background: var(--color-accent);
    color: var(--color-accent-fg);
    font-weight: var(--weight-semibold);
  }

  .seg-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .seg-btn:focus-visible {
    outline: none;
  }

  .font-dropdown {
    width: 100%;
    min-width: 0;
  }

  .font-dropdown :global(.dropdown-host) {
    display: flex;
    width: 100%;
    min-width: 0;
  }

  .font-dropdown :global(.dropdown-menu) {
    box-sizing: border-box;
    width: 100%;
    min-width: 0;
    margin-top: 4px;
    padding: 2px;
    border-radius: var(--radius-sm);
  }

  .font-dropdown :global(.dropdown-menu button) {
    justify-content: center;
    height: 24px;
    padding: 0 6px;
    border-radius: 3px;
    font-size: 11px;
  }

  .font-trigger {
    box-sizing: border-box;
    flex: 1 1 auto;
    min-width: 0;
    width: 100%;
    height: 24px;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 6px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.2px;
    cursor: pointer;
    text-align: left;
    transition: border-color var(--motion-fast) var(--motion-easing);
  }

  .font-trigger:hover:not(:disabled) {
    border-color: var(--color-border-strong);
  }

  .font-trigger:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .font-label {
    flex: 0 0 var(--inspector-k-w, 56px);
    width: var(--inspector-k-w, 56px);
    font-family: var(--font-mono);
    color: var(--color-fg-muted);
    text-transform: uppercase;
    font-size: 10px;
    letter-spacing: 0.4px;
    text-align: left;
  }

  .font-value {
    flex: 1 1 auto;
    min-width: 0;
    text-align: center;
  }

  .font-caret {
    flex: 0 0 14px;
    width: 14px;
    text-align: center;
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
  }

  .font-option.font-preview-sans,
  .font-preview-sans {
    font-family: var(--font-sans);
  }

  .font-option.font-preview-serif,
  .font-preview-serif {
    font-family: var(--font-serif);
  }

  .font-option.font-preview-mono,
  .font-preview-mono {
    font-family: var(--font-mono);
  }

  .icon-segments .seg-btn {
    color: var(--color-fg-muted);
  }

  .icon-segments .seg-btn.active {
    color: var(--color-accent-fg);
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

  /* "Inherited" hint chip — group color self.null 상태 표시. */
  .hint.mono {
    font-size: var(--text-xs);
    color: var(--color-fg-muted);
    font-style: italic;
  }

  /* ADR-0010 D22.7 + design handover §8.2.2 — drill-state ancestor breadcrumb.
     Inspector 상단 row, deep nest 시 truncation 자연 발생 (text-overflow ellipsis). */
  .drill-breadcrumb {
    display: flex;
    flex-wrap: nowrap;
    align-items: center;
    gap: 2px;
    padding: var(--space-4) var(--space-8);
    border-bottom: 1px solid var(--color-border);
    overflow: hidden;
    font-size: var(--text-xs);
    line-height: 1.3;
    background: var(--color-glass-1);
    flex: 0 0 auto;
  }

  .breadcrumb-seg {
    border: 0;
    background: transparent;
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 1px 4px;
    border-radius: 2px;
    font: inherit;
    max-width: 80px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: color var(--motion-fast) var(--motion-easing);
  }

  .breadcrumb-seg:hover {
    color: var(--color-fg);
    background: var(--color-glass-2);
  }

  .breadcrumb-sep {
    color: var(--color-fg-subtle);
    font-size: var(--text-xs);
    flex: 0 0 auto;
  }

  .breadcrumb-current {
    color: var(--color-fg);
    font-weight: 500;
    padding: 1px 4px;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  /* Mixed: replace the state glyph with an indeterminate square. Overlaying a
   * dash on the normal icon made minimize/restore ambiguous in mixed selection. */
  .state-btn.mixed {
    color: var(--color-fg-subtle);
  }

  .state-btn.mixed:hover {
    color: var(--color-fg-muted);
  }

  /* batch-5 polish — 옛 .dash-picker (<select> 기반) 폐기. DashSegments
     컴포넌트가 icon-segmented control 로 대체 (사용자 요구: "line style 은
     icon 으로 대체"). */

  /* batch-5 R3 — Text weight (3-segment) + style (3 toggle) 버튼.
     align-btn 의 SVG 자리에 글자 (L/N/B / I/U/S) 표시. 각 글자가 그 weight/
     style 의 시각 단서 역할 — L 은 thin, N 은 regular, B 는 bold;
     I 는 italic, U 는 underline, S 는 line-through. */
  .weight-btn,
  .style-btn {
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.4px;
  }
  .weight-btn.weight-light  { font-weight: 300; }
  .weight-btn.weight-normal { font-weight: 400; }
  .weight-btn.weight-bold   { font-weight: 700; }

  .style-btn.style-italic    { font-style: italic; }
  .style-btn.style-underline { text-decoration: underline; }
  .style-btn.style-strike    { text-decoration: line-through; }

</style>
