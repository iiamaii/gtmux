<script lang="ts">
  // ADR-0027 D3 — Inspector 의 mixed-aware always-input field (text | number).
  //
  // 시안: ref/frontend-design/index-v2.html §Right panel `.input` 정합 —
  // height 28px / surface-2 bg / mono 11px / k-prefix label / hover glass.
  // wrapper 가 surface 를 그리고 native input 은 transparent.
  //
  // Commit semantic (ADR-0027 D3 broadcast):
  // - blur 또는 Enter → oncommit
  // - Esc → 취소 (draft restore)
  // - 빈 string commit 은 무시 (parent 의 reset 책임).

  interface Props {
    value: string;
    mixed: boolean;
    placeholder?: string;
    oncommit: (next: string) => void;
    type?: 'text' | 'number';
    /** number type 의 step (default 1). */
    step?: number;
    /** Optional prefix label inside the field (e.g. "X", "W"). */
    k?: string;
    ariaLabel?: string;
  }

  const {
    value,
    mixed,
    placeholder = '',
    oncommit,
    type = 'text',
    step = 1,
    k,
    ariaLabel,
  }: Props = $props();

  let draft = $state('');
  let editing = $state(false);

  $effect(() => {
    if (!editing) draft = mixed ? '' : value;
  });

  function commit(): void {
    editing = false;
    const trimmed = draft.trim();
    if (trimmed.length === 0) {
      draft = mixed ? '' : value;
      return;
    }
    if (type === 'number' && !Number.isFinite(Number(trimmed))) {
      draft = mixed ? '' : value;
      return;
    }
    oncommit(trimmed);
  }

  function cancel(el: HTMLInputElement): void {
    draft = mixed ? '' : value;
    editing = false;
    el.blur();
  }

  function onfocus(): void {
    editing = true;
  }

  function onblur(): void {
    if (editing) commit();
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.isComposing) return;
    if (e.key === 'Enter') {
      e.preventDefault();
      (e.currentTarget as HTMLInputElement).blur();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancel(e.currentTarget as HTMLInputElement);
    }
  }
</script>

<label class="inspector-input" class:mixed class:editing>
  {#if k !== undefined}
    <span class="k" aria-hidden="true">{k}</span>
  {/if}
  <input
    type={type === 'number' ? 'number' : 'text'}
    inputmode={type === 'number' ? 'numeric' : undefined}
    {step}
    class="field"
    value={draft}
    placeholder={mixed ? 'Mixed' : placeholder}
    aria-label={ariaLabel ?? k}
    oninput={(e) => (draft = (e.currentTarget as HTMLInputElement).value)}
    {onfocus}
    {onblur}
    {onkeydown}
  />
</label>

<style>
  /* ref/frontend-design/index-v2.html `.input` 정합. */
  .inspector-input {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    min-width: 0;
    height: 28px;
    padding: 0 8px;
    box-sizing: border-box;
    background: var(--color-surface-2);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.2px;
    color: var(--color-fg);
    cursor: text;
    transition:
      background var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing);
  }

  .inspector-input:hover {
    background: var(--color-glass-1);
  }

  .inspector-input.editing,
  .inspector-input:focus-within {
    background: var(--color-surface);
    border-color: var(--color-accent);
  }

  .inspector-input .k {
    flex: 0 0 auto;
    color: var(--color-fg-muted);
    text-transform: uppercase;
    font-size: 10px;
    letter-spacing: 0.4px;
    pointer-events: none;
  }

  .field {
    flex: 1 1 auto;
    min-width: 0;
    width: 100%;
    height: 100%;
    padding: 0;
    margin: 0;
    background: transparent;
    border: 0;
    outline: 0;
    color: inherit;
    font: inherit;
    letter-spacing: inherit;
  }

  .field::placeholder {
    color: var(--color-fg-subtle);
  }

  .inspector-input.mixed .field {
    color: var(--color-fg-subtle);
    font-style: italic;
  }

  .inspector-input.mixed .field::placeholder {
    color: var(--color-fg-subtle);
    font-style: italic;
  }

  /* Strip native number arrows — Figma 패턴. */
  .field[type='number']::-webkit-inner-spin-button,
  .field[type='number']::-webkit-outer-spin-button {
    -webkit-appearance: none;
    appearance: none;
    margin: 0;
  }

  .field[type='number'] {
    -moz-appearance: textfield;
    appearance: textfield;
  }
</style>
