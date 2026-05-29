<script lang="ts">
  // ADR-0027 D3 — Inspector 의 mixed-aware always-input field (text | number).
  //
  // 디자인: ColorPicker 의 inline-hex / inline-alpha 와 *동일한 box style* 로
  // 통일 (2026-05-17). h22 / bg = var(--color-bg) / 1px var(--color-border) /
  // hover border-strong / focus border-accent / mono 11px / k-prefix label.
  // 옛 시안 (index-v2.html `.input` h28 / bg-2 / transparent border) supersede.
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
    disabled?: boolean;
    /** Apply valid edits on every keystroke so canvas changes preview immediately. */
    live?: boolean;
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
    disabled = false,
    live = false,
  }: Props = $props();

  let draft = $state('');
  let editing = $state(false);

  $effect(() => {
    if (!editing) draft = mixed ? '' : value;
  });

  function commit(): void {
    if (disabled) return;
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
    if (disabled) return;
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

  function oninput(e: Event): void {
    draft = (e.currentTarget as HTMLInputElement).value;
    if (!live || disabled) return;
    const trimmed = draft.trim();
    if (trimmed.length === 0) return;
    if (type === 'number' && !Number.isFinite(Number(trimmed))) return;
    oncommit(trimmed);
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
    {disabled}
    {oninput}
    {onfocus}
    {onblur}
    {onkeydown}
  />
</label>

<style>
  /* Inspector component box style — 디자인 규칙 (2026-05-21 사용자 정의):
     모든 component 24px 통일 + width full + label 내부 좌측. */
  .inspector-input {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    min-width: 0;
    height: 24px;
    padding: 0 6px;
    box-sizing: border-box;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.2px;
    color: var(--color-fg);
    cursor: text;
    transition: border-color var(--motion-fast) var(--motion-easing);
  }

  .inspector-input:hover {
    border-color: var(--color-border-strong);
  }

  .inspector-input.editing,
  .inspector-input:focus-within {
    border-color: var(--color-accent);
  }

  /* Fixed-width — display-row 의 .k 와 동일 (56px). 2-column geometry rows 는
   * parent 가 --inspector-k-w 를 줄여 numeric value 영역을 확보한다. */
  .inspector-input .k {
    flex: 0 0 var(--inspector-k-w, 56px);
    width: var(--inspector-k-w, 56px);
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
    box-sizing: border-box;
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

  .field:disabled {
    opacity: 0.45;
    cursor: not-allowed;
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
    font-variant-numeric: tabular-nums;
  }
</style>
