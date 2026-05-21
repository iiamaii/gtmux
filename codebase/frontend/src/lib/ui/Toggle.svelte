<script lang="ts">
  interface Props {
    checked?: boolean;
    mixed?: boolean;
    disabled?: boolean;
    ariaLabel: string;
    onchange?: (next: boolean) => void;
  }

  const {
    checked = false,
    mixed = false,
    disabled = false,
    ariaLabel,
    onchange,
  }: Props = $props();

  function onclick(): void {
    if (disabled) return;
    onchange?.(mixed ? true : !checked);
  }
</script>

<button
  type="button"
  class="toggle"
  class:on={checked && !mixed}
  class:mixed
  {disabled}
  role="switch"
  aria-checked={mixed ? 'mixed' : checked}
  aria-label={ariaLabel}
  {onclick}
>
  <span class="knob" aria-hidden="true"></span>
</button>

<style>
  .toggle {
    position: relative;
    width: 28px;
    height: 16px;
    flex: 0 0 28px;
    padding: 0;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-pill);
    background: var(--color-surface-2);
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing),
      opacity var(--motion-fast) var(--motion-easing);
  }

  .knob {
    position: absolute;
    top: 1px;
    left: 1px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--color-fg-muted);
    transition:
      transform 120ms var(--motion-easing),
      background var(--motion-fast) var(--motion-easing);
  }

  .toggle.on {
    background: var(--color-accent);
    border-color: var(--color-accent);
  }

  .toggle.on .knob {
    background: var(--color-accent-fg);
    transform: translateX(12px);
  }

  .toggle.mixed .knob {
    background:
      repeating-linear-gradient(
        45deg,
        var(--color-fg-muted) 0 2px,
        var(--color-fg-subtle) 2px 4px
      );
    transform: translateX(6px);
  }

  .toggle:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .toggle:focus-visible {
    outline: none;
  }
</style>
