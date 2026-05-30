<script lang="ts">
  import type { Head } from '$lib/types/canvas';

  interface Props {
    id: string;
    head: Head | undefined;
    stroke: string;
    orient?: 'auto' | 'auto-start-reverse';
  }

  const { id, head = 'none', stroke, orient = 'auto' }: Props = $props();
</script>

{#if head !== 'none'}
  <marker
    {id}
    viewBox="0 0 10 10"
    refX={head === 'arrow' ? 9 : 5}
    refY="5"
    markerWidth={head === 'arrow' ? 6 : 5}
    markerHeight={head === 'arrow' ? 6 : 5}
    markerUnits="strokeWidth"
    {orient}
  >
    {#if head === 'arrow'}
      <path d="M 0 0 L 10 5 L 0 10 z" fill={stroke} />
    {:else if head === 'circle'}
      <circle cx="5" cy="5" r="3.5" fill={stroke} />
    {:else if head === 'diamond'}
      <path d="M 5 0 L 10 5 L 5 10 L 0 5 z" fill={stroke} />
    {/if}
  </marker>
{/if}
