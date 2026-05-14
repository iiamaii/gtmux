<script lang="ts">
  /**
   * HelpBar — top-center floating kbd hint pill (plan 0005 Stage F,
   * ADR-0017 §D2).
   *
   * Pure visual hint — pointer-events:none so it never intercepts
   * canvas pan / click. Lists the three most-used input bindings:
   *   - ⌘+drag / Space+drag → pan
   *   - ⌘+wheel             → zoom
   *   - ⌘N                  → new panel (Stage D 시점에 wire 예정)
   *
   * Mono uppercase 11px (ref §5.5 정합).
   */
</script>

<div class="help-bar" role="presentation" aria-hidden="true">
  <span><kbd>Space</kbd> + drag · pan</span>
  <span class="sep">|</span>
  <span><kbd>⌘</kbd> + scroll · zoom</span>
  <span class="sep">|</span>
  <span><kbd>right-click</kbd> · menu</span>
</div>

<style>
  .help-bar {
    position: absolute;
    top: var(--space-8);
    left: 50%;
    transform: translateX(-50%);
    display: inline-flex;
    align-items: center;
    gap: var(--space-8);
    padding: var(--space-4) var(--space-12);
    background: var(--color-surface);
    border-radius: var(--radius-pill);
    box-shadow: var(--shadow-sm);
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-base);
    letter-spacing: 0.4px;
    z-index: var(--z-canvas-overlay);
    pointer-events: none;
    user-select: none;
    white-space: nowrap;
  }

  .help-bar kbd {
    background: var(--color-glass-2);
    color: var(--color-fg);
    padding: 1px var(--space-6);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }

  .sep {
    color: var(--color-fg-subtle);
  }

  /* On narrow viewport the help-bar can crowd the toolbar overlay; hide. */
  @media (max-width: 720px) {
    .help-bar {
      display: none;
    }
  }
</style>
