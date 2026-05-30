<script lang="ts">
  import type { Head } from '$lib/types/canvas';

  interface Props {
    head?: Head;
    size?: number;
  }

  const { head = 'none', size = 18 }: Props = $props();
</script>

<svg
  class="head-icon"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <line x1="4" y1="12" x2={head === 'none' ? 20 : 16} y2="12" />
  {#if head === 'arrow'}
    <path d="M14 7l5 5-5 5" />
  {:else if head === 'circle'}
    <circle cx="18" cy="12" r="3.5" />
  {:else if head === 'diamond'}
    <path d="M18 7l5 5-5 5-5-5z" />
  {/if}
</svg>

<style>
  .head-icon {
    display: block;
    flex: 0 0 auto;
  }
</style>
