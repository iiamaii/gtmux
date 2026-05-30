<script lang="ts">
  import type { PathRouting } from '$lib/types/canvas';

  interface Props {
    routing: PathRouting;
    size?: number;
  }

  const { routing, size = 18 }: Props = $props();
</script>

<svg
  class="routing-icon"
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
  {#if routing === 'orthogonal'}
    <path d="M5 17V8h14" />
  {:else if routing === 'bezier'}
    <path d="M4 16C8 5 16 19 20 8" />
  {:else}
    <path d="M5 17L19 7" />
  {/if}
</svg>

<style>
  .routing-icon {
    display: block;
    flex: 0 0 auto;
  }
</style>
