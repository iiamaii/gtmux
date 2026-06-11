<script lang="ts">
  let {
    html,
    label = 'Markdown document',
    eyebrow,
    scale = 1,
  }: {
    html: string;
    label?: string;
    eyebrow?: string;
    scale?: number;
  } = $props();
</script>

<article
  class="document-markdown-view"
  aria-label={label}
  style:--document-markdown-scale={scale}
>
  {#if eyebrow !== undefined && eyebrow.length > 0}
    <div class="dmv-eyebrow">{eyebrow}</div>
  {/if}
  <div class="dmv-content">{@html html}</div>
</article>

<style>
  .document-markdown-view {
    flex: 1 1 auto;
    min-height: 0;
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    overflow: auto;
    padding: var(--document-markdown-padding, 28px 36px 24px);
    background: var(--document-markdown-bg, transparent);
    color: var(--color-fg-muted);
    font-family: var(--font-sans);
    font-size: calc(12.5px * var(--document-markdown-scale, 1));
    line-height: 1.55;
    letter-spacing: 0;
    overflow-wrap: anywhere;
    scrollbar-width: thin;
  }

  .dmv-eyebrow {
    font-family: var(--font-mono);
    font-size: calc(10px * var(--document-markdown-scale, 1));
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
    margin-bottom: 18px;
  }

  .dmv-content :global(h1),
  .dmv-content :global(h2) {
    margin: 0 0 12px;
    font-family: var(--font-sans);
    font-size: calc(26px * var(--document-markdown-scale, 1));
    font-weight: 540;
    line-height: 1.15;
    color: var(--color-fg);
  }

  .dmv-content :global(h3) {
    margin: 16px 0 8px;
    font-size: calc(18px * var(--document-markdown-scale, 1));
    font-weight: 600;
    color: var(--color-fg);
  }

  .dmv-content :global(h4),
  .dmv-content :global(h5),
  .dmv-content :global(h6) {
    margin: 14px 0 6px;
    font-size: calc(14px * var(--document-markdown-scale, 1));
    font-weight: 600;
    color: var(--color-fg);
  }

  .dmv-content :global(p) {
    margin: 0 0 10px;
  }

  .dmv-content :global(ul),
  .dmv-content :global(ol) {
    margin: 0 0 10px;
    padding-left: calc(22px * var(--document-markdown-scale, 1));
  }

  .dmv-content :global(li) {
    margin: 2px 0;
  }

  .dmv-content :global(blockquote) {
    margin: 0 0 10px;
    padding-left: 12px;
    border-left: 2px solid var(--color-border-strong);
    color: var(--color-fg-subtle);
  }

  .dmv-content :global(code) {
    font-family: var(--font-mono);
    font-size: calc(11.5px * var(--document-markdown-scale, 1));
    padding: 0 4px;
    background: var(--color-surface-2);
    border-radius: var(--radius-sm);
  }

  .dmv-content :global(pre) {
    margin: 0 0 10px;
    padding: 10px 12px;
    background: var(--color-surface-2);
    border-radius: var(--radius-sm);
    overflow-x: auto;
    font-family: var(--font-mono);
    font-size: calc(11.5px * var(--document-markdown-scale, 1));
    line-height: 1.5;
  }

  .dmv-content :global(pre code) {
    padding: 0;
    background: transparent;
    border-radius: 0;
  }

  .dmv-content :global(a) {
    color: var(--color-accent);
    text-decoration: underline;
  }

  .dmv-content :global(table) {
    border-collapse: collapse;
    margin: 0 0 10px;
    font-size: calc(12px * var(--document-markdown-scale, 1));
  }

  .dmv-content :global(th),
  .dmv-content :global(td) {
    border: 1px solid var(--color-border);
    padding: 4px 8px;
    text-align: left;
  }

  .dmv-content :global(th) {
    background: var(--color-surface-2);
    font-weight: 600;
    color: var(--color-fg);
  }

  .dmv-content :global(hr) {
    border: 0;
    border-top: 1px solid var(--color-border);
    margin: 14px 0;
  }

  .dmv-content :global(img) {
    max-width: 100%;
    height: auto;
  }
</style>
