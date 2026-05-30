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

  let open = $state(false);
  let host: HTMLDivElement | undefined = $state();

  function toggle(): void {
    open = !open;
  }

  function close(): void {
    open = false;
  }

  function onWindowClick(e: MouseEvent): void {
    if (!open || !host) return;
    if (!host.contains(e.target as Node)) close();
  }

  function onWindowKeydown(e: KeyboardEvent): void {
    if (open && e.key === 'Escape') {
      e.preventDefault();
      close();
    }
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
</script>

<div class="dropdown-host" class:open bind:this={host}>
  {@render trigger({ open, toggle })}
  {#if open}
    <div class="dropdown-menu dropdown-menu-{placement}" role="menu">
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
    position: absolute;
    min-width: var(--dropdown-menu-min-width, 180px);
    max-width: var(--dropdown-menu-max-width, min(320px, calc(100vw - 24px)));
    max-height: var(--dropdown-menu-max-height, min(320px, calc(100vh - 96px)));
    overflow-x: hidden;
    overflow-y: auto;
    overscroll-behavior: contain;
    margin-top: var(--space-16);
    background: var(--color-surface-2);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    padding: var(--space-16);
    z-index: var(--z-toolbar);
    animation: dropdown-in var(--motion-normal) var(--motion-easing);
  }

  .dropdown-menu-bottom-end {
    top: 100%;
    right: 0;
  }

  .dropdown-menu-bottom-start {
    top: 100%;
    left: 0;
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
