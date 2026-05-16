<script lang="ts">
  // LayerTreeView — Figma-식 layer tree (Sidebar from previous incarnation).
  // Now embedded as a *tab content* inside LeftPanel (ADR-0017 §D2 amend
  // 2026-05-16). Outer chrome (aside, fold, absolute positioning) is owned
  // by the host (LeftPanel.svelte).
  //
  // 책임:
  // - sessionStore.{groups,items} 의 `parent_id` 트리를 재귀 렌더 (Group / Panel mixed).
  // - Layer mode toggle: Tree (parent_id DFS) vs Z (flat z desc, no groups).
  // - Group 행은 펼침/접힘 toggle (component-local SvelteSet<string>, P1+에서 영속화 검토).
  // - 클릭 → sessionStore.M 동기화.
  // - dead 표시: muxStore.panes.get(N).dead === true → 회색 + 취소선.
  //
  // 불변식 (CLAUDE.md):
  // - tmux state / web state 분리 (#1).
  // - 사용자 입력 escape (#4) — Svelte 자동 HTML escape.

  import { SvelteSet } from 'svelte/reactivity';
  import { muxStore } from '$lib/stores/mux.svelte';
  import { ensureMutationOk, sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { mutateLayout, UnauthorizedError } from '$lib/http/sessions';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import type { CanvasItem, CanvasItemType } from '$lib/types/canvas';
  import { groupCloseDialog } from '$lib/stores/groupCloseDialog.svelte';
  import InlineEditField from '$lib/common/InlineEditField.svelte';

  /** Currently inline-editing group id, or `null`. Component-local. */
  let editingGroupId = $state<string | null>(null);

  function onStartRenameGroup(id: string, e: MouseEvent): void {
    e.preventDefault();
    e.stopPropagation();
    editingGroupId = id;
  }

  async function onCommitRenameGroup(id: string, next: string): Promise<void> {
    editingGroupId = null;
    const trimmed = next.trim();
    await mutateActiveLayout((cur) => ({
      ...cur,
      groups: cur.groups.map((g) =>
        g.id === id ? { ...g, label: trimmed.length === 0 ? null : trimmed } : g,
      ),
    }));
  }

  function onCancelRenameGroup(): void {
    editingGroupId = null;
  }

  /* ── Group propagation 시각화 (ADR-0010 D6) ────────────────────────
   * effectiveVisibility = AND (self visible ∧ every ancestor visible)
   * effectiveLocked     = OR  (self locked ∨ any ancestor locked)
   *
   * "inherited" 상태 = self 는 통과 (visible 또는 unlocked) 인데 ancestor
   * 중 하나가 그 상태를 *덮어쓰고 있는* 경우. 사용자가 사이드바에서 self
   * toggle 을 해도 행동이 안 바뀌므로 시각 단서 필요.
   *
   * 본 helper 는 sessionStore.groups 의 ancestor chain 만 walk. */
  function walkAncestors(parentId: string | null): Array<{ id: string; visibility?: string; locked?: boolean; label?: string | null }> {
    const out: Array<{ id: string; visibility?: string; locked?: boolean; label?: string | null }> = [];
    let cur = parentId;
    const seen = new Set<string>();
    while (cur !== null && !seen.has(cur)) {
      seen.add(cur);
      const g = sessionStore.groups.get(cur);
      if (g === undefined) break;
      out.push({ id: g.id, visibility: g.visibility, locked: g.locked, label: g.label });
      cur = g.parent_id ?? null;
    }
    return out;
  }

  /** Ancestor 중 visibility='hidden' 인 가장 가까운 group — 없으면 null. */
  function inheritedHiddenFrom(parentId: string | null): { id: string; label: string | null } | null {
    for (const a of walkAncestors(parentId)) {
      if (a.visibility === 'hidden') return { id: a.id, label: a.label ?? null };
    }
    return null;
  }

  /** Ancestor 중 locked=true 인 가장 가까운 group — 없으면 null. */
  function inheritedLockedFrom(parentId: string | null): { id: string; label: string | null } | null {
    for (const a of walkAncestors(parentId)) {
      if (a.locked === true) return { id: a.id, label: a.label ?? null };
    }
    return null;
  }

  function inheritedSourceLabel(src: { id: string; label: string | null }): string {
    return src.label != null && src.label.length > 0 ? src.label : src.id.slice(0, 8);
  }

  function openGroupClose(id: string, e: MouseEvent): void {
    stopRowAction(e);
    groupCloseDialog.show(id);
  }

  // SSoT (`docs/ssot/canvas-layout-schema.md` §1 `$defs/Group`) 의 부분 view —
  // `codebase/frontend/src/lib/types/canvas-layout.d.ts` 가 codegen 으로 채워지기 전까지의
  // 잠정 타입. Canvas.svelte 의 PanelData 패턴과 동일 (R8 §F3 명시).
  interface GroupData {
    id: string;
    parent_id?: string | null;
    label?: string | null;
    visibility?: boolean;
    locked?: boolean;
    order?: number;
  }
  interface PanelData {
    id: string;
    parent_id?: string | null;
    pane_id?: string; // e.g. "%0" — SSoT pattern `^%[0-9]+$`
    type?: CanvasItemType | 'panel';
    label?: string | null;
    visibility?: boolean;
    locked?: boolean;
    minimized?: boolean;
    z?: number;
  }

  // 트리 노드 union — 사이드바 한 줄에 해당.
  type TreeNode =
    | { kind: 'group'; id: string; depth: number; group: GroupData; hasChildren: boolean }
    | { kind: 'panel'; id: string; depth: number; panel: PanelData };

  // 펼침 상태 — component-local. P1+에서 ephemeralStore 또는 web-store 영속화 검토.
  const expanded = $state(new SvelteSet<string>());

  // ADR-0024 D1 — Tree order ≠ Z. Sidebar 는 두 시점을 toggle:
  //   - 'tree' : organization (parent_id 트리, 사용자 grouping. drag reorder 영역)
  //   - 'z'    : rendering stack (z 내림차순 flat. group 미포함 — group 은 z 없음)
  // P1+: ephemeralStore 영속화. MVP 는 component-local.
  type LayerMode = 'tree' | 'z';
  let layerMode = $state<LayerMode>('tree');

  // 트리 평탄화 — Group/Panel 을 parent_id 기준으로 묶어 DFS 순회.
  // 평탄화된 결과를 {each} 로 렌더하면 들여쓰기는 depth * 16 px 로 표현 가능.
  // SvelteMap entry-level reactivity 의 비용은 본 derived 의 *전체 재계산* — Panel/Group 수가
  // 수십 단위인 MVP 에서는 무시 가능. R8 §F3 의 가이드 위반 시 keyed each + 자식 재귀 컴포넌트로 분해.
  const tree = $derived.by<TreeNode[]>(() => {
    // Multi-session 의 v2 schema 필드를 GroupData/PanelData 모양으로 어댑트:
    //   - visibility string ("visible"|"hidden") → boolean
    //   - terminal item.id 는 schema v2 의 UUID — legacy `pane_id` 슬롯에도 노출.
    const groups: GroupData[] = Array.from(sessionStore.groups.values()).map((g) => ({
      id: g.id,
      parent_id: g.parent_id,
      label: g.label,
      visibility: g.visibility === 'visible',
      locked: g.locked,
      order: g.order,
    }));

    const panels: PanelData[] = Array.from(sessionStore.items.values()).map((it) => ({
      id: it.id,
      parent_id: it.parent_id,
      pane_id: it.type === 'terminal' ? it.id : undefined,
      type: it.type,
      label: it.label ?? null,
      visibility: it.visibility === 'visible',
      locked: it.locked,
      minimized: it.minimized,
      z: it.z,
    }));

    // Z mode — flat z 내림차순, group 미포함 (ADR-0024 D3). depth=0 일관.
    if (layerMode === 'z') {
      const flat = [...panels].sort((a, b) => {
        const za = (a as PanelData & { z?: number }).z ?? 0;
        const zb = (b as PanelData & { z?: number }).z ?? 0;
        return zb - za;
      });
      return flat.map((p) => ({ kind: 'panel' as const, id: p.id, depth: 0, panel: p }));
    }

    // 부모 별 children 인덱스 — null = canvas root.
    const childGroups = new Map<string | null, GroupData[]>();
    for (const g of groups) {
      const key = g.parent_id ?? null;
      const bucket = childGroups.get(key);
      if (bucket) bucket.push(g);
      else childGroups.set(key, [g]);
    }
    const childPanels = new Map<string | null, PanelData[]>();
    for (const p of panels) {
      const key = p.parent_id ?? null;
      const bucket = childPanels.get(key);
      if (bucket) bucket.push(p);
      else childPanels.set(key, [p]);
    }

    // 같은 부모 안에서의 정렬: Group.order 오름차순 → Panel id 오름차순.
    // SSoT 는 Panel 의 sibling 정렬 키를 별도로 명시하지 않음 — id 정렬은 결정성만 확보.
    const sortGroups = (xs: GroupData[]): GroupData[] =>
      [...xs].sort((a, b) => (a.order ?? 0) - (b.order ?? 0));
    const sortPanels = (xs: PanelData[]): PanelData[] =>
      [...xs].sort((a, b) => a.id.localeCompare(b.id));

    const out: TreeNode[] = [];
    const walk = (parentId: string | null, depth: number): void => {
      // Groups first, then leaf Panels (Figma convention: 그룹이 위, 단일 leaf 가 아래).
      const gs = sortGroups(childGroups.get(parentId) ?? []);
      for (const g of gs) {
        const ownChildren =
          (childGroups.get(g.id)?.length ?? 0) + (childPanels.get(g.id)?.length ?? 0);
        out.push({ kind: 'group', id: g.id, depth, group: g, hasChildren: ownChildren > 0 });
        if (expanded.has(g.id) && ownChildren > 0) {
          walk(g.id, depth + 1);
        }
      }
      const ps = sortPanels(childPanels.get(parentId) ?? []);
      for (const p of ps) {
        out.push({ kind: 'panel', id: p.id, depth, panel: p });
      }
    };
    walk(null, 0);
    return out;
  });

  // Panel.pane_id (e.g. "%3") → muxStore.panes 의 정수 key (3) 변환.
  // SSoT pattern `^%[0-9]+$` 이므로 substring(1) 은 안전. NaN 방어만.
  function paneNumeric(paneIdStr: string | undefined): number | null {
    if (!paneIdStr || paneIdStr[0] !== '%') return null;
    const n = Number.parseInt(paneIdStr.slice(1), 10);
    return Number.isNaN(n) ? null : n;
  }

  // Panel 행 표시 라벨 우선순위 (Task 2 fix — terminal_meta 우선):
  //   1) terminalPool 의 terminal_meta label (server-wide, ADR-0021 D7) — terminal/
  //      panel type 에만 적용. 빈 문자열은 미설정 으로 간주.
  //   2) layout Panel.label (legacy, disk stale 가능)
  //   3) "%${paneNum}" — pane id fallback
  function panelDisplayLabel(p: PanelData): string {
    const isTerminal = p.type === 'terminal' || p.type === 'panel' || p.type == null;
    if (isTerminal) {
      const poolLabel = terminalPool.byId(p.id)?.label?.trim();
      if (poolLabel != null && poolLabel.length > 0) return poolLabel;
    }
    if (p.label != null && p.label.length > 0) return p.label;
    const n = paneNumeric(p.pane_id);
    if (n !== null) return `%${n}`;
    const type = p.type === 'file_path' ? 'file' : (p.type ?? 'panel');
    return `${type}:${p.id.slice(0, 8)}`;
  }

  function panelTypeIcon(p: PanelData): string {
    switch (p.type) {
      case 'terminal':
      case 'panel':
        return '▣';
      case 'text':
        return 'T';
      case 'note':
        return 'N';
      case 'rect':
        return '□';
      case 'ellipse':
        return '○';
      case 'line':
        return '╱';
      case 'free_draw':
        return '⌁';
      case 'image':
        return '▧';
      case 'document':
        return 'D';
      case 'file_path':
        return 'F';
      default:
        return '•';
    }
  }

  // Group 행 표시 라벨 — Group.label || id.
  // ADR-0010 D6 의 ancestor inherit 은 effective 계산이며 self.label null 일 때 ancestor 라벨을
  // *추론* 한다고 명시. MVP 본 v0 은 self.label 만 표시 (inherit 은 P1+에서 effective 계산기 도입).
  function groupDisplayLabel(g: GroupData): string {
    if (g.label != null && g.label.length > 0) return g.label;
    return g.id;
  }

  function toggleExpand(id: string): void {
    if (expanded.has(id)) expanded.delete(id);
    else expanded.add(id);
  }

  /**
   * Selection anchor — 단일 클릭이 직전에 set 한 row id. Shift+click 의 range
   * 시작점. anchor 가 visible tree 에서 사라지면 (예: ancestor collapse) 첫
   * shift-click 이 fallback 으로 target 만 add.
   */
  let selectionAnchor = $state<string | null>(null);

  /**
   * 선택 동기화 — ADR-0024 의 layer list 1차 가치 "다중 선택 + bulk action".
   *   - plain                : M = [id]. anchor = id.
   *   - meta/ctrl + click    : M.toggle(id). anchor = id.
   *   - shift + click        : visible tree 의 anchor↔id range 일괄 add (set
   *                            anchor 가 null 이면 toggle fallback). anchor 는
   *                            그대로 유지 — 동일 anchor 에서 연속 shift-click
   *                            가능 (Finder/VSCode 컨벤션).
   */
  function selectNode(id: string, e?: MouseEvent | KeyboardEvent): void {
    if (e instanceof MouseEvent) {
      if (e.shiftKey) {
        const anchor = selectionAnchor;
        if (anchor === null || anchor === id) {
          sessionStore.toggleM(id);
          if (anchor === null) selectionAnchor = id;
          return;
        }
        const ids = visibleRangeIds(anchor, id);
        if (ids.length === 0) {
          sessionStore.toggleM(id);
          return;
        }
        // setM 으로 anchor↔target range 만 선택. (multi-select 의 일반 직관 —
        // shift 는 range *select*, ctrl 와 결합 시에만 add-to.)
        if (e.metaKey || e.ctrlKey) {
          for (const rid of ids) sessionStore.addToM(rid);
        } else {
          sessionStore.setM(ids);
        }
        return;
      }
      if (e.metaKey || e.ctrlKey) {
        sessionStore.toggleM(id);
        selectionAnchor = id;
        return;
      }
    }
    sessionStore.setM([id]);
    selectionAnchor = id;
  }

  /**
   * Visible tree 안에서 두 row id 사이의 inclusive range 를 순서대로 반환.
   * 두 id 중 어느 쪽이 위인지 무관 — visible tree 의 순방향 (위→아래) 정렬.
   * 둘 중 하나라도 invisible 면 빈 배열.
   */
  function visibleRangeIds(a: string, b: string): string[] {
    const order = tree.map((n) => n.id);
    const ia = order.indexOf(a);
    const ib = order.indexOf(b);
    if (ia < 0 || ib < 0) return [];
    const [lo, hi] = ia <= ib ? [ia, ib] : [ib, ia];
    return order.slice(lo, hi + 1);
  }

  // Panel 행이 dead pane 인지 — 회색/취소선 표시 트리거.
  function isPanelDead(p: PanelData): boolean {
    const n = paneNumeric(p.pane_id);
    if (n === null) return false;
    return muxStore.panes.get(n)?.dead === true;
  }

  /* ── Drag reorder / reparent (ADR-0024 D1 organization-only) ──────────
   * 흐름:
   *   - 행 draggable=true. dragstart 에 sourceIds 캡처 (M 에 dragged id 포함되면
   *     selected set 전체, 아니면 dragged id 한 개).
   *   - 각 행 dragover 시 mouse Y 가 행 높이의 1/4 미만 → 'before', 3/4 초과 →
   *     'after', 중간 + 행 kind === 'group' 이면 'inside'. effectiveLocked 행은
   *     drop 거부.
   *   - drop 시 mutation:
   *       * 'before'/'after': dragged 들의 parent_id 를 target.parent_id 로
   *         교체. 그 parent 안 group 들의 order 를 dragged 위치에 맞춰 재책정.
   *         (item 은 sibling order field 없음 — 시각 위치 정확 일치 X, parent
   *         이동만 보장. BE schema 의 item order 추가 시점에 보강.)
   *       * 'inside': target 이 group 이어야 함. dragged 의 parent_id = target.id.
   *         dragged group 의 order = (max order in target) + 1.
   *   - Cycle 보호: dragged group 의 descendantGroups 에 target 의 ancestor 가
   *     포함되면 drop 거부. dragged 가 target 자신이거나 target.parent 이면 noop.
   *   - Layer mode === 'z' 일 때 drag 비활성 — z mode 는 rendering stack
   *     관점이라 reorder 가 z 변경을 의미하지 않음 (ADR-0024 D2 의 4 액션과
   *     혼동 방지). */

  type DropPos = 'before' | 'inside' | 'after';
  interface DragState {
    sourceIds: string[];
    invalidTargets: Set<string>;
  }
  let dragState = $state<DragState | null>(null);
  let dropTargetId = $state<string | null>(null);
  let dropTargetPos = $state<DropPos | null>(null);

  function groupDescendantIds(groupId: string): Set<string> {
    const out = new Set<string>([groupId]);
    let added = true;
    while (added) {
      added = false;
      for (const g of sessionStore.groups.values()) {
        if (g.parent_id !== null && out.has(g.parent_id) && !out.has(g.id)) {
          out.add(g.id);
          added = true;
        }
      }
    }
    return out;
  }

  function isItemLocked(id: string): boolean {
    const it = sessionStore.items.get(id);
    if (it !== undefined) return it.locked === true;
    const g = sessionStore.groups.get(id);
    if (g !== undefined) return g.locked === true;
    return false;
  }

  function onRowDragStart(id: string, e: DragEvent): void {
    if (layerMode === 'z') {
      e.preventDefault();
      return;
    }
    if (isItemLocked(id)) {
      e.preventDefault();
      return;
    }
    const dragged = sessionStore.M.has(id) && sessionStore.M.size > 0
      ? Array.from(sessionStore.M)
      : [id];
    // locked 가 섞여 있으면 unlocked 만 drag (silent).
    const draggable = dragged.filter((d) => !isItemLocked(d));
    if (draggable.length === 0) {
      e.preventDefault();
      return;
    }
    // Cycle 보호 대상 — dragged 중 group 인 것들의 descendant 합집합.
    const invalid = new Set<string>(draggable);
    for (const did of draggable) {
      if (sessionStore.groups.has(did)) {
        for (const desc of groupDescendantIds(did)) invalid.add(desc);
      }
    }
    dragState = { sourceIds: draggable, invalidTargets: invalid };
    if (e.dataTransfer !== null) {
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', draggable.join(','));
    }
  }

  function onRowDragOver(id: string, kind: 'group' | 'panel', e: DragEvent): void {
    const state = dragState;
    if (state === null) return;
    if (state.invalidTargets.has(id)) return;
    e.preventDefault();
    if (e.dataTransfer !== null) e.dataTransfer.dropEffect = 'move';
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const ratio = (e.clientY - rect.top) / rect.height;
    let pos: DropPos;
    if (ratio < 0.25) pos = 'before';
    else if (ratio > 0.75) pos = 'after';
    else pos = kind === 'group' ? 'inside' : (ratio < 0.5 ? 'before' : 'after');
    dropTargetId = id;
    dropTargetPos = pos;
  }

  function onRowDragLeave(id: string, _e: DragEvent): void {
    // 다른 행으로 진입하면 그쪽이 dragover 로 덮어쓰므로 noop 이지만, drop zone
    // 밖으로 나가는 경우 대비 약간 지연 clear (다음 dragover 까지의 깜빡임 회피).
    if (dropTargetId === id) {
      queueMicrotask(() => {
        if (dropTargetId === id) {
          dropTargetId = null;
          dropTargetPos = null;
        }
      });
    }
  }

  function onRowDrop(id: string, kind: 'group' | 'panel', e: DragEvent): void {
    const state = dragState;
    dragState = null;
    const pos = dropTargetPos;
    dropTargetId = null;
    dropTargetPos = null;
    if (state === null || pos === null) return;
    if (state.invalidTargets.has(id)) return;
    e.preventDefault();
    void commitReparent(state.sourceIds, id, kind, pos);
  }

  function onTreeDragEnd(_e: DragEvent): void {
    dragState = null;
    dropTargetId = null;
    dropTargetPos = null;
  }

  /**
   * Dragged ids 를 target 의 (parent_id, position) 으로 이동. Single
   * mutateActiveLayout call 로 items/groups 두 배열 동시 갱신.
   *
   * - 'inside' (target = group): dragged.parent_id = target.id, dragged group 의
   *   order = max(target 안 group order) + 1, item 은 order 무관.
   * - 'before' / 'after': dragged.parent_id = target.parent_id. group 만 sibling
   *   order 를 재배열 — dragged group 들을 target 의 order 직전/직후로 삽입 +
   *   남은 group 들 sequential 재번호.
   * - dragged 가 다중일 때 입력 순서 보존.
   */
  async function commitReparent(
    sourceIds: string[],
    targetId: string,
    targetKind: 'group' | 'panel',
    pos: DropPos,
  ): Promise<void> {
    // Resolve target's effective parent depending on pos.
    let parentTargetId: string | null;
    let beforeTargetId: string | null = null;
    if (pos === 'inside') {
      if (targetKind !== 'group') return;
      parentTargetId = targetId;
    } else {
      const targetGroup = sessionStore.groups.get(targetId);
      const targetItem = sessionStore.items.get(targetId);
      parentTargetId =
        (targetGroup?.parent_id ?? targetItem?.parent_id) ?? null;
      beforeTargetId = pos === 'before' ? targetId : null;
    }
    // No-op fast paths.
    if (sourceIds.length === 1 && sourceIds[0] === targetId) return;

    await mutateActiveLayout((cur) => {
      // 1) parent_id 갱신 (items + groups).
      const movedItemSet = new Set(
        sourceIds.filter((id) => cur.items.some((it) => it.id === id)),
      );
      const movedGroupSet = new Set(
        sourceIds.filter((id) => cur.groups.some((g) => g.id === id)),
      );
      const itemsNext = cur.items.map((it) =>
        movedItemSet.has(it.id) ? { ...it, parent_id: parentTargetId } : it,
      );
      const groupsParented = cur.groups.map((g) =>
        movedGroupSet.has(g.id) ? { ...g, parent_id: parentTargetId } : g,
      );

      // 2) Group sibling order 재배열 — 같은 parentTargetId 의 groups 만.
      const siblingsBefore = groupsParented
        .filter((g) => g.parent_id === parentTargetId)
        .sort((a, b) => (a.order ?? 0) - (b.order ?? 0));
      const movedInOrder = sourceIds
        .map((id) => siblingsBefore.find((g) => g.id === id))
        .filter((g): g is typeof siblingsBefore[number] => g !== undefined);
      const others = siblingsBefore.filter((g) => !movedGroupSet.has(g.id));
      const finalSequence: typeof siblingsBefore = [];
      if (pos === 'inside') {
        finalSequence.push(...others, ...movedInOrder);
      } else {
        // 'before' or 'after' a sibling row.
        for (const g of others) {
          if (g.id === beforeTargetId) {
            finalSequence.push(...movedInOrder);
          }
          finalSequence.push(g);
          if (pos === 'after' && g.id === targetId) {
            finalSequence.push(...movedInOrder);
          }
        }
        // beforeTargetId 가 others 안에 없거나 (target 이 group 이 아님), pos
        // 'after' 의 target 이 others 안에 없으면 끝에 append (fall-through).
        if (
          movedInOrder.length > 0 &&
          !finalSequence.some((g) => movedGroupSet.has(g.id))
        ) {
          finalSequence.push(...movedInOrder);
        }
      }
      // 재번호 — sparse → 1, 2, 3 … (충돌 방지).
      const reorderedIds = new Set(finalSequence.map((g) => g.id));
      const groupsNext = groupsParented.map((g) => {
        if (!reorderedIds.has(g.id)) return g;
        const idx = finalSequence.findIndex((f) => f.id === g.id);
        return { ...g, order: idx + 1 };
      });

      return { ...cur, items: itemsNext, groups: groupsNext };
    });
  }

  async function mutateActiveLayout(
    mutator: Parameters<typeof mutateLayout>[1],
  ): Promise<void> {
    const active = sessionStore.active;
    if (active === null) return;
    if (!(await ensureMutationOk('Layer mutation aborted — session reconnect failed.'))) return;
    try {
      const { layout } = await mutateLayout(active.name, mutator);
      sessionStore.loadLayout(layout);
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Layout update failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  function stopRowAction(e: MouseEvent): void {
    e.preventDefault();
    e.stopPropagation();
  }

  function togglePanelVisibility(id: string, e: MouseEvent): void {
    stopRowAction(e);
    const item = sessionStore.items.get(id);
    if (item === undefined) return;
    const nextVisibility = item.visibility === 'visible' ? 'hidden' : 'visible';
    void mutateActiveLayout((cur) => ({
      ...cur,
      items: cur.items.map((it) =>
        it.id === id ? ({ ...it, visibility: nextVisibility } as CanvasItem) : it,
      ),
    }));
  }

  function togglePanelLock(id: string, e: MouseEvent): void {
    stopRowAction(e);
    const item = sessionStore.items.get(id);
    if (item === undefined) return;
    const nextLocked = item.locked !== true;
    void mutateActiveLayout((cur) => ({
      ...cur,
      items: cur.items.map((it) =>
        it.id === id ? ({ ...it, locked: nextLocked } as CanvasItem) : it,
      ),
    }));
  }

  // plan-0010 Task 1 — focus row action.
  // min/max 는 PanelNode header 가 단일 entry — LayerTreeView 에서 제거.
  function focusPanel(id: string, e: MouseEvent): void {
    stopRowAction(e);
    sessionStore.zoomToItem(id);
  }

  function toggleGroupVisibility(id: string, e: MouseEvent): void {
    stopRowAction(e);
    const group = sessionStore.groups.get(id);
    if (group === undefined) return;
    const nextVisibility = group.visibility === 'visible' ? 'hidden' : 'visible';
    void mutateActiveLayout((cur) => ({
      ...cur,
      groups: cur.groups.map((g) =>
        g.id === id ? { ...g, visibility: nextVisibility } : g,
      ),
    }));
  }

  function toggleGroupLock(id: string, e: MouseEvent): void {
    stopRowAction(e);
    const group = sessionStore.groups.get(id);
    if (group === undefined) return;
    const nextLocked = group.locked !== true;
    void mutateActiveLayout((cur) => ({
      ...cur,
      groups: cur.groups.map((g) => (g.id === id ? { ...g, locked: nextLocked } : g)),
    }));
  }
</script>

<div class="layer-tree-view" aria-label="Layer tree">
  <div class="layer-tree-toolbar">
    <div class="mode-toggle" role="tablist" aria-label="Layer order mode">
      <button
        type="button"
        role="tab"
        class="mode-btn"
        class:active={layerMode === 'tree'}
        aria-selected={layerMode === 'tree'}
        title="Organization tree (parent_id grouping)"
        onclick={() => (layerMode = 'tree')}
      >Tree</button>
      <button
        type="button"
        role="tab"
        class="mode-btn"
        class:active={layerMode === 'z'}
        aria-selected={layerMode === 'z'}
        title="Rendering stack (z-index descending, no groups)"
        onclick={() => (layerMode = 'z')}
      >Z</button>
    </div>
  </div>
  <ul class="tree" role="tree">
    {#each tree as node (node.kind + ':' + node.id)}
      {#if node.kind === 'group'}
        {@const g = node.group}
        {@const selected = sessionStore.M.has(node.id)}
        {@const isOpen = expanded.has(node.id)}
        <li
          class="row group-row"
          class:selected
          class:drop-before={dropTargetId === node.id && dropTargetPos === 'before'}
          class:drop-inside={dropTargetId === node.id && dropTargetPos === 'inside'}
          class:drop-after={dropTargetId === node.id && dropTargetPos === 'after'}
          class:dragging={dragState !== null && dragState.sourceIds.includes(node.id)}
          role="treeitem"
          aria-expanded={node.hasChildren ? isOpen : undefined}
          aria-selected={selected}
          draggable={layerMode === 'tree' && !isItemLocked(node.id)}
          ondragstart={(e: DragEvent) => onRowDragStart(node.id, e)}
          ondragover={(e: DragEvent) => onRowDragOver(node.id, 'group', e)}
          ondragleave={(e: DragEvent) => onRowDragLeave(node.id, e)}
          ondrop={(e: DragEvent) => onRowDrop(node.id, 'group', e)}
          ondragend={onTreeDragEnd}
          style:padding-left={`${node.depth * 16 + 4}px`}
        >
          <div class="row-inner">
            <!-- caret 은 span (button 중첩 금지) — keyboard 접근은 row-button 의 Enter/Space 가
                 select 만 트리거하며 expand toggle 은 별도 키 (P1+에서 Right/Left arrow 처리). -->
            <span
              class="caret"
              class:caret-disabled={!node.hasChildren}
              role="presentation"
              onclick={(e: MouseEvent) => {
                e.stopPropagation();
                if (node.hasChildren) toggleExpand(node.id);
              }}
              onkeydown={() => {}}
            >
              {node.hasChildren ? (isOpen ? '▾' : '▸') : ''}
            </span>
            {#if editingGroupId === node.id}
              <span class="row-button row-button-edit">
                <InlineEditField
                  value={g.label ?? ''}
                  editing={true}
                  allowEmpty={true}
                  placeholder={node.id.slice(0, 8)}
                  class="group-label-edit"
                  onCommit={(next: string) => void onCommitRenameGroup(node.id, next)}
                  onCancel={onCancelRenameGroup}
                />
              </span>
            {:else}
              <button
                type="button"
                class="row-button"
                onclick={(e: MouseEvent) => selectNode(node.id, e)}
                ondblclick={(e: MouseEvent) => onStartRenameGroup(node.id, e)}
                title={`${groupDisplayLabel(g)} (double-click to rename)`}
              >
                <span class="label">{groupDisplayLabel(g)}</span>
              </button>
            {/if}
            {#snippet groupIcons()}
              {@const groupInheritedHidden = g.visibility !== false ? inheritedHiddenFrom(g.parent_id ?? null) : null}
              {@const groupInheritedLocked = g.locked !== true ? inheritedLockedFrom(g.parent_id ?? null) : null}
              <span class="icons" class:has-active={g.visibility === false || g.locked === true || groupInheritedHidden !== null || groupInheritedLocked !== null}>
              <button
                type="button"
                class="icon"
                class:on={g.visibility === false}
                class:inherited={groupInheritedHidden !== null}
                title={groupInheritedHidden !== null
                  ? `Hidden by ${inheritedSourceLabel(groupInheritedHidden)}`
                  : g.visibility === false ? 'Show group' : 'Hide group'}
                aria-label={g.visibility === false ? 'Show group' : 'Hide group'}
                onclick={(e: MouseEvent) => toggleGroupVisibility(node.id, e)}
              >
                {#if g.visibility === false}
                  <!-- EyeOff (lucide path) -->
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <path d="M9.88 9.88a3 3 0 1 0 4.24 4.24"/>
                    <path d="M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68"/>
                    <path d="M6.61 6.61A13.526 13.526 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61"/>
                    <line x1="2" y1="2" x2="22" y2="22"/>
                  </svg>
                {:else}
                  <!-- Eye -->
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z"/>
                    <circle cx="12" cy="12" r="3"/>
                  </svg>
                {/if}
              </button>
              <button
                type="button"
                class="icon"
                class:on={g.locked === true}
                class:inherited={groupInheritedLocked !== null}
                title={groupInheritedLocked !== null
                  ? `Locked by ${inheritedSourceLabel(groupInheritedLocked)}`
                  : g.locked === true ? 'Unlock group' : 'Lock group'}
                aria-label={g.locked === true ? 'Unlock group' : 'Lock group'}
                onclick={(e: MouseEvent) => toggleGroupLock(node.id, e)}
              >
                {#if g.locked === true}
                  <!-- Lock -->
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <rect x="3" y="11" width="18" height="11" rx="2"/>
                    <path d="M7 11V7a5 5 0 0 1 10 0v4"/>
                  </svg>
                {:else}
                  <!-- Unlock -->
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <rect x="3" y="11" width="18" height="11" rx="2"/>
                    <path d="M7 11V7a5 5 0 0 1 9.9-1"/>
                  </svg>
                {/if}
              </button>
              <button
                type="button"
                class="icon icon-close"
                title="Close group (bulk)"
                aria-label="Close group"
                onclick={(e: MouseEvent) => openGroupClose(node.id, e)}
              >
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <line x1="18" y1="6" x2="6" y2="18"/>
                  <line x1="6" y1="6" x2="18" y2="18"/>
                </svg>
              </button>
            </span>
            {/snippet}
            {@render groupIcons()}
          </div>
        </li>
      {:else}
        {@const p = node.panel}
        {@const selected = sessionStore.M.has(node.id)}
        {@const dead = isPanelDead(p)}
        <li
          class="row panel-row"
          class:selected
          class:dead
          class:drop-before={dropTargetId === node.id && dropTargetPos === 'before'}
          class:drop-after={dropTargetId === node.id && dropTargetPos === 'after'}
          class:dragging={dragState !== null && dragState.sourceIds.includes(node.id)}
          role="treeitem"
          aria-selected={selected}
          draggable={layerMode === 'tree' && !isItemLocked(node.id)}
          ondragstart={(e: DragEvent) => onRowDragStart(node.id, e)}
          ondragover={(e: DragEvent) => onRowDragOver(node.id, 'panel', e)}
          ondragleave={(e: DragEvent) => onRowDragLeave(node.id, e)}
          ondrop={(e: DragEvent) => onRowDrop(node.id, 'panel', e)}
          ondragend={onTreeDragEnd}
          style:padding-left={`${node.depth * 16 + 24}px`}
        >
          <div class="row-inner">
            <button
              type="button"
              class="row-button"
              onclick={(e: MouseEvent) => selectNode(node.id, e)}
              title={panelDisplayLabel(p)}
            >
              <span class="type-icon" aria-hidden="true">{panelTypeIcon(p)}</span>
              <span class="label">{panelDisplayLabel(p)}{dead ? ' (Dead)' : ''}</span>
              {#if layerMode === 'z' && typeof p.z === 'number'}
                <span class="z-tag mono" title="z-index">z={p.z}</span>
              {/if}
            </button>
            {#snippet panelIcons()}
              {@const panelInheritedHidden = p.visibility !== false ? inheritedHiddenFrom(p.parent_id ?? null) : null}
              {@const panelInheritedLocked = p.locked !== true ? inheritedLockedFrom(p.parent_id ?? null) : null}
              <span class="icons" class:has-active={p.visibility === false || p.locked === true || panelInheritedHidden !== null || panelInheritedLocked !== null}>
              <button
                type="button"
                class="icon"
                class:on={p.visibility === false}
                class:inherited={panelInheritedHidden !== null}
                title={panelInheritedHidden !== null
                  ? `Hidden by ${inheritedSourceLabel(panelInheritedHidden)}`
                  : p.visibility === false ? 'Show item' : 'Hide item'}
                aria-label={p.visibility === false ? 'Show item' : 'Hide item'}
                onclick={(e: MouseEvent) => togglePanelVisibility(node.id, e)}
              >
                {#if p.visibility === false}
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <path d="M9.88 9.88a3 3 0 1 0 4.24 4.24"/>
                    <path d="M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68"/>
                    <path d="M6.61 6.61A13.526 13.526 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61"/>
                    <line x1="2" y1="2" x2="22" y2="22"/>
                  </svg>
                {:else}
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z"/>
                    <circle cx="12" cy="12" r="3"/>
                  </svg>
                {/if}
              </button>
              <button
                type="button"
                class="icon"
                class:on={p.locked === true}
                class:inherited={panelInheritedLocked !== null}
                title={panelInheritedLocked !== null
                  ? `Locked by ${inheritedSourceLabel(panelInheritedLocked)}`
                  : p.locked === true ? 'Unlock item' : 'Lock item'}
                aria-label={p.locked === true ? 'Unlock item' : 'Lock item'}
                onclick={(e: MouseEvent) => togglePanelLock(node.id, e)}
              >
                {#if p.locked === true}
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <rect x="3" y="11" width="18" height="11" rx="2"/>
                    <path d="M7 11V7a5 5 0 0 1 10 0v4"/>
                  </svg>
                {:else}
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <rect x="3" y="11" width="18" height="11" rx="2"/>
                    <path d="M7 11V7a5 5 0 0 1 9.9-1"/>
                  </svg>
                {/if}
              </button>
              <!-- min/max 는 LayerTreeView 에서 제거 — PanelNode header 가 단일 entry.
                   focus 만 유지 (모든 type 에서 viewport zoom-to-item). -->
              <button
                type="button"
                class="icon"
                title="Focus item — zoom viewport to item"
                aria-label="Focus item"
                onclick={(e: MouseEvent) => focusPanel(node.id, e)}
              >
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <circle cx="12" cy="12" r="9" />
                  <circle cx="12" cy="12" r="3" fill="currentColor" />
                </svg>
              </button>
            </span>
            {/snippet}
            {@render panelIcons()}
          </div>
        </li>
      {/if}
    {:else}
      <li class="empty">No panels yet.</li>
    {/each}
  </ul>
</div>

<style>
  /* Embedded view — host (LeftPanel) owns floating chrome, fold + tabs.
   * Fills the available content area inside the active tab. */
  .layer-tree-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    font-size: var(--text-lg);
    line-height: var(--leading-normal);
    user-select: none;
  }

  .layer-tree-toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-6);
    padding: var(--space-6) var(--space-12);
    border-bottom: 1px solid var(--color-border);
    flex: 0 0 auto;
  }

  .tree {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    list-style: none;
    margin: 0;
    padding: var(--space-4) 0;
  }

  .row {
    display: block;
    position: relative;
  }

  /* Drag-reorder/reparent — drop indicator (ADR-0024 D1 layer list V2).
   * before/after = 2px accent line at top/bottom edge.
   * inside (group only) = accent tint background + dashed outline. */
  .row.dragging {
    opacity: 0.4;
  }
  .row.drop-before::before,
  .row.drop-after::after {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    height: 2px;
    background: var(--color-accent);
    pointer-events: none;
    z-index: 1;
  }
  .row.drop-before::before {
    top: 0;
  }
  .row.drop-after::after {
    bottom: 0;
  }
  .row.drop-inside {
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    outline: 1px dashed var(--color-accent);
    outline-offset: -1px;
  }

  /* Row hover / selected 는 row-inner 단위 — label / icons 가 *별도* hover 영역으로
   * 분절되지 않도록 두 child 를 담는 container 가 hover target. icon 자체의 추가
   * hover (background-tint 강화) 는 layered 으로 유지. */
  .row-inner {
    display: flex;
    align-items: center;
    gap: 0;
    width: 100%;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing),
      box-shadow var(--motion-fast) var(--motion-easing);
  }

  .row:hover .row-inner {
    background: var(--color-glass-1);
  }

  .row.selected .row-inner {
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    color: var(--color-accent);
    box-shadow: inset 2px 0 0 var(--color-accent);
  }

  .row-button {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex: 1 1 auto;
    padding: var(--space-2) var(--space-8) var(--space-2) 0;
    background: transparent;
    border: 0;
    color: inherit;
    text-align: left;
    cursor: pointer;
    font: inherit;
  }

  /* Inline-edit host occupies the same slot as the button — let the
   * embedded field fill it without breaking row layout. */
  .row-button-edit {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    padding: var(--space-2) var(--space-8) var(--space-2) 0;
    min-width: 0;
  }

  .row-button-edit :global(.group-label-edit) {
    flex: 1 1 auto;
    min-width: 0;
  }

  .row.dead .row-button .label {
    color: var(--color-fg-subtle);
    text-decoration: line-through;
  }

  /* Panel rows don't have a caret block — use the same padding-left as group rows so the
     leading edge of the label aligns with group labels. Achieved via inline style:padding-left
     (depth * 16 + 24) on the panel-row <li>. */

  .caret {
    width: 16px;
    flex: 0 0 16px;
    display: inline-block;
    text-align: center;
    color: var(--color-fg-muted);
    cursor: pointer;
    user-select: none;
  }

  .caret-disabled {
    cursor: default;
    color: transparent;
  }

  .label {
    flex: 1 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .type-icon {
    flex: 0 0 16px;
    width: 16px;
    text-align: center;
    font-family: var(--font-mono);
    font-size: var(--text-base);
    color: var(--color-fg-muted);
  }

  /* Icons (visibility / lock) — Figma 컨벤션:
   *   - 평소엔 숨김 (opacity 0)
   *   - row hover/selected 시 모두 표시
   *   - hidden/locked 상태는 호버 없어도 항상 표시 */
  .icons {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: var(--space-4);
    padding-right: var(--space-8);
    font-size: var(--text-base);
    color: var(--color-fg-muted);
    opacity: 0;
    transition: opacity var(--motion-fast) var(--motion-easing);
  }

  .row:hover .icons,
  .row.selected .icons,
  .icons.has-active {
    opacity: 1;
  }

  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-fg-muted);
    cursor: pointer;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .icon:hover {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  /* `.on` 상태 (visibility=false / locked=true) 강조. */
  .icon.on {
    color: var(--color-fg);
  }

  /* Group close (X) — destructive hover treatment. */
  .icon.icon-close:hover {
    background: var(--color-danger);
    color: white;
  }

  /* Inherited (ADR-0010 D6) — ancestor 가 visibility/lock 을 덮어쓰는 상태.
   * Self 는 정상이지만 effective 값이 다름. 작은 dot overlay + 회색 톤 으로
   * "건드려도 안 바뀜" 시각 단서. tooltip 으로 source group 알림. */
  .icon.inherited {
    position: relative;
    color: var(--color-fg-subtle);
    opacity: 0.7;
  }

  .icon.inherited::after {
    content: '';
    position: absolute;
    right: 1px;
    bottom: 1px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--color-fg-muted);
  }

  .empty {
    padding: var(--space-8) var(--space-12);
    color: var(--color-fg-subtle);
    font-style: italic;
  }

  /* Segmented Tree/Z toggle — ADR-0024 D1. */
  .mode-toggle {
    display: inline-flex;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    overflow: hidden;
    background: var(--color-surface);
    flex: 0 0 auto;
  }

  .mode-btn {
    padding: 1px var(--space-6);
    border: 0;
    background: transparent;
    color: var(--color-fg-muted);
    font: inherit;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    text-transform: uppercase;
    letter-spacing: 0.4px;
    cursor: pointer;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .mode-btn:hover {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .mode-btn.active {
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    color: var(--color-accent);
  }

  .z-tag {
    flex: 0 0 auto;
    margin-left: var(--space-4);
    padding: 0 4px;
    border-radius: var(--radius-pill);
    background: var(--color-surface-2);
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
    letter-spacing: 0.2px;
  }
</style>
