<script lang="ts">
  // Temporary toolbar (plan 0005 Stage B stopgap — real Titlebar arrives in Stage C).
  //
  // For now this renders a thin 40px header chrome with branding + theme
  // toggle so the user has an interactive surface to verify the design
  // tokens (Stage A) + theme store (Stage B). Stage C replaces this with
  // a Figma-style 44px Titlebar + 56px Toolbar split.

  import ThemeToggle from '$lib/ui/ThemeToggle.svelte';
</script>

<header class="toolbar">
  <div class="brand">gtmux</div>
  <div class="spacer"></div>
  <div class="actions">
    <ThemeToggle />
  </div>
</header>

<style>
  .toolbar {
    /* Explicit 40px stopgap. Stage C splits this into 44px Titlebar +
     * 56px Toolbar — at that point the `--layout-*` tokens drive each
     * half. For now keep the chrome compact. */
    display: flex;
    align-items: center;
    height: 40px;
    padding: 0 var(--space-12);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    z-index: var(--z-toolbar);
    flex: 0 0 auto;
    user-select: none;
  }

  .brand {
    font-family: var(--font-mono);
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    color: var(--color-fg);
    letter-spacing: 0.2px;
  }

  .spacer {
    flex: 1 1 auto;
  }

  .actions {
    display: inline-flex;
    align-items: center;
    gap: var(--space-4);
  }
</style>
