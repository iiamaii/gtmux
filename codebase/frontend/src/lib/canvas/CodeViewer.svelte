<script lang="ts">
  import { highlightLines, type HlLine } from './codeHighlight';
  import { themeStore } from '$lib/stores/theme.svelte';

  let {
    text,
    lang = 'text',
    filename = 'Source code',
  }: {
    text: string;
    lang?: string;
    filename?: string;
  } = $props();

  let highlighted = $state<HlLine[] | null>(null);
  const lines = $derived(text.split('\n'));
  const cleanLang = $derived(lang.trim().toLowerCase() || 'text');

  $effect(() => {
    const raw = text;
    const nextLang = cleanLang;
    const theme = themeStore.resolved;
    let cancelled = false;
    highlighted = null;

    async function runHighlight(): Promise<void> {
      const next = await highlightLines(raw, nextLang, theme);
      if (!cancelled) highlighted = next;
    }

    void runHighlight();
    return () => {
      cancelled = true;
    };
  });
</script>

<div class="code-viewer" data-code-viewer data-lang={cleanLang} aria-label={filename}>
  {#each lines as line, index (index)}
    {@const tokens = highlighted?.[index]}
    <div class="cv-line" data-line={index + 1}>
      <span class="cv-gutter" aria-hidden="true">{index + 1}</span>
      <code class="cv-code" data-code>
        {#if tokens !== undefined && tokens.length > 0}
          {#each tokens as token, tokenIndex (tokenIndex)}
            <span style:color={token.color}>{token.content}</span>
          {/each}
        {:else}
          {line}
        {/if}
      </code>
    </div>
  {/each}
</div>

<style>
  .code-viewer {
    flex: 1 1 auto;
    min-height: 0;
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    overflow: auto;
    padding: var(--code-viewer-padding, var(--space-8) 0);
    background: var(--code-viewer-bg, var(--color-surface));
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: var(--code-viewer-font-size, 10.5px);
    line-height: var(--code-viewer-line-height, 1.6);
    scrollbar-width: thin;
    user-select: text;
  }

  .cv-line {
    display: grid;
    grid-template-columns: var(--code-viewer-gutter-width, 32px) max-content;
    gap: var(--code-viewer-gap, var(--space-8));
    min-width: max-content;
    padding-right: var(--code-viewer-line-pr, var(--space-12));
  }

  .cv-gutter {
    color: var(--color-fg-subtle);
    text-align: right;
    user-select: none;
  }

  .cv-code {
    color: var(--color-fg);
    white-space: pre;
    font: inherit;
  }
</style>
