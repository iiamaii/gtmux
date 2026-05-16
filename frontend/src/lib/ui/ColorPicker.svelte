<script lang="ts">
  /**
   * ColorPicker — hex color 편집 + transparent 지원 (plan-0010 Task 3, ADR-0027 D3).
   *
   * 사용:
   *   <ColorPicker value={item.fill} oncommit={(hex) => applyFill(hex)} />
   *
   * 입력:
   *   - value: 현재 hex string (`#rrggbb` 또는 `#rgb`) 또는 `'transparent'`. null/undefined 면 빈 swatch.
   *   - oncommit: 사용자가 색 변경 commit 시 호출 (input change / blur / transparent toggle).
   *   - disabled: input 비활성.
   *   - mixed: multi-select 시 mixed value placeholder 표시 (ADR-0027 D3).
   *   - allowTransparent: transparent toggle 버튼 노출. text input 도 'transparent'/'none' 입력 허용.
   *
   * 구성:
   *   - 좌: 24×22 swatch (native color picker 트리거 — transparent 시 checker 패턴)
   *   - 중: hex text input ('transparent' 도 허용)
   *   - 우 (allowTransparent 시): T 토글 버튼
   */
  interface Props {
    value: string | null | undefined;
    oncommit: (value: string) => void;
    disabled?: boolean;
    mixed?: boolean;
    allowTransparent?: boolean;
  }

  const {
    value,
    oncommit,
    disabled = false,
    mixed = false,
    allowTransparent = false,
  }: Props = $props();

  const isTransparent = $derived(typeof value === 'string' && isTransparentValue(value));

  function isTransparentValue(s: string): boolean {
    const v = s.trim().toLowerCase();
    return v === 'transparent' || v === 'none' || v === '';
  }

  // 입력 표시 — mixed 시 빈 placeholder. transparent 시 'transparent' 텍스트.
  const displayValue = $derived.by(() => {
    if (mixed) return '';
    if (typeof value !== 'string' || value.length === 0) return '';
    if (isTransparentValue(value)) return 'transparent';
    return normalizeHex(value) ?? value;
  });

  // swatch native input 의 value — 반드시 #rrggbb. transparent 면 black placeholder.
  const swatchValue = $derived.by(() => {
    if (mixed || isTransparent) return '#000000';
    if (typeof value !== 'string') return '#000000';
    const norm = normalizeHex(value);
    return norm ?? '#000000';
  });

  let textInput = $state('');
  let editing = $state(false);

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
    const trimmed = textInput.trim().toLowerCase();
    if (allowTransparent && isTransparentValue(trimmed)) {
      if (value !== 'transparent') oncommit('transparent');
      textInput = 'transparent';
      return;
    }
    const norm = normalizeHex(textInput);
    if (norm === null) {
      textInput = displayValue;
      return;
    }
    if (norm !== value) oncommit(norm);
  }

  function toggleTransparent(): void {
    if (disabled) return;
    if (isTransparent) {
      // transparent → 기본 hex (검정). 사용자가 picker / text 로 수정.
      oncommit('#000000');
    } else {
      oncommit('transparent');
    }
  }
</script>

<div class="color-picker" class:disabled class:mixed class:transparent={isTransparent}>
  <label class="swatch-wrap">
    <span
      class="swatch"
      class:checker={isTransparent}
      style:background={mixed || isTransparent ? undefined : swatchValue}
    >
      {#if mixed}
        <svg width="24" height="22" viewBox="0 0 24 22" aria-hidden="true">
          <line x1="2" y1="20" x2="22" y2="2" stroke="var(--color-fg-subtle)" stroke-width="1" />
        </svg>
      {:else if isTransparent}
        <svg width="24" height="22" viewBox="0 0 24 22" aria-hidden="true">
          <line x1="2" y1="20" x2="22" y2="2" stroke="var(--color-danger)" stroke-width="1.2" />
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
  {#if allowTransparent}
    <button
      type="button"
      class="transparent-btn"
      class:active={isTransparent}
      title={isTransparent ? 'Disable transparent' : 'Make transparent'}
      aria-label={isTransparent ? 'Disable transparent' : 'Make transparent'}
      aria-pressed={isTransparent}
      {disabled}
      onclick={toggleTransparent}
    >
      <!-- diagonal-line / "no" glyph -->
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" aria-hidden="true">
        <circle cx="8" cy="8" r="6"/>
        <line x1="3.5" y1="12.5" x2="12.5" y2="3.5"/>
      </svg>
    </button>
  {/if}
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

  /* transparent — checkerboard. CSS-only conic-gradient pattern. */
  .swatch.checker {
    background:
      conic-gradient(
        var(--color-surface-2) 0 25%,
        var(--color-surface) 0 50%,
        var(--color-surface-2) 0 75%,
        var(--color-surface) 0 100%
      ) 0 0 / 8px 8px;
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

  .transparent-btn {
    width: 22px;
    height: 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing);
  }
  .transparent-btn:hover:not(:disabled) {
    background: var(--color-glass-1);
    color: var(--color-fg);
    border-color: var(--color-border-strong);
  }
  .transparent-btn.active {
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    border-color: var(--color-accent);
    color: var(--color-accent);
  }
  .transparent-btn:disabled {
    cursor: not-allowed;
    opacity: 0.45;
  }
</style>
