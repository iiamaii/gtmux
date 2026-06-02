<script lang="ts">
  /**
   * Shared empty state for left/right panel tabs.
   *
   * Visual contract follows FileTreeView's original panel-state pattern:
   * centered disc icon, compact lead, and short muted description.
   */

  type PanelEmptyIcon = 'files' | 'layers' | 'terminal' | 'preview' | 'inspect' | 'alert';
  type Tone = 'muted' | 'danger';

  interface Props {
    icon: PanelEmptyIcon;
    lead: string;
    description?: string;
    tone?: Tone;
    role?: 'status' | 'alert';
    live?: 'polite' | 'assertive';
  }

  const {
    icon,
    lead,
    description,
    tone = 'muted',
    role,
    live,
  }: Props = $props();
</script>

<div class="panel-empty panel-empty-{tone}" {role} aria-live={live}>
  <span class="state-disc" aria-hidden="true">
    {#if icon === 'layers'}
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round">
        <path d="M8 2 2.5 4.8 8 7.6l5.5-2.8L8 2Z"/>
        <path d="m2.5 8 5.5 2.8L13.5 8"/>
        <path d="m2.5 11.2 5.5 2.8 5.5-2.8"/>
      </svg>
    {:else if icon === 'terminal'}
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round">
        <path d="m3.2 5 3 3-3 3"/>
        <path d="M8 11h4.8"/>
      </svg>
    {:else if icon === 'preview'}
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round">
        <path d="M3.5 2.5h5L12.5 6.5v7h-9v-11Z"/>
        <path d="M8.5 2.5v4h4"/>
        <path d="M5.2 10s1-1.7 2.8-1.7 2.8 1.7 2.8 1.7-1 1.7-2.8 1.7S5.2 10 5.2 10Z"/>
        <circle cx="8" cy="10" r="0.7"/>
      </svg>
    {:else if icon === 'inspect'}
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="8" cy="8" r="5.7"/>
        <path d="M8 7.4v3.4"/>
        <path d="M8 5.2h.01"/>
      </svg>
    {:else if icon === 'alert'}
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round">
        <path d="M8 2.5 14 13H2L8 2.5Z"/>
        <path d="M8 6.3v3.1"/>
        <path d="M8 11.5h.01"/>
      </svg>
    {:else}
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
        <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.2 1.5h5.2A1.5 1.5 0 0 1 14 6v5.5A1.5 1.5 0 0 1 12.5 13h-9A1.5 1.5 0 0 1 2 11.5z"/>
      </svg>
    {/if}
  </span>
  <span class="state-lead">{lead}</span>
  {#if description !== undefined && description.length > 0}
    <span class="state-desc">{description}</span>
  {/if}
</div>

<style>
  .panel-empty {
    box-sizing: border-box;
    flex: 1 1 auto;
    width: 100%;
    min-height: 0;
    display: grid;
    place-items: center;
    align-content: center;
    gap: var(--space-8);
    padding: var(--space-24) var(--space-12);
    text-align: center;
    color: var(--color-fg-muted);
  }

  .state-disc {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--color-glass-1);
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
  }

  .state-disc svg {
    width: 15px;
    height: 15px;
    display: block;
  }

  .panel-empty-danger .state-disc {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 14%, transparent);
  }

  .state-lead {
    color: var(--color-fg);
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
  }

  .state-desc {
    max-width: 190px;
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
    letter-spacing: -0.1px;
    line-height: var(--leading-normal);
  }
</style>
