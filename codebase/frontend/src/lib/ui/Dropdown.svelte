<script lang="ts">
  /**
   * Anchored dropdown menu primitive (ADR-0016 §D4).
   *
   * Pattern:
   *   <Dropdown>
   *     {#snippet trigger({ open, toggle })}
   *       <IconButton aria-label="Menu" onclick={toggle}>...</IconButton>
   *     {/snippet}
   *     {#snippet menu({ close })}
   *       <button onclick={() => { ...; close(); }}>Item 1</button>
   *     {/snippet}
   *   </Dropdown>
   *
   * Behaviour:
   *   - Click outside → close
   *   - Esc → close
   *   - Focus moves to first menu item when opened by keyboard
   *   - role=menu on the body
   */

  import { tick } from 'svelte';
  import type { Snippet } from 'svelte';

  type TriggerArgs = { open: boolean; toggle: () => void };
  type MenuArgs = { close: () => void };

  interface Props {
    /** Menu placement relative to trigger. Default `bottom-end`. */
    placement?: 'bottom-end' | 'bottom-start';
    trigger: Snippet<[TriggerArgs]>;
    menu: Snippet<[MenuArgs]>;
  }

  const { placement = 'bottom-end', trigger, menu }: Props = $props();

  const VIEWPORT_PADDING = 12;
  const MENU_GAP = 8;
  const MIN_MENU_HEIGHT = 96;

  let open = $state(false);
  let host: HTMLDivElement | undefined = $state();
  let menuEl: HTMLDivElement | undefined = $state();
  let menuStyle = $state('left: 0px; top: 0px; visibility: hidden;');

  async function openMenu(): Promise<void> {
    open = true;
    menuStyle = 'left: 0px; top: 0px; visibility: hidden;';
    await tick();
    positionMenu();
    menuEl?.querySelector<HTMLElement>('button:not([disabled]), a[href]')?.focus();
  }

  function toggle(): void {
    if (open) {
      close();
      return;
    }
    void openMenu();
  }

  function close(): void {
    open = false;
  }

  function onWindowClick(e: MouseEvent): void {
    if (!open || !host) return;
    const target = e.target as Node;
    if (host.contains(target) || menuEl?.contains(target)) return;
    close();
  }

  function onWindowKeydown(e: KeyboardEvent): void {
    if (open && e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  }

  function portal(node: HTMLElement): { destroy: () => void } {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }

  function positionMenu(): void {
    if (typeof window === 'undefined' || !host || !menuEl) return;
    const triggerRect = host.getBoundingClientRect();
    const menuRect = menuEl.getBoundingClientRect();
    const maxLeft = window.innerWidth - menuRect.width - VIEWPORT_PADDING;
    const rawLeft = placement === 'bottom-start'
      ? triggerRect.left
      : triggerRect.right - menuRect.width;
    const left = Math.max(
      VIEWPORT_PADDING,
      Math.min(rawLeft, Math.max(VIEWPORT_PADDING, maxLeft)),
    );

    const roomBelow = window.innerHeight - triggerRect.bottom - MENU_GAP - VIEWPORT_PADDING;
    const roomAbove = triggerRect.top - MENU_GAP - VIEWPORT_PADDING;
    const shouldFlip = roomBelow < Math.min(menuRect.height, 160) && roomAbove > roomBelow;
    const maxHeight = Math.max(
      MIN_MENU_HEIGHT,
      shouldFlip ? roomAbove : roomBelow,
    );
    const rawTop = shouldFlip
      ? triggerRect.top - MENU_GAP - Math.min(menuRect.height, maxHeight)
      : triggerRect.bottom + MENU_GAP;
    const top = Math.max(
      VIEWPORT_PADDING,
      Math.min(rawTop, window.innerHeight - VIEWPORT_PADDING - Math.min(menuRect.height, maxHeight)),
    );
    menuStyle = `left: ${left}px; top: ${top}px; max-height: ${maxHeight}px; visibility: visible;`;
  }

  $effect(() => {
    if (typeof window === 'undefined') return;
    window.addEventListener('mousedown', onWindowClick);
    window.addEventListener('keydown', onWindowKeydown);
    return () => {
      window.removeEventListener('mousedown', onWindowClick);
      window.removeEventListener('keydown', onWindowKeydown);
    };
  });

  $effect(() => {
    if (typeof window === 'undefined' || !open) return;
    positionMenu();
    window.addEventListener('resize', positionMenu);
    window.addEventListener('scroll', positionMenu, true);
    return () => {
      window.removeEventListener('resize', positionMenu);
      window.removeEventListener('scroll', positionMenu, true);
    };
  });
</script>

<div class="dropdown-host" class:open bind:this={host}>
  {@render trigger({ open, toggle })}
  {#if open}
    <div
      bind:this={menuEl}
      use:portal
      class="dropdown-menu dropdown-menu-{placement}"
      style={menuStyle}
      role="menu"
    >
      {@render menu({ close })}
    </div>
  {/if}
</div>

<style>
  .dropdown-host {
    position: relative;
    display: inline-flex;
  }

  .dropdown-menu {
    box-sizing: border-box;
    position: fixed;
    min-width: var(--dropdown-menu-min-width, 180px);
    max-width: var(--dropdown-menu-max-width, min(320px, calc(100vw - 24px)));
    max-height: var(--dropdown-menu-max-height, min(320px, calc(100vh - 96px)));
    overflow-x: hidden;
    overflow-y: auto;
    overscroll-behavior: contain;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    padding: var(--space-16);
    z-index: var(--dropdown-menu-z-index, calc(var(--z-modal) + 1));
    animation: dropdown-in var(--motion-normal) var(--motion-easing);
  }

  @keyframes dropdown-in {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  /* Slotted items get a default item style — children opt out by
   * setting their own class. white-space: nowrap 으로 label wrap 차단 —
   * 메뉴는 공통 max width 안에서 가장 긴 라벨을 ellipsis 처리한다. */
  .dropdown-menu :global(button),
  .dropdown-menu :global(a) {
    display: flex;
    align-items: center;
    gap: var(--space-8);
    width: 100%;
    padding: var(--space-8) var(--space-12);
    background: transparent;
    border: 0;
    border-radius: var(--radius-md);
    color: var(--color-fg);
    font-size: var(--text-base);
    font-family: inherit;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .dropdown-menu :global(button > span),
  .dropdown-menu :global(a > span) {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dropdown-menu :global(button:hover:not(:disabled)),
  .dropdown-menu :global(a:hover) {
    background: var(--color-glass-2);
  }

  .dropdown-menu :global(button:disabled) {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .dropdown-menu :global(.danger) {
    color: var(--color-danger);
  }
</style>
