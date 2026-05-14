<script lang="ts">
  /**
   * Banner primitive — sticky top notifications (ADR-0016 §D4).
   *
   * ReconnectBanner is the canonical caller. Other use cases:
   *   - Auth expired
   *   - Server shutting down
   *   - Layout out of sync (412)
   */

  import type { Snippet } from 'svelte';

  type Tone = 'warn' | 'error' | 'info';

  interface Props {
    /** Visual tone. `warn` = transient (yellow), `error` = blocking (red),
     *  `info` = neutral (accent). */
    tone?: Tone;
    /** Optional aria-live politeness — default 'polite' for non-disruptive
     *  announcements. Use 'assertive' only for urgent / blocking. */
    live?: 'polite' | 'assertive';
    children: Snippet;
  }

  const { tone = 'warn', live = 'polite', children }: Props = $props();
</script>

<div class="banner banner-{tone}" role="status" aria-live={live}>
  {@render children()}
</div>

<style>
  .banner {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    height: var(--layout-banner-h);
    padding: 0 var(--space-4);
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    border-bottom: 1px solid transparent;
    z-index: var(--z-banner);
    flex: 0 0 auto;
    animation: banner-in var(--motion-normal) var(--motion-easing);
  }

  .banner-warn {
    background: var(--banner-warn-bg);
    color: var(--banner-warn-fg);
    border-bottom-color: var(--banner-warn-border);
  }

  .banner-error {
    background: var(--banner-error-bg);
    color: var(--banner-error-fg);
    border-bottom-color: var(--banner-error-border);
  }

  .banner-info {
    background: var(--color-surface-2);
    color: var(--color-info);
    border-bottom-color: var(--color-border-strong);
  }

  @keyframes banner-in {
    from {
      opacity: 0;
      transform: translateY(-100%);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
</style>
