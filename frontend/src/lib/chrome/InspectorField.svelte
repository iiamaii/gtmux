<script lang="ts">
  // ADR-0027 D3 — Inspector 의 mixed-aware always-input field (text | number).
  //
  // Figma 패턴: input 이 항상 visible. 외부 value 변경은 editing 중이 아닐
  // 때만 draft 동기화 (typing 도중 reset 방지). mixed === true 일 때 draft
  // 를 비우고 placeholder 에 "Mixed".
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
    class?: string;
    ariaLabel?: string;
  }

  const {
    value,
    mixed,
    placeholder = '',
    oncommit,
    type = 'text',
    step = 1,
    class: extraClass = '',
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

<input
  type={type === 'number' ? 'number' : 'text'}
  inputmode={type === 'number' ? 'numeric' : undefined}
  {step}
  class="inspector-field {extraClass}"
  class:mixed
  value={draft}
  placeholder={mixed ? 'Mixed' : placeholder}
  aria-label={ariaLabel}
  oninput={(e) => (draft = (e.currentTarget as HTMLInputElement).value)}
  {onfocus}
  {onblur}
  {onkeydown}
/>

<style>
  .inspector-field {
    width: 100%;
    min-width: 0;
    height: 22px;
    padding: 0 6px;
    box-sizing: border-box;
    background: transparent;
    color: var(--color-fg);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: var(--text-base);
    line-height: 1.2;
    transition:
      background var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing);
  }

  .inspector-field:hover {
    background: var(--color-glass-1);
  }

  .inspector-field:focus-visible,
  .inspector-field:focus {
    outline: none;
    background: var(--color-surface);
    border-color: var(--color-accent);
  }

  .inspector-field.mixed {
    color: var(--color-fg-subtle);
    font-style: italic;
  }

  .inspector-field.mixed::placeholder {
    color: var(--color-fg-subtle);
    font-style: italic;
  }

  /* Strip native number arrows — Figma 패턴. */
  .inspector-field[type='number']::-webkit-inner-spin-button,
  .inspector-field[type='number']::-webkit-outer-spin-button {
    -webkit-appearance: none;
    appearance: none;
    margin: 0;
  }

  .inspector-field[type='number'] {
    -moz-appearance: textfield;
    appearance: textfield;
  }
</style>
