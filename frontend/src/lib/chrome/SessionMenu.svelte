<script lang="ts">
  /**
   * Session menu — Titlebar 좌측 kebab dropdown (plan 0005 Stage C, ADR-0017 §D2).
   *
   * Items:
   *   - Session shutdown → ShutdownModal (destructive, danger class)
   *   - Rotate token (P1+ — currently shows toast hint)
   *   - About → simple alert (P1+ proper About modal)
   */

  import Dropdown from '$lib/ui/Dropdown.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';
  import ShutdownModal from './ShutdownModal.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';

  interface Props {
    sessionName: string;
  }

  const { sessionName }: Props = $props();

  let shutdownOpen = $state(false);

  function onRotateToken(): void {
    toastStore.show({
      message: `Run \`gtmux rotate-token --session ${sessionName}\` in the CLI`,
      tone: 'info',
      durationMs: 6_000,
    });
  }

  function onAbout(): void {
    toastStore.show({
      message: 'gtmux — tmux-backed web canvas workspace. ADR-0013 PTY-direct.',
      tone: 'info',
    });
  }
</script>

<Dropdown placement="bottom-start">
  {#snippet trigger({ toggle })}
    <IconButton aria-label="Session menu" size="sm" onclick={toggle}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <line x1="3" y1="6" x2="21" y2="6"/>
        <line x1="3" y1="12" x2="21" y2="12"/>
        <line x1="3" y1="18" x2="21" y2="18"/>
      </svg>
    </IconButton>
  {/snippet}
  {#snippet menu({ close })}
    <button
      type="button"
      class="danger"
      onclick={() => {
        shutdownOpen = true;
        close();
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M18.36 6.64a9 9 0 1 1-12.73 0"/>
        <line x1="12" y1="2" x2="12" y2="12"/>
      </svg>
      <span>Session shutdown</span>
    </button>
    <button
      type="button"
      onclick={() => {
        onRotateToken();
        close();
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <polyline points="23 4 23 10 17 10"/>
        <polyline points="1 20 1 14 7 14"/>
        <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/>
      </svg>
      <span>Rotate token</span>
    </button>
    <button
      type="button"
      onclick={() => {
        onAbout();
        close();
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="10"/>
        <line x1="12" y1="16" x2="12" y2="12"/>
        <line x1="12" y1="8" x2="12.01" y2="8"/>
      </svg>
      <span>About</span>
    </button>
  {/snippet}
</Dropdown>

<ShutdownModal open={shutdownOpen} {sessionName} onclose={() => (shutdownOpen = false)} />
