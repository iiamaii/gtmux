<script lang="ts">
  import { gestureShield } from '$lib/stores/gestureShield.svelte';

  let {
    srcdoc,
    title,
    sandbox,
    dragIsolated = false,
  }: {
    srcdoc: string;
    title: string;
    sandbox: string;
    dragIsolated?: boolean;
  } = $props();

  // ADR-0059 D7 — drop pointer-events during any canvas gesture (in addition to
  // this node's own drag), so a pan/lasso/drag over the iframe reaches the canvas.
  const isolated = $derived(dragIsolated || gestureShield.active);
</script>

<iframe
  class="html-viewer-frame"
  class:drag-isolated={isolated}
  {sandbox}
  {title}
  referrerpolicy="no-referrer"
  loading="lazy"
  {srcdoc}
></iframe>

<style>
  .html-viewer-frame {
    display: block;
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    width: 100%;
    height: 100%;
    border: 0;
    background: #ffffff;
  }

  .html-viewer-frame.drag-isolated {
    pointer-events: none;
  }
</style>
