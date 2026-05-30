<script lang="ts">
  import type { Anchor } from '$lib/types/canvas';

  interface Props {
    value: Anchor;
    disabled?: boolean;
    ariaLabel: string;
    onpick: (next: Anchor) => void;
  }

  const { value, disabled = false, ariaLabel, onpick }: Props = $props();

  const ANCHORS: {
    value: Anchor;
    code: string;
    name: string;
    col: 'l' | 'c' | 'r';
    row: 't' | 'm' | 'b';
  }[] = [
    { value: 'NW', code: 'NW', name: 'North west', col: 'l', row: 't' },
    { value: 'N', code: 'N', name: 'North', col: 'c', row: 't' },
    { value: 'NE', code: 'NE', name: 'North east', col: 'r', row: 't' },
    { value: 'W', code: 'W', name: 'West', col: 'l', row: 'm' },
    { value: 'center', code: 'C', name: 'Center', col: 'c', row: 'm' },
    { value: 'E', code: 'E', name: 'East', col: 'r', row: 'm' },
    { value: 'SW', code: 'SW', name: 'South west', col: 'l', row: 'b' },
    { value: 'S', code: 'S', name: 'South', col: 'c', row: 'b' },
    { value: 'SE', code: 'SE', name: 'South east', col: 'r', row: 'b' },
  ];

  let open = $state(false);
  let rootEl: HTMLDivElement | undefined = $state();
  let popoverEl: HTMLDivElement | undefined = $state();
  let popoverPos = $state<{ top: number; left: number }>({ top: 0, left: 0 });

  const current = $derived(ANCHORS.find((anchor) => anchor.value === value) ?? ANCHORS[4]!);

  function close(): void {
    open = false;
  }

  function updatePopoverPos(): void {
    if (typeof window === 'undefined') return;
    if (rootEl === undefined || popoverEl === undefined) return;
    const tRect = rootEl.getBoundingClientRect();
    const pRect = popoverEl.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const margin = 8;
    const gap = 8;

    const rightPanel = document.querySelector('.right-panel') as HTMLElement | null;
    let left: number;
    if (rightPanel !== null) {
      const rRect = rightPanel.getBoundingClientRect();
      left = rRect.left - pRect.width - gap;
    } else {
      left = tRect.left - pRect.width - gap;
    }
    if (left < margin) left = margin;
    if (left + pRect.width > vw - margin) left = vw - pRect.width - margin;

    let top = tRect.top;
    if (top + pRect.height > vh - margin) top = vh - pRect.height - margin;
    if (top < margin) top = margin;

    popoverPos = { top, left };
  }

  $effect(() => {
    if (!open || typeof window === 'undefined') return;
    function onDocPointerDown(e: PointerEvent): void {
      const target = e.target as Node;
      if (rootEl?.contains(target)) return;
      if (popoverEl?.contains(target)) return;
      close();
    }
    function onDocKey(e: KeyboardEvent): void {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      close();
    }
    const onReflow = () => updatePopoverPos();
    queueMicrotask(() => {
      document.addEventListener('pointerdown', onDocPointerDown, true);
      document.addEventListener('keydown', onDocKey);
      updatePopoverPos();
    });
    window.addEventListener('resize', onReflow);
    window.addEventListener('scroll', onReflow, true);
    return () => {
      document.removeEventListener('pointerdown', onDocPointerDown, true);
      document.removeEventListener('keydown', onDocKey);
      window.removeEventListener('resize', onReflow);
      window.removeEventListener('scroll', onReflow, true);
    };
  });

  function pick(next: Anchor): void {
    if (disabled) return;
    onpick(next);
    close();
  }
</script>

<div bind:this={rootEl} class="anchor-picker inspector-input" class:open class:disabled>
  <span class="k" aria-hidden="true">anchor</span>
  <button
    type="button"
    class="anchor-trigger"
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-label={ariaLabel}
    title={`${current.code} · ${current.name}`}
    {disabled}
    onclick={(e) => {
      e.stopPropagation();
      if (!disabled) open = !open;
    }}
  >
    <span class="anchor-mini" aria-hidden="true">
      {#each ANCHORS as anchor (anchor.value)}
        <i class:on={anchor.value === value}></i>
      {/each}
    </span>
    <span class="anchor-button-text">
      <span class="anchor-code">{current.code}</span>
      <span class="anchor-name">{current.name}</span>
    </span>
  </button>

  {#if open}
    <div
      class="anchor-popover"
      bind:this={popoverEl}
      style="top: {popoverPos.top}px; left: {popoverPos.left}px;"
      role="dialog"
      aria-label={ariaLabel}
    >
      <div class="anchor-popover-head">
        <span class="anchor-title">Anchor</span>
        <button type="button" class="anchor-close" title="Close" aria-label="Close" onclick={close}>
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
            <path d="M3 3l6 6M9 3l-6 6" />
          </svg>
        </button>
      </div>
      <div class="anchor-popover-body">
        <div class="anchor-grid" role="listbox" aria-label="Anchor positions">
          {#each ANCHORS as anchor (anchor.value)}
            <button
              type="button"
              class="anchor-cell"
              class:selected={anchor.value === value}
              data-col={anchor.col}
              data-row={anchor.row}
              role="option"
              aria-label={anchor.name}
              aria-selected={anchor.value === value}
              onclick={() => pick(anchor.value)}
            >
              <span class="cell-code">{anchor.code}</span>
              <span class="dot"></span>
            </button>
          {/each}
        </div>
        <div class="anchor-caption">
          <b>{current.code}</b><span> · {current.name}</span>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .inspector-input {
    --inspector-k-w: 56px;
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

  .inspector-input:hover {
    border-color: var(--color-border-strong);
  }

  .inspector-input.open {
    border-color: var(--color-accent);
  }

  .inspector-input.disabled {
    opacity: 0.45;
    pointer-events: none;
  }

  .k {
    flex: 0 0 var(--inspector-k-w);
    width: var(--inspector-k-w);
    color: var(--color-fg-muted);
    text-transform: uppercase;
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.4px;
    pointer-events: none;
  }

  .anchor-trigger {
    box-sizing: border-box;
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0;
    padding: 0 6px;
    border: 0;
    background: transparent;
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.2px;
    cursor: pointer;
    text-align: center;
  }

  .anchor-trigger:disabled {
    cursor: not-allowed;
  }

  .anchor-button-text {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    max-width: calc(100% - 22px);
    min-width: 0;
    margin: 0 auto;
  }

  .anchor-mini {
    position: absolute;
    left: 6px;
    top: 50%;
    width: 14px;
    height: 14px;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    grid-template-rows: repeat(3, 1fr);
    place-items: center;
    transform: translateY(-50%);
    pointer-events: none;
  }

  .anchor-mini i {
    width: 2.5px;
    height: 2.5px;
    border-radius: 50%;
    background: var(--color-fg-muted);
    opacity: 0.38;
  }

  .anchor-mini i.on {
    background: var(--color-accent);
    opacity: 1;
    transform: scale(1.5);
  }

  .anchor-code {
    flex: 0 0 auto;
    font-weight: var(--weight-semibold);
    letter-spacing: 0.5px;
  }

  .anchor-name {
    flex: 0 1 auto;
    min-width: 0;
    color: var(--color-fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .anchor-popover {
    position: fixed;
    z-index: var(--z-popover, 100);
    display: grid;
    width: 240px;
    box-sizing: border-box;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    color: var(--color-fg);
    font-family: var(--font-sans);
    user-select: none;
  }

  .anchor-popover-head {
    display: flex;
    align-items: center;
    height: 32px;
    gap: 8px;
    min-width: 0;
    padding: 0 4px 0 12px;
    border-bottom: 1px solid var(--color-border);
  }

  .anchor-title {
    font-size: 12px;
    font-weight: var(--weight-medium);
    letter-spacing: -0.1px;
  }

  .anchor-close {
    width: 24px;
    height: 24px;
    margin-left: auto;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    display: grid;
    place-items: center;
    background: transparent;
    color: var(--color-fg-muted);
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .anchor-close:hover {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  .anchor-popover-body {
    display: grid;
    gap: 8px;
    padding: 10px 12px 12px;
  }

  .anchor-grid {
    width: 164px;
    aspect-ratio: 1;
    justify-self: center;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    grid-template-rows: repeat(3, 1fr);
    border: 1.5px solid var(--color-border-strong);
    background: var(--color-surface-2);
    overflow: visible;
  }

  .anchor-cell {
    position: relative;
    display: grid;
    place-items: center;
    min-width: 0;
    min-height: 0;
    padding: 0;
    border: 0;
    border-right: 1px solid var(--color-border);
    border-bottom: 1px solid var(--color-border);
    background: transparent;
    color: var(--color-fg-muted);
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .anchor-cell:nth-child(3n) {
    border-right: 0;
  }

  .anchor-cell:nth-child(n+7) {
    border-bottom: 0;
  }

  .cell-code {
    color: currentColor;
    font-family: var(--font-mono);
    font-size: 9px;
    letter-spacing: 0.4px;
    opacity: 0.55;
    pointer-events: none;
  }

  .dot {
    position: absolute;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-border-strong);
    transform: translate(-50%, -50%);
    pointer-events: none;
  }

  .anchor-cell[data-col='l'] .dot {
    left: 0;
  }

  .anchor-cell[data-col='c'] .dot {
    left: 50%;
  }

  .anchor-cell[data-col='r'] .dot {
    left: 100%;
  }

  .anchor-cell[data-row='t'] .dot {
    top: 0;
  }

  .anchor-cell[data-row='m'] .dot {
    top: 50%;
  }

  .anchor-cell[data-row='b'] .dot {
    top: 100%;
  }

  .anchor-cell:hover:not(.selected) {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .anchor-cell:hover:not(.selected) .dot {
    background: var(--color-accent);
  }

  .anchor-cell.selected {
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }

  .anchor-cell.selected .cell-code {
    opacity: 1;
    font-weight: var(--weight-semibold);
  }

  .anchor-cell.selected .dot {
    background: var(--color-accent-fg);
  }

  .anchor-cell:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: -2px;
  }

  .anchor-caption {
    min-width: 0;
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.3px;
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .anchor-caption b {
    color: var(--color-fg);
    font-weight: var(--weight-semibold);
  }
</style>
