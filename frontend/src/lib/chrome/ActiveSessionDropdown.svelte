<script lang="ts">
  /**
   * ActiveSessionDropdown — Toolbar 우측 현재 session 표시 + 클릭 시 switch.
   *
   * 정본:
   * - plan-0007 §14 FE-NEW-1 (Session UI)
   * - ADR-0019 D5 (session-scoped store)
   * - frontend-handover §1 mental model (Toolbar 우측)
   *
   * 동작:
   * - `sessionStore.active.name` 보여줌. null 이면 "No session" placeholder.
   * - 클릭 → `onSwitch()` (부모가 SessionListModal 띄움).
   * - Detach 흐름은 SessionMenu 또는 SettingsOverlay 의 책임 — 본 컴포넌트는
   *   순수 *진입점*.
   */

  import { onMount } from 'svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';

  interface Props {
    /** 클릭 시 호출 — 부모가 SessionListModal 띄움. */
    onSwitch: () => void;
  }

  const { onSwitch }: Props = $props();

  let active = $derived(sessionStore.active);
  // Badge — 활성 session 의 canvas 에 attach 된 terminal 수 (server-wide pool size 아님).
  // sessionStore.items 중 type==='terminal' 카운트 = 현 canvas 의 terminal panel 수.
  let sessionTerminalCount = $derived.by(() => {
    if (active === null) return 0;
    let n = 0;
    for (const item of sessionStore.items.values()) {
      if (item.type === 'terminal') n += 1;
    }
    return n;
  });

  // Pool 폴링 구독 유지 — TerminalListView 등 다른 consumer 가 폴링 상태 의존.
  onMount(() => terminalPool.subscribe());
</script>

<button
  type="button"
  class="active-session"
  aria-label={active ? `Switch session (current: ${active.name})` : 'Choose a session'}
  onclick={onSwitch}
>
  <span class="dot" aria-hidden="true" class:on={active !== null}></span>
  <span class="name">
    {#if active}
      {active.name}
    {:else}
      <em>No session</em>
    {/if}
  </span>
  {#if sessionTerminalCount > 0}
    <span class="pool-badge" title="{sessionTerminalCount} terminal(s) in this session">
      {sessionTerminalCount}
    </span>
  {/if}
</button>

<style>
  .active-session {
    display: inline-flex;
    align-items: center;
    gap: var(--space-8);
    height: 36px;
    padding: 0 var(--space-10);
    border-radius: var(--radius-md);
    background: var(--color-surface-2);
    color: var(--color-fg);
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    border: 1px solid var(--color-border);
    transition:
      background var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing);
    max-width: 240px;
  }

  .active-session:hover {
    background: var(--color-glass-1);
    border-color: var(--color-border-strong);
  }

  .active-session:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-fg-subtle);
    flex-shrink: 0;
  }

  .dot.on {
    background: var(--color-success);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-success) 28%, transparent);
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .name em {
    font-style: normal;
    color: var(--color-fg-muted);
    font-weight: var(--weight-regular);
  }

  .pool-badge {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    padding: 1px 6px;
    border-radius: var(--radius-pill);
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    color: var(--color-accent);
    letter-spacing: 0.2px;
    flex-shrink: 0;
  }
</style>
