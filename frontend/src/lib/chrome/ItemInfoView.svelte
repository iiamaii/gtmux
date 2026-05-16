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
  import type {
    CanvasItem,
    TextAlign,
    TextItem,
    TextVerticalAlign,
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
      <section class="section">
        <h4 class="section-head">Identity</h4>
        <div class="kv">
          <span class="k">type</span>
          <span class="v mono" class:mixed={isMultiMixed}>
            {#if selectionCount === 1}
              {strOr(selectedPanel['type'], 'panel')}
            {:else if isMultiMixed}
              Mixed
            {:else}
              {commonType}
            {/if}
          </span>
        </div>
        {#if selectionCount === 1 && isSelectedTerminal}
          <div class="kv">
            <span class="k">terminal</span>
            <span class="v mono">{strOr(selectedPanel['pane_id'], '—')}</span>
          </div>
        {/if}
        <div class="kv">
          <span class="k">label</span>
          <span class="v" class:mixed={commonField('label') === 'Mixed'}>
            {commonStrOr('label')}
          </span>
        </div>
        {#if selectionCount === 1}
          <div class="kv">
            <span class="k">id</span>
            <span class="v mono">{selectedPanel.id}</span>
          </div>
        {/if}
      </section>

      <section class="section">
        <h4 class="section-head">Geometry</h4>
        <div class="kv-pair">
          <div class="kv">
            <span class="k">x</span>
            <span class="v mono" class:mixed={commonField('x') === 'Mixed'}>
              {commonNumStr('x', '0')}
            </span>
          </div>
          <div class="kv">
            <span class="k">y</span>
            <span class="v mono" class:mixed={commonField('y') === 'Mixed'}>
              {commonNumStr('y', '0')}
            </span>
          </div>
        </div>
        <div class="kv-pair">
          <div class="kv">
            <span class="k">w</span>
            <span class="v mono" class:mixed={commonField('w') === 'Mixed'}>
              {commonNumStr('w')}
            </span>
          </div>
          <div class="kv">
            <span class="k">h</span>
            <span class="v mono" class:mixed={commonField('h') === 'Mixed'}>
              {commonNumStr('h')}
            </span>
          </div>
        </div>
        <div class="kv">
          <span class="k">z</span>
          <span class="v mono" class:mixed={commonField('z') === 'Mixed'}>
            {commonNumStr('z', '0')}
          </span>
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
        <section class="section">
          <h4 class="section-head">Terminal · Pool</h4>
          {#if terminalPoolEntry !== null}
            <div class="kv">
              <span class="k">attach</span>
              <span class="v mono">×{terminalPoolEntry.attach_count}</span>
            </div>
            {#if terminalPoolEntry.attached_sessions.length > 0}
              <div class="kv">
                <span class="k">sessions</span>
                <span class="v">
                  {#each terminalPoolEntry.attached_sessions as s, i (s)}
                    {#if i > 0}, {/if}<span
                      class="session-chip"
                      class:current={sessionStore.active?.name === s}
                    >{s}</span>
                  {/each}
                </span>
              </div>
            {/if}
            <div class="kv">
              <span class="k">alive</span>
              <span class="v mono" class:dead={!terminalPoolEntry.alive}>
                {terminalPoolEntry.alive ? 'live' : 'dangling'}
              </span>
            </div>
          {:else if sessionItem !== null && sessionItem.type === 'terminal'}
            <div class="kv">
              <span class="k">pool</span>
              <span class="v">
                <span class="warn">missing</span> — terminal not in server pool
              </span>
            </div>
          {/if}
        </section>
      {/if}

      {#if selectionCount === 1 && sessionItem !== null && (sessionItem.type === 'rect' || sessionItem.type === 'ellipse' || sessionItem.type === 'line' || sessionItem.type === 'text' || sessionItem.type === 'note' || sessionItem.type === 'file_path')}
        <section class="section">
          <h4 class="section-head">Item Payload</h4>
          {#if sessionItem.type === 'rect' || sessionItem.type === 'ellipse'}
            <div class="kv">
              <span class="k">stroke</span>
              <span class="v mono">{sessionItem.stroke}</span>
            </div>
            <div class="kv">
              <span class="k">fill</span>
              <span class="v mono">{sessionItem.fill}</span>
            </div>
          {:else if sessionItem.type === 'line'}
            <div class="kv-pair">
              <div class="kv">
                <span class="k">x2</span>
                <span class="v mono">{Math.round(sessionItem.x2)}</span>
              </div>
              <div class="kv">
                <span class="k">y2</span>
                <span class="v mono">{Math.round(sessionItem.y2)}</span>
              </div>
            </div>
            <div class="kv">
              <span class="k">stroke</span>
              <span class="v mono">{sessionItem.stroke}</span>
            </div>
          {:else if sessionItem.type === 'text'}
            {@const txt = sessionItem}
            {@const h = txt.text_align ?? 'center'}
            {@const v = txt.text_vertical_align ?? 'middle'}
            <div class="kv">
              <span class="k">chars</span>
              <span class="v mono">{txt.text.length}</span>
            </div>
            <div class="kv align-row">
              <span class="k">align</span>
              <div class="v">
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
            <div class="kv align-row">
              <span class="k">v-align</span>
              <div class="v">
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
            <div class="kv">
              <span class="k">title</span>
              <span class="v">{strOr(sessionItem.title, 'Untitled')}</span>
            </div>
          {:else if sessionItem.type === 'file_path'}
            <div class="kv">
              <span class="k">path</span>
              <span class="v mono">{strOr(sessionItem.path, '—')}</span>
            </div>
          {/if}
        </section>
      {/if}

      <section class="section">
        <h4 class="section-head">State</h4>
        <div class="kv">
          <span class="k">visible</span>
          <span class="v mono">{boolStr(selectedPanel['visibility'] ?? true)}</span>
        </div>
        <div class="kv">
          <span class="k">locked</span>
          <span class="v mono">{boolStr(selectedPanel['locked'])}</span>
        </div>
        <div class="kv">
          <span class="k">minimized</span>
          <span class="v mono">{boolStr(selectedPanel['minimized'])}</span>
        </div>
        <div class="kv">
          <span class="k">alive</span>
          <span class="v mono" class:dead={isDead}>
            {isDead ? 'dead' : 'live'}
          </span>
        </div>
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

  /* ADR-0027 D3 — mixed value (placeholder 같은 muted italic). */
  .kv .v.mixed {
    color: var(--color-fg-subtle);
    font-style: italic;
  }

  .section {
    padding: var(--space-8) var(--space-12) var(--space-12);
    border-bottom: 1px solid var(--color-border);
  }

  .section:last-child {
    border-bottom: 0;
  }

  .section-head {
    margin: 0 0 var(--space-8);
    font-family: var(--font-mono);
    font-size: var(--text-base);
    text-transform: uppercase;
    letter-spacing: 0.6px;
    font-weight: var(--weight-regular);
    color: var(--color-fg-muted);
  }

  .kv {
    display: grid;
    grid-template-columns: 64px 1fr;
    align-items: center;
    gap: var(--space-6);
    padding: var(--space-4) 0;
    font-size: var(--text-md);
  }

  .kv .k {
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-base);
  }

  .kv .v {
    color: var(--color-fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .kv .v.mono {
    font-family: var(--font-mono);
    font-size: var(--text-base);
  }

  .kv .v.dead {
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

  .kv-pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-6);
  }

  .kv-pair .kv {
    grid-template-columns: 24px 1fr;
  }

  /* Figma-style segmented control for text alignment. */
  .align-row .v {
    display: inline-flex;
    justify-content: flex-start;
  }

  /* ADR-0027 D9 — multi-select alignment row (Common section 안). */
  .section > .align-row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-6);
    margin-top: var(--space-10);
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

  .align-btn:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .align-btn.active {
    background: var(--color-surface);
    color: var(--color-fg);
    box-shadow: var(--shadow-sm);
  }
</style>
