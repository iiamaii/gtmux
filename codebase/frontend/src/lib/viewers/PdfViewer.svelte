<script lang="ts">
  let {
    src,
    title,
    dragIsolated = false,
  }: {
    src: string;
    title: string;
    dragIsolated?: boolean;
  } = $props();
</script>

<iframe
  class="pdf-viewer-frame"
  class:drag-isolated={dragIsolated}
  {src}
  {title}
  referrerpolicy="no-referrer"
  loading="lazy"
></iframe>

<style>
  .pdf-viewer-frame {
    display: block;
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    width: 100%;
    height: 100%;
    border: 0;
    background: #ffffff;
  }

  .pdf-viewer-frame.drag-isolated {
    pointer-events: none;
  }
</style>
