<script lang="ts">
  let {
    src,
    alt,
  }: {
    src: string;
    alt: string;
  } = $props();
</script>

<div class="image-viewer">
  <img {src} {alt} />
</div>

<style>
  .image-viewer {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    place-items: center;
    padding: var(--space-12);
    overflow: auto;
    background:
      linear-gradient(45deg, var(--color-glass-1) 25%, transparent 25%, transparent 75%, var(--color-glass-1) 75%) 0 0/16px 16px,
      linear-gradient(45deg, var(--color-glass-1) 25%, transparent 25%, transparent 75%, var(--color-glass-1) 75%) 8px 8px/16px 16px,
      var(--color-surface);
  }

  .image-viewer img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }
</style>
