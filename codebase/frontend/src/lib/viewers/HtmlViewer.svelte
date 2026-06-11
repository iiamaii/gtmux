<script lang="ts">
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
</script>

<iframe
  class="html-viewer-frame"
  class:drag-isolated={dragIsolated}
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
