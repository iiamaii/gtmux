<script lang="ts">
  // Sidebar — Figma-식 layer panel (read-only v0, sketch §6.5 / §10.2).
  //
  // 책임 (Sprint 5-B 추가 작업 범위):
  // - groupsStore (SvelteMap<id, Group>) 와 panelsStore (SvelteMap<id, Panel>) 의
  //   `parent_id` 트리를 재귀 렌더 (Group / Panel 노드 mixed).
  // - Group 행은 펼침/접힘 toggle (component-local SvelteSet<string>, P1+에서 영속화 검토).
  // - Panel 행은 leaf — visibility / lock / dead 아이콘 *표시만* (toggle handler 없음, read-only).
  // - 클릭 → ephemeralStore.m.set([nodeId]) (단일 선택). ADR-0010 D7 의 "Group 클릭 = 후손 Panel
  //   모두 M 등록"은 P1+ — MVP는 클릭한 노드 id 하나만 M 으로 세팅.
  // - dead 표시: muxStore.panes.get(N).dead === true → 회색 + 취소선.
  //
  // 불변식 (CLAUDE.md):
  // - 본 컴포넌트는 *어떤 mutation 도 store 에 대해 발생시키지 않는다* (selection 동기화만 예외 —
  //   ephemeralStore.m 은 기존 패턴, Canvas.svelte 도 동일하게 직접 set/add/delete).
  // - tmux state 와 web state 분리 (불변식 #1): 본 컴포넌트는 panelsStore (web) + groupsStore (web)
  //   만 author 가능 영역으로 보지 않으며, 표시용 read-only. muxStore (tmux mirror) 는 dead 마킹
  //   조회 목적에만 사용.
  // - 사용자 입력 escape (불변식 #4): 모든 label / pane_id 는 텍스트 노드로만 렌더 (`{value}` 보간 —
  //   Svelte 가 자동 HTML escape). dangerouslySetInnerHTML 등 사용 없음.
  //
  // 레이아웃 결정 (옵션 A 채택, +page.svelte 의 .workspace flex-row 가 280px Sidebar + 1fr Canvas):
  // - Sidebar 가 sibling 으로 배치되어 캔버스 가용 영역을 280px 깎는다.
  // - position: fixed 보다 sibling 이 정직하고 SvelteFlow 의 viewport 계산과 충돌하지 않는다.
  //
  // 의존성: 추가 npm 패키지 없음. Svelte 5 runes ($state / $derived) 만 사용.

  import { SvelteSet } from 'svelte/reactivity';
  import { groupsStore } from '$lib/stores/groups.svelte';
  import { panelsStore } from '$lib/stores/panels.svelte';
  import { muxStore } from '$lib/stores/mux.svelte';
  import { ephemeralStore } from '$lib/stores/ephemeral.svelte';

  /** Floating chrome state — driven by chromeStore (Stage E). */
  interface Props {
    collapsed?: boolean;
  }
  const { collapsed = false }: Props = $props();

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
    label?: string | null;
    visibility?: boolean;
    locked?: boolean;
    minimized?: boolean;
  }

  // 트리 노드 union — 사이드바 한 줄에 해당.
  type TreeNode =
    | { kind: 'group'; id: string; depth: number; group: GroupData; hasChildren: boolean }
    | { kind: 'panel'; id: string; depth: number; panel: PanelData };

  // 펼침 상태 — component-local. P1+에서 ephemeralStore 또는 web-store 영속화 검토.
  const expanded = $state(new SvelteSet<string>());

  // 트리 평탄화 — Group/Panel 을 parent_id 기준으로 묶어 DFS 순회.
  // 평탄화된 결과를 {each} 로 렌더하면 들여쓰기는 depth * 16 px 로 표현 가능.
  // SvelteMap entry-level reactivity 의 비용은 본 derived 의 *전체 재계산* — Panel/Group 수가
  // 수십 단위인 MVP 에서는 무시 가능. R8 §F3 의 가이드 위반 시 keyed each + 자식 재귀 컴포넌트로 분해.
  const tree = $derived.by<TreeNode[]>(() => {
    const groups = Array.from(groupsStore.groups.values() as Iterable<GroupData>);
    const panels = Array.from(panelsStore.panels.values() as Iterable<PanelData>);

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

  // Panel 행 표시 라벨 우선순위 (Stage B 이후 — tmux window 어휘 폐기):
  //   1) Panel.label (사용자 지정)
  //   2) "%${paneNum}" — pane id fallback
  function panelDisplayLabel(p: PanelData): string {
    if (p.label != null && p.label.length > 0) return p.label;
    const n = paneNumeric(p.pane_id);
    if (n !== null) return `%${n}`;
    return p.id;
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

  // 선택 동기화 — Canvas.svelte 의 toggle 방식과 달리 사이드바 클릭은 *단일 선택* (set [id]).
  // ctrl/cmd-click multi-select 는 P1+.
  function selectNode(id: string): void {
    ephemeralStore.m.clear();
    ephemeralStore.m.add(id);
  }

  // Panel 행이 dead pane 인지 — 회색/취소선 표시 트리거.
  function isPanelDead(p: PanelData): boolean {
    const n = paneNumeric(p.pane_id);
    if (n === null) return false;
    return muxStore.panes.get(n)?.dead === true;
  }
</script>

<aside class="sidebar" class:collapsed aria-label="Layer panel">
  <header class="sidebar-header">
    <span class="sidebar-title">Layers</span>
  </header>
  <ul class="tree" role="tree">
    {#each tree as node (node.kind + ':' + node.id)}
      {#if node.kind === 'group'}
        {@const g = node.group}
        {@const selected = ephemeralStore.m.has(node.id)}
        {@const isOpen = expanded.has(node.id)}
        <li
          class="row group-row"
          class:selected
          role="treeitem"
          aria-expanded={node.hasChildren ? isOpen : undefined}
          aria-selected={selected}
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
            <button
              type="button"
              class="row-button"
              onclick={() => selectNode(node.id)}
              title={groupDisplayLabel(g)}
            >
              <span class="label">{groupDisplayLabel(g)}</span>
              <span class="icons">
                <span class="icon" class:on={g.visibility === false} title="Visibility" aria-label="Visibility">
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
                </span>
                <span class="icon" class:on={g.locked === true} title="Locked" aria-label="Locked">
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
                </span>
              </span>
            </button>
          </div>
        </li>
      {:else}
        {@const p = node.panel}
        {@const selected = ephemeralStore.m.has(node.id)}
        {@const dead = isPanelDead(p)}
        <li
          class="row panel-row"
          class:selected
          class:dead
          role="treeitem"
          aria-selected={selected}
          style:padding-left={`${node.depth * 16 + 24}px`}
        >
          <button
            type="button"
            class="row-button"
            onclick={() => selectNode(node.id)}
            title={panelDisplayLabel(p)}
          >
            <span class="label">{panelDisplayLabel(p)}{dead ? ' (Dead)' : ''}</span>
            <span class="icons">
              <span class="icon" class:on={p.visibility === false} title="Visibility" aria-label="Visibility">
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
              </span>
              <span class="icon" class:on={p.locked === true} title="Locked" aria-label="Locked">
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
              </span>
            </span>
          </button>
        </li>
      {/if}
    {:else}
      <li class="empty">No panels yet.</li>
    {/each}
  </ul>
</aside>

<style>
  /* Stage E — floating panel. Sits over the canvas with shadow + radius,
   * 8px gap from the workspace edges. Collapse animates outward to the
   * viewport edge (transform translateX) so the RailToggle can pull it
   * back. */
  .sidebar {
    position: absolute;
    top: var(--space-8);
    bottom: var(--space-8);
    left: var(--space-8);
    width: var(--layout-sidebar-w);
    box-sizing: border-box;
    overflow: auto;
    background: var(--color-surface);
    color: var(--color-fg);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-md);
    z-index: var(--z-side-panel);
    font-size: var(--text-lg);
    line-height: var(--leading-normal);
    user-select: none;
    transition:
      transform var(--motion-slow) var(--motion-easing),
      opacity var(--motion-normal) var(--motion-easing);
  }

  .sidebar.collapsed {
    transform: translateX(calc(-1 * (var(--layout-sidebar-w) + var(--space-12))));
    opacity: 0;
    pointer-events: none;
  }

  .sidebar-header {
    position: sticky;
    top: 0;
    padding: var(--space-8) var(--space-12);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    font-family: var(--font-mono);
    font-weight: var(--weight-regular);
    letter-spacing: 0.6px;
    text-transform: uppercase;
    font-size: var(--text-base);
    color: var(--color-fg-muted);
    z-index: 1;
  }

  .tree {
    list-style: none;
    margin: 0;
    padding: var(--space-4) 0;
  }

  .row {
    display: block;
  }

  .row-inner {
    display: flex;
    align-items: center;
    gap: 0;
    width: 100%;
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

  .row-button {
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing),
      box-shadow var(--motion-fast) var(--motion-easing);
  }

  .row-button:hover {
    background: var(--color-glass-1);
  }

  /* Figma-style selected — accent text + accent-tint background + 2px
   * border-left indicator on the row inner. */
  .row.selected .row-button {
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    color: var(--color-accent);
    box-shadow: inset 2px 0 0 var(--color-accent);
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

  /* Icons (visibility / lock) — Figma 컨벤션:
   *   - 평소엔 숨김 (opacity 0)
   *   - row hover 시 모두 표시
   *   - .on 상태 (locked/hidden) 는 호버 없어도 항상 표시 — 사용자에게
   *     *현재 활성 토글* 을 가시화
   * 현 마크업은 toggle handler 미배선 (read-only v0) 이라 .on 분기는
   * P1+ 에서 lock/vis 토글 wire 시 row 에 직접 클래스로 적용. */
  .icons {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: var(--space-4);
    font-size: var(--text-base);
    color: var(--color-fg-muted);
    opacity: 0;
    transition: opacity var(--motion-fast) var(--motion-easing);
  }

  .row:hover .icons,
  .row.selected .icons {
    opacity: 1;
  }

  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .icon:hover {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  /* `.on` 상태 (visibility=false / locked=true) — 호버 없이 항상 표시 +
   * 약간 강조. parent `.icons` 의 opacity 0 default 를 override. */
  .icons :global(.icon.on) {
    opacity: 1;
    color: var(--color-fg);
  }

  .empty {
    padding: var(--space-8) var(--space-12);
    color: var(--color-fg-subtle);
    font-style: italic;
  }
</style>
