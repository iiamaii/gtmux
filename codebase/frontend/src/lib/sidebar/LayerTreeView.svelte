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

  import { getContext } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { muxStore } from '$lib/stores/mux.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import PanelEmptyState from '$lib/chrome/PanelEmptyState.svelte';
  import type { CanvasItem, CanvasItemType, CanvasLayout } from '$lib/types/canvas';
  import { groupHover } from '$lib/stores/groupHover.svelte';
  import { buildChildBlocks, normalizeLayout } from '$lib/stores/zSpace';
  import { renameItemLabel } from '$lib/canvas/terminalLabel';
  import { directParentGroupId, effectiveLocked } from '$lib/types/group';
  import InlineEditField from '$lib/common/InlineEditField.svelte';
  import { readExpandedTreeState, writeExpandedTreeState } from './treeExpansionState';
  import { matchNamePath } from '$lib/sidebar/treeMatch';
  import { ancestorIndices } from '$lib/sidebar/stickyAncestors';

  // ADR-0052 D2 — the single unified search bar now lives in LeftPanel's footer.
  // This component no longer renders its own input; it receives the active tab's
  // query text as a prop and filters in-memory off it. LeftPanel updates `query`
  // per keystroke; Layers filtering is cheap (all data is already in
  // sessionStore), so no local debounce is needed.
  let { query = '' }: { query?: string } = $props();

  interface ContextMenuHolder {
    openAt: (args: {
      clientX: number;
      clientY: number;
      paneId?: string | null;
      panelId?: string | null;
      groupId?: string | null;
      hidePaste?: boolean;
    }) => void;
  }

  const contextMenuHolder = getContext<ContextMenuHolder | undefined>('contextMenu');

  /** Currently inline-editing group id, or `null`. Component-local. */
  let editingGroupId = $state<string | null>(null);
  /** Currently inline-editing item id, or `null`. Component-local. */
  let editingItemId = $state<string | null>(null);

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

  function onStartRenameItem(id: string, e: MouseEvent): void {
    e.preventDefault();
    e.stopPropagation();
    editingItemId = id;
  }

  async function onCommitRenameItem(id: string, next: string): Promise<void> {
    editingItemId = null;
    const trimmed = next.trim();
    const item = sessionStore.items.get(id);
    if (item === undefined) return;
    // All item types (terminal included) persist the rename through the shared
    // layout-mutation path — terminal labels live in layout item.label per
    // ADR-0050 D2, no longer in the in-memory terminal_meta.
    await mutateActiveLayout((cur) => ({
      ...cur,
      items: cur.items.map((it) => (it.id === id ? renameItemLabel(it, trimmed) : it)),
    }));
  }

  function onCancelRenameItem(): void {
    editingItemId = null;
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
    title?: string;
    file_name?: string;
    /** ADR-0038 v8 §12.3 — snippets entries (key/body pairs). */
    entries?: ReadonlyArray<{ id: string; key: string; body: string }>;
    visibility?: boolean;
    locked?: boolean;
    minimized?: boolean;
    z?: number;
  }

  // 트리 노드 union — 사이드바 한 줄에 해당.
  type TreeNode =
    | {
        kind: 'group';
        id: string;
        depth: number;
        group: GroupData;
        hasChildren: boolean;
      }
    | { kind: 'panel'; id: string; depth: number; panel: PanelData };

  const LAYER_TREE_EXPANSION_STORAGE_KEY = 'gtmux:layer-tree-expanded:v1';
  const MAX_LAYER_TREE_EXPANSIONS = 200;

  const activeSessionName = $derived(sessionStore.active?.name ?? null);

  /* ── Layers search (ADR-0052 D6 — client-side filter + reveal) ─────────
   * web-only, ephemeral (D8). The query text comes from the `query` PROP owned
   * by LeftPanel's footer search bar (ADR-0052 D2). All layer data is already in
   * sessionStore, so the tree structure is KEPT while searching: a node is
   * "kept" when its label (or its ancestor-group label path) matches the query,
   * and every kept node's ancestor groups are revealed (force-expanded,
   * non-destructively) so the match is visible. No tmux/layout mutation.
   *
   * The prop is read directly for both filter and highlight — Layers filtering
   * is in-memory and cheap, so no debounce is needed (LeftPanel updates the prop
   * per keystroke). The prop resets naturally on tab unmount/remount. */
  const searching = $derived(query.trim().length > 0);

  // 펼침 상태 — 세션별로 localStorage 에 저장해 탭 전환/새로고침 후에도 복원.
  const expanded = new SvelteSet<string>();
  let restoredExpansionKey = $state<string | null | undefined>(undefined);

  function expandAncestorsOf(id: string): void {
    let parentId = directParentGroupId(id, sessionStore.items, sessionStore.groups);
    const seen = new Set<string>();
    let changed = false;
    while (parentId !== null && !seen.has(parentId)) {
      seen.add(parentId);
      if (!expanded.has(parentId)) {
        expanded.add(parentId);
        changed = true;
      }
      parentId = directParentGroupId(parentId, sessionStore.items, sessionStore.groups);
    }
    if (changed) persistExpandedGroups();
  }

  $effect(() => {
    for (const id of sessionStore.M) {
      expandAncestorsOf(id);
    }
  });

  $effect(() => {
    if (editingGroupId !== null && !sessionStore.M.has(editingGroupId)) {
      editingGroupId = null;
    }
    if (editingItemId !== null && !sessionStore.M.has(editingItemId)) {
      editingItemId = null;
    }
  });

  // ADR-0024 의 2026-05-22 ② amend (Tree=Z): Sidebar 는 *단일 view* (Tree).
  // 옛 Z tab 의 group atomic row + fold/unfold 는 Tree 가 이미 동일 affordance 제공
  // → Z tab UI 폐기. z-index 값 표시는 Inspector 로 단일화.

  /* ── Search kept-set + reveal (ADR-0052 D6) ───────────────────────────
   * Compute, over the *full* in-memory group/panel data, which nodes are kept
   * for the current query and which ancestor groups must be revealed so each
   * match stays visible. `labelPath` for a node = the `/`-joined ancestor group
   * labels + own label, so a query can match by ancestor path too (matchNamePath
   * uses `relpath` as the second candidate key). A node is kept when
   * matchNamePath(query, ownLabel, labelPath).matched; for every kept node, its
   * ancestor groups are added to the reveal set. Branches with no kept descendant
   * stay hidden because `visibleTree` filters on the kept ∪ ancestor sets. */
  interface SearchSets {
    kept: Set<string>;
    forced: Set<string>;
  }

  const searchSets = $derived.by<SearchSets>(() => {
    const kept = new Set<string>();
    const forced = new Set<string>();
    if (!searching) return { kept, forced };

    const q = query;

    // Ancestor-group label chain (root → … → direct parent) for a node, using
    // the same groups map walkAncestors reads. Outermost first.
    const ancestorLabels = (parentId: string | null): { ids: string[]; labels: string[] } => {
      const ids: string[] = [];
      const labels: string[] = [];
      // walkAncestors returns nearest-first; reverse to outermost-first.
      const chain = walkAncestors(parentId);
      for (let i = chain.length - 1; i >= 0; i -= 1) {
        const node = chain[i];
        if (node === undefined) continue;
        const g = sessionStore.groups.get(node.id);
        if (g === undefined) continue;
        ids.push(g.id);
        labels.push(groupDisplayLabel(g));
      }
      return { ids, labels };
    };

    const consider = (id: string, ownLabel: string, parentId: string | null): void => {
      const { ids, labels } = ancestorLabels(parentId);
      const labelPath = [...labels, ownLabel].join('/');
      if (matchNamePath(q, ownLabel, labelPath).matched) {
        kept.add(id);
        // Reveal every ancestor group so the match is visible without mutating
        // the persisted `expanded` set.
        for (const aid of ids) {
          kept.add(aid);
          forced.add(aid);
        }
      }
    };

    for (const g of sessionStore.groups.values()) {
      consider(g.id, groupDisplayLabel(g), g.parent_id ?? null);
    }
    for (const it of sessionStore.items.values()) {
      const panelView: PanelData = {
        id: it.id,
        parent_id: it.parent_id,
        pane_id: it.type === 'terminal' ? it.id : undefined,
        type: it.type,
        label: it.label ?? null,
        title: it.type === 'note' ? it.title : undefined,
        file_name: it.type === 'document' ? it.file_name : undefined,
        entries: it.type === 'snippets' ? it.entries : undefined,
      };
      consider(it.id, panelDisplayLabel(panelView), it.parent_id ?? null);
    }
    return { kept, forced };
  });

  /**
   * Effective expansion used by the tree walk: the persisted `expanded` set,
   * unioned with the search-forced ancestor set while searching. This keeps the
   * reveal non-destructive — `expanded` (and its localStorage mirror) is never
   * mutated by search, so clearing the query restores the user's expand state.
   */
  const effectiveExpanded = $derived.by<(id: string) => boolean>(() => {
    if (!searching) {
      // Reference `expanded` for reactivity even on the non-search path.
      void expanded.size;
      return (id: string) => expanded.has(id);
    }
    const forced = searchSets.forced;
    return (id: string) => expanded.has(id) || forced.has(id);
  });

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
      title: it.type === 'note' ? it.title : undefined,
      file_name: it.type === 'document' ? it.file_name : undefined,
      entries: it.type === 'snippets' ? it.entries : undefined,
      visibility: it.visibility === 'visible',
      locked: it.locked,
      minimized: it.minimized,
      z: it.z,
    }));

    // 부모 별 children 인덱스 — null = canvas root. 두 mode 공용.
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

    // ADR-0024 2026-05-22 ② amend (Tree=Z): sidebar row order is the z order.
    // buildChildBlocks returns each parent level in ascending z; visual tree shows
    // front-most first, so render the list in reverse order at every depth.
    const layoutForZ: CanvasLayout = {
      schema_version: 2,
      items: Array.from(sessionStore.items.values()),
      groups: Array.from(sessionStore.groups.values()),
      viewport: sessionStore.viewport,
    };
    const zChildren = buildChildBlocks(layoutForZ);
    const groupById = new Map(groups.map((g) => [g.id, g] as const));
    const panelById = new Map(panels.map((p) => [p.id, p] as const));

    const out: TreeNode[] = [];
    const walk = (parentId: string | null, depth: number): void => {
      const blocks = [...(zChildren.get(parentId) ?? [])].reverse();
      for (const block of blocks) {
        if (block.kind === 'group') {
          const g = groupById.get(block.id);
          if (g === undefined) continue;
          const ownChildren =
            (childGroups.get(g.id)?.length ?? 0) + (childPanels.get(g.id)?.length ?? 0);
          out.push({
            kind: 'group',
            id: g.id,
            depth,
            group: g,
            hasChildren: ownChildren > 0,
          });
          // Use effective expansion so search can reveal ancestor groups
          // without mutating the persisted `expanded` set (ADR-0052 D6).
          if (effectiveExpanded(g.id) && ownChildren > 0) {
            walk(g.id, depth + 1);
          }
        } else {
          const p = panelById.get(block.id);
          if (p === undefined) continue;
          out.push({ kind: 'panel', id: p.id, depth, panel: p });
        }
      }
    };
    walk(null, 0);
    return out;
  });

  /* Visible tree (ADR-0052 D6) — while searching, drop rows that are neither a
   * match nor a revealed ancestor; the structure (z-order, depth, indent) is
   * preserved. When not searching this is `tree` unchanged. All selection / drag
   * / sticky logic reads `visibleTree` so they stay in sync with what is on
   * screen. */
  const visibleTree = $derived.by<TreeNode[]>(() => {
    if (!searching) return tree;
    const kept = searchSets.kept;
    return tree.filter((n) => kept.has(n.id));
  });

  // Panel.pane_id (e.g. "%3") → muxStore.panes 의 정수 key (3) 변환.
  // SSoT pattern `^%[0-9]+$` 이므로 substring(1) 은 안전. NaN 방어만.
  function paneNumeric(paneIdStr: string | undefined): number | null {
    if (!paneIdStr || paneIdStr[0] !== '%') return null;
    const n = Number.parseInt(paneIdStr.slice(1), 10);
    return Number.isNaN(n) ? null : n;
  }

  // Panel 행 표시 라벨 우선순위 (ADR-0050 D3 — layout item.label 정본):
  //   1) layout Panel.label (persisted, per-panel). terminal/panel type 도
  //      동일 — terminal_meta 우선순위 제거(매 부팅 소실되던 stale source).
  //   2) "%${paneNum}" — pane id fallback (terminal/panel)
  //   3) "${type}:${shortId}"
  function panelDisplayLabel(p: PanelData): string {
    if (p.type === 'note' && p.title != null && p.title.length > 0) return p.title;
    if (p.type === 'snippets') {
      // Sync with canvas head displayLabel + inspector identity. User-set
      // label takes precedence; "Snippets" is the type fallback. Entry
      // count is communicated via the inline `.snippets-count` badge — no
      // need to duplicate firstKey in the label string.
      if (p.label != null && p.label.length > 0) return p.label;
      return 'Snippets';
    }
    if (p.label != null && p.label.length > 0) return p.label;
    if (p.type === 'document' && p.file_name != null && p.file_name.length > 0) {
      const base = p.file_name.trim().split('/').pop() ?? p.file_name.trim();
      const dot = base.lastIndexOf('.');
      return dot > 0 ? base.slice(0, dot) : base;
    }
    const n = paneNumeric(p.pane_id);
    if (n !== null) return `%${n}`;
    const type = p.type === 'file_path' ? 'file' : (p.type ?? 'panel');
    return `${type}:${p.id.slice(0, 8)}`;
  }

  /** ADR-0038 v8 §12.3 — snippets row inline count badge (entries.length). */
  function snippetsCount(p: PanelData): number | null {
    if (p.type !== 'snippets') return null;
    return p.entries?.length ?? 0;
  }

  function panelTypeIcon(p: PanelData): string {
    // Character fallback — only used for unknown/legacy types. Known types
    // render via the typeIconSvg snippet below (full inline SVG matching
    // the toolbar icon for visual consistency).
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
      case 'path':
        return '↱';
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
  // Accepts any object with `{ id, label? }` — both the GroupData view and the
  // session-store Group (whose `visibility` is a string enum) satisfy this, so
  // search code can resolve a group label without an unsound `as GroupData` cast.
  function groupDisplayLabel(g: { id: string; label?: string | null }): string {
    if (g.label != null && g.label.length > 0) return g.label;
    return g.id;
  }

  /* ── Search highlight (ADR-0052 D8 — text-safe, no innerHTML) ──────────
   * Split a label into alternating {text, hit} segments from matchNamePath
   * `ranges` so the template can wrap matched runs in <mark> via {#each}. All
   * segment text is rendered as Svelte text (auto-escaped) — user input is never
   * passed through innerHTML (CLAUDE.md invariant 4). When not searching (or no
   * ranges) the whole label is one non-hit segment. `labelPath` is passed so a
   * path-only match still highlights name occurrences consistently. */
  interface LabelSegment {
    text: string;
    hit: boolean;
  }

  function labelSegments(label: string, labelPath: string): LabelSegment[] {
    if (!searching) return [{ text: label, hit: false }];
    const { matched, ranges } = matchNamePath(query, label, labelPath);
    if (!matched || ranges.length === 0) return [{ text: label, hit: false }];
    const segs: LabelSegment[] = [];
    let cursor = 0;
    for (const [start, end] of ranges) {
      if (start > cursor) segs.push({ text: label.slice(cursor, start), hit: false });
      segs.push({ text: label.slice(start, end), hit: true });
      cursor = end;
    }
    if (cursor < label.length) segs.push({ text: label.slice(cursor), hit: false });
    return segs;
  }

  /** `/`-joined ancestor group label path + own label — the second match key. */
  function nodeLabelPath(parentId: string | null, ownLabel: string): string {
    const chain = walkAncestors(parentId); // nearest-first
    const labels: string[] = [];
    for (let i = chain.length - 1; i >= 0; i -= 1) {
      const node = chain[i];
      if (node === undefined) continue;
      const g = sessionStore.groups.get(node.id);
      if (g !== undefined) labels.push(groupDisplayLabel(g));
    }
    labels.push(ownLabel);
    return labels.join('/');
  }

  function toggleExpand(id: string): void {
    if (expanded.has(id)) expanded.delete(id);
    else expanded.add(id);
    persistExpandedGroups();
  }

  /**
   * Selection anchor — 단일 클릭이 직전에 set 한 row id. Shift+click 의 range
   * 시작점. anchor 가 visible tree 에서 사라지면 (예: ancestor collapse) 첫
   * shift-click 이 fallback 으로 target 만 add.
   */
  let selectionAnchor = $state<string | null>(null);

  $effect(() => {
    if (activeSessionName === restoredExpansionKey) return;
    restoredExpansionKey = activeSessionName;
    selectionAnchor = null;
    expanded.clear();
    for (const id of readExpandedTreeState(
      LAYER_TREE_EXPANSION_STORAGE_KEY,
      layerTreeStateKey(activeSessionName),
    )) {
      expanded.add(id);
    }
  });

  $effect(() => {
    if (sessionStore.M.size === 0) selectionAnchor = null;
  });

  function layerTreeStateKey(sessionName: string | null): string | null {
    return sessionName;
  }

  function persistExpandedGroups(): void {
    writeExpandedTreeState(
      LAYER_TREE_EXPANSION_STORAGE_KEY,
      layerTreeStateKey(activeSessionName),
      expanded,
      MAX_LAYER_TREE_EXPANSIONS,
    );
  }

  function applyDrillForTreeSelection(ids: readonly string[]): void {
    const parentIds = ids.map((rowId) =>
      directParentGroupId(rowId, sessionStore.items, sessionStore.groups),
    );
    const first = parentIds[0] ?? null;
    if (ids.length === 1) {
      sessionStore.setDrillRoot(first);
      return;
    }
    if (first !== null && parentIds.every((parentId) => parentId === first)) {
      sessionStore.setDrillRoot(first);
      return;
    }
    sessionStore.clearDrill();
  }

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
          applyDrillForTreeSelection([...sessionStore.M]);
          if (anchor === null) selectionAnchor = id;
          return;
        }
        const ids = visibleRangeIds(anchor, id);
        if (ids.length === 0) {
          sessionStore.toggleM(id);
          applyDrillForTreeSelection([...sessionStore.M]);
          return;
        }
        // setM 으로 anchor↔target range 만 선택. (multi-select 의 일반 직관 —
        // shift 는 range *select*, ctrl 와 결합 시에만 add-to.)
        if (e.metaKey || e.ctrlKey) {
          for (const rid of ids) sessionStore.addToM(rid);
        } else {
          sessionStore.setM(ids);
        }
        applyDrillForTreeSelection([...sessionStore.M]);
        return;
      }
      if (e.metaKey || e.ctrlKey) {
        sessionStore.toggleM(id);
        applyDrillForTreeSelection([...sessionStore.M]);
        selectionAnchor = id;
        return;
      }
    }
    sessionStore.setM([id]);
    applyDrillForTreeSelection([id]);
    selectionAnchor = id;
  }

  function onPanelContextMenu(id: string, p: PanelData, e: MouseEvent): void {
    e.preventDefault();
    e.stopPropagation();
    const target = id;
    if (sessionStore.M.size >= 2 && sessionStore.M.has(target)) {
      selectionAnchor = target;
      contextMenuHolder?.openAt({
        clientX: e.clientX,
        clientY: e.clientY,
        paneId: null,
        panelId: target,
        hidePaste: true,
      });
      return;
    }
    sessionStore.setM([target]);
    sessionStore.setDrillRoot(directParentGroupId(id, sessionStore.items, sessionStore.groups));
    selectionAnchor = target;
    contextMenuHolder?.openAt({
      clientX: e.clientX,
      clientY: e.clientY,
      paneId: p.type === 'terminal' || p.type === 'panel' ? (p.pane_id ?? id) : null,
      panelId: target,
      hidePaste: true,
    });
  }

  /**
   * ADR-0010 D16 + plan-0012 §3.3 B / §3.4 D — Sidebar group row right-click →
   * ContextMenu 의 groupEntity mode. M = {groupId} 로 set 후 broker 경유와 동일하게
   * `openAt({groupId})`.
   */
  function onGroupContextMenu(id: string, e: MouseEvent): void {
    e.preventDefault();
    e.stopPropagation();
    if (sessionStore.M.size >= 2 && sessionStore.M.has(id)) {
      selectionAnchor = id;
      contextMenuHolder?.openAt({
        clientX: e.clientX,
        clientY: e.clientY,
        paneId: null,
        panelId: id,
        hidePaste: true,
      });
      return;
    }
    sessionStore.setM([id]);
    sessionStore.setDrillRoot(directParentGroupId(id, sessionStore.items, sessionStore.groups));
    selectionAnchor = id;
    contextMenuHolder?.openAt({
      clientX: e.clientX,
      clientY: e.clientY,
      paneId: null,
      panelId: null,
      groupId: id,
      hidePaste: true,
    });
  }

  /**
   * Visible tree 안에서 두 row id 사이의 inclusive range 를 순서대로 반환.
   * 두 id 중 어느 쪽이 위인지 무관 — visible tree 의 순방향 (위→아래) 정렬.
   * 둘 중 하나라도 invisible 면 빈 배열.
   */
  function visibleRangeIds(a: string, b: string): string[] {
    const order = visibleTree.map((n) => n.id);
    const ia = order.indexOf(a);
    const ib = order.indexOf(b);
    if (ia < 0 || ib < 0) return [];
    const [lo, hi] = ia <= ib ? [ia, ib] : [ib, ia];
    return order.slice(lo, hi + 1);
  }

  function onLayerTreeBackgroundClick(e: MouseEvent): void {
    const target = e.target as HTMLElement | null;
    if (target?.closest('.row') !== null) return;
    sessionStore.clearM();
    sessionStore.clearDrill();
    selectionAnchor = null;
  }

  /* ── Sticky parent headers (ADR-0052 D7 — VSCode-style sticky scroll) ──
   * When NOT searching, pin the FULL ancestor chain (root → … → direct parent)
   * of the topmost visible row to the top of the `.tree` scroll container,
   * stacked like VSCode. The tree is non-virtualized with uniform row pitch, so
   * the chain is pure arithmetic over `visibleTree`:
   *   rowHeight  = measured per-row PITCH (see measureRowPitch)
   *   topIndex   = floor(scrollTop / rowHeight)
   *   indices    = ancestorIndices(visibleTree, topIndex, MAX_STICKY)
   * `ancestorIndices` already returns the full chain top-down; the earlier bug
   * (only the top-most ancestor showing) was a rowHeight measurement error, not
   * an algorithm error — see measureRowPitch. Recomputes on scroll, on mount,
   * and whenever the visible tree changes. Hidden while searching (the filtered
   * tree has no reliable hierarchy to pin). */
  const MAX_STICKY = 6;
  const STICKY_ROW_HEIGHT_FALLBACK = 28;
  const STICKY_STACK_BORDER = 1; // .sticky-stack border-bottom (px)

  let treeScrollEl = $state<HTMLUListElement | null>(null);
  let scrollTop = $state(0);
  let rowHeight = $state(STICKY_ROW_HEIGHT_FALLBACK);

  /**
   * Measure the true per-row vertical PITCH of a single tree row.
   *
   * Bug fix (ADR-0052 D7): the row pitch is NOT `.row` `offsetHeight`. Rows are
   * separated by `.row + .row { margin-top: 2px }`, and margins live OUTSIDE the
   * offset box, so `offsetHeight` undercounts the real advance per index. A wrong
   * pitch makes `topIndex = floor(scrollTop / rowHeight)` inaccurate at depth: an
   * over-estimated pitch yields a too-small `topIndex`, so the scan starts above
   * the real top row and only the outermost ancestor is recovered (the reported
   * symptom — only the top-level group sticks).
   *
   * Robust measure: take the offset DELTA between the first two real tree rows
   * (`row1.offsetTop - row0.offsetTop`). This is the exact pitch including any
   * inter-row margin, and is immune to which element is the row vs. a wrapper.
   * We explicitly query `.row` (the `<li>` tree rows) so the taller `.sticky-row`
   * stack is never measured. Fallbacks: single-row `offsetHeight`, then the CSS
   * constant.
   */
  function measureRowPitch(): void {
    const el = treeScrollEl;
    if (el === null) return;
    const rows = el.querySelectorAll('.row');
    const row0 = rows[0] as HTMLElement | undefined;
    if (row0 === undefined) return;
    const row1 = rows[1] as HTMLElement | undefined;
    if (row1 !== undefined) {
      const pitch = row1.offsetTop - row0.offsetTop;
      if (pitch > 0) {
        rowHeight = pitch;
        return;
      }
    }
    // Single row visible — fall back to its own height (no inter-row delta yet).
    const h = row0.offsetHeight;
    if (h > 0) rowHeight = h;
  }

  function onTreeScroll(e: Event): void {
    scrollTop = (e.currentTarget as HTMLElement).scrollTop;
  }

  // Re-measure the row pitch on mount and whenever the visible tree changes
  // (rows may have just mounted / changed depth). Keyed on the live scroll el so
  // it also runs once `bind:this` resolves after the first render.
  $effect(() => {
    // Touch the scroll el + visibleTree so this re-runs on structural change.
    void treeScrollEl;
    void visibleTree.length;
    measureRowPitch();
  });

  // Topmost fully-or-partially visible row index from the live scroll position.
  const stickyTopIndex = $derived.by<number>(() => {
    if (rowHeight <= 0) return 0;
    const idx = Math.floor(scrollTop / rowHeight);
    return idx < 0 ? 0 : idx;
  });

  // Ancestor rows (TreeNode + their flattened index) to pin, top-down.
  const stickyRows = $derived.by<Array<{ index: number; node: TreeNode }>>(() => {
    if (searching) return [];
    const indices = ancestorIndices(visibleTree, stickyTopIndex, MAX_STICKY);
    const out: Array<{ index: number; node: TreeNode }> = [];
    for (const idx of indices) {
      const node = visibleTree[idx];
      if (node !== undefined) out.push({ index: idx, node });
    }
    return out;
  });

  /**
   * Click a sticky header → reveal that ancestor row just BELOW the sticky
   * ancestors that stay pinned for it (ADR-0052 D7), not at content-top where its
   * own pinned ancestors would cover it. `btn.offsetTop` within the absolutely-
   * positioned `.sticky-stack` is exactly the cumulative height of the ancestors
   * above the clicked row — the strip that will remain occluded after the jump.
   */
  function scrollRowToTop(index: number, btn?: HTMLElement): void {
    const el = treeScrollEl;
    if (el === null) return;
    const stickyOffset =
      btn && btn.offsetTop > 0 ? btn.offsetTop + STICKY_STACK_BORDER : 0;
    el.scrollTop = Math.max(0, index * rowHeight - stickyOffset);
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
    if (it !== undefined) {
      return effectiveLocked(it.locked, it.parent_id, sessionStore.groups);
    }
    const g = sessionStore.groups.get(id);
    if (g !== undefined) {
      return effectiveLocked(g.locked, g.parent_id, sessionStore.groups);
    }
    return false;
  }

  function onRowDragStart(id: string, e: DragEvent): void {
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

      // 3) ADR-0024 D13 (2026-05-22 amend) — drop indicator 별 z 효과.
      //    'inside' → moved 가 target group 의 자손 *top* (max z + 1).
      //    'before' → moved 가 target 의 *higher z* slot (after target in ascending z order).
      //    'after'  → moved 가 target 의 *lower z* slot (before target).
      // 변경 후 normalizeLayout 으로 consecutive invariant 정합.
      const interim: CanvasLayout = { ...cur, items: itemsNext, groups: groupsNext };
      const childMap = buildChildBlocks(interim);
      const currentBlocks = (childMap.get(parentTargetId) ?? []).map((b) => b.id);
      const movedAll = new Set(sourceIds);
      const movedOrdered = currentBlocks.filter((id) => movedAll.has(id));
      const otherBlocks = currentBlocks.filter((id) => !movedAll.has(id));
      let newOrder: string[];
      if (pos === 'inside') {
        // moved 가 target group 의 자손 top.
        newOrder = [...otherBlocks, ...movedOrdered];
      } else {
        newOrder = [];
        let inserted = false;
        for (const id of otherBlocks) {
          if (id === targetId) {
            if (pos === 'after') {
              // moved → lower z than target.
              newOrder.push(...movedOrdered);
              newOrder.push(id);
            } else {
              // 'before' → higher z than target.
              newOrder.push(id);
              newOrder.push(...movedOrdered);
            }
            inserted = true;
          } else {
            newOrder.push(id);
          }
        }
        if (!inserted) {
          // Defensive — target 이 같은 parent 안 없으면 끝에 append.
          newOrder.push(...movedOrdered);
        }
      }
      const overrides = new Map<string | null, readonly string[]>();
      overrides.set(parentTargetId, newOrder);
      return normalizeLayout(interim, overrides);
    });
  }

  async function mutateActiveLayout(
    mutator: (cur: CanvasLayout) => CanvasLayout,
  ): Promise<void> {
    await sessionStore.applyMutation(mutator, {
      abortMessage: 'Layer mutation aborted — session reconnect failed.',
      failMessage: 'Layout update failed',
    });
  }

  function stopRowAction(e: MouseEvent): void {
    e.preventDefault();
    e.stopPropagation();
  }

  function togglePanelVisibility(id: string, e: MouseEvent): void {
    stopRowAction(e);
    const item = sessionStore.items.get(id);
    if (item === undefined) return;
    if (inheritedHiddenFrom(item.parent_id ?? null) !== null) return;
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
    if (inheritedLockedFrom(item.parent_id ?? null) !== null) return;
    const nextLocked = item.locked !== true;
    void mutateActiveLayout((cur) => ({
      ...cur,
      items: cur.items.map((it) =>
        it.id === id ? ({ ...it, locked: nextLocked } as CanvasItem) : it,
      ),
    }));
  }

  // Focus 는 ViewportCtrl 로 이동 — Layer row 의 select 후 ViewportCtrl 에서
  // focus 트리거. focusPanel / zoomToItem 호출은 본 file 에서 제거.
  // ADR-0024 의 2026-05-22 ② amend — Z mode UI 폐기. Tree drag + ContextMenu 가
  // z mutation 의 진입점이라 본 file 의 zMoveUp/Down 행 버튼은 제거됨.

  function toggleGroupVisibility(id: string, e: MouseEvent): void {
    stopRowAction(e);
    const group = sessionStore.groups.get(id);
    if (group === undefined) return;
    if (inheritedHiddenFrom(group.parent_id ?? null) !== null) return;
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
    if (inheritedLockedFrom(group.parent_id ?? null) !== null) return;
    const nextLocked = group.locked !== true;
    void mutateActiveLayout((cur) => ({
      ...cur,
      groups: cur.groups.map((g) => (g.id === id ? { ...g, locked: nextLocked } : g)),
    }));
  }
</script>

<!-- Reusable per-type icon block. Single source of truth for layer row
     icons across the editing + display branches. note/snippets use 12-unit
     paths matching their canvas-head glyphs; the rest use the same 24-unit
     paths as Toolbar2 for consistency between toolbar and layer tree. -->
{#snippet typeIconSvg(p: PanelData)}
  <span class="type-icon" aria-hidden="true">
    {#if p.type === 'note'}
      <svg width="13" height="13" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round">
        <rect x="1.5" y="2" width="9" height="8" rx="1.5"/>
        <path d="M3.5 4.5h5M3.5 6.5h5M3.5 8.5h3"/>
      </svg>
    {:else if p.type === 'snippets'}
      <svg width="13" height="13" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round">
        <rect x="1.5" y="1.5" width="9" height="9" rx="1"/>
        <path d="M3.5 3.5v5"/>
        <path d="M5.5 3.5v5"/>
        <path d="m7.5 3.5 1 5"/>
      </svg>
    {:else if p.type === 'terminal' || p.type === 'panel' || p.type == null}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <rect x="3" y="4" width="18" height="16" rx="2"/>
        <path d="M7 9l3 3-3 3"/>
        <path d="M13 15h4"/>
      </svg>
    {:else if p.type === 'text'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round">
        <path d="M5 5h14M12 5v14M9 19h6"/>
      </svg>
    {:else if p.type === 'rect'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <rect x="4" y="5" width="16" height="14" rx="1.5"/>
      </svg>
    {:else if p.type === 'ellipse'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <ellipse cx="12" cy="12" rx="8.5" ry="7"/>
      </svg>
    {:else if p.type === 'line'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <line x1="4.5" y1="19" x2="19.5" y2="5"/>
      </svg>
    {:else if p.type === 'path'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <path d="M4 18h6V7h8"/>
        <path d="m15 4 3 3-3 3"/>
      </svg>
    {:else if p.type === 'free_draw'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <path d="M4 17c2-4 4-2 6-5s2-7 5-7 5 4 5 6"/>
      </svg>
    {:else if p.type === 'image'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <rect x="3" y="4" width="18" height="16" rx="2"/>
        <circle cx="9" cy="10" r="1.5"/>
        <path d="M3 17l5-4 4 3 5-5 4 4"/>
      </svg>
    {:else if p.type === 'document'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <path d="M6 3h8l4 4v14a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z"/>
        <path d="M14 3v4h4"/>
        <path d="M8 13h8M8 17h5"/>
      </svg>
    {:else if p.type === 'file_path'}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
        <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>
      </svg>
    {:else}
      {panelTypeIcon(p)}
    {/if}
  </span>
{/snippet}

<!-- Reusable highlighted-label snippet — renders matchNamePath segments as
     text-safe runs (auto-escaped); matched runs wrapped in <mark>. No innerHTML
     (ADR-0052 D8). -->
{#snippet highlightedLabel(label: string, labelPath: string, suffix: string)}{#each labelSegments(label, labelPath) as seg, i (i)}{#if seg.hit}<mark class="search-hit">{seg.text}</mark>{:else}{seg.text}{/if}{/each}{suffix}{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="layer-tree-view" aria-label="Layer tree" onclick={onLayerTreeBackgroundClick} onkeydown={() => {}}>
  <!-- ADR-0024 의 2026-05-22 ② amend (Tree=Z) — Z tab UI 제거. Tree 가 단일 view.
       z-index 값 표시는 Inspector 로 단일화. -->

  <!-- ADR-0052 D2 — the search input is owned by LeftPanel's footer; this
       component receives the active query as the `query` prop and only renders
       the (filtered) tree. -->

  {#if tree.length === 0}
    <PanelEmptyState
      icon="layers"
      lead="No canvas items"
      description="Add items from the toolbar to build the layer tree."
    />
  {:else if visibleTree.length === 0}
    <PanelEmptyState
      icon="layers"
      lead="No matching layers"
      description="No group or panel label matches the search."
    />
  {:else}
    <div class="tree-viewport">
      <!-- ADR-0052 D7 (clarify amend ③) — sticky ancestor overlay lives OUTSIDE
           the scrolling <ul>, as a sibling pinned at CSS top:0 in this
           non-scrolling relative wrapper. No per-scroll JS positioning → it
           rides the compositor with the rows instead of chasing scrollTop a
           frame behind (the prior jitter). Mirrors FileTreeView. -->
      {#if stickyRows.length > 0}
        <div
          class="sticky-stack"
          aria-hidden="true"
        >
          {#each stickyRows as sr (sr.node.id)}
            {#if sr.node.kind === 'group'}
              {@const sg = sr.node.group}
              <button
                type="button"
                class="sticky-row"
                tabindex="-1"
                style:padding-left={`${sr.node.depth * 16 + 4}px`}
                title={groupDisplayLabel(sg)}
                onclick={(e) => scrollRowToTop(sr.index, e.currentTarget as HTMLElement)}
              >
                <span class="type-icon group-type-icon" aria-hidden="true">
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M4 8V5a1 1 0 0 1 1-1h3"/>
                    <path d="M16 4h3a1 1 0 0 1 1 1v3"/>
                    <path d="M20 16v3a1 1 0 0 1-1 1h-3"/>
                    <path d="M8 20H5a1 1 0 0 1-1-1v-3"/>
                    <rect x="8" y="8" width="4" height="4" rx="0.8"/>
                    <rect x="13" y="13" width="3" height="3" rx="0.7"/>
                  </svg>
                </span>
                <span class="label">{groupDisplayLabel(sg)}</span>
              </button>
            {/if}
          {/each}
        </div>
      {/if}
      <ul
        class="tree"
        role="tree"
        aria-label="Canvas layer tree"
        bind:this={treeScrollEl}
        onscroll={onTreeScroll}
      >
      {#each visibleTree as node (node.kind + ':' + node.id)}
        {#if node.kind === 'group'}
        {@const g = node.group}
        {@const selected = sessionStore.M.has(node.id)}
        {@const isOpen = effectiveExpanded(node.id)}
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
          draggable={!isItemLocked(node.id)}
          ondragstart={(e: DragEvent) => onRowDragStart(node.id, e)}
          ondragover={(e: DragEvent) => onRowDragOver(node.id, 'group', e)}
          ondragleave={(e: DragEvent) => onRowDragLeave(node.id, e)}
          ondrop={(e: DragEvent) => onRowDrop(node.id, 'group', e)}
          ondragend={onTreeDragEnd}
          onmouseenter={() => groupHover.set(node.id)}
          onmouseleave={() => groupHover.clearIf(node.id)}
          oncontextmenu={(e: MouseEvent) => onGroupContextMenu(node.id, e)}
        >
          <div class="row-inner" style:padding-left={`${node.depth * 16 + 4}px`}>
            <!-- caret 은 span (button 중첩 금지) — keyboard 접근은 row-button 의 Enter/Space 가
                 select 만 트리거하며 expand toggle 은 별도 키 (P1+에서 Right/Left arrow 처리). -->
            <span
              class="caret"
              class:caret-disabled={!node.hasChildren}
              role="presentation"
              onclick={(e: MouseEvent) => {
                e.stopPropagation();
                if (!node.hasChildren) return;
                // ADR-0024 D25 — replace-select the group before toggling so that
                // (a) a selected descendant hidden by collapse moves selection to the
                // visible group row, and (b) it breaks the reveal-on-select $effect
                // (expandAncestorsOf) ↔ manual-collapse feedback loop: while a
                // descendant is still in M the effect re-expands this group the instant
                // toggleExpand removes it. Replace-select (no modifiers) is required —
                // a toggle/range select would keep the descendant in M and the loop
                // would persist. Multi/range stays a row-button (label) concern.
                selectNode(node.id);
                toggleExpand(node.id);
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
                <span class="type-icon group-type-icon" aria-hidden="true">
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M4 8V5a1 1 0 0 1 1-1h3"/>
                    <path d="M16 4h3a1 1 0 0 1 1 1v3"/>
                    <path d="M20 16v3a1 1 0 0 1-1 1h-3"/>
                    <path d="M8 20H5a1 1 0 0 1-1-1v-3"/>
                    <rect x="8" y="8" width="4" height="4" rx="0.8"/>
                    <rect x="13" y="13" width="3" height="3" rx="0.7"/>
                  </svg>
                </span>
                <span class="label"
                  >{@render highlightedLabel(
                    groupDisplayLabel(g),
                    nodeLabelPath(g.parent_id ?? null, groupDisplayLabel(g)),
                    '',
                  )}</span>
              </button>
            {/if}
            {#snippet groupIcons()}
              {@const groupInheritedHidden = inheritedHiddenFrom(g.parent_id ?? null)}
              {@const groupInheritedLocked = g.locked !== true ? inheritedLockedFrom(g.parent_id ?? null) : null}
              <span class="icons" class:has-active={g.visibility === false || g.locked === true || groupInheritedHidden !== null || groupInheritedLocked !== null}>
              <button
                type="button"
                class="icon"
                class:on={g.visibility === false}
                class:inherited={groupInheritedHidden !== null}
                disabled={groupInheritedHidden !== null}
                title={groupInheritedHidden !== null
                  ? `Hidden by ${inheritedSourceLabel(groupInheritedHidden)}; show that group first`
                  : g.visibility === false ? 'Show group' : 'Hide group'}
                aria-label={groupInheritedHidden !== null
                  ? 'Hidden by ancestor group'
                  : g.visibility === false ? 'Show group' : 'Hide group'}
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
                disabled={groupInheritedLocked !== null}
                title={groupInheritedLocked !== null
                  ? `Locked by ${inheritedSourceLabel(groupInheritedLocked)}; unlock that group first`
                  : g.locked === true ? 'Unlock group' : 'Lock group'}
                aria-label={groupInheritedLocked !== null
                  ? 'Locked by ancestor group'
                  : g.locked === true ? 'Unlock group' : 'Lock group'}
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
          draggable={!isItemLocked(node.id)}
          ondragstart={(e: DragEvent) => onRowDragStart(node.id, e)}
          ondragover={(e: DragEvent) => onRowDragOver(node.id, 'panel', e)}
          ondragleave={(e: DragEvent) => onRowDragLeave(node.id, e)}
          ondrop={(e: DragEvent) => onRowDrop(node.id, 'panel', e)}
          ondragend={onTreeDragEnd}
          oncontextmenu={(e: MouseEvent) => onPanelContextMenu(node.id, p, e)}
        >
          <div
            class="row-inner"
            style:padding-left={`${node.depth * 16 + 20}px`}
          >
            {#if editingItemId === node.id}
              <span class="row-button row-button-edit">
                {@render typeIconSvg(p)}
                <InlineEditField
                  value={panelDisplayLabel(p)}
                  editing={true}
                  allowEmpty={true}
                  placeholder={node.id.slice(0, 8)}
                  class="item-label-edit"
                  onCommit={(next: string) => void onCommitRenameItem(node.id, next)}
                  onCancel={onCancelRenameItem}
                />
              </span>
            {:else}
              <button
                type="button"
                class="row-button"
                onclick={(e: MouseEvent) => selectNode(node.id, e)}
                ondblclick={(e: MouseEvent) => onStartRenameItem(node.id, e)}
                title={`${panelDisplayLabel(p)} (double-click to rename)`}
              >
                {@render typeIconSvg(p)}
                <span class="label"
                  >{@render highlightedLabel(
                    panelDisplayLabel(p),
                    nodeLabelPath(p.parent_id ?? null, panelDisplayLabel(p)),
                    dead ? ' (Dead)' : '',
                  )}</span>
                {#if p.type === 'snippets'}
                  {@const sc = snippetsCount(p)}
                  {#if sc !== null}
                    <span class="snippets-count" aria-label={`${sc} entries`}>{sc}</span>
                  {/if}
                {/if}
              </button>
            {/if}
            {#snippet panelIcons()}
              {@const panelInheritedHidden = inheritedHiddenFrom(p.parent_id ?? null)}
              {@const panelInheritedLocked = p.locked !== true ? inheritedLockedFrom(p.parent_id ?? null) : null}
              <span class="icons" class:has-active={p.visibility === false || p.locked === true || panelInheritedHidden !== null || panelInheritedLocked !== null}>
              <button
                type="button"
                class="icon"
                class:on={p.visibility === false}
                class:inherited={panelInheritedHidden !== null}
                disabled={panelInheritedHidden !== null}
                title={panelInheritedHidden !== null
                  ? `Hidden by ${inheritedSourceLabel(panelInheritedHidden)}; show that group first`
                  : p.visibility === false ? 'Show item' : 'Hide item'}
                aria-label={panelInheritedHidden !== null
                  ? 'Hidden by ancestor group'
                  : p.visibility === false ? 'Show item' : 'Hide item'}
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
                disabled={panelInheritedLocked !== null}
                title={panelInheritedLocked !== null
                  ? `Locked by ${inheritedSourceLabel(panelInheritedLocked)}; unlock that group first`
                  : p.locked === true ? 'Unlock item' : 'Lock item'}
                aria-label={panelInheritedLocked !== null
                  ? 'Locked by ancestor group'
                  : p.locked === true ? 'Unlock item' : 'Lock item'}
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
              <!-- Focus 는 ViewportCtrl 의 focus 버튼으로 이동 — Layer row 의
                   click 으로 selection 후 viewport 컨트롤에서 focus 트리거. -->
            </span>
            {/snippet}
            {@render panelIcons()}
          </div>
        </li>
        {/if}
      {/each}
      </ul>
    </div>
  {/if}
</div>

<style>
  /* Embedded view — host (LeftPanel) owns floating chrome, fold + tabs.
   * Fills the available content area inside the active tab. */
  .layer-tree-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    font-size: var(--text-md);
    line-height: var(--leading-normal);
    user-select: none;
  }

  /* The Layers search input lives in LeftPanel's footer (ADR-0052 D2); this
   * component keeps only the matched-substring highlight style below. */

  /* Matched-substring highlight on labels (text-safe segments). */
  .search-hit {
    background: color-mix(in srgb, var(--color-accent) 28%, transparent);
    color: inherit;
    border-radius: 2px;
  }

  /* ── Tree viewport (sticky overlay anchor) (ADR-0052 D7 clarify ③) ──
   * Non-scrolling relative wrapper that fills the available height. The inner
   * <ul.tree> scrolls; the .sticky-stack overlay is pinned to this wrapper's
   * top edge (top:0), so it no longer chases scrollTop per-frame. Mirrors
   * FileTreeView's .tree-viewport. */
  .tree-viewport {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .tree {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    list-style: none;
    margin: 0;
    padding: var(--space-4) 0;
  }

  /* Sticky parent header stack (ADR-0052 D7 clarify ③). Sibling overlay OUTSIDE
   * the scrolling <ul>, absolutely pinned to the non-scrolling .tree-viewport's
   * top edge (top:0, no per-scroll JS). Above rows, below the context menu;
   * rows are clickable. Mirrors FileTreeView. */
  .sticky-stack {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 2;
    pointer-events: none; /* container ignores events; rows opt back in below. */
    display: flex;
    flex-direction: column;
    background: var(--color-surface); /* opaque base occludes scrolling rows, incl. on hover (matches Files) */
    border-bottom: 1px solid var(--color-border);
    box-shadow: 0 4px 8px rgba(0, 0, 0, 0.12);
  }

  .sticky-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    width: 100%;
    height: 24px;
    padding-right: var(--space-8);
    border: 0;
    background: var(--color-surface); /* match Files token (was --color-bg) */
    color: var(--color-fg-muted);
    text-align: left;
    cursor: pointer;
    font: inherit;
    font-size: var(--text-md);
    pointer-events: auto; /* clickable even though the container opted out. */
  }

  .sticky-row:hover {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .sticky-row .label {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row {
    display: block;
    position: relative;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  /* 행 간 세로 간격 — gap-like spacing. drop indicator (::before/::after, 2px) 는
     row 의 top/bottom edge 에 위치하므로 margin 영역과 겹치지 않음. */
  .row + .row {
    margin-top: 2px;
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

  /* Row hover / selected 는 row 단위 — panel width 가 resize 되어도 가로 전체
   * 적용 (.row 가 panel 의 full width 차지). row-inner 는 padding-left (indent)
   * 안쪽의 content container — accent rail 만 row-inner 의 left edge 에 유지. */
  .row-inner {
    display: flex;
    align-items: center;
    gap: 0;
    width: 100%;
    min-width: 0;
    transition:
      box-shadow var(--motion-fast) var(--motion-easing);
  }

  .row:hover {
    background: var(--color-glass-1);
  }

  .row.selected {
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    color: var(--color-accent);
  }

  .row.selected .row-inner {
    box-shadow: inset 2px 0 0 var(--color-accent);
  }

  /* drop-inside (drag target) 는 hover/selected 보다 우선 — drag 중 시각 단서가
     명확해야 함. CSS source order 로 specificity 같은 selector 의 우선순위
     역전. */
  .row.drop-inside {
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
  }

  .row-button {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex: 1 1 auto;
    min-width: 0;
    padding: var(--space-4) var(--space-8) var(--space-4) 0;
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
    padding: var(--space-4) var(--space-8) var(--space-4) 0;
    min-width: 0;
  }

  .row-button-edit :global(.group-label-edit),
  .row-button-edit :global(.item-label-edit) {
    flex: 1 1 auto;
    min-width: 0;
  }

  .row-button-edit :global(.inline-edit-input) {
    height: 24px;
    padding: 0 6px;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.2px;
    transition: border-color var(--motion-fast) var(--motion-easing);
  }

  .row-button-edit :global(.inline-edit-input:hover) {
    border-color: var(--color-border-strong);
  }

  .row-button-edit :global(.inline-edit-input:focus-visible) {
    outline: 0;
    border-color: var(--color-accent);
  }

  .row.dead .row-button .label {
    color: var(--color-fg-subtle);
    text-decoration: line-through;
  }

  /* Z-mode reorder buttons — left 24px slot 안에 up/down 두 button 수직 분할.
     tree mode 의 padding-left: 24 와 동일 width 유지. */

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
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ADR-0038 v8 §12.3 — snippets entry count badge.
     Inline (not a new grid column) to avoid affecting other row types. */
  .snippets-count {
    flex: 0 0 auto;
    font-family: var(--font-mono);
    font-size: 9.5px;
    color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
    padding: 1px 6px;
    border-radius: var(--radius-pill);
    letter-spacing: 0.4px;
    line-height: 1.4;
    margin-left: 4px;
  }
  .row.selected .snippets-count {
    background: color-mix(in srgb, var(--color-accent) 24%, transparent);
  }

  .type-icon {
    flex: 0 0 16px;
    width: 16px;
    height: 16px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-family: var(--font-mono);
    font-size: var(--text-base);
    color: var(--color-fg-muted);
  }

  .group-type-icon {
    color: var(--color-accent);
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

  .icon:disabled {
    cursor: not-allowed;
  }

  .icon:disabled:hover {
    background: transparent;
    color: var(--color-fg-subtle);
  }

  /* `.on` 상태 (visibility=false / locked=true) 강조. */
  .icon.on {
    color: var(--color-fg);
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

  /* Tree/Z toggle CSS 제거됨 — ADR-0024 의 2026-05-22 ② amend (Tree=Z). */
</style>
