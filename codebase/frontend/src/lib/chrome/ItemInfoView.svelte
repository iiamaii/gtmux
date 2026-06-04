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
  import { UnauthorizedError } from '$lib/http/sessions';
  import {
    DOCUMENT_EXTENSIONS,
    IMAGE_EXTENSIONS,
    basename,
    fileStem as workspaceFileStem,
    guessMimeFromPath,
    resolveWorkspacePath,
    workspaceRelativePath,
  } from '$lib/files/workspaceAssets';
  import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { snippetEditPanel } from '$lib/stores/snippetEditPanel.svelte';
  import {
    ancestorChain,
    descendantGroups,
    descendantItems,
    directParentGroupId,
    effectiveLocked,
    effectiveVisibility,
    inheritedLabel,
    type Group,
  } from '$lib/types/group';
  import {
    alignItems,
    alignBoxes,
    distributeItems,
    distributeBoxes,
    type AlignMode,
    type AlignBox,
    type DistributeMode,
    type MoveDelta,
  } from '$lib/canvas/alignment';
  import ColorPicker from '$lib/ui/ColorPicker.svelte';
  import Toggle from '$lib/ui/Toggle.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import DropdownChevron from '$lib/ui/DropdownChevron.svelte';
  import PanelEmptyState from './PanelEmptyState.svelte';
  import DashSegments from './DashSegments.svelte';
  import HeadIcon from './HeadIcon.svelte';
  import InspectorField from './InspectorField.svelte';
  import PathAnchorPicker from './PathAnchorPicker.svelte';
  import RoutingIcon from './RoutingIcon.svelte';
  import {
    MINIMIZED_TERMINAL_PANEL_HEIGHT,
    type Anchor,
    type CanvasItem,
    type CanvasLayout,
    type FigureStrokeDash,
    type FontFamily,
    type FreeDrawItem,
    type Head,
    type LineItem,
    type NoteItem,
    type PathEndpoint,
    type PathItem,
    type PathRouting,
    type Point,
    type RectItem,
    type EllipseItem,
    type TextAlign,
    type TextItem,
    type TextVerticalAlign,
    type Visibility,
  } from '$lib/types/canvas';
  import {
    anchorPoint,
    connectedEndpointPoint,
    computePathBBox,
    editPathGeometry,
    isConnectableItem,
    isPathConnectedToAny,
    resolveEndpoint,
    translatePath,
    updatePathBBoxCache,
    type ConnectableItem,
  } from '$lib/canvas/pathGeometry';
  import { lineBoxFromEndpoints } from '$lib/canvas/itemFactory';
  import { rememberPathStyle } from '$lib/canvas/pathStyleMemory';

  // ADR-0027 D2 — multi-select aware. selectedIds = M 의 array snapshot.
  // selectedPanelId = first id (display 의 single-item fallback path 용).
  const selectedIds = $derived.by((): string[] => Array.from(sessionStore.M));
  const selectedPanelId = $derived.by((): string | null => {
    for (const id of selectedIds) {
      if (sessionStore.items.has(id)) return id;
    }
    return selectedIds.length === 0 ? null : (selectedIds[0] as string);
  });
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
  const selectedGroups = $derived.by((): Group[] => {
    const out: Group[] = [];
    for (const id of selectedIds) {
      const group = sessionStore.groups.get(id);
      if (group !== undefined) out.push(group);
    }
    return out;
  });
  const isGroupOnlySelection = $derived(
    selectionCount > 0 && selectedGroups.length === selectionCount,
  );
  const isMultiGroupSelection = $derived(
    selectionCount > 1 && isGroupOnlySelection,
  );
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

  function groupDirectCount(group: Group): number {
    const gid = group.id;
    let direct = 0;
    for (const it of sessionStore.items.values()) {
      if (it.parent_id === gid) direct += 1;
    }
    for (const g of sessionStore.groups.values()) {
      if (g.parent_id === gid) direct += 1;
    }
    return direct;
  }

  function groupDescendantItemIds(groups: readonly Group[]): Set<string> {
    const groupsArr = Array.from(sessionStore.groups.values());
    const itemsArr = Array.from(sessionStore.items.values());
    const ids = new Set<string>();
    for (const group of groups) {
      for (const item of descendantItems(group.id, groupsArr, itemsArr)) {
        ids.add(item.id);
      }
    }
    return ids;
  }

  function commonGroupBool(
    groups: readonly Group[],
    reader: (group: Group) => boolean,
  ): boolean | null {
    const first = groups[0];
    if (first === undefined) return null;
    const firstValue = reader(first);
    for (const group of groups) {
      if (reader(group) !== firstValue) return null;
    }
    return firstValue;
  }

  function itemAlignBox(item: CanvasItem): AlignBox {
    if (item.type === 'line') {
      const box = lineBoxFromEndpoints(
        { x: item.x, y: item.y },
        { x: item.x2, y: item.y2 },
      );
      return { id: item.id, x: box.x, y: box.y, w: box.w, h: box.h, locked: item.locked };
    }
    if (item.type === 'path') {
      const box = computePathBBox(item, sessionStore.items);
      return { id: item.id, x: box.x, y: box.y, w: box.w, h: box.h, locked: item.locked };
    }
    return { id: item.id, x: item.x, y: item.y, w: item.w, h: item.h, locked: item.locked };
  }

  function unionAlignBox(id: string, boxes: readonly AlignBox[], locked: boolean): AlignBox | null {
    if (boxes.length === 0) return null;
    let minX = Number.POSITIVE_INFINITY;
    let minY = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;
    let maxY = Number.NEGATIVE_INFINITY;
    for (const box of boxes) {
      minX = Math.min(minX, box.x);
      minY = Math.min(minY, box.y);
      maxX = Math.max(maxX, box.x + box.w);
      maxY = Math.max(maxY, box.y + box.h);
    }
    if (!Number.isFinite(minX) || !Number.isFinite(minY)) return null;
    return { id, x: minX, y: minY, w: maxX - minX, h: maxY - minY, locked };
  }

  function topLevelSelectedGroups(groups: readonly Group[]): Group[] {
    if (groups.length <= 1) return [...groups];
    const selectedGroupIds = new Set(groups.map((group) => group.id));
    const groupsById = sessionStore.groups;
    return groups.filter((group) => {
      let parentId = group.parent_id;
      while (parentId !== null) {
        if (selectedGroupIds.has(parentId)) return false;
        parentId = groupsById.get(parentId)?.parent_id ?? null;
      }
      return true;
    });
  }

  const selectedGroupAlignTargets = $derived.by((): AlignBox[] => {
    if (!isGroupOnlySelection) return [];
    const groupsArr = Array.from(sessionStore.groups.values());
    const itemsArr = Array.from(sessionStore.items.values());
    const groupsById = sessionStore.groups;
    const targets: AlignBox[] = [];
    for (const group of topLevelSelectedGroups(selectedGroups)) {
      const descendants = descendantItems(group.id, groupsArr, itemsArr).filter((item) =>
        effectiveVisibility(item.visibility, item.parent_id, groupsById),
      );
      const boxes = descendants.map(itemAlignBox);
      const locked = descendants.every((item) =>
        effectiveLocked(item.locked, item.parent_id, groupsById),
      );
      const box = unionAlignBox(group.id, boxes, locked);
      if (box !== null) targets.push(box);
    }
    return targets;
  });
  const canAlignSelectedGroups = $derived(selectedGroupAlignTargets.length >= 2);
  const canDistributeSelectedGroups = $derived(selectedGroupAlignTargets.length >= 3);

  const selectedGroupStats = $derived.by((): { direct: number; total: number } => {
    if (selectedGroups.length === 0) return { direct: 0, total: 0 };
    let direct = 0;
    for (const group of selectedGroups) direct += groupDirectCount(group);
    return {
      direct,
      total: groupDescendantItemIds(selectedGroups).size,
    };
  });
  const selectedGroupVisibilityState = $derived.by(() =>
    commonGroupBool(selectedGroups, (group) => group.visibility === 'visible'),
  );
  const selectedGroupLockedState = $derived.by(() =>
    commonGroupBool(selectedGroups, (group) => group.locked),
  );

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

  async function applyGroupSetMutation(
    groupIds: Iterable<string>,
    transform: (g: Group) => Group,
    failMessage: string,
  ): Promise<void> {
    const ids = new Set(groupIds);
    if (ids.size === 0) return;
    for (const id of ids) {
      const cur = sessionStore.groups.get(id);
      if (cur !== undefined) sessionStore.groups.set(id, transform(cur));
    }
    await sessionStore.applyMutation(
      (layout) => ({
        ...layout,
        groups: layout.groups.map((g) => (ids.has(g.id) ? transform(g) : g)),
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

  async function applyGroupVisibility(visible: boolean): Promise<void> {
    await applyGroupMutation(
      (g) => ({ ...g, visibility: visible ? 'visible' : 'hidden' }),
      'Group visibility edit failed',
    );
  }

  async function applyGroupLocked(locked: boolean): Promise<void> {
    await applyGroupMutation((g) => ({ ...g, locked }), 'Group lock edit failed');
  }

  async function applySelectedGroupVisibility(visible: boolean): Promise<void> {
    await applyGroupSetMutation(
      selectedGroups.map((g) => g.id),
      (g) => ({ ...g, visibility: visible ? 'visible' : 'hidden' }),
      'Group visibility edit failed',
    );
  }

  async function applySelectedGroupLocked(locked: boolean): Promise<void> {
    await applyGroupSetMutation(
      selectedGroups.map((g) => g.id),
      (g) => ({ ...g, locked }),
      'Group lock edit failed',
    );
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
  type BoxStylableItem = TextItem | RectItem | EllipseItem;
  type StrokeStylableItem =
    | RectItem
    | EllipseItem
    | LineItem
    | PathItem
    | FreeDrawItem
    | TextItem;
  type StrokeDashStylableItem = RectItem | EllipseItem | LineItem | PathItem | TextItem;
  type HeadStylableItem = LineItem | PathItem;

  function isTextStylable(it: CanvasItem): it is TextStylableItem {
    return it.type === 'text' || it.type === 'rect' || it.type === 'ellipse';
  }

  function isBoxStylable(it: CanvasItem): it is BoxStylableItem {
    return it.type === 'text' || it.type === 'rect' || it.type === 'ellipse';
  }

  function isStrokeStylable(it: CanvasItem): it is StrokeStylableItem {
    return (
      it.type === 'rect' ||
      it.type === 'ellipse' ||
      it.type === 'line' ||
      it.type === 'path' ||
      it.type === 'free_draw' ||
      it.type === 'text'
    );
  }

  function isStrokeDashStylable(it: CanvasItem): it is StrokeDashStylableItem {
    return (
      it.type === 'rect' ||
      it.type === 'ellipse' ||
      it.type === 'line' ||
      it.type === 'path' ||
      it.type === 'text'
    );
  }

  function isHeadStylable(it: CanvasItem): it is HeadStylableItem {
    return it.type === 'line' || it.type === 'path';
  }

  function commonMapped<T, V>(
    items: readonly T[],
    reader: (item: T) => V,
  ): V | 'Mixed' | null {
    const head = items[0];
    if (head === undefined) return null;
    const first = reader(head);
    for (const it of items) {
      if (!Object.is(reader(it), first)) return 'Mixed';
    }
    return first;
  }

  function allLocked(items: readonly CanvasItem[]): boolean {
    return items.length === 0 || items.every((it) => it.locked);
  }

  function boxFillEnabled(it: BoxStylableItem): boolean {
    return it.type === 'text' ? it.fill_enabled === true : it.fill_enabled !== false;
  }

  function boxStrokeEnabled(it: BoxStylableItem): boolean {
    return it.type === 'text' ? it.stroke_enabled === true : it.stroke_enabled !== false;
  }

  function boxFillColor(it: BoxStylableItem): string {
    return it.type === 'text' ? (it.fill ?? 'var(--color-surface)') : it.fill;
  }

  function strokeColor(it: StrokeStylableItem): string {
    return it.type === 'text' ? (it.stroke ?? 'var(--color-fg)') : it.stroke;
  }

  function strokeWidth(it: StrokeStylableItem): number {
    return it.type === 'text' ? (it.stroke_width ?? 2) : it.stroke_width;
  }

  const textStyleItems = $derived.by((): TextStylableItem[] =>
    selectedItems.filter(isTextStylable),
  );
  const boxStyleItems = $derived.by((): BoxStylableItem[] =>
    selectedItems.filter(isBoxStylable),
  );
  const strokeStyleItems = $derived.by((): StrokeStylableItem[] =>
    selectedItems.filter(isStrokeStylable),
  );
  const strokeDashStyleItems = $derived.by((): StrokeDashStylableItem[] =>
    selectedItems.filter(isStrokeDashStylable),
  );
  const headStyleItems = $derived.by((): HeadStylableItem[] =>
    selectedItems.filter(isHeadStylable),
  );
  const pathStyleItems = $derived.by((): PathItem[] =>
    selectedItems.filter((it): it is PathItem => it.type === 'path'),
  );
  const roundedStyleItems = $derived.by((): Array<TextItem | RectItem> =>
    selectedItems.filter((it): it is TextItem | RectItem => it.type === 'text' || it.type === 'rect'),
  );

  const hasSharedTextStyle = $derived(selectionCount > 1 && textStyleItems.length > 0);
  const hasSharedFillStyle = $derived(selectionCount > 1 && boxStyleItems.length > 0);
  const hasSharedStrokeStyle = $derived(selectionCount > 1 && strokeStyleItems.length > 0);
  const hasSharedHeadStyle = $derived(selectionCount > 1 && headStyleItems.length > 0);
  const hasSharedPathRouting = $derived(selectionCount > 1 && pathStyleItems.length > 0);
  const hasSharedRoundedStyle = $derived(selectionCount > 1 && roundedStyleItems.length > 0);
  const hasSharedStyleSection = $derived(
    hasSharedTextStyle ||
      hasSharedFillStyle ||
      hasSharedStrokeStyle ||
      hasSharedHeadStyle ||
      hasSharedPathRouting ||
      hasSharedRoundedStyle,
  );

  const textStyleLocked = $derived(allLocked(textStyleItems));
  const boxStyleLocked = $derived(allLocked(boxStyleItems));
  const strokeStyleLocked = $derived(allLocked(strokeStyleItems));
  const strokeDashStyleLocked = $derived(allLocked(strokeDashStyleItems));
  const headStyleLocked = $derived(allLocked(headStyleItems));
  const pathStyleLocked = $derived(allLocked(pathStyleItems));
  const roundedStyleLocked = $derived(allLocked(roundedStyleItems));
  const canToggleSharedStroke = $derived(
    strokeStyleItems.length > 0 && strokeStyleItems.every(isBoxStylable),
  );

  const sharedTextFontFamily = $derived.by(() =>
    commonMapped(textStyleItems, (it) => it.font_family ?? 'sans'),
  );
  const sharedTextFontSize = $derived.by(() =>
    commonMapped(textStyleItems, (it) => it.font_size ?? 14),
  );
  const sharedTextColor = $derived.by(() =>
    commonMapped(textStyleItems, (it) => it.color ?? 'var(--color-fg)'),
  );
  const sharedTextWeight = $derived.by(() =>
    commonMapped(textStyleItems, (it) => it.font_weight ?? 'normal'),
  );
  const sharedTextItalic = $derived.by(() =>
    commonMapped(textStyleItems, (it) => it.italic === true),
  );
  const sharedTextUnderline = $derived.by(() =>
    commonMapped(textStyleItems, (it) => it.underline === true),
  );
  const sharedTextStrike = $derived.by(() =>
    commonMapped(textStyleItems, (it) => it.strikethrough === true),
  );
  const sharedTextAlign = $derived.by(() =>
    commonMapped(textStyleItems, (it) => it.text_align ?? 'center'),
  );
  const sharedTextVerticalAlign = $derived.by(() =>
    commonMapped(textStyleItems, (it) => it.text_vertical_align ?? 'middle'),
  );
  const sharedFillEnabled = $derived.by(() =>
    commonMapped(boxStyleItems, boxFillEnabled),
  );
  const sharedFillColor = $derived.by(() =>
    commonMapped(boxStyleItems, boxFillColor),
  );
  const sharedStrokeEnabled = $derived.by(() =>
    commonMapped(boxStyleItems, boxStrokeEnabled),
  );
  const sharedStrokeColor = $derived.by(() =>
    commonMapped(strokeStyleItems, strokeColor),
  );
  const sharedStrokeWidth = $derived.by(() =>
    commonMapped(strokeStyleItems, strokeWidth),
  );
  const sharedStrokeDash = $derived.by(() =>
    commonMapped(strokeDashStyleItems, (it) => it.stroke_dash ?? 'solid'),
  );
  const sharedRounded = $derived.by(() =>
    commonMapped(roundedStyleItems, (it) => it.corner_rounded === true),
  );
  const sharedHeadFrom = $derived.by(() =>
    commonMapped(headStyleItems, (it) => it.head_from ?? 'none'),
  );
  const sharedHeadTo = $derived.by(() =>
    commonMapped(headStyleItems, (it) => it.head_to ?? 'none'),
  );
  const sharedPathRouting = $derived.by(() =>
    commonMapped(pathStyleItems, (it) => it.routing),
  );

  function fontLabel(family: FontFamily | undefined): string {
    if (family === 'serif') return 'Serif';
    if (family === 'mono') return 'Mono';
    return 'Sans';
  }
  const FONT_FAMILIES: FontFamily[] = ['sans', 'serif', 'mono'];
  let figureTextSettingsOpen = $state(false);
  const INSPECTOR_DROPDOWN_LABEL_INSET = 66;
  const INSPECTOR_DROPDOWN_GAP = 2;
  const INSPECTOR_DROPDOWN_MENU_CLASS = 'inspector-dropdown-menu';

  type PathEndpointId = 'from' | 'to';

  const HEAD_OPTIONS: { value: Head; label: string }[] = [
    { value: 'none', label: 'None' },
    { value: 'arrow', label: 'Arrow' },
    { value: 'circle', label: 'Circle' },
    { value: 'diamond', label: 'Diamond' },
  ];
  const ROUTING_OPTIONS: { value: PathRouting; label: string }[] = [
    { value: 'orthogonal', label: 'Orthogonal' },
    { value: 'bezier', label: 'Smooth' },
    { value: 'straight', label: 'Straight' },
  ];
  const ANCHOR_OPTIONS: { value: Anchor; label: string }[] = [
    { value: 'N', label: 'N' },
    { value: 'NE', label: 'NE' },
    { value: 'E', label: 'E' },
    { value: 'SE', label: 'SE' },
    { value: 'S', label: 'S' },
    { value: 'SW', label: 'SW' },
    { value: 'W', label: 'W' },
    { value: 'NW', label: 'NW' },
    { value: 'center', label: 'Center' },
  ];

  function headLabel(head: Head | undefined): string {
    return HEAD_OPTIONS.find((option) => option.value === (head ?? 'none'))?.label ?? 'None';
  }

  function fontValueLabel(value: FontFamily | 'Mixed' | null): string {
    if (value === 'Mixed' || value === null) return 'Mixed';
    return fontLabel(value);
  }

  function headValueLabel(value: Head | 'Mixed' | null): string {
    if (value === 'Mixed' || value === null) return 'Mixed';
    return headLabel(value);
  }

  function concreteHead(value: Head | 'Mixed' | null): Head {
    return value === 'Mixed' || value === null ? 'none' : value;
  }

  function concreteDash(value: FigureStrokeDash | 'Mixed' | null): FigureStrokeDash {
    return value === 'Mixed' || value === null ? 'solid' : value;
  }

  function mixedColorValue(value: string | 'Mixed' | null, fallback: string): string {
    return value === 'Mixed' || value === null ? fallback : value;
  }

  function mixedNumberValue(value: number | 'Mixed' | null): string {
    return typeof value === 'number' ? String(value) : '';
  }

  function toggleFromMixed(value: boolean | 'Mixed' | null): boolean {
    return value !== true;
  }

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
    if (it.type === 'document') return it.label?.trim() || fileStem(it.file_name ?? it.path ?? 'document');
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

  const TARGET_LABEL_MAX = 30;

  function truncateTargetLabel(value: string, max = TARGET_LABEL_MAX): string {
    if (value.length <= max) return value;
    if (max <= 3) return value.slice(0, max);
    return `${value.slice(0, max - 3)}...`;
  }

  function itemOptionTitle(item: CanvasItem): string {
    const label = displayLabel(item).trim();
    return label.length > 0 ? `${label} · ${item.type}` : `${item.type} · ${item.id.slice(0, 8)}`;
  }

  function itemOptionLabel(item: CanvasItem): string {
    return truncateTargetLabel(itemOptionTitle(item));
  }

  function endpointOf(path: PathItem, endpointId: PathEndpointId): PathEndpoint {
    return endpointId === 'from' ? path.from : path.to;
  }

  function otherEndpointOf(path: PathItem, endpointId: PathEndpointId): PathEndpoint {
    return endpointId === 'from' ? path.to : path.from;
  }

  function setPathEndpoint(
    path: PathItem,
    endpointId: PathEndpointId,
    endpoint: PathEndpoint,
  ): PathItem {
    return endpointId === 'from'
      ? { ...path, from: endpoint }
      : { ...path, to: endpoint };
  }

  function endpointOffset(endpoint: PathEndpoint): Point {
    if (endpoint.kind !== 'connected') return { x: 0, y: 0 };
    return endpoint.offset ?? { x: 0, y: 0 };
  }

  function normalizeEndpointOffset(offset: Point): Point | undefined {
    return offset.x === 0 && offset.y === 0 ? undefined : offset;
  }

  function nearestAnchor(item: CanvasItem, point: Point): Anchor {
    let best = ANCHOR_OPTIONS[0]!.value;
    let bestDistance = Number.POSITIVE_INFINITY;
    for (const option of ANCHOR_OPTIONS) {
      const anchor = anchorPoint(item, option.value);
      const distance = Math.hypot(anchor.x - point.x, anchor.y - point.y);
      if (distance < bestDistance) {
        best = option.value;
        bestDistance = distance;
      }
    }
    return best;
  }

  function nearestConnectableTarget(
    path: PathItem,
    endpointId: PathEndpointId,
    point: Point,
    itemMap: ReadonlyMap<string, CanvasItem>,
  ): ConnectableItem | null {
    const other = otherEndpointOf(path, endpointId);
    const blockedId = other.kind === 'connected' ? other.item_id : null;
    let best: ConnectableItem | null = null;
    let bestDistance = Number.POSITIVE_INFINITY;
    for (const item of itemMap.values()) {
      if (!isConnectableItem(item)) continue;
      if (item.id === blockedId) continue;
      const center = anchorPoint(item, 'center');
      const distance = Math.hypot(center.x - point.x, center.y - point.y);
      if (distance < bestDistance) {
        best = item;
        bestDistance = distance;
      }
    }
    return best;
  }

  function pathEndpointTargetOptions(
    path: PathItem,
    endpointId: PathEndpointId,
  ): ConnectableItem[] {
    const other = otherEndpointOf(path, endpointId);
    const blockedId = other.kind === 'connected' ? other.item_id : null;
    return connectableItems.filter((item) => item.id !== blockedId);
  }

  function connectedEndpointTarget(endpoint: PathEndpoint): CanvasItem | null {
    if (endpoint.kind !== 'connected') return null;
    return sessionStore.items.get(endpoint.item_id) ?? null;
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
  const canvasItemMap = $derived.by(() => new Map<string, CanvasItem>(sessionStore.items));
  const connectableItems = $derived.by((): ConnectableItem[] =>
    Array.from(sessionStore.items.values()).filter(isConnectableItem),
  );
  const singleLineGeometryItem = $derived.by((): LineItem | null => {
    const it = selectionCount === 1 ? selectedItems[0] : undefined;
    return it?.type === 'line' ? it : null;
  });
  const singlePathGeometryItem = $derived.by((): PathItem | null => {
    const it = selectionCount === 1 ? selectedItems[0] : undefined;
    return it?.type === 'path' ? it : null;
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

  /* ── ColorPicker live preview + single-undo commit (ADR-0016 amend ④ D19) ──
   * 드래그 중 onpreview → previewColorLayout 은 store(items) 만 local 갱신(network·
   * history 없음). pointerup 의 oncommit → commitColorLayout 은 드래그 시작 snapshot
   * 으로 1회 applyMutation(history 1 entry, 실패 시 rollback). pointer capture 로
   * 동시에 하나의 picker 만 드래그하므로 세션은 단일 변수로 충분하고, selection 이
   * 바뀌면 stale prior 를 폐기한다. */
  let colorPreviewPrior: CanvasLayout | null = null;

  $effect(() => {
    void selectedIds; // selection 변경/해제 시 진행 중 color preview 세션 무효화.
    colorPreviewPrior = null;
  });

  function previewColorLayout(transform: (cur: CanvasLayout) => CanvasLayout): void {
    if (colorPreviewPrior === null) colorPreviewPrior = sessionStore.layoutSnapshot();
    sessionStore.previewLayoutMutation(transform);
  }

  async function commitColorLayout(
    transform: (cur: CanvasLayout) => CanvasLayout,
    opts: { abortMessage?: string; failMessage?: string },
  ): Promise<void> {
    const prior = colorPreviewPrior;
    colorPreviewPrior = null;
    if (prior !== null) {
      // 드래그 세션 종료 — store 는 이미 preview 로 갱신됨. PRE-drag snapshot 으로
      // 단일 history entry + 실패 시 rollback.
      await sessionStore.applyMutation(transform, {
        ...opts,
        priorSnapshot: prior,
        captureHistory: true,
      });
    } else {
      // 비드래그 commit (text/hex/eyedropper/token click) — 기존 optimistic 경로.
      await sessionStore.optimisticMutation(transform, opts);
    }
  }

  function shapeColorTransform(
    field: 'fill' | 'stroke',
    hex: string,
  ): (cur: CanvasLayout) => CanvasLayout {
    const ids = new Set(selectedIds);
    return (cur) => ({
      ...cur,
      items: cur.items.map((it) => {
        if (!ids.has(it.id)) return it;
        if (it.locked) return it;
        if (it.type !== 'rect' && it.type !== 'ellipse' && it.type !== 'line' && it.type !== 'text' && it.type !== 'path' && it.type !== 'free_draw') return it;
        // line/path/free_draw 에는 fill 이 없음 — 무시.
        if (field === 'fill' && (it.type === 'line' || it.type === 'path' || it.type === 'free_draw')) return it;
        return { ...it, [field]: hex } as CanvasItem;
      }),
    });
  }

  function textColorTransform(hex: string): (cur: CanvasLayout) => CanvasLayout {
    const ids = new Set(selectedIds);
    return (cur) => ({
      ...cur,
      items: cur.items.map((it: CanvasItem) =>
        ids.has(it.id) && isTextStylable(it) && !it.locked
          ? ({ ...it, color: hex } as CanvasItem)
          : it,
      ),
    });
  }

  function noteColorTransform(hex: string): (cur: CanvasLayout) => CanvasLayout {
    const ids = new Set(selectedIds);
    return (cur) => ({
      ...cur,
      items: cur.items.map((it) =>
        ids.has(it.id) && it.type === 'note'
          ? ({ ...(it as NoteItem), color: hex } as NoteItem)
          : it,
      ),
    });
  }

  function previewShapeColor(field: 'fill' | 'stroke', hex: string): void {
    if (selectedItems.length === 0) return;
    previewColorLayout(shapeColorTransform(field, hex));
  }

  function previewTextColor(hex: string): void {
    if (selectedIds.length === 0) return;
    previewColorLayout(textColorTransform(hex));
  }

  function previewNoteColor(hex: string): void {
    if (selectedItems.length === 0) return;
    previewColorLayout(noteColorTransform(hex));
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
    if (selectedItems.length === 0) return;
    const ids = new Set(selectedIds);
    await sessionStore.optimisticMutation(
      (cur) => {
        const itemMap = new Map<string, CanvasItem>(cur.items.map((it) => [it.id, it] as const));
        return {
          ...cur,
          items: cur.items.map((it) => {
            // locked item 은 geometry 변경 skip (D7 정합). z 는 lock 과 무관 — UI 의
            // ordering 만 영향이라 lock 된 item 도 z 갱신 OK.
            if (!ids.has(it.id)) return it;
            if (it.locked && key !== 'z') return it;
            if (it.type === 'path' && key !== 'z') {
              return editPathGeometry(it, key, value, itemMap);
            }
            return { ...it, [key]: value } as CanvasItem;
          }),
        };
      },
      {
        abortMessage: 'Edit aborted — session reconnect failed.',
        failMessage: 'Inspector edit failed',
      },
    );
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

  async function applyLineEndpoint(field: 'x' | 'y' | 'x2' | 'y2', value: number): Promise<void> {
    await broadcastMutation('Edit aborted — session reconnect failed.', (it) => {
      if (it.type !== 'line' || it.locked) return it;
      const next = { ...(it as LineItem), [field]: value };
      const box = lineBoxFromEndpoints(
        { x: next.x, y: next.y },
        { x: next.x2, y: next.y2 },
      );
      return { ...next, w: box.w, h: box.h } as LineItem;
    });
  }

  async function applyLineHead(field: 'head_from' | 'head_to', value: Head): Promise<void> {
    rememberPathStyle(field === 'head_from' ? { head_from: value } : { head_to: value });
    await broadcastMutation('Line head edit aborted — session reconnect failed.', (it) => {
      if (it.type !== 'line' || it.locked) return it;
      return { ...(it as LineItem), [field]: value } as LineItem;
    });
  }

  async function applyPathMutation(
    transform: (path: PathItem, itemMap: ReadonlyMap<string, CanvasItem>) => PathItem,
    failMessage: string,
  ): Promise<void> {
    if (selectedItems.length === 0) return;
    const ids = new Set(selectedIds);
    await sessionStore.optimisticMutation(
      (cur) => {
        const itemMap = new Map<string, CanvasItem>(cur.items.map((it) => [it.id, it] as const));
        return {
          ...cur,
          items: cur.items.map((it) => {
            if (!ids.has(it.id) || it.type !== 'path' || it.locked) return it;
            return transform(it, itemMap);
          }),
        };
      },
      {
        abortMessage: 'Path edit aborted — session reconnect failed.',
        failMessage,
      },
    );
  }

  async function applyPathRouting(routing: PathRouting): Promise<void> {
    rememberPathStyle({ routing });
    await applyPathMutation(
      (path, itemMap) => updatePathBBoxCache({ ...path, routing }, itemMap),
      'Path routing failed',
    );
  }

  async function applyPathHead(field: 'head_from' | 'head_to', value: Head): Promise<void> {
    rememberPathStyle(field === 'head_from' ? { head_from: value } : { head_to: value });
    await applyPathMutation(
      (path, itemMap) => updatePathBBoxCache({ ...path, [field]: value }, itemMap),
      'Path head edit failed',
    );
  }

  async function applySharedHead(field: 'head_from' | 'head_to', value: Head): Promise<void> {
    if (selectedItems.length === 0) return;
    rememberPathStyle(field === 'head_from' ? { head_from: value } : { head_to: value });
    const ids = new Set(selectedIds);
    await sessionStore.optimisticMutation(
      (cur) => {
        const itemMap = new Map<string, CanvasItem>(cur.items.map((it) => [it.id, it] as const));
        return {
          ...cur,
          items: cur.items.map((it) => {
            if (!ids.has(it.id) || it.locked) return it;
            if (it.type === 'line') {
              return { ...it, [field]: value } as LineItem;
            }
            if (it.type === 'path') {
              return updatePathBBoxCache({ ...it, [field]: value } as PathItem, itemMap);
            }
            return it;
          }),
        };
      },
      {
        abortMessage: 'Head edit aborted — session reconnect failed.',
        failMessage: 'Head edit failed',
      },
    );
  }

  async function applyPathEndpointKind(
    endpointId: PathEndpointId,
    kind: PathEndpoint['kind'],
  ): Promise<void> {
    await applyPathMutation((path, itemMap) => {
      const endpoint = endpointOf(path, endpointId);
      if (endpoint.kind === kind) return path;
      const point = resolveEndpoint(endpoint, itemMap);
      if (kind === 'free') {
        return updatePathBBoxCache(
          setPathEndpoint(path, endpointId, { kind: 'free', point }),
          itemMap,
        );
      }
      const target = nearestConnectableTarget(path, endpointId, point, itemMap);
      if (target === null) return path;
      const anchor = nearestAnchor(target, point);
      return updatePathBBoxCache(
        setPathEndpoint(path, endpointId, {
          kind: 'connected',
          item_id: target.id,
          anchor,
          fallback_point: connectedEndpointPoint(target, anchor),
        }),
        itemMap,
      );
    }, 'Path endpoint edit failed');
  }

  async function applyPathEndpointTarget(
    endpointId: PathEndpointId,
    targetId: string,
  ): Promise<void> {
    await applyPathMutation((path, itemMap) => {
      const target = itemMap.get(targetId);
      if (target === undefined || !isConnectableItem(target)) return path;
      const other = otherEndpointOf(path, endpointId);
      if (other.kind === 'connected' && other.item_id === target.id) return path;
      const point = resolveEndpoint(endpointOf(path, endpointId), itemMap);
      const anchor = nearestAnchor(target, point);
      return updatePathBBoxCache(
        setPathEndpoint(path, endpointId, {
          kind: 'connected',
          item_id: target.id,
          anchor,
          fallback_point: connectedEndpointPoint(target, anchor),
        }),
        itemMap,
      );
    }, 'Path endpoint target failed');
  }

  async function applyPathEndpointAnchor(
    endpointId: PathEndpointId,
    anchor: Anchor,
  ): Promise<void> {
    await applyPathMutation((path, itemMap) => {
      const endpoint = endpointOf(path, endpointId);
      if (endpoint.kind !== 'connected') return path;
      const target = itemMap.get(endpoint.item_id);
      if (target === undefined || !isConnectableItem(target)) return path;
      const offset = endpoint.offset;
      return updatePathBBoxCache(
        setPathEndpoint(path, endpointId, {
          ...endpoint,
          anchor,
          fallback_point: connectedEndpointPoint(target, anchor, offset),
        }),
        itemMap,
      );
    }, 'Path endpoint anchor failed');
  }

  async function applyPathEndpointOffset(
    endpointId: PathEndpointId,
    axis: keyof Point,
    value: number,
  ): Promise<void> {
    await applyPathMutation((path, itemMap) => {
      const endpoint = endpointOf(path, endpointId);
      if (endpoint.kind !== 'connected') return path;
      const target = itemMap.get(endpoint.item_id);
      if (target === undefined || !isConnectableItem(target)) return path;
      const current = endpointOffset(endpoint);
      const offset = normalizeEndpointOffset({ ...current, [axis]: value });
      return updatePathBBoxCache(
        setPathEndpoint(path, endpointId, {
          ...endpoint,
          offset,
          fallback_point: connectedEndpointPoint(target, endpoint.anchor, offset),
        }),
        itemMap,
      );
    }, 'Path endpoint offset failed');
  }

  async function applyPathEndpointPoint(
    endpointId: PathEndpointId,
    axis: keyof Point,
    value: number,
  ): Promise<void> {
    await applyPathMutation((path, itemMap) => {
      const endpoint = endpointOf(path, endpointId);
      const point = resolveEndpoint(endpoint, itemMap);
      return updatePathBBoxCache(
        setPathEndpoint(path, endpointId, {
          kind: 'free',
          point: { ...point, [axis]: value },
        }),
        itemMap,
      );
    }, 'Path endpoint point failed');
  }

  async function applyNoteColor(hex: string): Promise<void> {
    if (selectedItems.length === 0) return;
    await commitColorLayout(noteColorTransform(hex), {
      abortMessage: 'Color change aborted — session reconnect failed.',
      failMessage: 'Inspector edit failed',
    });
  }

  function workspaceRootOrToast(): string | null {
    const root = sessionStore.effectiveWorkspaceRoot;
    if (root.length > 0) return root;
    toastStore.show({
      message: 'Workspace root is not available yet.',
      tone: 'error',
    });
    return null;
  }

  function itemCopyPath(item: CanvasItem): string | null {
    if (item.type === 'file_path') {
      const path = item.path.trim();
      return path.length > 0 ? path : null;
    }
    if ((item.type === 'image' || item.type === 'document') && (item.path ?? '').length > 0) {
      return resolveWorkspacePath(sessionStore.effectiveWorkspaceRoot, item.path ?? '');
    }
    return null;
  }

  async function copyInspectorPath(path: string | null): Promise<void> {
    if (path === null) return;
    const result = await copyTextToSystemClipboard(path);
    toastStore.show({
      message: result.ok ? 'Copied file path.' : (result.reason ?? 'Copy failed.'),
      tone: result.ok ? 'success' : 'error',
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

  function changeImageFromInspector(): void {
    const item = sessionItem;
    if (item?.type !== 'image' || item.locked) return;
    const workspaceRoot = workspaceRootOrToast();
    if (workspaceRoot === null) return;
    filePicker.openFor(workspaceRoot, (absolutePath) => {
      const nextPath = workspaceRelativePath(workspaceRoot, absolutePath);
      if (nextPath === null) {
        toastStore.show({
          message: 'Image files must be inside the active project workspace.',
          tone: 'error',
        });
        return;
      }
      void sessionStore.optimisticMutation(
        (cur) => ({
          ...cur,
          items: cur.items.map((it: CanvasItem) =>
            it.id === item.id && it.type === 'image'
              ? ({
                  ...it,
                  label: basename(absolutePath),
                  path: nextPath,
                  asset_id: undefined,
                  mime: guessMimeFromPath(absolutePath),
                  original_w: undefined,
                  original_h: undefined,
                } as CanvasItem)
              : it,
          ),
        }),
        {
          abortMessage: 'Image change aborted — session reconnect failed.',
          failMessage: 'Image change failed',
        },
      );
    }, {
      accept: { extensions: [...IMAGE_EXTENSIONS], description: 'image files' },
      rootKind: 'workspace',
      rootPath: workspaceRoot,
    });
  }

  function changeDocumentFromInspector(): void {
    const item = sessionItem;
    if (item?.type !== 'document' || item.locked) return;
    const workspaceRoot = workspaceRootOrToast();
    if (workspaceRoot === null) return;
    filePicker.openFor(workspaceRoot, (absolutePath) => {
      const nextPath = workspaceRelativePath(workspaceRoot, absolutePath);
      if (nextPath === null) {
        toastStore.show({
          message: 'Document files must be inside the active project workspace.',
          tone: 'error',
        });
        return;
      }
      const nextFileName = basename(absolutePath);
      void sessionStore.optimisticMutation(
        (cur) => ({
          ...cur,
          items: cur.items.map((it: CanvasItem) =>
            it.id === item.id && it.type === 'document'
              ? ({
                  ...it,
                  path: nextPath,
                  asset_id: undefined,
                  label: workspaceFileStem(nextFileName),
                  file_name: nextFileName,
                  mime: guessMimeFromPath(absolutePath),
                  size_bytes: undefined,
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
    }, {
      accept: { extensions: [...DOCUMENT_EXTENSIONS], description: 'document files' },
      rootKind: 'workspace',
      rootPath: workspaceRoot,
    });
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
      (cur) => {
        const movedById = new Map<string, CanvasItem>();
        for (const it of cur.items) {
          const m = moves.get(it.id);
          if (m === undefined) continue;
          movedById.set(it.id, moveItemByDelta(it, m.x - it.x, m.y - it.y));
        }
        return {
          ...cur,
          items: mergeMovedItemsWithPathCaches(cur.items, movedById),
        };
      },
      { abortMessage, failMessage: 'Align failed' },
    );
  }

  function moveItemByDelta(item: CanvasItem, dx: number, dy: number): CanvasItem {
    if (item.type === 'line') {
      const nextP1 = { x: item.x + dx, y: item.y + dy };
      const nextP2 = { x: item.x2 + dx, y: item.y2 + dy };
      const nextBox = lineBoxFromEndpoints(nextP1, nextP2);
      return {
        ...item,
        x: nextP1.x,
        y: nextP1.y,
        x2: nextP2.x,
        y2: nextP2.y,
        w: nextBox.w,
        h: nextBox.h,
      } as CanvasItem;
    }
    if (item.type === 'free_draw') {
      return {
        ...item,
        x: item.x + dx,
        y: item.y + dy,
        points: item.points.map((point) => ({ x: point.x + dx, y: point.y + dy })),
      } as CanvasItem;
    }
    if (item.type === 'path') {
      return translatePath(item, dx, dy) as CanvasItem;
    }
    return { ...item, x: item.x + dx, y: item.y + dy } as CanvasItem;
  }

  function mergeMovedItemsWithPathCaches(
    items: readonly CanvasItem[],
    movedById: ReadonlyMap<string, CanvasItem>,
  ): CanvasItem[] {
    const merged = items.map((item) => movedById.get(item.id) ?? item);
    const itemMap = new Map(merged.map((item) => [item.id, item] as const));
    const movedIds = new Set(movedById.keys());
    return merged.map((item) =>
      item.type === 'path' && (movedById.has(item.id) || isPathConnectedToAny(item, movedIds))
        ? updatePathBBoxCache(item, itemMap)
        : item,
    );
  }

  function groupMoveDeltas(
    groupDeltas: ReadonlyMap<string, MoveDelta>,
    itemsMap: ReadonlyMap<string, CanvasItem>,
    groupsMap: Map<string, Group>,
  ): Map<string, CanvasItem> {
    const groupsArr = Array.from(groupsMap.values());
    const itemsArr = Array.from(itemsMap.values());
    const movedById = new Map<string, CanvasItem>();
    for (const group of topLevelSelectedGroups(selectedGroups)) {
      const delta = groupDeltas.get(group.id);
      if (delta === undefined) continue;
      for (const item of descendantItems(group.id, groupsArr, itemsArr)) {
        if (effectiveLocked(item.locked, item.parent_id, groupsMap)) continue;
        if (movedById.has(item.id)) continue;
        movedById.set(item.id, moveItemByDelta(item, delta.dx, delta.dy));
      }
    }
    return movedById;
  }

  async function applyGroupAlignDeltas(
    groupDeltas: ReadonlyMap<string, MoveDelta>,
    abortMessage: string,
  ): Promise<void> {
    if (groupDeltas.size === 0) return;
    await sessionStore.optimisticMutation(
      (cur) => {
        const itemMap = new Map<string, CanvasItem>(cur.items.map((item) => [item.id, item] as const));
        const groupMap = new Map<string, Group>(cur.groups.map((group) => [group.id, group] as const));
        const movedById = groupMoveDeltas(groupDeltas, itemMap, groupMap);
        return {
          ...cur,
          items: mergeMovedItemsWithPathCaches(cur.items, movedById),
        };
      },
      { abortMessage, failMessage: 'Group align failed' },
    );
  }

  async function onAlign(mode: AlignMode): Promise<void> {
    if (isGroupOnlySelection) {
      const moves = alignBoxes(selectedGroupAlignTargets, mode);
      await applyGroupAlignDeltas(moves, 'Group align aborted — session reconnect failed.');
      return;
    }
    const moves = alignItems(selectedItems, mode);
    await applyAlignMutation(moves, 'Align aborted — session reconnect failed.');
  }

  async function onDistribute(mode: DistributeMode): Promise<void> {
    if (isGroupOnlySelection) {
      const moves = distributeBoxes(selectedGroupAlignTargets, mode);
      await applyGroupAlignDeltas(moves, 'Group distribute aborted — session reconnect failed.');
      return;
    }
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
    if (field === 'stroke' && selectedItems.some((it) => it.type === 'line' || it.type === 'path')) {
      rememberPathStyle({ stroke: hex });
    }
    await commitColorLayout(shapeColorTransform(field, hex), {
      abortMessage: 'Color change aborted — session reconnect failed.',
      failMessage: 'Color change failed',
    });
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
    if (selectedItems.some((it) => it.type === 'line' || it.type === 'path')) {
      rememberPathStyle({ stroke_width: clamped });
    }
    const ids = new Set(selectedIds);
    await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it) => {
          if (!ids.has(it.id)) return it;
          if (it.locked) return it;
          if (it.type !== 'rect' && it.type !== 'ellipse' && it.type !== 'line' && it.type !== 'text' && it.type !== 'path' && it.type !== 'free_draw') return it;
          const next = { ...it, stroke_width: clamped } as CanvasItem;
          if (next.type === 'path') {
            const itemMap = new Map<string, CanvasItem>(cur.items.map((curItem) => [curItem.id, curItem] as const));
            return updatePathBBoxCache(next, itemMap);
          }
          return next;
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
    if (selectedItems.some((it) => it.type === 'line' || it.type === 'path')) {
      rememberPathStyle({ stroke_dash: dash ?? 'solid' });
    }
    const ids = new Set(selectedIds);
    await sessionStore.optimisticMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it) => {
          if (!ids.has(it.id)) return it;
          if (it.locked) return it;
          if (it.type !== 'rect' && it.type !== 'ellipse' && it.type !== 'line' && it.type !== 'text' && it.type !== 'path') return it;
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
    if (selectedIds.length === 0) return;
    await commitColorLayout(textColorTransform(hex), { failMessage: 'Text color failed' });
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

{#snippet horizontalAlignIcon(value: TextAlign)}
  {#if value === 'left'}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
      <line x1="4" y1="6" x2="20" y2="6" />
      <line x1="4" y1="12" x2="14" y2="12" />
      <line x1="4" y1="18" x2="18" y2="18" />
    </svg>
  {:else if value === 'center'}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
      <line x1="4" y1="6" x2="20" y2="6" />
      <line x1="7" y1="12" x2="17" y2="12" />
      <line x1="5" y1="18" x2="19" y2="18" />
    </svg>
  {:else}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
      <line x1="4" y1="6" x2="20" y2="6" />
      <line x1="10" y1="12" x2="20" y2="12" />
      <line x1="6" y1="18" x2="20" y2="18" />
    </svg>
  {/if}
{/snippet}

{#snippet verticalAlignIcon(value: TextVerticalAlign)}
  {#if value === 'top'}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
      <line x1="5" y1="5" x2="19" y2="5" />
      <line x1="8" y1="10" x2="16" y2="10" />
      <line x1="10" y1="15" x2="14" y2="15" />
    </svg>
  {:else if value === 'middle'}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
      <line x1="6" y1="7" x2="18" y2="7" />
      <line x1="4" y1="12" x2="20" y2="12" />
      <line x1="6" y1="17" x2="18" y2="17" />
    </svg>
  {:else}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
      <line x1="10" y1="9" x2="14" y2="9" />
      <line x1="8" y1="14" x2="16" y2="14" />
      <line x1="5" y1="19" x2="19" y2="19" />
    </svg>
  {/if}
{/snippet}

{#snippet alignmentControls(showDistribute: boolean)}
  <!-- ADR-0027 D4/D9 + ADR-0010 D41 — shared item/group alignment controls. -->
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
    {#if showDistribute}
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
{/snippet}

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

    {:else if isMultiGroupSelection}
      <div class="multi-header">
        <span class="multi-count mono">{selectionCount}</span>
        <span class="multi-label">groups selected</span>
      </div>

      <section class="prop-section">
        <div class="prop-head"><h4>State</h4></div>
        <div class="state-row" role="group" aria-label="Group state">
          <button
            type="button"
            class="state-btn"
            class:active={selectedGroupVisibilityState === true}
            class:mixed={selectedGroupVisibilityState === null}
            aria-pressed={selectedGroupVisibilityState === true}
            aria-label={selectedGroupVisibilityState === null ? 'Show all groups' : selectedGroupVisibilityState ? 'Hide groups' : 'Show groups'}
            title={selectedGroupVisibilityState === null ? 'Visibility mixed (click to show all groups)' : selectedGroupVisibilityState ? 'Visible (click to hide)' : 'Hidden (click to show)'}
            onclick={() => void applySelectedGroupVisibility(!(selectedGroupVisibilityState ?? false))}
          >
            {#if selectedGroupVisibilityState === null}
              <svg class="mixed-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <rect x="4" y="4" width="16" height="16" rx="3" stroke="currentColor" stroke-width="1.8"/>
                <path d="M8 12h8" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
              </svg>
            {:else if selectedGroupVisibilityState === true}
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
            class:active={selectedGroupLockedState === true}
            class:mixed={selectedGroupLockedState === null}
            aria-pressed={selectedGroupLockedState === true}
            aria-label={selectedGroupLockedState === null ? 'Lock all groups' : selectedGroupLockedState ? 'Unlock groups' : 'Lock groups'}
            title={selectedGroupLockedState === null ? 'Lock mixed (click to lock all groups)' : selectedGroupLockedState ? 'Locked (click to unlock)' : 'Unlocked (click to lock)'}
            onclick={() => void applySelectedGroupLocked(!(selectedGroupLockedState ?? false))}
          >
            {#if selectedGroupLockedState === null}
              <svg class="mixed-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <rect x="4" y="4" width="16" height="16" rx="3" stroke="currentColor" stroke-width="1.8"/>
                <path d="M8 12h8" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
              </svg>
            {:else if selectedGroupLockedState === true}
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

      {#if canAlignSelectedGroups}
        <section class="prop-section">
          <div class="prop-head"><h4>Align</h4></div>
          {@render alignmentControls(canDistributeSelectedGroups)}
        </section>
      {/if}

      <section class="prop-section">
        <div class="prop-head"><h4>Contents</h4></div>
        <div class="prop-row full">
          <div class="display-row">
            <span class="k">direct</span>
            <span class="display-val mono">{selectedGroupStats.direct}</span>
          </div>
        </div>
        <div class="prop-row full">
          <div class="display-row">
            <span class="k">total</span>
            <span class="display-val mono">{selectedGroupStats.total}</span>
          </div>
        </div>
      </section>

    {:else if selectedPanel === null}
      <PanelEmptyState
        icon="inspect"
        lead="No canvas item selected"
        description="Select an item on the canvas to inspect its properties."
      />
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
        {#if singleLineGeometryItem !== null}
          {@const line = singleLineGeometryItem}
          <div class="endpoint-geometry-row">
            <span class="endpoint-label">from</span>
            <InspectorField
              type="number"
              k="X"
              value={String(Math.round(line.x))}
              mixed={false}
              ariaLabel="line from x"
              disabled={line.locked}
              live={true}
              oncommit={(s) => void applyLineEndpoint('x', Number(s))}
            />
            <InspectorField
              type="number"
              k="Y"
              value={String(Math.round(line.y))}
              mixed={false}
              ariaLabel="line from y"
              disabled={line.locked}
              live={true}
              oncommit={(s) => void applyLineEndpoint('y', Number(s))}
            />
          </div>
          <div class="endpoint-geometry-row">
            <span class="endpoint-label">to</span>
            <InspectorField
              type="number"
              k="X"
              value={String(Math.round(line.x2))}
              mixed={false}
              ariaLabel="line to x"
              disabled={line.locked}
              live={true}
              oncommit={(s) => void applyLineEndpoint('x2', Number(s))}
            />
            <InspectorField
              type="number"
              k="Y"
              value={String(Math.round(line.y2))}
              mixed={false}
              ariaLabel="line to y"
              disabled={line.locked}
              live={true}
              oncommit={(s) => void applyLineEndpoint('y2', Number(s))}
            />
          </div>
        {:else if singlePathGeometryItem !== null}
          {@const path = singlePathGeometryItem}
          {@const fromPoint = resolveEndpoint(path.from, canvasItemMap)}
          {@const toPoint = resolveEndpoint(path.to, canvasItemMap)}
          <div class="endpoint-geometry-row">
            <span class="endpoint-label">from</span>
            <InspectorField
              type="number"
              k="X"
              value={String(Math.round(fromPoint.x))}
              mixed={false}
              ariaLabel="path from x"
              disabled={path.locked}
              live={true}
              oncommit={(s) => void applyPathEndpointPoint('from', 'x', Number(s))}
            />
            <InspectorField
              type="number"
              k="Y"
              value={String(Math.round(fromPoint.y))}
              mixed={false}
              ariaLabel="path from y"
              disabled={path.locked}
              live={true}
              oncommit={(s) => void applyPathEndpointPoint('from', 'y', Number(s))}
            />
          </div>
          <div class="endpoint-geometry-row">
            <span class="endpoint-label">to</span>
            <InspectorField
              type="number"
              k="X"
              value={String(Math.round(toPoint.x))}
              mixed={false}
              ariaLabel="path to x"
              disabled={path.locked}
              live={true}
              oncommit={(s) => void applyPathEndpointPoint('to', 'x', Number(s))}
            />
            <InspectorField
              type="number"
              k="Y"
              value={String(Math.round(toPoint.y))}
              mixed={false}
              ariaLabel="path to y"
              disabled={path.locked}
              live={true}
              oncommit={(s) => void applyPathEndpointPoint('to', 'y', Number(s))}
            />
          </div>
        {:else}
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
        {/if}
        {#if selectionCount >= 2}
          {@render alignmentControls(selectionCount >= 3)}
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

      {#if selectionCount > 1 && hasSharedStyleSection}
        <section class="prop-section">
          <div class="prop-head"><h4>Shared Style</h4></div>

          {#if hasSharedTextStyle}
            <div class="fig-group is-on">
              <div class="fig-group-head">
                <span class="k">text</span>
              </div>
              <div class="fig-group-body">
                <div class="font-dropdown">
                  <Dropdown
                    placement="bottom-start"
                    menuClass={INSPECTOR_DROPDOWN_MENU_CLASS}
                    menuInsetLeft={INSPECTOR_DROPDOWN_LABEL_INSET}
                    matchTriggerWidth={true}
                    menuGap={INSPECTOR_DROPDOWN_GAP}
                  >
                    {#snippet trigger({ toggle })}
                      <button
                        type="button"
                        class="font-trigger"
                        disabled={textStyleLocked}
                        aria-label="Font family"
                        title="Font family"
                        onclick={toggle}
                      >
                        <span class="font-label">font</span>
                        <span class="font-value" class:mixed={sharedTextFontFamily === 'Mixed'}>
                          {fontValueLabel(sharedTextFontFamily)}
                        </span>
                        <DropdownChevron />
                      </button>
                    {/snippet}
                    {#snippet menu({ close })}
                      {#each FONT_FAMILIES as f (f)}
                        <button
                          type="button"
                          class="font-option font-preview-{f}"
                          class:selected={sharedTextFontFamily === f}
                          disabled={textStyleLocked}
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
                  value={mixedNumberValue(sharedTextFontSize)}
                  mixed={sharedTextFontSize === 'Mixed'}
                  ariaLabel="Font size"
                  disabled={textStyleLocked}
                  live={true}
                  oncommit={(s) => void applyTextFontSize(Number(s))}
                />
                <ColorPicker
                  value={mixedColorValue(sharedTextColor, 'var(--color-fg)')}
                  mixed={sharedTextColor === 'Mixed'}
                  onpreview={(hex) => previewTextColor(hex)}
                  allowAlpha={true}
                  disabled={textStyleLocked}
                  oncommit={(hex) => void applyTextColor(hex)}
                />
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">weight</span>
                  <div class="segmented-control" role="group" aria-label="Font weight">
                    <button type="button" class="seg-btn weight-btn weight-light" class:active={sharedTextWeight === 'light'} aria-pressed={sharedTextWeight === 'light'} title="Light (300)" aria-label="Light weight" disabled={textStyleLocked} onclick={() => void applyTextFontWeight('light')}>L</button>
                    <button type="button" class="seg-btn weight-btn weight-normal" class:active={sharedTextWeight === 'normal'} aria-pressed={sharedTextWeight === 'normal'} title="Normal (400)" aria-label="Normal weight" disabled={textStyleLocked} onclick={() => void applyTextFontWeight('normal')}>N</button>
                    <button type="button" class="seg-btn weight-btn weight-bold" class:active={sharedTextWeight === 'bold'} aria-pressed={sharedTextWeight === 'bold'} title="Bold (700)" aria-label="Bold weight" disabled={textStyleLocked} onclick={() => void applyTextFontWeight('bold')}>B</button>
                  </div>
                </div>
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">style</span>
                  <div class="segmented-control multi" role="group" aria-label="Text style">
                    <button type="button" class="seg-btn style-btn style-italic" class:active={sharedTextItalic === true} aria-pressed={sharedTextItalic === true} title="Italic" aria-label="Toggle italic" disabled={textStyleLocked} onclick={() => void applyTextBoolean('italic', toggleFromMixed(sharedTextItalic))}>I</button>
                    <button type="button" class="seg-btn style-btn style-underline" class:active={sharedTextUnderline === true} aria-pressed={sharedTextUnderline === true} title="Underline" aria-label="Toggle underline" disabled={textStyleLocked} onclick={() => void applyTextBoolean('underline', toggleFromMixed(sharedTextUnderline))}>U</button>
                    <button type="button" class="seg-btn style-btn style-strike" class:active={sharedTextStrike === true} aria-pressed={sharedTextStrike === true} title="Strikethrough" aria-label="Toggle strikethrough" disabled={textStyleLocked} onclick={() => void applyTextBoolean('strikethrough', toggleFromMixed(sharedTextStrike))}>S</button>
                  </div>
                </div>
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">align</span>
                  <div class="segmented-control icon-segments" role="group" aria-label="Horizontal alignment">
                    <button type="button" class="seg-btn" class:active={sharedTextAlign === 'left'} aria-pressed={sharedTextAlign === 'left'} title="Align left" aria-label="Align left" disabled={textStyleLocked} onclick={() => void applyTextAlign('left')}>{@render horizontalAlignIcon('left')}</button>
                    <button type="button" class="seg-btn" class:active={sharedTextAlign === 'center'} aria-pressed={sharedTextAlign === 'center'} title="Align center" aria-label="Align center" disabled={textStyleLocked} onclick={() => void applyTextAlign('center')}>{@render horizontalAlignIcon('center')}</button>
                    <button type="button" class="seg-btn" class:active={sharedTextAlign === 'right'} aria-pressed={sharedTextAlign === 'right'} title="Align right" aria-label="Align right" disabled={textStyleLocked} onclick={() => void applyTextAlign('right')}>{@render horizontalAlignIcon('right')}</button>
                  </div>
                </div>
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">v-align</span>
                  <div class="segmented-control icon-segments" role="group" aria-label="Vertical alignment">
                    <button type="button" class="seg-btn" class:active={sharedTextVerticalAlign === 'top'} aria-pressed={sharedTextVerticalAlign === 'top'} title="Align top" aria-label="Align top" disabled={textStyleLocked} onclick={() => void applyTextVerticalAlign('top')}>{@render verticalAlignIcon('top')}</button>
                    <button type="button" class="seg-btn" class:active={sharedTextVerticalAlign === 'middle'} aria-pressed={sharedTextVerticalAlign === 'middle'} title="Align middle" aria-label="Align middle" disabled={textStyleLocked} onclick={() => void applyTextVerticalAlign('middle')}>{@render verticalAlignIcon('middle')}</button>
                    <button type="button" class="seg-btn" class:active={sharedTextVerticalAlign === 'bottom'} aria-pressed={sharedTextVerticalAlign === 'bottom'} title="Align bottom" aria-label="Align bottom" disabled={textStyleLocked} onclick={() => void applyTextVerticalAlign('bottom')}>{@render verticalAlignIcon('bottom')}</button>
                  </div>
                </div>
              </div>
            </div>
          {/if}

          {#if hasSharedFillStyle}
            <div class="fig-group" class:is-on={sharedFillEnabled !== false}>
              <div class="fig-group-head">
                <span class="k">fill</span>
                <span class="fig-spacer"></span>
                <Toggle
                  checked={sharedFillEnabled === true}
                  disabled={boxStyleLocked}
                  ariaLabel="Toggle fill"
                  onchange={(next) => void applyShapeBoolean('fill_enabled', next)}
                />
              </div>
              {#if sharedFillEnabled !== false}
                <div class="fig-group-body">
                  <ColorPicker
                    value={mixedColorValue(sharedFillColor, 'var(--color-surface)')}
                    mixed={sharedFillColor === 'Mixed'}
                    onpreview={(hex) => previewShapeColor('fill', hex)}
                    allowAlpha={true}
                    allowTransparent={true}
                    disabled={boxStyleLocked}
                    oncommit={(hex) => void applyShapeColor('fill', hex)}
                  />
                </div>
              {/if}
            </div>
          {/if}

          {#if hasSharedPathRouting}
            <div class="fig-group is-on">
              <div class="fig-group-head">
                <span class="k">path</span>
              </div>
              <div class="fig-group-body">
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">routing</span>
                  <div class="segmented-control icon-segments routing-segments" role="group" aria-label="Path routing">
                    {#each ROUTING_OPTIONS as option (option.value)}
                      <button
                        type="button"
                        class="seg-btn"
                        class:active={sharedPathRouting === option.value}
                        aria-pressed={sharedPathRouting === option.value}
                        aria-label={option.label}
                        title={option.label}
                        disabled={pathStyleLocked}
                        onclick={() => void applyPathRouting(option.value)}
                      >
                        <RoutingIcon routing={option.value} />
                      </button>
                    {/each}
                  </div>
                </div>
              </div>
            </div>
          {/if}

          {#if hasSharedStrokeStyle}
            {@const showStrokeBody = !canToggleSharedStroke || sharedStrokeEnabled !== false}
            <div class="fig-group" class:is-on={showStrokeBody}>
              <div class="fig-group-head">
                <span class="k">stroke</span>
                <span class="fig-spacer"></span>
                {#if canToggleSharedStroke}
                  <Toggle
                    checked={sharedStrokeEnabled === true}
                    disabled={boxStyleLocked}
                    ariaLabel="Toggle stroke"
                    onchange={(next) => void applyShapeBoolean('stroke_enabled', next)}
                  />
                {/if}
              </div>
              {#if showStrokeBody}
                <div class="fig-group-body">
                  <ColorPicker
                    value={mixedColorValue(sharedStrokeColor, 'var(--color-fg)')}
                    mixed={sharedStrokeColor === 'Mixed'}
                    onpreview={(hex) => previewShapeColor('stroke', hex)}
                    allowAlpha={true}
                    disabled={strokeStyleLocked}
                    oncommit={(hex) => void applyShapeColor('stroke', hex)}
                  />
                  <InspectorField
                    type="number"
                    k="width"
                    value={mixedNumberValue(sharedStrokeWidth)}
                    mixed={sharedStrokeWidth === 'Mixed'}
                    ariaLabel="Stroke width"
                    disabled={strokeStyleLocked}
                    live={true}
                    oncommit={(s) => void applyShapeStrokeWidth(Number(s))}
                  />
                  {#if strokeDashStyleItems.length > 0}
                    <DashSegments
                      value={concreteDash(sharedStrokeDash)}
                      mixed={sharedStrokeDash === 'Mixed'}
                      disabled={strokeDashStyleLocked}
                      onpick={(next) => void applyShapeDash(next)}
                    />
                  {/if}
                </div>
              {/if}
            </div>
          {/if}

          {#if hasSharedHeadStyle}
            <div class="fig-group is-on">
              <div class="fig-group-head">
                <span class="k">heads</span>
              </div>
              <div class="fig-group-body">
                <div class="font-dropdown">
                  <Dropdown
                    placement="bottom-start"
                    menuClass={INSPECTOR_DROPDOWN_MENU_CLASS}
                    menuInsetLeft={INSPECTOR_DROPDOWN_LABEL_INSET}
                    matchTriggerWidth={true}
                    menuGap={INSPECTOR_DROPDOWN_GAP}
                  >
                    {#snippet trigger({ toggle })}
                      <button
                        type="button"
                        class="font-trigger"
                        disabled={headStyleLocked}
                        aria-label="Start head"
                        title={headValueLabel(sharedHeadFrom)}
                        onclick={toggle}
                      >
                        <span class="font-label">from</span>
                        <span class="font-value head-value" class:mixed={sharedHeadFrom === 'Mixed'}>
                          {#if sharedHeadFrom === 'Mixed'}
                            Mixed
                          {:else}
                            <HeadIcon head={concreteHead(sharedHeadFrom)} />
                          {/if}
                        </span>
                        <DropdownChevron />
                      </button>
                    {/snippet}
                    {#snippet menu({ close })}
                      {#each HEAD_OPTIONS as option (option.value)}
                        <button
                          type="button"
                          class="font-option"
                          class:selected={sharedHeadFrom === option.value}
                          disabled={headStyleLocked}
                          aria-label={option.label}
                          title={option.label}
                          onclick={() => {
                            void applySharedHead('head_from', option.value);
                            close();
                          }}
                        >
                          <HeadIcon head={option.value} />
                        </button>
                      {/each}
                    {/snippet}
                  </Dropdown>
                </div>
                <div class="font-dropdown">
                  <Dropdown
                    placement="bottom-start"
                    menuClass={INSPECTOR_DROPDOWN_MENU_CLASS}
                    menuInsetLeft={INSPECTOR_DROPDOWN_LABEL_INSET}
                    matchTriggerWidth={true}
                    menuGap={INSPECTOR_DROPDOWN_GAP}
                  >
                    {#snippet trigger({ toggle })}
                      <button
                        type="button"
                        class="font-trigger"
                        disabled={headStyleLocked}
                        aria-label="End head"
                        title={headValueLabel(sharedHeadTo)}
                        onclick={toggle}
                      >
                        <span class="font-label">to</span>
                        <span class="font-value head-value" class:mixed={sharedHeadTo === 'Mixed'}>
                          {#if sharedHeadTo === 'Mixed'}
                            Mixed
                          {:else}
                            <HeadIcon head={concreteHead(sharedHeadTo)} />
                          {/if}
                        </span>
                        <DropdownChevron />
                      </button>
                    {/snippet}
                    {#snippet menu({ close })}
                      {#each HEAD_OPTIONS as option (option.value)}
                        <button
                          type="button"
                          class="font-option"
                          class:selected={sharedHeadTo === option.value}
                          disabled={headStyleLocked}
                          aria-label={option.label}
                          title={option.label}
                          onclick={() => {
                            void applySharedHead('head_to', option.value);
                            close();
                          }}
                        >
                          <HeadIcon head={option.value} />
                        </button>
                      {/each}
                    {/snippet}
                  </Dropdown>
                </div>
              </div>
            </div>
          {/if}

          {#if hasSharedRoundedStyle}
            <div class="fig-group" class:is-on={sharedRounded === true}>
              <div class="fig-group-head">
                <span class="k">rounded</span>
                <span class="fig-spacer"></span>
                <Toggle
                  checked={sharedRounded === true}
                  disabled={roundedStyleLocked}
                  ariaLabel="Toggle rounded corners"
                  onchange={(next) => void applyShapeBoolean('corner_rounded', next)}
                />
              </div>
            </div>
          {/if}
        </section>
      {/if}

      {#if sessionItem !== null && selectionCount === 1 && (sessionItem.type === 'rect' || sessionItem.type === 'ellipse' || sessionItem.type === 'line' || sessionItem.type === 'path' || sessionItem.type === 'text' || sessionItem.type === 'note' || sessionItem.type === 'file_path' || sessionItem.type === 'image' || sessionItem.type === 'document' || sessionItem.type === 'snippets')}
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
                    onpreview={(hex) => previewShapeColor('fill', hex)}
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
                    onpreview={(hex) => previewShapeColor('stroke', hex)}
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
                  <Dropdown
                    placement="bottom-start"
                    menuClass={INSPECTOR_DROPDOWN_MENU_CLASS}
                    menuInsetLeft={INSPECTOR_DROPDOWN_LABEL_INSET}
                    matchTriggerWidth={true}
                    menuGap={INSPECTOR_DROPDOWN_GAP}
                  >
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
                        <DropdownChevron />
                      </button>
                    {/snippet}
                    {#snippet menu({ close })}
                      {#each FONT_FAMILIES as f (f)}
                        <button
                          type="button"
                          class="font-option font-preview-{f}"
                          class:selected={shapeFamily === f}
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
                  onpreview={(hex) => previewTextColor(hex)}
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
                  <div class="segmented-control icon-segments" role="group" aria-label="Horizontal alignment">
                    <button type="button" class="seg-btn" class:active={shapeH === 'left'} aria-pressed={shapeH === 'left'} title="Align left" aria-label="Align left" disabled={shape.locked} onclick={() => void applyTextAlign('left')}>{@render horizontalAlignIcon('left')}</button>
                    <button type="button" class="seg-btn" class:active={shapeH === 'center'} aria-pressed={shapeH === 'center'} title="Align center" aria-label="Align center" disabled={shape.locked} onclick={() => void applyTextAlign('center')}>{@render horizontalAlignIcon('center')}</button>
                    <button type="button" class="seg-btn" class:active={shapeH === 'right'} aria-pressed={shapeH === 'right'} title="Align right" aria-label="Align right" disabled={shape.locked} onclick={() => void applyTextAlign('right')}>{@render horizontalAlignIcon('right')}</button>
                  </div>
                </div>
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">v-align</span>
                  <div class="segmented-control icon-segments" role="group" aria-label="Vertical alignment">
                    <button type="button" class="seg-btn" class:active={shapeV === 'top'} aria-pressed={shapeV === 'top'} title="Align top" aria-label="Align top" disabled={shape.locked} onclick={() => void applyTextVerticalAlign('top')}>{@render verticalAlignIcon('top')}</button>
                    <button type="button" class="seg-btn" class:active={shapeV === 'middle'} aria-pressed={shapeV === 'middle'} title="Align middle" aria-label="Align middle" disabled={shape.locked} onclick={() => void applyTextVerticalAlign('middle')}>{@render verticalAlignIcon('middle')}</button>
                    <button type="button" class="seg-btn" class:active={shapeV === 'bottom'} aria-pressed={shapeV === 'bottom'} title="Align bottom" aria-label="Align bottom" disabled={shape.locked} onclick={() => void applyTextVerticalAlign('bottom')}>{@render verticalAlignIcon('bottom')}</button>
                  </div>
                </div>
                </div>
              {/if}
            </div>
          {:else if sessionItem.type === 'line'}
            {@const line = sessionItem}
            <!-- line stroke: figma-style group (no toggle, color + w + style 항상 노출) -->
            <div class="fig-group is-on">
              <div class="fig-group-head">
                <span class="k">stroke</span>
              </div>
              <div class="fig-group-body">
                <ColorPicker
                  value={line.stroke}
                  onpreview={(hex) => previewShapeColor('stroke', hex)}
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
            <div class="fig-group is-on">
              <div class="fig-group-head">
                <span class="k">heads</span>
              </div>
              <div class="fig-group-body">
                <div class="font-dropdown">
                  <Dropdown
                    placement="bottom-start"
                    menuClass={INSPECTOR_DROPDOWN_MENU_CLASS}
                    menuInsetLeft={INSPECTOR_DROPDOWN_LABEL_INSET}
                    matchTriggerWidth={true}
                    menuGap={INSPECTOR_DROPDOWN_GAP}
                  >
                    {#snippet trigger({ toggle })}
                      <button
                        type="button"
                        class="font-trigger"
                        disabled={line.locked}
                        aria-label="Line start head"
                        title={headLabel(line.head_from)}
                        onclick={toggle}
                      >
                        <span class="font-label">from</span>
                        <span class="font-value head-value"><HeadIcon head={line.head_from} /></span>
                        <DropdownChevron />
                      </button>
                    {/snippet}
                    {#snippet menu({ close })}
                      {#each HEAD_OPTIONS as option (option.value)}
                        <button
                          type="button"
                          class="font-option"
                          class:selected={(line.head_from ?? 'none') === option.value}
                          disabled={line.locked}
                          aria-label={option.label}
                          title={option.label}
                          onclick={() => {
                            void applyLineHead('head_from', option.value);
                            close();
                          }}
                        >
                          <HeadIcon head={option.value} />
                        </button>
                      {/each}
                    {/snippet}
                  </Dropdown>
                </div>
                <div class="font-dropdown">
                  <Dropdown
                    placement="bottom-start"
                    menuClass={INSPECTOR_DROPDOWN_MENU_CLASS}
                    menuInsetLeft={INSPECTOR_DROPDOWN_LABEL_INSET}
                    matchTriggerWidth={true}
                    menuGap={INSPECTOR_DROPDOWN_GAP}
                  >
                    {#snippet trigger({ toggle })}
                      <button
                        type="button"
                        class="font-trigger"
                        disabled={line.locked}
                        aria-label="Line end head"
                        title={headLabel(line.head_to)}
                        onclick={toggle}
                      >
                        <span class="font-label">to</span>
                        <span class="font-value head-value"><HeadIcon head={line.head_to} /></span>
                        <DropdownChevron />
                      </button>
                    {/snippet}
                    {#snippet menu({ close })}
                      {#each HEAD_OPTIONS as option (option.value)}
                        <button
                          type="button"
                          class="font-option"
                          class:selected={(line.head_to ?? 'none') === option.value}
                          disabled={line.locked}
                          aria-label={option.label}
                          title={option.label}
                          onclick={() => {
                            void applyLineHead('head_to', option.value);
                            close();
                          }}
                        >
                          <HeadIcon head={option.value} />
                        </button>
                      {/each}
                    {/snippet}
                  </Dropdown>
                </div>
              </div>
            </div>
          {:else if sessionItem.type === 'path'}
            {@const path = sessionItem}
            <div class="fig-group is-on">
              <div class="fig-group-head">
                <span class="k">path</span>
              </div>
              <div class="fig-group-body">
                <div class="display-row control-row fig-body-row">
                  <span class="control-label">routing</span>
                  <div class="segmented-control icon-segments routing-segments" role="group" aria-label="Path routing">
                    {#each ROUTING_OPTIONS as option (option.value)}
                      <button
                        type="button"
                        class="seg-btn"
                        class:active={path.routing === option.value}
                        aria-pressed={path.routing === option.value}
                        aria-label={option.label}
                        title={option.label}
                        disabled={path.locked}
                        onclick={() => void applyPathRouting(option.value)}
                      >
                        <RoutingIcon routing={option.value} />
                      </button>
                    {/each}
                  </div>
                </div>
                <div class="font-dropdown">
                  <Dropdown
                    placement="bottom-start"
                    menuClass={INSPECTOR_DROPDOWN_MENU_CLASS}
                    menuInsetLeft={INSPECTOR_DROPDOWN_LABEL_INSET}
                    matchTriggerWidth={true}
                    menuGap={INSPECTOR_DROPDOWN_GAP}
                  >
                    {#snippet trigger({ toggle })}
                      <button
                        type="button"
                        class="font-trigger"
                        disabled={path.locked}
                        aria-label="Path start head"
                        title={headLabel(path.head_from)}
                        onclick={toggle}
                      >
                        <span class="font-label">from</span>
                        <span class="font-value head-value"><HeadIcon head={path.head_from} /></span>
                        <DropdownChevron />
                      </button>
                    {/snippet}
                    {#snippet menu({ close })}
                      {#each HEAD_OPTIONS as option (option.value)}
                        <button
                          type="button"
                          class="font-option"
                          class:selected={(path.head_from ?? 'none') === option.value}
                          disabled={path.locked}
                          aria-label={option.label}
                          title={option.label}
                          onclick={() => {
                            void applyPathHead('head_from', option.value);
                            close();
                          }}
                        >
                          <HeadIcon head={option.value} />
                        </button>
                      {/each}
                    {/snippet}
                  </Dropdown>
                </div>
                <div class="font-dropdown">
                  <Dropdown
                    placement="bottom-start"
                    menuClass={INSPECTOR_DROPDOWN_MENU_CLASS}
                    menuInsetLeft={INSPECTOR_DROPDOWN_LABEL_INSET}
                    matchTriggerWidth={true}
                    menuGap={INSPECTOR_DROPDOWN_GAP}
                  >
                    {#snippet trigger({ toggle })}
                      <button
                        type="button"
                        class="font-trigger"
                        disabled={path.locked}
                        aria-label="Path end head"
                        title={headLabel(path.head_to)}
                        onclick={toggle}
                      >
                        <span class="font-label">to</span>
                        <span class="font-value head-value"><HeadIcon head={path.head_to} /></span>
                        <DropdownChevron />
                      </button>
                    {/snippet}
                    {#snippet menu({ close })}
                      {#each HEAD_OPTIONS as option (option.value)}
                        <button
                          type="button"
                          class="font-option"
                          class:selected={(path.head_to ?? 'none') === option.value}
                          disabled={path.locked}
                          aria-label={option.label}
                          title={option.label}
                          onclick={() => {
                            void applyPathHead('head_to', option.value);
                            close();
                          }}
                        >
                          <HeadIcon head={option.value} />
                        </button>
                      {/each}
                    {/snippet}
                  </Dropdown>
                </div>
              </div>
            </div>
            <div class="fig-group is-on">
              <div class="fig-group-head">
                <span class="k">stroke</span>
              </div>
              <div class="fig-group-body">
                <ColorPicker
                  value={path.stroke}
                  onpreview={(hex) => previewShapeColor('stroke', hex)}
                  allowAlpha={true}
                  disabled={path.locked}
                  oncommit={(hex) => void applyShapeColor('stroke', hex)}
                />
                <InspectorField
                  type="number"
                  k="width"
                  value={String(path.stroke_width)}
                  mixed={false}
                  ariaLabel="Path stroke width"
                  disabled={path.locked}
                  live={true}
                  oncommit={(s) => void applyShapeStrokeWidth(Number(s))}
                />
                <DashSegments
                  value={path.stroke_dash ?? 'solid'}
                  disabled={path.locked}
                  onpick={(next) => void applyShapeDash(next)}
                />
              </div>
            </div>
            <div class="fig-group connect-endpoint-group" class:is-on={path.from.kind === 'connected'}>
              <div class="fig-group-head">
                <span class="k">connect from</span>
                <span class="fig-spacer"></span>
                <Toggle
                  checked={path.from.kind === 'connected'}
                  disabled={path.locked || (path.from.kind !== 'connected' && pathEndpointTargetOptions(path, 'from').length === 0)}
                  ariaLabel="Toggle path start connection"
                  onchange={(next) => void applyPathEndpointKind('from', next ? 'connected' : 'free')}
                />
              </div>
              {#if path.from.kind === 'connected'}
                {@const fromTarget = connectedEndpointTarget(path.from)}
                <div class="fig-group-body">
                  <div class="font-dropdown">
                    <Dropdown
                      placement="bottom-start"
                      menuClass={INSPECTOR_DROPDOWN_MENU_CLASS}
                      menuInsetLeft={INSPECTOR_DROPDOWN_LABEL_INSET}
                      matchTriggerWidth={true}
                      menuGap={INSPECTOR_DROPDOWN_GAP}
                    >
                      {#snippet trigger({ toggle })}
                        <button
                          type="button"
                          class="font-trigger"
                          disabled={path.locked || pathEndpointTargetOptions(path, 'from').length === 0}
                          aria-label="Path start component"
                          title={fromTarget === null ? 'Missing' : itemOptionTitle(fromTarget)}
                          onclick={toggle}
                        >
                          <span class="font-label">target</span>
                          <span class="font-value">{fromTarget === null ? 'Missing' : itemOptionLabel(fromTarget)}</span>
                          <DropdownChevron />
                        </button>
                      {/snippet}
                      {#snippet menu({ close })}
                        {#each pathEndpointTargetOptions(path, 'from') as item (item.id)}
                          <button
                            type="button"
                            class="font-option"
                            class:selected={path.from.kind === 'connected' && path.from.item_id === item.id}
                            disabled={path.locked}
                            title={itemOptionTitle(item)}
                            onclick={() => {
                              void applyPathEndpointTarget('from', item.id);
                              close();
                            }}
                          >
                            {itemOptionLabel(item)}
                          </button>
                        {/each}
                      {/snippet}
                    </Dropdown>
                  </div>
                  <PathAnchorPicker
                    value={path.from.anchor}
                    disabled={path.locked}
                    ariaLabel="Path start anchor"
                    onpick={(next) => void applyPathEndpointAnchor('from', next)}
                  />
                  <div class="endpoint-offset-row">
                    <span class="endpoint-label">offset</span>
                    <InspectorField
                      type="number"
                      k="X"
                      value={String(Math.round(endpointOffset(path.from).x))}
                      mixed={false}
                      ariaLabel="Path start anchor offset x"
                      disabled={path.locked}
                      live={true}
                      oncommit={(s) => void applyPathEndpointOffset('from', 'x', Number(s))}
                    />
                    <InspectorField
                      type="number"
                      k="Y"
                      value={String(Math.round(endpointOffset(path.from).y))}
                      mixed={false}
                      ariaLabel="Path start anchor offset y"
                      disabled={path.locked}
                      live={true}
                      oncommit={(s) => void applyPathEndpointOffset('from', 'y', Number(s))}
                    />
                  </div>
                </div>
              {/if}
            </div>
            <div class="fig-group connect-endpoint-group" class:is-on={path.to.kind === 'connected'}>
              <div class="fig-group-head">
                <span class="k">connect to</span>
                <span class="fig-spacer"></span>
                <Toggle
                  checked={path.to.kind === 'connected'}
                  disabled={path.locked || (path.to.kind !== 'connected' && pathEndpointTargetOptions(path, 'to').length === 0)}
                  ariaLabel="Toggle path end connection"
                  onchange={(next) => void applyPathEndpointKind('to', next ? 'connected' : 'free')}
                />
              </div>
              {#if path.to.kind === 'connected'}
                {@const toTarget = connectedEndpointTarget(path.to)}
                <div class="fig-group-body">
                  <div class="font-dropdown">
                    <Dropdown
                      placement="bottom-start"
                      menuClass={INSPECTOR_DROPDOWN_MENU_CLASS}
                      menuInsetLeft={INSPECTOR_DROPDOWN_LABEL_INSET}
                      matchTriggerWidth={true}
                      menuGap={INSPECTOR_DROPDOWN_GAP}
                    >
                      {#snippet trigger({ toggle })}
                        <button
                          type="button"
                          class="font-trigger"
                          disabled={path.locked || pathEndpointTargetOptions(path, 'to').length === 0}
                          aria-label="Path end component"
                          title={toTarget === null ? 'Missing' : itemOptionTitle(toTarget)}
                          onclick={toggle}
                        >
                          <span class="font-label">target</span>
                          <span class="font-value">{toTarget === null ? 'Missing' : itemOptionLabel(toTarget)}</span>
                          <DropdownChevron />
                        </button>
                      {/snippet}
                      {#snippet menu({ close })}
                        {#each pathEndpointTargetOptions(path, 'to') as item (item.id)}
                          <button
                            type="button"
                            class="font-option"
                            class:selected={path.to.kind === 'connected' && path.to.item_id === item.id}
                            disabled={path.locked}
                            title={itemOptionTitle(item)}
                            onclick={() => {
                              void applyPathEndpointTarget('to', item.id);
                              close();
                            }}
                          >
                            {itemOptionLabel(item)}
                          </button>
                        {/each}
                      {/snippet}
                    </Dropdown>
                  </div>
                  <PathAnchorPicker
                    value={path.to.anchor}
                    disabled={path.locked}
                    ariaLabel="Path end anchor"
                    onpick={(next) => void applyPathEndpointAnchor('to', next)}
                  />
                  <div class="endpoint-offset-row">
                    <span class="endpoint-label">offset</span>
                    <InspectorField
                      type="number"
                      k="X"
                      value={String(Math.round(endpointOffset(path.to).x))}
                      mixed={false}
                      ariaLabel="Path end anchor offset x"
                      disabled={path.locked}
                      live={true}
                      oncommit={(s) => void applyPathEndpointOffset('to', 'x', Number(s))}
                    />
                    <InspectorField
                      type="number"
                      k="Y"
                      value={String(Math.round(endpointOffset(path.to).y))}
                      mixed={false}
                      ariaLabel="Path end anchor offset y"
                      disabled={path.locked}
                      live={true}
                      oncommit={(s) => void applyPathEndpointOffset('to', 'y', Number(s))}
                    />
                  </div>
                </div>
              {/if}
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
                  <Dropdown
                    placement="bottom-start"
                    menuClass={INSPECTOR_DROPDOWN_MENU_CLASS}
                    menuInsetLeft={INSPECTOR_DROPDOWN_LABEL_INSET}
                    matchTriggerWidth={true}
                    menuGap={INSPECTOR_DROPDOWN_GAP}
                  >
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
                        <DropdownChevron />
                      </button>
                    {/snippet}
                    {#snippet menu({ close })}
                      {#each FONT_FAMILIES as f (f)}
                        <button
                          type="button"
                          class="font-option font-preview-{f}"
                          class:selected={family === f}
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
                  onpreview={(hex) => previewTextColor(hex)}
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
                    onpreview={(hex) => previewShapeColor('fill', hex)}
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
                    onpreview={(hex) => previewShapeColor('stroke', hex)}
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
                onpreview={(hex) => previewNoteColor(hex)}
                oncommit={(hex) => void applyNoteColor(hex)}
              />
            </div>
          {:else if sessionItem.type === 'file_path'}
            {@const copyPath = itemCopyPath(sessionItem)}
            <div class="prop-row full">
              <div class="display-row">
                <span class="k">path</span>
                <span class="display-val mono" title={sessionItem.path}>{strOr(sessionItem.path, '—')}</span>
                {#if copyPath !== null}
                  <button
                    type="button"
                    class="inline-action"
                    title="Copy path"
                    aria-label="Copy path"
                    onclick={() => void copyInspectorPath(copyPath)}
                  >
                    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                      <rect x="5" y="5" width="8" height="9" rx="1.2"/>
                      <path d="M3 11V3a1 1 0 0 1 1-1h6"/>
                    </svg>
                  </button>
                {/if}
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
            {@const copyPath = itemCopyPath(sessionItem)}
            <div class="prop-row full">
              <div class="display-row">
                <span class="k">file</span>
                <span class="display-val mono" title={sessionItem.label ?? sessionItem.path ?? sessionItem.asset_id}>
                  {strOr(sessionItem.label ?? sessionItem.path ?? sessionItem.asset_id, '—')}
                </span>
                {#if copyPath !== null}
                  <button
                    type="button"
                    class="inline-action"
                    title="Copy path"
                    aria-label="Copy path"
                    onclick={() => void copyInspectorPath(copyPath)}
                  >
                    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                      <rect x="5" y="5" width="8" height="9" rx="1.2"/>
                      <path d="M3 11V3a1 1 0 0 1 1-1h6"/>
                    </svg>
                  </button>
                {/if}
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
            {@const copyPath = itemCopyPath(sessionItem)}
            <div class="prop-row full">
              <div class="display-row">
                <span class="k">file</span>
                <span class="display-val mono" title={sessionItem.file_name ?? sessionItem.path}>
                  {strOr(sessionItem.file_name ?? sessionItem.path, '—')}
                </span>
                {#if copyPath !== null}
                  <button
                    type="button"
                    class="inline-action"
                    title="Copy path"
                    aria-label="Copy path"
                    onclick={() => void copyInspectorPath(copyPath)}
                  >
                    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                      <rect x="5" y="5" width="8" height="9" rx="1.2"/>
                      <path d="M3 11V3a1 1 0 0 1 1-1h6"/>
                    </svg>
                  </button>
                {/if}
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
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    padding: var(--space-6) 0;
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

  .endpoint-geometry-row {
    display: grid;
    grid-template-columns: 42px minmax(0, 1fr) minmax(0, 1fr);
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
    min-width: 0;
  }

  .endpoint-geometry-row > .endpoint-label {
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.4px;
    color: var(--color-fg-muted);
    text-transform: uppercase;
  }

  .endpoint-geometry-row > :global(.inspector-input) {
    --inspector-k-w: 14px;
  }

  .endpoint-offset-row {
    display: grid;
    grid-template-columns: 56px minmax(0, 1fr) minmax(0, 1fr);
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .endpoint-offset-row > .endpoint-label {
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.4px;
    color: var(--color-fg-muted);
    text-transform: uppercase;
  }

  .endpoint-offset-row > :global(.inspector-input) {
    --inspector-k-w: 14px;
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

  .display-row:not(.control-row):hover {
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
  .connect-endpoint-group .fig-group-head > .k {
    flex: 0 1 auto;
    width: auto;
    white-space: nowrap;
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
  .fig-group-body > :global(.anchor-picker),
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
    color: var(--color-fg);
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
    --inspector-k-w: 56px;
    width: 100%;
    min-width: 0;
  }

  .font-dropdown :global(.dropdown-host) {
    position: relative;
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    min-width: 0;
    height: 24px;
    padding: 0 0 0 6px;
    box-sizing: border-box;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-fg);
    transition: border-color var(--motion-fast) var(--motion-easing);
  }

  .font-dropdown :global(.dropdown-host:hover) {
    border-color: var(--color-border-strong);
  }

  .font-dropdown :global(.dropdown-host.open) {
    border-color: var(--color-accent);
  }

  :global(.dropdown-menu.inspector-dropdown-menu[role='menu']) {
    box-sizing: border-box;
    min-width: 0;
    max-width: none;
    max-height: min(280px, calc(100vh - 96px));
    overflow-x: hidden;
    overflow-y: auto;
    overscroll-behavior: contain;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 2px;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.16);
    z-index: var(--z-context-menu);
  }

  :global(.dropdown-menu.inspector-dropdown-menu[role='menu'] button) {
    justify-content: center;
    height: 24px;
    padding: 0 6px;
    border-radius: 2px;
    font-size: 11px;
  }

  :global(.dropdown-menu.inspector-dropdown-menu[role='menu'] button.selected) {
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }

  .font-trigger {
    box-sizing: border-box;
    flex: 1 1 auto;
    min-width: 0;
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 6px 0 0;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.2px;
    cursor: pointer;
    text-align: left;
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
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .font-value.mixed {
    color: var(--color-fg-muted);
    font-style: italic;
  }

  .head-value {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--color-fg);
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

  .routing-segments :global(.routing-icon) {
    width: 16px;
    height: 16px;
  }

  .head-value :global(.head-icon),
  .font-option :global(.head-icon) {
    width: 14px;
    height: 14px;
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
