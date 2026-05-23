<script lang="ts">
  /**
   * ContextMenu — right-click menu (plan 0005 Stage F, ADR-0017 §D2).
   *
   * Pattern (ref §10):
   *   - Canvas's `oncontextmenu` / `onnodecontextmenu` calls `openAt(x, y, paneId)`
   *   - Menu opens at the event coordinates, clamped to viewport bounds
   *   - Click outside or Esc → close
   *   - Item activation closes the menu and dispatches the action
   *
   * Item set (v0):
   *   - Copy panel id (clipboard, falls back to selectAll if clipboard API
   *     unavailable)
   *   - Close pane (CTRL kill-pane — disabled when paneId missing)
   *   - (separator)
   *   - Hide / Lock — placeholders, future Stage G/E wire
   *
   * The menu is rendered absolutely-positioned within `+page.svelte`'s
   * workspace so coordinates are viewport-relative. It mounts at the top
   * of the workspace stack so the rail/sidebar/canvas don't intercept.
   */

  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { zStore } from '$lib/stores/zStore.svelte';
  import { clipboardStore } from '$lib/stores/clipboardStore.svelte';
  import { pasteItems } from '$lib/canvas/clipboardOps.svelte';
  import { doToggleLock, doToggleVisibility } from '$lib/keyboard/editingShortcuts.svelte';
  import { changeTerminalDialog } from '$lib/stores/changeTerminalDialog.svelte';
  import { workspaceSwitcher } from '$lib/stores/workspaceSwitcher.svelte';
  import { panelCloseDialog } from '$lib/stores/panelCloseDialog.svelte';
  import { groupCloseDialog } from '$lib/stores/groupCloseDialog.svelte';
  import { UnauthorizedError } from '$lib/http/sessions';
  import {
    commitNewItem,
    createCanvasItem,
    createShapeItem,
    createLineItem,
    createTerminalItem,
  } from '$lib/canvas/itemFactory';
  import { toolStore } from '$lib/stores/toolStore.svelte';
  import { useSvelteFlow } from '@xyflow/svelte';
  import type { CanvasItem, CanvasItemType } from '$lib/types/canvas';
  import {
    alignItems,
    distributeItems,
    type AlignMode,
    type DistributeMode,
  } from '$lib/canvas/alignment';
  import { descendantGroups, descendantItems, pruneEmptyGroups } from '$lib/types/group';

  let open = $state(false);
  let pos = $state<{ x: number; y: number }>({ x: 0, y: 0 });
  /** Original click viewport coords — pre-clamp, used as anchor for Paste / Add. */
  let clickPos = $state<{ x: number; y: number }>({ x: 0, y: 0 });
  let paneIdStr = $state<string | null>(null);
  let panelIdStr = $state<string | null>(null);
  /** ADR-0010 D16 + plan-0012 §3.4 — group entity 진입 (rail right-click).
   *  Slice D 에서 mode='groupEntity' 분기로 entry text + [Ungroup] 활성. */
  let groupIdStr = $state<string | null>(null);
  let hidePaste = $state(false);
  let menuEl: HTMLDivElement | undefined = $state();
  /** Add ▸ sub-menu hover state (empty-area branch only). */
  let addSubmenuOpen = $state(false);

  /** External trigger — Canvas passes the raw MouseEvent + (optional)
   *  panel + pane identifiers. */
  export function openAt(args: {
    clientX: number;
    clientY: number;
    paneId?: string | null;
    panelId?: string | null;
    groupId?: string | null;
    hidePaste?: boolean;
  }): void {
    paneIdStr = args.paneId ?? null;
    panelIdStr = args.panelId ?? null;
    groupIdStr = args.groupId ?? null;
    hidePaste = args.hidePaste === true;
    open = true;
    // Initial position; clamped after the menu lays out (next tick).
    pos = { x: args.clientX, y: args.clientY };
    // Anchor 용 원본 click 좌표 — clampPos 가 menu 위치를 viewport 안으로
    // 옮겨도 paste / add 의 anchor 는 사용자가 실제로 클릭한 곳이어야 함.
    clickPos = { x: args.clientX, y: args.clientY };
    queueMicrotask(clampPos);
  }

  function close(): void {
    open = false;
    addSubmenuOpen = false;
    hidePaste = false;
    groupIdStr = null;
  }

  function clampPos(): void {
    if (!menuEl) return;
    const rect = menuEl.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let nx = pos.x;
    let ny = pos.y;
    if (nx + rect.width > vw) nx = Math.max(0, vw - rect.width - 4);
    if (ny + rect.height > vh) ny = Math.max(0, vh - rect.height - 4);
    pos = { x: nx, y: ny };
  }

  // ADR-0032 Amend ② lifecycle — 어떤 user input event 가 발생하든 기존 menu 는
  // 즉시 close 되고 새 event 는 그대로 진행. capture phase 로 등록하여 underlying
  // element 의 handler 보다 먼저 close 가 일어나도록 한다. propagate 는 그대로 —
  // close 된 후 click / select / contextmenu 모두 정상 흐른다.
  //
  // Coverage:
  // - `pointerdown` capture: mouse / touch / pen 의 모든 down — 좌·우 클릭, drag 시작,
  //   다른 menu / item 진입 등 모두 cover. right-click → close → 새 contextmenu 가
  //   다시 open 하는 sequence 가 자연 동작.
  // - `Escape` keydown: 명시적 dismiss.
  // - `blur` window: 탭 전환 / focus 손실 시 close.
  function onWindowPointerDown(e: PointerEvent): void {
    if (!open || !menuEl) return;
    if (menuEl.contains(e.target as Node)) return;
    close();
  }

  function onWindowKey(e: KeyboardEvent): void {
    if (open && e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  }

  function onWindowBlur(): void {
    if (open) close();
  }

  $effect(() => {
    if (typeof window === 'undefined') return;
    window.addEventListener('pointerdown', onWindowPointerDown, { capture: true });
    window.addEventListener('keydown', onWindowKey);
    window.addEventListener('blur', onWindowBlur);
    return () => {
      window.removeEventListener('pointerdown', onWindowPointerDown, { capture: true });
      window.removeEventListener('keydown', onWindowKey);
      window.removeEventListener('blur', onWindowBlur);
    };
  });

  async function onCopyPaneId(): Promise<void> {
    if (!paneIdStr) return;
    try {
      await navigator.clipboard.writeText(paneIdStr);
      toastStore.show({ message: 'Copied panel id to clipboard', tone: 'success' });
    } catch (e) {
      toastStore.show({ message: `Clipboard failed: ${(e as Error).message ?? e}`, tone: 'error' });
    }
    close();
  }

  /** ADR-0017 D6 amend ⑨ + plan-0013 D22 — Hide / Lock toggle (selection batch).
   *  M = item + group mixed 도 허용. groupEntity mode 면 [groupId] 단독. */
  function effectiveToggleIds(): string[] {
    if (ctxMode === 'groupEntity') {
      return groupIdStr !== null ? [groupIdStr] : [];
    }
    if (panelIdStr === null) return [];
    if (sessionStore.M.has(panelIdStr) && sessionStore.M.size > 0) {
      return [...sessionStore.M].filter(
        (id) => sessionStore.items.has(id) || sessionStore.groups.has(id),
      );
    }
    if (sessionStore.items.has(panelIdStr) || sessionStore.groups.has(panelIdStr)) {
      return [panelIdStr];
    }
    return [];
  }

  /* ── ADR-0032 amend ① (2026-05-21) — Multi-select mode + type intersection.
   *
   * D10: clicked-item ∈ M && M.size ≥ 2 → multi mode.
   *      selectedItems 가 모두 같은 type 이면 commonType, 다르면 null (mixed).
   *      mixed 시 type-specific 액션 (Change terminal / Copy panel id / Rename)
   *      은 모두 hide — *공통 속성만 노출*.
   */
  const isMultiMode = $derived.by(() => {
    if (panelIdStr === null) return false;
    return sessionStore.M.has(panelIdStr) && sessionStore.M.size >= 2;
  });
  /** ADR-0032 D25~D28 + plan-0012 §3.4 D.1 — ContextMenu 의 4 mode. */
  type CtxMode = 'empty' | 'single' | 'multi' | 'groupEntity';
  const ctxMode = $derived.by((): CtxMode => {
    if (groupIdStr !== null) return 'groupEntity';
    if (isMultiMode) return 'multi';
    if (panelIdStr !== null) return 'single';
    return 'empty';
  });
  /** ADR-0024 D9 / D14 + plan-0012 §3.4 D.3 — Z 액션의 effective target.
   *  groupEntity → [groupId], multi → M (group/item mixed 허용),
   *  single → [panelIdStr], empty → []. */
  const zTargetIds = $derived.by((): string[] => {
    if (ctxMode === 'groupEntity') return groupIdStr !== null ? [groupIdStr] : [];
    if (ctxMode === 'multi') {
      const out: string[] = [];
      for (const id of sessionStore.M) {
        if (sessionStore.items.has(id) || sessionStore.groups.has(id)) out.push(id);
      }
      return out;
    }
    if (ctxMode === 'single') return panelIdStr !== null ? [panelIdStr] : [];
    return [];
  });
  /** Slice E 의 entry text prefix — groupEntity 에서만 "group " (ADR-0010 D16). */
  const zPrefix = $derived(ctxMode === 'groupEntity' ? 'group ' : '');
  const effectiveItems = $derived.by((): CanvasItem[] => {
    if (panelIdStr === null) return [];
    if (isMultiMode) {
      const out: CanvasItem[] = [];
      for (const id of sessionStore.M) {
        const it = sessionStore.items.get(id);
        if (it !== undefined) out.push(it);
      }
      return out;
    }
    const it = sessionStore.items.get(panelIdStr);
    return it !== undefined ? [it] : [];
  });
  const commonType = $derived.by((): CanvasItemType | null => {
    if (effectiveItems.length === 0) return null;
    const first = effectiveItems[0]!.type;
    return effectiveItems.every((it) => it.type === first) ? first : null;
  });
  const targetIds = $derived(effectiveItems.map((it) => it.id));

  function onHide(): void {
    doToggleVisibility(effectiveToggleIds());
    close();
  }

  function onLock(): void {
    doToggleLock(effectiveToggleIds());
    close();
  }

  /* ── EDIT — Copy / Cut / Paste (ADR-0030 D10). ──────────────────── */

  /**
   * Effective copy/cut targets. ADR-0030 의 batch 동작 정합:
   * - 클릭된 panel 이 M ∈ 이면 M 전체 (batch)
   * - 클릭된 panel 이 M ∉ 이면 그 single item (M 은 변경 안 함 — ADR-0032 D1 의
   *   click-to-replace 는 별 batch 에서 적용)
   */
  function effectiveCopyTargets(): CanvasItem[] {
    if (panelIdStr === null) return [];
    const inM = sessionStore.M.has(panelIdStr);
    if (inM && sessionStore.M.size > 0) {
      const out: CanvasItem[] = [];
      for (const id of sessionStore.M) {
        const it = sessionStore.items.get(id);
        if (it !== undefined) out.push(it);
      }
      return out;
    }
    const it = sessionStore.items.get(panelIdStr);
    return it ? [it] : [];
  }

  function onCopy(): void {
    const targets = effectiveCopyTargets();
    if (targets.length > 0) clipboardStore.copy(targets);
    close();
  }

  async function onCut(): Promise<void> {
    // ADR-0030 D5 — locked 제외.
    const targets = effectiveCopyTargets().filter((it) => !it.locked);
    if (targets.length === 0) {
      close();
      return;
    }
    clipboardStore.cut(targets);
    close();
    // ADR-0032 Amend ④ — terminal 포함 batch 는 PanelCloseConfirmModal 경유.
    // terminal 없으면 store 가 즉시 onConfirm(false) → 기존 동작과 동일.
    panelCloseDialog.show({
      items: targets,
      onConfirm: async (killTerminal) => {
        await sessionStore.applyDeletion(
          targets.map((it) => it.id),
          { killTerminal },
        );
      },
    });
  }

  async function onPaste(): Promise<void> {
    if (!clipboardStore.hasItems) {
      close();
      return;
    }
    // ADR-0030 O2 정합 (Amend 2026-05-21 ④) — right-click paste 는 *마우스
    // 위치 anchor*. clipboard items 의 bbox top-left 이 클릭 위치로 오도록
    // offset 계산. clickPos = pre-clamp 원본 viewport 좌표.
    const flow = screenToFlowPosition({ x: clickPos.x, y: clickPos.y });
    const sources = clipboardStore.entries;
    const bboxX = sources.reduce(
      (m, it) => Math.min(m, it.x),
      Number.POSITIVE_INFINITY,
    );
    const bboxY = sources.reduce(
      (m, it) => Math.min(m, it.y),
      Number.POSITIVE_INFINITY,
    );
    const offset = { dx: flow.x - bboxX, dy: flow.y - bboxY };
    close();
    await pasteItems(sources, { offset, failMessage: 'Paste failed' });
  }

  /* ── ARRANGE 4 z actions (ADR-0024 D2 / ADR-0032 D11) — multi 시 batch. ── */

  function onBringToFront(): void {
    if (!zStore.canBringToFront(zTargetIds)) return;
    zStore.bringToFront(zTargetIds);
    close();
  }

  function onSendToBack(): void {
    if (!zStore.canSendToBack(zTargetIds)) return;
    zStore.sendToBack(zTargetIds);
    close();
  }

  function onBringForward(): void {
    if (!zStore.canBringForward(zTargetIds)) return;
    zStore.bringForward(zTargetIds);
    close();
  }

  function onSendBackward(): void {
    if (!zStore.canSendBackward(zTargetIds)) return;
    zStore.sendBackward(zTargetIds);
    close();
  }

  /** ADR-0010 D14 + plan-0012 §3.4 D.5 — [Group] entry handler (single/multi). */
  async function onGroup(): Promise<void> {
    if (sessionStore.active === null) return;
    // single 모드는 단일 id 그룹화 (D14 single-element group 허용).
    // multi 모드는 M 전체 그룹화. 둘 다 zTargetIds 가 정합.
    const ids = ctxMode === 'multi' ? [...sessionStore.M] : zTargetIds;
    if (ids.length === 0) return;
    close();
    await sessionStore.createGroup(ids);
  }

  /** ADR-0010 D12 + plan-0012 §3.4 D.5 — [Ungroup] entry handler (groupEntity). */
  async function onUngroup(): Promise<void> {
    if (sessionStore.active === null || groupIdStr === null) return;
    const gid = groupIdStr;
    close();
    await sessionStore.ungroup(gid);
  }

  /** ADR-0032 D29 — groupEntity [Delete all items]. */
  async function onDeleteGroupAll(): Promise<void> {
    if (sessionStore.active === null || groupIdStr === null) return;
    const gid = groupIdStr;
    const groupsArr = [...sessionStore.groups.values()];
    const itemsArr = [...sessionStore.items.values()];
    const items = descendantItems(gid, groupsArr, itemsArr);
    const terminalCount = items.filter((it) => it.type === 'terminal').length;
    close();
    if (terminalCount > 0) {
      groupCloseDialog.show(gid);
      return;
    }
    const groupIds = new Set([gid, ...descendantGroups(gid, groupsArr).map((g) => g.id)]);
    const itemIds = new Set(items.map((it) => it.id));
    const result = await sessionStore.applyMutation(
      (cur) =>
        pruneEmptyGroups({
          ...cur,
          groups: cur.groups.filter((g) => !groupIds.has(g.id)),
          items: cur.items.filter((it) => !itemIds.has(it.id)),
        }),
      { failMessage: 'Delete group items failed' },
    );
    if (result.ok) {
      if (sessionStore.drillRootId !== null && groupIds.has(sessionStore.drillRootId)) {
        sessionStore.clearDrill();
      }
      sessionStore.clearM();
    }
  }

  /* ── ALIGN / DISTRIBUTE (ADR-0027 / ADR-0032 D13) ──────────────────── */

  async function applyAlignBatch(
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
    const moves = alignItems(effectiveItems, mode);
    close();
    await applyAlignBatch(moves, 'Align aborted — session reconnect failed.');
  }

  async function onDistribute(mode: DistributeMode): Promise<void> {
    const moves = distributeItems(effectiveItems, mode);
    close();
    await applyAlignBatch(moves, 'Distribute aborted — session reconnect failed.');
  }

  /* ── "Add ___" sub-menu (pane right-click only) ──────────────────── */

  const { screenToFlowPosition } = useSvelteFlow();

  type AddableType = Extract<
    CanvasItemType,
    'text' | 'note' | 'file_path' | 'terminal' | 'rect' | 'ellipse' | 'line'
  >;

  const ADDABLE: { type: AddableType; label: string }[] = [
    { type: 'terminal', label: 'Terminal' },
    { type: 'text', label: 'Text' },
    { type: 'note', label: 'Note' },
    { type: 'rect', label: 'Rectangle' },
    { type: 'ellipse', label: 'Ellipse' },
    { type: 'line', label: 'Line' },
    { type: 'file_path', label: 'File path' },
  ];

  const DEFAULT_LINE_DELTA = { dx: 240, dy: 80 };
  const DEFAULT_SHAPE_BOUNDS = { w: 0, h: 0 } as const;

  async function onAddItem(type: AddableType): Promise<void> {
    // Add 도 paste 와 동일하게 *click 위치 anchor* — clampPos 가 menu 위치를
    // 옮겨도 새 item 은 사용자가 실제 클릭한 곳에 spawn.
    const flow = screenToFlowPosition({ x: clickPos.x, y: clickPos.y });
    let item: CanvasItem;
    switch (type) {
      case 'text':
      case 'note':
      case 'file_path':
        item = createCanvasItem(type, flow);
        break;
      case 'terminal':
        item = createTerminalItem(flow);
        break;
      case 'rect':
      case 'ellipse':
        item = createShapeItem(type, { x: flow.x, y: flow.y, ...DEFAULT_SHAPE_BOUNDS });
        break;
      case 'line':
        item = createLineItem(
          { x: flow.x, y: flow.y },
          { x: flow.x + DEFAULT_LINE_DELTA.dx, y: flow.y + DEFAULT_LINE_DELTA.dy },
        );
        break;
    }
    close();
    try {
      await commitNewItem(item);
      // Match toolbar one-shot behaviour — return to select after a creation.
      toolStore.consume();
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Add failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  /* ── EMPTY-AREA only — Select all / Clear all / Switch session ──────── */

  /** Select all visible items (matches editingShortcuts 의 ⌘A 동작). */
  function onSelectAll(): void {
    if (sessionStore.active === null) {
      close();
      return;
    }
    const ids: string[] = [];
    for (const [id, it] of sessionStore.items) {
      if (it.visibility === 'visible') ids.push(id);
    }
    if (ids.length === 0) {
      close();
      return;
    }
    sessionStore.setM(ids);
    close();
  }

  /** Clear all — applyDeletion 로 canvas 전체 비움. Terminal 은 pool 유지
   *  (ADR-0030 D5 패턴). 사용자 확인 dialog 없음 — Cmd+Z 로 복원 가능. */
  const canClearAll = $derived(
    sessionStore.active !== null && sessionStore.items.size > 0,
  );
  async function onClearAll(): Promise<void> {
    if (!canClearAll) {
      close();
      return;
    }
    const items = [...sessionStore.items.values()];
    close();
    // ADR-0032 Amend ④ — terminal 포함 시 PanelCloseConfirmModal 경유.
    panelCloseDialog.show({
      items,
      onConfirm: async (killTerminal) => {
        const ids = items.map((it) => it.id);
        const { ok, fail } = await sessionStore.applyDeletion(ids, {
          killTerminal,
        });
        if (ok > 0) {
          toastStore.show({
            message: killTerminal
              ? `${ok} items removed (terminals killed).`
              : `${ok} items removed from canvas.`,
            tone: 'success',
          });
        } else if (fail > 0) {
          toastStore.show({ message: 'Clear failed.', tone: 'error' });
        }
      },
    });
  }

  /** Switch session — workspaceSwitcher 모달 open (SessionMenu 의 entry 와 동일). */
  function onSwitchSession(): void {
    workspaceSwitcher.open();
    close();
  }

  /** Paste 활성 여부 — clipboard 에 내용이 있을 때만. */
  const canPasteEmpty = $derived(clipboardStore.hasItems);

  /** ChangeTerminal — open the ChangeTerminalModal targeting this panel. */
  function onChangeTerminal(): void {
    if (!panelIdStr) return;
    changeTerminalDialog.show(panelIdStr);
    close();
  }

  /** ADR-0032 D10 — type-specific 액션 가시성. mixed (commonType=null) 또는
   *  multi-mode 의 single-only 액션은 hide. */
  const isPanelTerminal = $derived(commonType === 'terminal' && !isMultiMode);
  /** Change terminal: terminal common type 일 때 single + multi 모두 노출
   *  가능. 다만 multi 시 일괄 교체 의도 모호 (ADR-0032 D3) → single 만. */
  const canChangeTerminal = $derived(commonType === 'terminal' && !isMultiMode);
  /** Copy panel id: single terminal 에서만 의미. */
  const canCopyPaneId = $derived(paneIdStr !== null && !isMultiMode);
  /** Align/Distribute: M.size ≥ 2 / ≥ 3. */
  const canAlign = $derived(isMultiMode && effectiveItems.length >= 2);
  const canDistribute = $derived(isMultiMode && effectiveItems.length >= 3);

  /** Delete (ADR-0032 D12 / Amend ④) — batch in multi mode. Terminal 포함 시
   *  PanelCloseConfirmModal 경유 (Panel only / Panel+Terminal 선택). */
  async function onDeleteItem(): Promise<void> {
    if (sessionStore.active === null || targetIds.length === 0) {
      close();
      return;
    }
    const items = targetIds
      .map((id) => sessionStore.items.get(id))
      .filter((it): it is NonNullable<typeof it> => it !== undefined);
    close();
    panelCloseDialog.show({
      items,
      onConfirm: async (killTerminal) => {
        const { ok, fail } = await sessionStore.applyDeletion(targetIds, {
          killTerminal,
        });
        const total = ok + fail;
        if (ok > 0) {
          toastStore.show({
            message:
              total === 1
                ? killTerminal
                  ? 'Panel + terminal closed.'
                  : 'Panel removed from canvas. Terminal still in pool.'
                : killTerminal
                  ? `${ok} items removed (terminals killed).`
                  : `${ok} items removed from canvas.`,
            tone: 'success',
          });
        } else if (fail > 0) {
          toastStore.show({ message: 'Delete failed.', tone: 'error' });
        }
      },
    });
  }
</script>

{#if open}
  <div
    bind:this={menuEl}
    class="ctx-menu"
    role="menu"
    style="left: {pos.x}px; top: {pos.y}px;"
  >
    {#if !panelIdStr}
      <button
        type="button"
        class="ctx-item"
        disabled={!canPasteEmpty}
        onclick={() => void onPaste()}
      >
        <span class="label">Paste</span>
        <span class="kbd mono">⌘V</span>
      </button>
      <div class="ctx-sep"></div>

      <button type="button" class="ctx-item" onclick={onSelectAll}>
        <span class="label">Select all</span>
        <span class="kbd mono">⌘A</span>
      </button>

      <div
        class="ctx-item-with-sub"
        role="none"
        onmouseenter={() => (addSubmenuOpen = true)}
        onmouseleave={() => (addSubmenuOpen = false)}
      >
        <button type="button" class="ctx-item" aria-haspopup="menu" aria-expanded={addSubmenuOpen}>
          <span class="label">Add</span>
          <span class="kbd">▸</span>
        </button>
        {#if addSubmenuOpen}
          <div class="ctx-submenu" role="menu">
            {#each ADDABLE as a (a.type)}
              <button
                type="button"
                class="ctx-item"
                onclick={() => void onAddItem(a.type)}
              >
                <span class="label">{a.label}</span>
              </button>
            {/each}
          </div>
        {/if}
      </div>
      <div class="ctx-sep"></div>

      <button
        type="button"
        class="ctx-item danger"
        disabled={!canClearAll}
        onclick={() => void onClearAll()}
      >
        <span class="label">Clear all</span>
      </button>
      <div class="ctx-sep"></div>

      <button type="button" class="ctx-item" onclick={onSwitchSession}>
        <span class="label">Switch session…</span>
      </button>
    {/if}

    {#if canCopyPaneId}
      <div class="ctx-section">Panel</div>
      <button
        type="button"
        class="ctx-item"
        onclick={onCopyPaneId}
      >
        <span class="label">Copy panel id</span>
      </button>
      <div class="ctx-sep"></div>
    {/if}

    {#if ctxMode === 'groupEntity'}
      <!-- ADR-0032 D25~D28 + plan-0012 §3.4 D — Group entity mode. -->
      <div class="ctx-section">Arrange</div>
      <button
        type="button"
        class="ctx-item"
        disabled={!zStore.canBringToFront(zTargetIds)}
        onclick={onBringToFront}
      >
        <span class="label">Bring {zPrefix}to front</span>
        <span class="kbd mono">⇧]</span>
      </button>
      <button
        type="button"
        class="ctx-item"
        disabled={!zStore.canBringForward(zTargetIds)}
        onclick={onBringForward}
      >
        <span class="label">Bring {zPrefix}forward</span>
        <span class="kbd mono">]</span>
      </button>
      <button
        type="button"
        class="ctx-item"
        disabled={!zStore.canSendBackward(zTargetIds)}
        onclick={onSendBackward}
      >
        <span class="label">Send {zPrefix}backward</span>
        <span class="kbd mono">[</span>
      </button>
      <button
        type="button"
        class="ctx-item"
        disabled={!zStore.canSendToBack(zTargetIds)}
        onclick={onSendToBack}
      >
        <span class="label">Send {zPrefix}to back</span>
        <span class="kbd mono">⇧[</span>
      </button>
      <div class="ctx-sep"></div>
      <!-- Group visibility / lock toggle — ADR-0010 D6 의 self-state 갱신.
           descendant effective state 는 AND (visibility) / OR (lock) 으로 자연 전파. -->
      <div class="ctx-section">Visibility</div>
      <button type="button" class="ctx-item" onclick={onHide}>
        <span class="label">Hide / Show group</span>
        <span class="kbd mono">⇧⌘H</span>
      </button>
      <button type="button" class="ctx-item" onclick={onLock}>
        <span class="label">Lock / Unlock group</span>
        <span class="kbd mono">⌘L</span>
      </button>
      <div class="ctx-sep"></div>
      <button type="button" class="ctx-item" onclick={() => void onUngroup()}>
        <span class="label">Ungroup</span>
        <span class="kbd mono">⇧⌘G</span>
      </button>
      <button type="button" class="ctx-item danger" onclick={() => void onDeleteGroupAll()}>
        <span class="label">Delete all items</span>
      </button>
    {/if}

    {#if panelIdStr}
      <div class="ctx-section">Edit</div>
      <button type="button" class="ctx-item" onclick={onCopy}>
        <span class="label">Copy</span>
        <span class="kbd mono">⌘C</span>
      </button>
      <button type="button" class="ctx-item" onclick={() => void onCut()}>
        <span class="label">Cut</span>
        <span class="kbd mono">⌘X</span>
      </button>
      {#if !hidePaste}
        <button
          type="button"
          class="ctx-item"
          disabled={!clipboardStore.hasItems}
          onclick={() => void onPaste()}
        >
          <span class="label">Paste</span>
          <span class="kbd mono">⌘V</span>
        </button>
      {/if}
      <div class="ctx-sep"></div>

      <div class="ctx-section">Arrange</div>
      <button
        type="button"
        class="ctx-item"
        disabled={!zStore.canBringToFront(zTargetIds)}
        onclick={onBringToFront}
      >
        <span class="label">Bring to front</span>
        <span class="kbd mono">⇧]</span>
      </button>
      <button
        type="button"
        class="ctx-item"
        disabled={!zStore.canBringForward(zTargetIds)}
        onclick={onBringForward}
      >
        <span class="label">Bring forward</span>
        <span class="kbd mono">]</span>
      </button>
      <button
        type="button"
        class="ctx-item"
        disabled={!zStore.canSendBackward(zTargetIds)}
        onclick={onSendBackward}
      >
        <span class="label">Send backward</span>
        <span class="kbd mono">[</span>
      </button>
      <button
        type="button"
        class="ctx-item"
        disabled={!zStore.canSendToBack(zTargetIds)}
        onclick={onSendToBack}
      >
        <span class="label">Send to back</span>
        <span class="kbd mono">⇧[</span>
      </button>
      <!-- ADR-0010 D14 — [Group] entry (single/multi). zTargetIds 가 비어있지 않은 한 가능. -->
      <button
        type="button"
        class="ctx-item"
        disabled={zTargetIds.length === 0}
        onclick={() => void onGroup()}
      >
        <span class="label">Group{isMultiMode ? ' (batch)' : ''}</span>
        <span class="kbd mono">⌘G</span>
      </button>
      <div class="ctx-sep"></div>

      <div class="ctx-section">Visibility</div>
      <button type="button" class="ctx-item" onclick={onHide}>
        <span class="label">Hide / Show{isMultiMode ? ' (batch)' : ''}</span>
        <span class="kbd mono">⇧⌘H</span>
      </button>
      <button type="button" class="ctx-item" onclick={onLock}>
        <span class="label">Lock / Unlock{isMultiMode ? ' (batch)' : ''}</span>
        <span class="kbd mono">⌘L</span>
      </button>

      {#if canAlign}
        <!-- ADR-0032 D13 — Align / Distribute (M.size ≥ 2). mixed type 도
             적용 가능 — bbox 기반 이동만 (type-agnostic). ContextMenu 는 모든
             entry 가 text-only line-by-line (사용자 규칙 2026-05-21). -->
        <div class="ctx-sep"></div>
        <div class="ctx-section">Align ({effectiveItems.length})</div>
        <button type="button" class="ctx-item" onclick={() => void onAlign('left')}>
          <span class="label">Align left</span>
        </button>
        <button type="button" class="ctx-item" onclick={() => void onAlign('center-x')}>
          <span class="label">Align center horizontally</span>
        </button>
        <button type="button" class="ctx-item" onclick={() => void onAlign('right')}>
          <span class="label">Align right</span>
        </button>
        <button type="button" class="ctx-item" onclick={() => void onAlign('top')}>
          <span class="label">Align top</span>
        </button>
        <button type="button" class="ctx-item" onclick={() => void onAlign('center-y')}>
          <span class="label">Align center vertically</span>
        </button>
        <button type="button" class="ctx-item" onclick={() => void onAlign('bottom')}>
          <span class="label">Align bottom</span>
        </button>
        {#if canDistribute}
          <div class="ctx-sep"></div>
          <div class="ctx-section">Distribute</div>
          <button type="button" class="ctx-item" onclick={() => void onDistribute('horizontal')}>
            <span class="label">Distribute horizontally</span>
          </button>
          <button type="button" class="ctx-item" onclick={() => void onDistribute('vertical')}>
            <span class="label">Distribute vertically</span>
          </button>
        {/if}
      {/if}

      {#if canChangeTerminal}
        <div class="ctx-sep"></div>
        <div class="ctx-section">Terminal</div>
        <button type="button" class="ctx-item" onclick={onChangeTerminal}>
          <span class="label">Change terminal…</span>
        </button>
      {/if}
      <div class="ctx-sep"></div>
      <button type="button" class="ctx-item danger" onclick={() => void onDeleteItem()}>
        <span class="label">Remove from canvas{isMultiMode ? ` (${effectiveItems.length})` : ''}</span>
        <span class="kbd mono">⌫</span>
      </button>
    {/if}
  </div>
{/if}

<style>
  .ctx-menu {
    position: fixed;
    min-width: 220px;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-lg);
    padding: var(--space-6) 0;
    z-index: var(--z-context-menu);
    color: var(--color-fg);
    font-size: var(--text-md);
    user-select: none;
    animation: ctx-in var(--motion-fast) var(--motion-easing);
  }

  .ctx-section {
    padding: var(--space-4) var(--space-14);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--color-fg-muted);
  }

  .ctx-sep {
    height: 1px;
    background: var(--color-border);
    margin: var(--space-4) 0;
  }

  .ctx-item {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    width: 100%;
    padding: var(--space-6) var(--space-14);
    background: transparent;
    border: 0;
    color: inherit;
    text-align: left;
    cursor: pointer;
    font-family: inherit;
    font-size: inherit;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .ctx-item:hover:not(:disabled) {
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }

  .ctx-item:hover:not(:disabled) .kbd {
    color: rgba(255, 255, 255, 0.85);
  }

  .ctx-item:disabled {
    color: var(--color-fg-subtle);
    cursor: not-allowed;
  }

  .ctx-item.danger:not(:disabled) {
    color: var(--color-danger);
  }

  .ctx-item.danger:hover:not(:disabled) {
    background: var(--color-danger);
    color: white;
  }

  .kbd {
    color: var(--color-fg-muted);
    font-size: var(--text-base);
    letter-spacing: 0.4px;
  }

  .kbd.mono {
    font-family: var(--font-mono);
  }

  /* ADR-0032 D13 amend (2026-05-21) — Align / Distribute icon grid 폐기.
     ContextMenu 의 모든 entry 는 text-only line-by-line (사용자 design 규칙). */

  /* Add ▸ hover submenu — empty-area branch only. */
  .ctx-item-with-sub {
    position: relative;
  }

  .ctx-submenu {
    position: absolute;
    left: 100%;
    top: calc(-1 * var(--space-6));
    min-width: 180px;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-lg);
    padding: var(--space-6) 0;
    z-index: 1;
    animation: ctx-in var(--motion-fast) var(--motion-easing);
  }

  @keyframes ctx-in {
    from {
      opacity: 0;
      transform: translateY(-2px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
</style>
