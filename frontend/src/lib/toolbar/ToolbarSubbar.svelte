<script lang="ts">
  /**
   * ToolbarSubbar — Toolbar2 바로 아래 row 의 context-detail button group.
   *
   * 의도:
   * - Toolbar2 의 각 도구 button 의 *바로 아래* 자리에 그 도구의 detail
   *   option group 을 가로 배치. 사용자 명시 — "기존 text bbox 위에 있던
   *   alignment 버튼을 toolbar 의 text 도구 아래로 이동".
   * - 본 컴포넌트는 *현재는 text-align context 만 cover*. 후속 (shape stroke /
   *   pencil size 등) 은 같은 패턴으로 확장.
   * - 버튼 크기는 Toolbar2 의 36×36 과 동일.
   *
   * 표시 조건 (현 context):
   * - `sessionStore.M.size === 1` && 단일 item 이 text 타입 → 'text-align'
   * - 그 외 → null (subbar 자체 unmount → canvas 영역 회복)
   *
   * Anchor 측정:
   * - Toolbar2 의 text tool button (`[data-tool-id="text"]`) 의 client x
   *   center 를 기준 — subbar 의 button group center 가 그 x 에 align.
   * - mount 시 + 윈도우 resize 시 재측정 (ResizeObserver on `<nav.toolbar>`).
   */

  import { onMount } from 'svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { mutateLayout, UnauthorizedError } from '$lib/http/sessions';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { isText } from '$lib/types/canvas';
  import type {
    CanvasItem,
    TextAlign,
    TextItem,
    TextVerticalAlign,
  } from '$lib/types/canvas';

  /**
   * 현재 active context. 단일 선택 item 의 type 기반으로 결정.
   * 후속 context (shape / line / image 등) 는 union 확장.
   */
  type Context = { kind: 'text-align'; target: TextItem } | null;

  const ctx: Context = $derived.by(() => {
    if (sessionStore.M.size !== 1) return null;
    // SvelteSet 의 destructuring 은 첫 iterator value — size 1 이 이미 확정이라
    // 항상 string. 그러나 타입 추론이 string|undefined 라 명시 fallback.
    const id = sessionStore.M.values().next().value;
    if (id === undefined) return null;
    const item = sessionStore.items.get(id);
    if (!item || !isText(item)) return null;
    return { kind: 'text-align', target: item };
  });

  /** Toolbar2 의 text tool button center — anchor for horizontal alignment. */
  let anchorX = $state<number | null>(null);
  let resizeObs: ResizeObserver | null = null;

  function anchorKey(): string {
    if (ctx === null) return '';
    // text-align context 는 toolbar 의 text 도구 아래에 정렬.
    return ctx.kind === 'text-align' ? 'text' : '';
  }

  function measureAnchor(): void {
    const key = anchorKey();
    if (key.length === 0) {
      anchorX = null;
      return;
    }
    const btn = document.querySelector<HTMLElement>(
      `[data-tool-id="${key}"]`,
    );
    if (btn === null) {
      anchorX = null;
      return;
    }
    const rect = btn.getBoundingClientRect();
    anchorX = rect.left + rect.width / 2;
  }

  // anchor 는 toolbar layout 변화에 영향 — toolbar element 자체에 resize obs.
  function bindResize(): void {
    const toolbar = document.querySelector<HTMLElement>('nav.toolbar');
    if (toolbar === null) return;
    resizeObs = new ResizeObserver(() => measureAnchor());
    resizeObs.observe(toolbar);
  }

  onMount(() => {
    measureAnchor();
    bindResize();
    window.addEventListener('resize', measureAnchor);
    return () => {
      resizeObs?.disconnect();
      resizeObs = null;
      window.removeEventListener('resize', measureAnchor);
    };
  });

  // ctx 가 변하면 anchor 재측정 — 새 context 의 target tool 이 다를 수 있음.
  $effect(() => {
    void ctx;
    measureAnchor();
  });

  /* ────────────────────────────────────────────────────────────────────── */
  /* Text alignment mutations — TextNode 의 옛 로직을 그대로 이전          */
  /* ────────────────────────────────────────────────────────────────────── */

  async function applyTextAlign(target: TextItem, next: TextAlign): Promise<void> {
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

{#if ctx !== null}
  <div class="subbar" role="toolbar" aria-label="Tool detail options">
    <div
      class="group"
      style:left={anchorX !== null ? `${anchorX}px` : '50%'}
    >
      {#if ctx.kind === 'text-align'}
        {@const target = ctx.target}
        {@const h = target.text_align ?? 'center'}
        {@const v = target.text_vertical_align ?? 'middle'}
        <button
          type="button"
          class="opt"
          class:active={h === 'left'}
          title="Align left"
          aria-label="Align left"
          aria-pressed={h === 'left'}
          onclick={() => void applyTextAlign(target, 'left')}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
            <line x1="4" y1="6" x2="20" y2="6" />
            <line x1="4" y1="12" x2="14" y2="12" />
            <line x1="4" y1="18" x2="18" y2="18" />
          </svg>
        </button>
        <button
          type="button"
          class="opt"
          class:active={h === 'center'}
          title="Align center"
          aria-label="Align center"
          aria-pressed={h === 'center'}
          onclick={() => void applyTextAlign(target, 'center')}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
            <line x1="4" y1="6" x2="20" y2="6" />
            <line x1="7" y1="12" x2="17" y2="12" />
            <line x1="5" y1="18" x2="19" y2="18" />
          </svg>
        </button>
        <button
          type="button"
          class="opt"
          class:active={h === 'right'}
          title="Align right"
          aria-label="Align right"
          aria-pressed={h === 'right'}
          onclick={() => void applyTextAlign(target, 'right')}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
            <line x1="4" y1="6" x2="20" y2="6" />
            <line x1="10" y1="12" x2="20" y2="12" />
            <line x1="6" y1="18" x2="20" y2="18" />
          </svg>
        </button>
        <span class="divider" aria-hidden="true"></span>
        <button
          type="button"
          class="opt"
          class:active={v === 'top'}
          title="Align top"
          aria-label="Align top"
          aria-pressed={v === 'top'}
          onclick={() => void applyTextVerticalAlign(target, 'top')}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
            <line x1="5" y1="5" x2="19" y2="5" />
            <line x1="8" y1="10" x2="16" y2="10" />
            <line x1="10" y1="15" x2="14" y2="15" />
          </svg>
        </button>
        <button
          type="button"
          class="opt"
          class:active={v === 'middle'}
          title="Align middle"
          aria-label="Align middle"
          aria-pressed={v === 'middle'}
          onclick={() => void applyTextVerticalAlign(target, 'middle')}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
            <line x1="6" y1="7" x2="18" y2="7" />
            <line x1="4" y1="12" x2="20" y2="12" />
            <line x1="6" y1="17" x2="18" y2="17" />
          </svg>
        </button>
        <button
          type="button"
          class="opt"
          class:active={v === 'bottom'}
          title="Align bottom"
          aria-label="Align bottom"
          aria-pressed={v === 'bottom'}
          onclick={() => void applyTextVerticalAlign(target, 'bottom')}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
            <line x1="10" y1="9" x2="14" y2="9" />
            <line x1="8" y1="14" x2="16" y2="14" />
            <line x1="5" y1="19" x2="19" y2="19" />
          </svg>
        </button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .subbar {
    position: relative;
    height: 44px;
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    z-index: calc(var(--z-toolbar) - 1);
    user-select: none;
    flex: 0 0 auto;
  }

  .group {
    position: absolute;
    top: 4px;
    transform: translateX(-50%);
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }

  .opt {
    position: relative;
    width: 36px;
    height: 36px;
    border: 0;
    border-radius: var(--radius-md);
    color: var(--color-fg-muted);
    background: transparent;
    display: grid;
    place-items: center;
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .opt:hover {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .opt:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .opt.active {
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }

  .opt.active:hover {
    background: color-mix(in srgb, var(--color-accent) 90%, white);
  }

  .divider {
    width: 1px;
    height: 22px;
    background: var(--color-border);
    margin: 0 var(--space-6);
  }
</style>
