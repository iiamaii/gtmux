<script lang="ts">
  /**
   * Text input primitive (ADR-0016 §D4).
   *
   * Pattern:
   *   <Input bind:value label="Panel label" placeholder="My panel" />
   *
   * For Group label edit, future panel rename, CommandPalette search.
   */

  interface Props {
    value?: string;
    label?: string;
    placeholder?: string;
    type?: 'text' | 'search' | 'password';
    disabled?: boolean;
    autofocus?: boolean;
    error?: string | null;
    'aria-label'?: string;
    oninput?: (event: Event) => void;
    onkeydown?: (event: KeyboardEvent) => void;
  }

  let {
    value = $bindable(''),
    label,
    placeholder,
    type = 'text',
    disabled = false,
    autofocus = false,
    error = null,
    'aria-label': ariaLabel,
    oninput,
    onkeydown,
  }: Props = $props();

  const inputId = `input-${Math.random().toString(36).slice(2, 10)}`;
</script>

<label class="input-field" class:has-error={error !== null}>
  {#if label}
    <span class="input-label">{label}</span>
  {/if}
  <!-- svelte-ignore a11y_autofocus — opt-in for CommandPalette etc. -->
  <input
    id={inputId}
    {type}
    bind:value
    {placeholder}
    {disabled}
    aria-label={ariaLabel ?? label}
    aria-invalid={error !== null}
    {oninput}
    {onkeydown}
    {autofocus}
    class="input-control"
  />
  {#if error}
    <span class="input-error" role="alert">{error}</span>
  {/if}
</label>

<style>
  .input-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-16);
  }

  .input-label {
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
    font-weight: var(--weight-medium);
  }

  .input-control {
    height: 32px;
    padding: 0 var(--space-12);
    background: var(--color-surface);
    color: var(--color-fg);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    font-family: inherit;
    font-size: var(--text-base);
    transition: border-color var(--motion-fast) var(--motion-easing);
  }

  .input-control:hover:not(:disabled) {
    border-color: var(--color-fg-subtle);
  }

  .input-control:focus-visible {
    outline: 2px solid var(--color-info);
    outline-offset: 1px;
    border-color: var(--color-info);
  }

  .input-control:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .input-field.has-error .input-control {
    border-color: var(--color-danger);
  }

  .input-error {
    font-size: var(--text-sm);
    color: var(--color-danger);
  }
</style>
