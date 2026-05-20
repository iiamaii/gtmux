<script lang="ts">
  /**
   * Light / dark theme toggle button (plan 0005 Stage B/C).
   *
   * Renders a sun (light → tap to switch) or moon (dark → tap to switch)
   * icon depending on the current `themeStore.theme`. The icon shown
   * represents the *target* theme — clicking flips to the icon's mode.
   *
   * Chrome icons (Sun/Moon) are inline SVG to avoid lucide-svelte 1.0.1's
   * `$$props` legacy syntax tripping Svelte 5's strict vite build.
   * Feature-level lucide imports stay viable when needed via $lib/ui/Icon.
   */

  import IconButton from './IconButton.svelte';
  import { themeStore } from '$lib/stores/theme.svelte';

  const isDark = $derived(themeStore.theme === 'dark');
  const ariaLabel = $derived(
    isDark ? 'Switch to light theme' : 'Switch to dark theme'
  );
</script>

<IconButton aria-label={ariaLabel} size="sm" onclick={() => themeStore.toggle()}>
  {#if isDark}
    <!-- Sun — shown when current is dark, clicking → light. -->
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="4"></circle>
      <path d="M12 2v2"></path>
      <path d="M12 20v2"></path>
      <path d="m4.93 4.93 1.41 1.41"></path>
      <path d="m17.66 17.66 1.41 1.41"></path>
      <path d="M2 12h2"></path>
      <path d="M20 12h2"></path>
      <path d="m6.34 17.66-1.41 1.41"></path>
      <path d="m19.07 4.93-1.41 1.41"></path>
    </svg>
  {:else}
    <!-- Moon — shown when current is light, clicking → dark. -->
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"></path>
    </svg>
  {/if}
</IconButton>
