<script lang="ts">
  /**
   * Focus mode toggle (plan 0005 Stage C, ADR-0017 §D5).
   *
   * Toggles `sessionStore.focusMode.enabled`. The actual visual effect
   * (canvas darken / single-panel highlight) is wired in a later phase —
   * this Stage C deliverable lands the *UI surface* only so the Titlebar
   * has a coherent action layout (theme + focus + menu) from day one.
   */

  import IconButton from '$lib/ui/IconButton.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';

  const enabled = $derived(sessionStore.focusMode.enabled === true);
  const label = $derived(enabled ? 'Exit focus mode' : 'Enter focus mode');

  function toggle(): void {
    sessionStore.focusMode = {
      enabled: !enabled,
      targetPanelId: sessionStore.focusMode.targetPanelId ?? null,
    };
  }
</script>

<IconButton aria-label={label} size="sm" onclick={toggle}>
  {#if enabled}
    <!-- Minimize2 — currently in focus, click to exit. -->
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <polyline points="4 14 10 14 10 20"/>
      <polyline points="20 10 14 10 14 4"/>
      <line x1="14" y1="10" x2="21" y2="3"/>
      <line x1="3" y1="21" x2="10" y2="14"/>
    </svg>
  {:else}
    <!-- Maximize2 — enter focus. -->
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <polyline points="15 3 21 3 21 9"/>
      <polyline points="9 21 3 21 3 15"/>
      <line x1="21" y1="3" x2="14" y2="10"/>
      <line x1="3" y1="21" x2="10" y2="14"/>
    </svg>
  {/if}
</IconButton>
