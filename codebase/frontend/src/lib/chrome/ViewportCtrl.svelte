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
   *   - ⊙ Focus selection — 선택된 item/group 의 union BBox 로 viewport 이동.
   *     M.size === 0 일 때 disabled.
   *   - M:N  current Manipulation Selection count badge (live)
   */

  import { onMount } from 'svelte';
  import { useSvelteFlow, type Viewport } from '@xyflow/svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { shortcutRegistry, type ShortcutDescriptor } from '$lib/keyboard/shortcutRegistry.svelte';
  import { labelWithShortcut, shortcutForAction } from '$lib/keyboard/shortcutDisplay';
  import { VIEWPORT_ZOOM_STEP, clampViewportZoom } from '$lib/canvas/viewportPolicy';

  const flow = useSvelteFlow();

  const zoomPct = $derived(Math.round(sessionStore.viewport.zoom * 100));
  const mCount = $derived(sessionStore.M.size);
  const shortcutActions = $derived.by(() => shortcutRegistry.listActions());

  const zoomOutShortcut = $derived(shortcutForAction(shortcutActions, 'viewport.zoom_out'));
  const resetShortcut = $derived(shortcutForAction(shortcutActions, 'viewport.reset_100'));
  const zoomInShortcut = $derived(shortcutForAction(shortcutActions, 'viewport.zoom_in'));
  const fitAllShortcut = $derived(shortcutForAction(shortcutActions, 'viewport.fit_all'));
  const fitSelectionShortcut = $derived(
    shortcutForAction(shortcutActions, 'viewport.fit_selection'),
  );

  function onZoomIn(): void {
    setZoom(sessionStore.viewport.zoom + VIEWPORT_ZOOM_STEP);
  }
  function onZoomOut(): void {
    setZoom(sessionStore.viewport.zoom - VIEWPORT_ZOOM_STEP);
  }
  function onReset100(): void {
    setZoom(1);
  }
  function onFit(): void {
    void flow.fitView({ duration: 200, padding: 0.2 });
  }
  function onFitSelection(): void {
    sessionStore.zoomToSelection({ mode: 'fit' });
  }
  function onGoToSelection(): void {
    sessionStore.zoomToSelection({ mode: 'center' });
  }

  function setZoom(nextZoom: number): void {
    const next = viewportForZoom(clampViewportZoom(nextZoom));
    sessionStore.updateViewport(next);
    void flow.setViewport(next, { duration: 150 });
  }

  function viewportForZoom(nextZoom: number): Viewport {
    const current = sessionStore.viewport;
    const center = canvasCenter();
    const currentZoom = current.zoom <= 0 ? 1 : current.zoom;
    const centerFlow = {
      x: (center.x - current.x) / currentZoom,
      y: (center.y - current.y) / currentZoom,
    };
    return {
      x: center.x - centerFlow.x * nextZoom,
      y: center.y - centerFlow.y * nextZoom,
      zoom: nextZoom,
    };
  }

  function canvasCenter(): { x: number; y: number } {
    const root = document.querySelector('.canvas-root') as HTMLElement | null;
    if (root === null) return { x: window.innerWidth / 2, y: window.innerHeight / 2 };
    const rect = root.getBoundingClientRect();
    return { x: rect.width / 2, y: rect.height / 2 };
  }

  function registerViewportShortcuts(): () => void {
    const unsubs: Array<() => void> = [];
    const registerPair = (
      descriptor: Omit<ShortcutDescriptor, 'meta' | 'ctrl'>,
    ): void => {
      unsubs.push(shortcutRegistry.register({ ...descriptor, meta: true }));
      unsubs.push(shortcutRegistry.register({ ...descriptor, ctrl: true }));
    };
    const consume = (fn: () => void): boolean => {
      fn();
      return true;
    };

    registerPair({
      actionId: 'viewport.reset_100',
      key: '0',
      description: 'Reset viewport to 100%',
      category: 'Viewport',
      customizable: true,
      handler: () => consume(onReset100),
    });
    registerPair({
      actionId: 'viewport.fit_all',
      key: '1',
      description: 'Fit all panels',
      category: 'Viewport',
      customizable: true,
      handler: () => consume(onFit),
    });
    registerPair({
      actionId: 'viewport.fit_selection',
      key: '2',
      description: 'Fit selection',
      category: 'Viewport',
      customizable: true,
      handler: () => consume(onFitSelection),
    });
    registerPair({
      actionId: 'viewport.go_to_selection',
      key: '.',
      description: 'Go to selection',
      category: 'Viewport',
      customizable: true,
      handler: () => consume(onGoToSelection),
    });
    registerPair({
      actionId: 'viewport.zoom_in',
      key: '=',
      description: 'Zoom in',
      category: 'Viewport',
      customizable: true,
      handler: () => consume(onZoomIn),
    });
    registerPair({
      actionId: 'viewport.zoom_in',
      key: '+',
      shift: true,
      description: 'Zoom in',
      category: 'Viewport',
      customizable: true,
      handler: () => consume(onZoomIn),
    });
    registerPair({
      actionId: 'viewport.zoom_out',
      key: '-',
      description: 'Zoom out',
      category: 'Viewport',
      customizable: true,
      handler: () => consume(onZoomOut),
    });
    unsubs.push(
      shortcutRegistry.register({
        actionId: 'viewport.hold_pan',
        key: ' ',
        description: 'Hold to pan canvas',
        category: 'Viewport',
        customizable: false,
        protectedReason: 'Space is a hold gesture handled by the canvas.',
        allowInEditable: false,
        allowInXterm: false,
        handler: () => false,
      }),
    );

    return () => {
      for (const fn of unsubs) fn();
    };
  }

  onMount(() => registerViewportShortcuts());
</script>

<div class="viewport-ctrl" role="toolbar" aria-label="Viewport controls">
  <button
    type="button"
    class="vp-btn"
    aria-label="Zoom out"
    title={labelWithShortcut('Zoom out', zoomOutShortcut)}
    onclick={onZoomOut}
  >
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <line x1="5" y1="12" x2="19" y2="12"/>
    </svg>
  </button>
  <button
    type="button"
    class="vp-zoom"
    aria-label="Reset zoom to 100%"
    title={labelWithShortcut('Reset to 100%', resetShortcut)}
    onclick={onReset100}
  >
    {zoomPct}%
  </button>
  <button
    type="button"
    class="vp-btn"
    aria-label="Zoom in"
    title={labelWithShortcut('Zoom in', zoomInShortcut)}
    onclick={onZoomIn}
  >
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <line x1="12" y1="5" x2="12" y2="19"/>
      <line x1="5" y1="12" x2="19" y2="12"/>
    </svg>
  </button>
  <span class="vp-divider" aria-hidden="true"></span>
  <button
    type="button"
    class="vp-btn"
    aria-label="Fit all panels"
    title={labelWithShortcut('Fit all panels', fitAllShortcut)}
    onclick={onFit}
  >
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <polyline points="4 14 4 20 10 20"/>
      <polyline points="20 10 20 4 14 4"/>
      <line x1="14" y1="10" x2="21" y2="3"/>
      <line x1="3" y1="21" x2="10" y2="14"/>
    </svg>
  </button>
  <button
    type="button"
    class="vp-btn"
    aria-label={mCount > 1 ? `Fit ${mCount} selected elements` : 'Fit selected element'}
    title={mCount === 0
      ? labelWithShortcut('Fit selection (select item or group first)', fitSelectionShortcut)
      : mCount > 1
        ? labelWithShortcut(`Fit ${mCount} selected elements`, fitSelectionShortcut)
        : labelWithShortcut('Fit selected element', fitSelectionShortcut)}
    disabled={mCount === 0}
    onclick={onFitSelection}
  >
    <!-- target reticle -->
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="9"/>
      <circle cx="12" cy="12" r="3" fill="currentColor" stroke="none"/>
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

  .vp-btn:hover:not(:disabled) {
    background: var(--color-glass-1);
  }

  .vp-btn:disabled {
    cursor: not-allowed;
    opacity: 0.35;
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
