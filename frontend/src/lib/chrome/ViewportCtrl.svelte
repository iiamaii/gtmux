<script lang="ts">
  /**
   * ViewportCtrl — bottom-center floating zoom pill (plan 0005 Stage F,
   * ADR-0017 §D2).
   *
   * Uses `useSvelteFlow()` so it lives outside the SvelteFlow component
   * itself (the App's SvelteFlowProvider wrap makes the hook resolvable
   * everywhere in the subtree).
   *
   * Controls (left → right):
   *   - − Zoom out
   *   - 100%  / current zoom label (click → reset to 1.0)
   *   - + Zoom in
   *   - ⊟ Fit (fitView)
   *   - M:N  current Manipulation Selection count badge (live)
   */

  import { useSvelteFlow } from '@xyflow/svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';

  const flow = useSvelteFlow();

  const zoomPct = $derived(Math.round(sessionStore.viewport.zoom * 100));
  const mCount = $derived(sessionStore.M.size);

  function onZoomIn(): void {
    void flow.zoomIn({ duration: 150 });
  }
  function onZoomOut(): void {
    void flow.zoomOut({ duration: 150 });
  }
  function onReset100(): void {
    void flow.setViewport(
      { x: sessionStore.viewport.x, y: sessionStore.viewport.y, zoom: 1 },
      { duration: 150 }
    );
  }
  function onFit(): void {
    void flow.fitView({ duration: 200, padding: 0.2 });
  }
</script>

<div class="viewport-ctrl" role="toolbar" aria-label="Viewport controls">
  <button type="button" class="vp-btn" aria-label="Zoom out" title="Zoom out" onclick={onZoomOut}>
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <line x1="5" y1="12" x2="19" y2="12"/>
    </svg>
  </button>
  <button type="button" class="vp-zoom" aria-label="Reset zoom to 100%" title="Reset to 100%" onclick={onReset100}>
    {zoomPct}%
  </button>
  <button type="button" class="vp-btn" aria-label="Zoom in" title="Zoom in" onclick={onZoomIn}>
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <line x1="12" y1="5" x2="12" y2="19"/>
      <line x1="5" y1="12" x2="19" y2="12"/>
    </svg>
  </button>
  <span class="vp-divider" aria-hidden="true"></span>
  <button type="button" class="vp-btn" aria-label="Fit all panels" title="Fit all panels" onclick={onFit}>
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <polyline points="4 14 4 20 10 20"/>
      <polyline points="20 10 20 4 14 4"/>
      <line x1="14" y1="10" x2="21" y2="3"/>
      <line x1="3" y1="21" x2="10" y2="14"/>
    </svg>
  </button>
  <span class="vp-divider" aria-hidden="true"></span>
  <span class="vp-badge" title="Manipulation Selection count" aria-label={`${mCount} panels selected`}>
    M:{mCount}
  </span>
</div>

<style>
  .viewport-ctrl {
    position: absolute;
    bottom: var(--space-16);
    left: 50%;
    transform: translateX(-50%);
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-4);
    background: var(--color-surface);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    color: var(--color-fg);
    z-index: var(--z-canvas-overlay);
    user-select: none;
  }

  .vp-btn {
    width: 30px;
    height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 0;
    border-radius: 50%;
    color: var(--color-fg);
    cursor: pointer;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .vp-btn:hover {
    background: var(--color-glass-1);
  }

  .vp-zoom {
    min-width: 56px;
    text-align: center;
    font-family: var(--font-mono);
    font-size: var(--text-md);
    letter-spacing: 0.2px;
    color: var(--color-fg);
    background: transparent;
    border: 0;
    padding: 0 var(--space-8);
    height: 30px;
    cursor: pointer;
    border-radius: var(--radius-pill);
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .vp-zoom:hover {
    background: var(--color-glass-1);
  }

  .vp-divider {
    width: 1px;
    height: 18px;
    background: var(--color-border);
    margin: 0 var(--space-4);
  }

  .vp-badge {
    padding: 0 var(--space-8);
    height: 22px;
    display: inline-flex;
    align-items: center;
    border-radius: var(--radius-pill);
    background: var(--color-glass-1);
    color: var(--color-accent);
    font-family: var(--font-mono);
    font-size: var(--text-base);
    letter-spacing: 0.2px;
  }
</style>
