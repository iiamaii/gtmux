<script lang="ts">
  /**
   * ColorPicker — hex color 편집 (plan-0010 Task 3).
   *
   * 사용:
   *   <ColorPicker value={item.fill} oncommit={(hex) => applyFill(hex)} />
   *
   * 입력:
   *   - value: 현재 hex string (`#rrggbb` 또는 `#rgb`). null/undefined 면 빈 swatch.
   *   - oncommit: 사용자가 색 변경 commit 시 호출 (input change / blur).
   *   - disabled: input 비활성.
   *   - mixed: multi-select 시 mixed value placeholder 표시 (ADR-0027 D3).
   *
   * 구성:
   *   - 좌: 24×22 swatch (native color picker 트리거)
   *   - 우: hex text input (수동 입력)
   *
   * Spec:
   *   - native `<input type="color">` 는 항상 hex 7글자 (`#rrggbb`) 반환
   *   - text input 의 hex 정상화 (`#fff` → `#ffffff`)
   *   - 잘못된 hex 는 commit 안 함 (input UI revert)
   */
  interface Props {
    value: string | null | undefined;
    oncommit: (hex: string) => void;
    disabled?: boolean;
    mixed?: boolean;
  }

  const { value, oncommit, disabled = false, mixed = false }: Props = $props();

  // 입력 표시 — mixed 시 '' (placeholder), value null 시 '' , 그 외 normalize.
  const displayValue = $derived.by(() => {
    if (mixed) return '';
    if (typeof value !== 'string' || value.length === 0) return '';
    return normalizeHex(value) ?? value;
  });

  // swatch 색 — picker 의 native value (반드시 #rrggbb 7글자).
  const swatchValue = $derived.by(() => {
    if (mixed) return '#cccccc';
    if (typeof value !== 'string') return '#000000';
    const norm = normalizeHex(value);
    return norm ?? '#000000';
  });

  let textInput = $state('');
  let editing = $state(false);

  // displayValue 가 외부에서 변하면 input 갱신 — 단 사용자가 *편집 중* 이면 덮어쓰지 않음.
  $effect(() => {
    if (!editing) textInput = displayValue;
  });

  function normalizeHex(s: string): string | null {
    const v = s.trim().toLowerCase();
    if (/^#[0-9a-f]{6}$/.test(v)) return v;
    if (/^#[0-9a-f]{3}$/.test(v)) {
      return `#${v[1]}${v[1]}${v[2]}${v[2]}${v[3]}${v[3]}`;
    }
    if (/^[0-9a-f]{6}$/.test(v)) return `#${v}`;
    if (/^[0-9a-f]{3}$/.test(v)) {
      return `#${v[0]}${v[0]}${v[1]}${v[1]}${v[2]}${v[2]}`;
    }
    return null;
  }

  function onSwatchInput(e: Event): void {
    const t = e.currentTarget as HTMLInputElement;
    const next = t.value.toLowerCase();
    if (next !== value) oncommit(next);
  }

  function onTextChange(): void {
    const norm = normalizeHex(textInput);
    if (norm === null) {
      // 잘못된 hex — revert.
      textInput = displayValue;
      return;
    }
    if (norm !== value) oncommit(norm);
  }
</script>

<div class="color-picker" class:disabled class:mixed>
  <label class="swatch-wrap">
    <span class="swatch" style:background={mixed ? 'transparent' : swatchValue}>
      {#if mixed}
        <svg width="24" height="22" viewBox="0 0 24 22" aria-hidden="true">
          <line x1="2" y1="20" x2="22" y2="2" stroke="var(--color-fg-subtle)" stroke-width="1" />
        </svg>
      {/if}
    </span>
    <input
      type="color"
      class="native-picker"
      value={swatchValue}
      {disabled}
      oninput={onSwatchInput}
      aria-label="Pick color"
    />
  </label>
  <input
    type="text"
    class="hex-input mono"
    bind:value={textInput}
    placeholder={mixed ? 'Mixed' : '#000000'}
    {disabled}
    onfocus={() => (editing = true)}
    onblur={() => {
      editing = false;
      onTextChange();
    }}
    onkeydown={(e) => {
      if (e.key === 'Enter') (e.currentTarget as HTMLInputElement).blur();
    }}
    spellcheck="false"
    autocomplete="off"
  />
</div>

<style>
  .color-picker {
    display: inline-flex;
    align-items: center;
    gap: var(--space-4);
    height: 22px;
  }

  .color-picker.disabled {
    opacity: 0.55;
  }

  .swatch-wrap {
    position: relative;
    width: 24px;
    height: 22px;
    display: inline-block;
    cursor: pointer;
  }

  .swatch {
    display: block;
    width: 24px;
    height: 22px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--color-border);
  }

  .color-picker.mixed .swatch {
    background: var(--color-surface-2);
  }

  .native-picker {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
    width: 24px;
    height: 22px;
    padding: 0;
    border: 0;
    background: transparent;
  }

  .hex-input {
    width: 76px;
    height: 22px;
    padding: 0 6px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0;
    text-transform: lowercase;
  }

  .hex-input:focus {
    outline: none;
    border-color: var(--color-border-strong);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-accent) 20%, transparent);
  }

  .hex-input::placeholder {
    color: var(--color-fg-subtle);
    font-style: italic;
  }
</style>
