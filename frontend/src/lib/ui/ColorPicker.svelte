<script lang="ts">
  /**
   * ColorPicker — hex color 편집 + transparent + alpha 지원 (plan-0010 Task 3, ADR-0027 D3).
   *
   * 사용:
   *   <ColorPicker value={item.fill} oncommit={(hex) => applyFill(hex)} />
   *
   * 입력:
   *   - value: 현재 색 string (`#rrggbb`, `#rrggbbaa`, `transparent`, `var(--…)`,
   *     CSS named color 등). null/undefined 면 빈 swatch.
   *   - oncommit: 사용자가 색 변경 commit 시 호출. 출력은 normalize 된 hex
   *     (#rrggbb 또는 #rrggbbaa) 또는 'transparent'.
   *   - disabled: input 비활성.
   *   - mixed: multi-select 시 mixed value placeholder (ADR-0027 D3).
   *   - allowTransparent: transparent toggle 버튼 + 'transparent'/'none' 입력 허용.
   *   - allowAlpha: alpha 채널 input 노출. 출력이 #rrggbbaa 8-digit. allowAlpha 시
   *     allowTransparent 토글 hide (alpha=0 이 transparent 역할).
   */
  interface Props {
    value: string | null | undefined;
    oncommit: (value: string) => void;
    disabled?: boolean;
    mixed?: boolean;
    allowTransparent?: boolean;
    allowAlpha?: boolean;
  }

  const {
    value,
    oncommit,
    disabled = false,
    mixed = false,
    allowTransparent = false,
    allowAlpha = false,
  }: Props = $props();

  function isTransparentValue(s: string): boolean {
    const v = s.trim().toLowerCase();
    return v === 'transparent' || v === 'none' || v === '';
  }

  /** 6 또는 8자리 hex 정규화. 짧은 (#rgb / #rgba) 도 expand. */
  function normalizeHex(s: string): string | null {
    const v = s.trim().toLowerCase();
    if (/^#[0-9a-f]{6}$/.test(v)) return v;
    if (/^#[0-9a-f]{8}$/.test(v)) return v;
    if (/^#[0-9a-f]{3}$/.test(v)) {
      return `#${v[1]}${v[1]}${v[2]}${v[2]}${v[3]}${v[3]}`;
    }
    if (/^#[0-9a-f]{4}$/.test(v)) {
      return `#${v[1]}${v[1]}${v[2]}${v[2]}${v[3]}${v[3]}${v[4]}${v[4]}`;
    }
    if (/^[0-9a-f]{6}$/.test(v)) return `#${v}`;
    if (/^[0-9a-f]{8}$/.test(v)) return `#${v}`;
    if (/^[0-9a-f]{3}$/.test(v)) {
      return `#${v[0]}${v[0]}${v[1]}${v[1]}${v[2]}${v[2]}`;
    }
    return null;
  }

  /** rgb()/rgba() string → hex. */
  function rgbStringToHex(rgb: string): string | null {
    const m = rgb.match(/^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+))?\s*\)$/);
    if (m === null) return null;
    const r = Number.parseInt(m[1] ?? '0', 10);
    const g = Number.parseInt(m[2] ?? '0', 10);
    const b = Number.parseInt(m[3] ?? '0', 10);
    const a = m[4] !== undefined ? Number.parseFloat(m[4]) : 1;
    const base =
      '#' +
      [r, g, b].map((n) => Math.max(0, Math.min(255, n)).toString(16).padStart(2, '0')).join('');
    if (a >= 0.999) return base;
    const ah = Math.max(0, Math.min(255, Math.round(a * 255))).toString(16).padStart(2, '0');
    return base + ah;
  }

  /** 모든 CSS 색 string → hex (#rrggbb 또는 #rrggbbaa). browser computedStyle 활용. */
  function resolveCssColor(s: string): string | null {
    if (typeof s !== 'string') return null;
    const v = s.trim();
    if (v.length === 0) return null;
    if (isTransparentValue(v)) return null;
    const direct = normalizeHex(v);
    if (direct !== null) return direct;
    if (typeof document === 'undefined') return null;
    // var(--name) 와 named color (rgb 변환) 둘 다 probe 로 처리.
    const probe = document.createElement('div');
    probe.style.color = '#000';
    probe.style.color = v;
    document.body.appendChild(probe);
    const computed = getComputedStyle(probe).color;
    document.body.removeChild(probe);
    if (computed.length === 0) return null;
    return rgbStringToHex(computed);
  }

  /** hex 의 alpha 채널 percent (0-100). 6-digit 이면 100. */
  function hexAlphaPercent(hex: string): number {
    if (!/^#[0-9a-f]{8}$/i.test(hex)) return 100;
    const ah = hex.slice(7, 9);
    const a = Number.parseInt(ah, 16);
    return Math.round((a / 255) * 100);
  }

  /** rgb 부분만 (#rrggbb) 반환. */
  function hexRgb(hex: string): string {
    if (/^#[0-9a-f]{8}$/i.test(hex)) return hex.slice(0, 7);
    return hex;
  }

  /** rgb + alpha percent → hex output (alpha 100 이면 6-digit, else 8-digit). */
  function combineHexAlpha(rgb: string, alphaPct: number): string {
    const base = normalizeHex(rgb) ?? '#000000';
    const baseRgb = hexRgb(base);
    if (alphaPct >= 100) return baseRgb;
    const a = Math.max(0, Math.min(255, Math.round((alphaPct / 100) * 255)));
    return `${baseRgb}${a.toString(16).padStart(2, '0')}`;
  }

  // 현재 값의 hex 표현 (var 등 resolve). transparent 면 null.
  const resolvedHex = $derived.by(() => {
    if (mixed) return null;
    if (typeof value !== 'string') return null;
    return resolveCssColor(value);
  });

  const isTransparent = $derived(typeof value === 'string' && isTransparentValue(value));

  // text input 표시값 — resolvedHex 또는 빈 string (transparent / mixed / 없음).
  const displayValue = $derived.by(() => {
    if (mixed || isTransparent) return '';
    return resolvedHex ?? '';
  });

  // swatch native input 의 value (반드시 #rrggbb). transparent 면 검정 fallback.
  const swatchValue = $derived.by(() => {
    if (resolvedHex === null) return '#000000';
    return hexRgb(resolvedHex);
  });

  // alpha percent (UI). resolvedHex 가 8-digit 일 때 alpha 추출, 아니면 100.
  // transparent 면 0.
  const alphaPercent = $derived.by(() => {
    if (isTransparent) return 0;
    if (resolvedHex === null) return 100;
    return hexAlphaPercent(resolvedHex);
  });

  let textInput = $state('');
  let alphaInput = $state('100');
  let editing = $state(false);
  let editingAlpha = $state(false);

  $effect(() => {
    if (!editing) textInput = displayValue;
  });
  $effect(() => {
    if (!editingAlpha) alphaInput = String(alphaPercent);
  });

  function commitColor(next: string): void {
    if (next === value) return;
    oncommit(next);
  }

  function onSwatchInput(e: Event): void {
    const t = e.currentTarget as HTMLInputElement;
    const next = t.value.toLowerCase();
    // native picker 는 #rrggbb. allowAlpha 면 현재 alpha 유지.
    if (allowAlpha) {
      const a = Math.max(0, Math.min(100, Number.parseInt(alphaInput || '100', 10)));
      commitColor(combineHexAlpha(next, Number.isNaN(a) ? 100 : a));
    } else {
      commitColor(next);
    }
  }

  function onTextChange(): void {
    const trimmed = textInput.trim().toLowerCase();
    if (allowTransparent && !allowAlpha && isTransparentValue(trimmed)) {
      if (value !== 'transparent') oncommit('transparent');
      textInput = '';
      return;
    }
    const norm = normalizeHex(textInput);
    if (norm === null) {
      // 잘못된 hex — revert.
      textInput = displayValue;
      return;
    }
    if (allowAlpha) {
      // 8-digit 이면 alpha 채택, 6-digit 이면 현재 alphaInput 결합.
      if (/^#[0-9a-f]{8}$/.test(norm)) {
        commitColor(norm);
      } else {
        const a = Math.max(0, Math.min(100, Number.parseInt(alphaInput || '100', 10)));
        commitColor(combineHexAlpha(norm, Number.isNaN(a) ? 100 : a));
      }
    } else {
      commitColor(norm);
    }
  }

  function onAlphaChange(): void {
    const n = Number.parseInt(alphaInput, 10);
    const a = Number.isNaN(n) ? 100 : Math.max(0, Math.min(100, n));
    alphaInput = String(a);
    const rgb = resolvedHex !== null ? hexRgb(resolvedHex) : '#000000';
    commitColor(combineHexAlpha(rgb, a));
  }

  function toggleTransparent(): void {
    if (disabled) return;
    if (isTransparent) {
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
      class:checker={isTransparent || (allowAlpha && alphaPercent < 100)}
      style:background={mixed || isTransparent ? undefined : (allowAlpha && resolvedHex !== null && alphaPercent < 100 ? resolvedHex : swatchValue)}
    >
      {#if mixed}
        <svg width="26" height="26" viewBox="0 0 26 26" aria-hidden="true">
          <line x1="3" y1="23" x2="23" y2="3" stroke="var(--color-fg-subtle)" stroke-width="1" />
        </svg>
      {:else if isTransparent}
        <svg width="26" height="26" viewBox="0 0 26 26" aria-hidden="true">
          <line x1="3" y1="23" x2="23" y2="3" stroke="var(--color-danger)" stroke-width="1.2" />
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
    placeholder={mixed ? 'Mixed' : isTransparent ? 'transparent' : '#000000'}
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
  {#if allowAlpha}
    <input
      type="number"
      min="0"
      max="100"
      step="1"
      class="alpha-input mono"
      bind:value={alphaInput}
      placeholder="100"
      title="Alpha (0–100%)"
      aria-label="Alpha percent"
      {disabled}
      onfocus={() => (editingAlpha = true)}
      onblur={() => {
        editingAlpha = false;
        onAlphaChange();
      }}
      onkeydown={(e) => {
        if (e.key === 'Enter') (e.currentTarget as HTMLInputElement).blur();
      }}
    />
    <span class="alpha-suffix" aria-hidden="true">%</span>
  {/if}
  {#if allowTransparent && !allowAlpha}
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
    height: 28px;
  }

  .color-picker.disabled {
    opacity: 0.55;
  }

  .swatch-wrap {
    position: relative;
    width: 26px;
    height: 26px;
    display: inline-block;
    cursor: pointer;
  }

  .swatch {
    display: block;
    width: 26px;
    height: 26px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--color-border);
    background-clip: padding-box;
  }

  .color-picker.mixed .swatch {
    background: var(--color-surface-2);
  }

  /* transparent / 부분 alpha — checkerboard 배경. fg(swatch) 가 alpha 색이면
     checker 위로 색이 합성되어 alpha 가 직관적으로 보임. */
  .swatch.checker {
    background-image:
      conic-gradient(
        var(--color-surface-2) 0 25%,
        var(--color-surface) 0 50%,
        var(--color-surface-2) 0 75%,
        var(--color-surface) 0 100%
      );
    background-size: 8px 8px;
    background-color: transparent;
  }

  .native-picker {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
    width: 26px;
    height: 26px;
    padding: 0;
    border: 0;
    background: transparent;
  }

  .hex-input {
    flex: 1 1 auto;
    min-width: 0;
    width: 76px;
    height: 28px;
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

  .hex-input:focus,
  .alpha-input:focus {
    outline: none;
    border-color: var(--color-border-strong);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-accent) 20%, transparent);
  }

  .hex-input::placeholder,
  .alpha-input::placeholder {
    color: var(--color-fg-subtle);
    font-style: italic;
  }

  .alpha-input {
    width: 44px;
    height: 28px;
    padding: 0 4px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0;
    -moz-appearance: textfield;
    appearance: textfield;
  }
  .alpha-input::-webkit-outer-spin-button,
  .alpha-input::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }

  .alpha-suffix {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--color-fg-subtle);
    margin-left: -2px;
  }

  .transparent-btn {
    width: 26px;
    height: 26px;
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
