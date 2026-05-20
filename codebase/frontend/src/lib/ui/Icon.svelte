<script lang="ts">
  /**
   * Icon wrapper around lucide-svelte components (ADR-0016 §D5/D6).
   *
   * Usage:
   *   import Icon from '$lib/ui/Icon.svelte';
   *   import { Terminal } from 'lucide-svelte';
   *   ...
   *   <Icon icon={Terminal} size={16} />
   *
   * Decorative icons leave `aria-label` unset and inherit lucide's
   * automatic `aria-hidden=true`. Interactive icons (inside an
   * IconButton) attach an `aria-label`.
   */

  /* eslint-disable @typescript-eslint/no-explicit-any */
  interface Props {
    /** lucide-svelte icon component, e.g. `Terminal`, `X`, `MoreVertical`. */
    icon: any;
    size?: number;
    color?: string;
    strokeWidth?: number;
    'aria-label'?: string;
    class?: string;
  }

  const props: Props = $props();
  const IconComp = $derived(props.icon);
</script>

<IconComp
  size={props.size ?? 16}
  color={props.color ?? 'currentColor'}
  strokeWidth={props.strokeWidth ?? 2}
  aria-label={props['aria-label']}
  class={props.class}
/>
