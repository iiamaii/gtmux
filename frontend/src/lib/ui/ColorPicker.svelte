<script lang="ts">
  /**
   * ColorPicker — Figma-style popover (ref/frontend-design/components-v4.html
   * §.shape-colorpicker spec 정합).
   *
   * Phase 1 (현재):
   *   - Visual shell — trigger swatch + popover 의 head/modes/sv-square/sliders/
   *     value/swatches 의 markup + CSS. 시안 v4 정합.
   *   - Working interaction: hex input commit / alpha input commit / swatch grid
   *     클릭 commit / close button / click-outside close.
   *   - Static visual only (drag 없음): SV-square handle (hex → HSV 위치), hue
   *     slider handle (hue/360), alpha slider handle (alpha%), modes (Solid 고정),
   *     format dropdown (HEX 고정).
   *
   * Phase 2 (후속):
   *   - SV / hue / alpha pointer-down → drag → color update
   *   - Format toggle (Hex / RGB / HSL cycle)
   *   - Eyedropper (EyeDropper Web API)
   *   - Recent swatch history (clipboard 또는 sessionStore)
   *
   * 입력 (props):
   *   - value: string | null | undefined  — 현재 색 (#rrggbb / #rrggbbaa /
   *     'transparent' / var(--…) / named).
   *   - oncommit: (string) => void — 변경 commit. 출력은 hex normalize.
   *   - disabled / mixed / allowTransparent / allowAlpha — 기존 props 유지.
   *
   * 정본:
   *   - plan-0010 Task 3 (Q "OKLCH/HSL/hex" — 본 phase 는 hex 만)
   *   - ADR-0016 (design tokens) amend 후속 (token-aware preset palette 정책)
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

  // ── 기존 helpers (변경 없음) ─────────────────────────────────────────
  function isTransparentValue(s: string): boolean {
    const v = s.trim().toLowerCase();
    return v === 'transparent' || v === 'none' || v === '';
  }

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

  function resolveCssColor(s: string): string | null {
    if (typeof s !== 'string') return null;
    const v = s.trim();
    if (v.length === 0) return null;
    if (isTransparentValue(v)) return null;
    const direct = normalizeHex(v);
    if (direct !== null) return direct;
    if (typeof document === 'undefined') return null;
    const probe = document.createElement('div');
    probe.style.color = '#000';
    probe.style.color = v;
    document.body.appendChild(probe);
    const computed = getComputedStyle(probe).color;
    document.body.removeChild(probe);
    if (computed.length === 0) return null;
    return rgbStringToHex(computed);
  }

  function hexAlphaPercent(hex: string): number {
    if (!/^#[0-9a-f]{8}$/i.test(hex)) return 100;
    const ah = hex.slice(7, 9);
    const a = Number.parseInt(ah, 16);
    return Math.round((a / 255) * 100);
  }

  function hexRgb(hex: string): string {
    if (/^#[0-9a-f]{8}$/i.test(hex)) return hex.slice(0, 7);
    return hex;
  }

  function combineHexAlpha(rgb: string, alphaPct: number): string {
    const base = normalizeHex(rgb) ?? '#000000';
    const baseRgb = hexRgb(base);
    if (alphaPct >= 100) return baseRgb;
    const a = Math.max(0, Math.min(255, Math.round((alphaPct / 100) * 255)));
    return `${baseRgb}${a.toString(16).padStart(2, '0')}`;
  }

  // ── 신규: hex ↔ HSV 변환 (SV handle / hue handle 위치 계산용) ────────
  function hexToHsv(hex: string): { h: number; s: number; v: number } {
    const m = hex.match(/^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i);
    if (m === null) return { h: 0, s: 0, v: 0 };
    const r = Number.parseInt(m[1] ?? '0', 16) / 255;
    const g = Number.parseInt(m[2] ?? '0', 16) / 255;
    const b = Number.parseInt(m[3] ?? '0', 16) / 255;
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const d = max - min;
    let h = 0;
    if (d !== 0) {
      if (max === r) h = ((g - b) / d) % 6;
      else if (max === g) h = (b - r) / d + 2;
      else h = (r - g) / d + 4;
    }
    h = Math.round(h * 60);
    if (h < 0) h += 360;
    const s = max === 0 ? 0 : (d / max) * 100;
    const v = max * 100;
    return { h, s, v };
  }

  /** Pure hue → hex (saturation=100, value=100). SV-square gradient base. */
  function hueToHex(h: number): string {
    const c = 1;
    const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
    let r = 0, g = 0, b = 0;
    if (h < 60) { r = c; g = x; }
    else if (h < 120) { r = x; g = c; }
    else if (h < 180) { g = c; b = x; }
    else if (h < 240) { g = x; b = c; }
    else if (h < 300) { r = x; b = c; }
    else { r = c; b = x; }
    return '#' + [r, g, b].map((v2) => Math.round(v2 * 255).toString(16).padStart(2, '0')).join('');
  }

  // ── 파생값 ────────────────────────────────────────────────────────
  const resolvedHex = $derived.by(() => {
    if (mixed) return null;
    if (typeof value !== 'string') return null;
    return resolveCssColor(value);
  });

  const isTransparent = $derived(typeof value === 'string' && isTransparentValue(value));

  const displayValue = $derived.by(() => {
    if (mixed || isTransparent) return '';
    return resolvedHex ?? '';
  });

  const swatchValue = $derived.by(() => {
    if (resolvedHex === null) return '#000000';
    return hexRgb(resolvedHex);
  });

  const alphaPercent = $derived.by(() => {
    if (isTransparent) return 0;
    if (resolvedHex === null) return 100;
    return hexAlphaPercent(resolvedHex);
  });

  const hsv = $derived.by(() => {
    if (resolvedHex === null) return { h: 0, s: 0, v: 0 };
    return hexToHsv(hexRgb(resolvedHex));
  });

  const hueColor = $derived(hueToHex(hsv.h));

  // ── State ────────────────────────────────────────────────────────
  let open = $state(false);
  let textInput = $state('');
  let alphaInput = $state('100');
  let editing = $state(false);
  let editingAlpha = $state(false);
  let containerEl: HTMLDivElement | undefined = $state();

  $effect(() => {
    if (!editing) textInput = displayValue;
  });
  $effect(() => {
    if (!editingAlpha) alphaInput = String(alphaPercent);
  });

  // ── Click-outside close ─────────────────────────────────────────
  $effect(() => {
    if (!open) return;
    function onDocPointerDown(e: PointerEvent): void {
      if (containerEl === undefined) return;
      if (!containerEl.contains(e.target as Node)) {
        open = false;
      }
    }
    // 현재 click 의 propagation 끝나기 직전 attach (open=true 만든 click 자체가
    // outside-click 으로 잡히지 않도록 다음 tick).
    queueMicrotask(() => {
      document.addEventListener('pointerdown', onDocPointerDown, true);
    });
    return () => {
      document.removeEventListener('pointerdown', onDocPointerDown, true);
    };
  });

  // ── Commit paths ─────────────────────────────────────────────────
  function commitColor(next: string): void {
    if (next === value) return;
    oncommit(next);
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
      textInput = displayValue;
      return;
    }
    if (allowAlpha) {
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

  function onSwatchClick(hex: string): void {
    if (disabled) return;
    if (allowAlpha) {
      const a = Math.max(0, Math.min(100, Number.parseInt(alphaInput || '100', 10)));
      commitColor(combineHexAlpha(hex, Number.isNaN(a) ? 100 : a));
    } else {
      commitColor(hex);
    }
  }

  // ── Preset palette (phase 1 hardcoded; phase 4 token-aware) ──────
  const documentSwatches: readonly string[] = [
    '#0d99ff', '#1abc9c', '#2dc26b', '#ffc857', '#f97316',
    '#e5484d', '#b894ff', '#000000', '#6b6b6b',
  ];
  const recentSwatches: readonly string[] = []; // phase 2+ wire
</script>

<div
  class="color-picker"
  class:disabled
  class:mixed
  class:transparent={isTransparent}
  bind:this={containerEl}
>
  <button
    type="button"
    class="swatch-trigger"
    {disabled}
    aria-label="Open color picker"
    aria-expanded={open}
    onclick={(e) => {
      e.stopPropagation();
      if (!disabled) open = !open;
    }}
  >
    <span
      class="swatch"
      class:checker={isTransparent || (allowAlpha && alphaPercent < 100)}
      style:background={mixed || isTransparent ? undefined : (allowAlpha && resolvedHex !== null && alphaPercent < 100 ? resolvedHex : swatchValue)}
    >
      {#if mixed}
        <svg width="22" height="22" viewBox="0 0 22 22" aria-hidden="true">
          <line x1="2" y1="20" x2="20" y2="2" stroke="var(--color-fg-subtle)" stroke-width="1" />
        </svg>
      {:else if isTransparent}
        <svg width="22" height="22" viewBox="0 0 22 22" aria-hidden="true">
          <line x1="2" y1="20" x2="20" y2="2" stroke="var(--color-danger)" stroke-width="1.2" />
        </svg>
      {/if}
    </span>
  </button>

  {#if open}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="shape-colorpicker"
      style="--cp-hue: {hueColor}"
      role="dialog"
      aria-label="Color picker"
    >
      <!-- Head — title + close. (Pin button 은 phase 2+ — visible only) -->
      <div class="cp-head">
        <span class="cp-title">Color</span>
        <div class="cp-actions">
          <button
            type="button"
            class="cp-btn"
            title="Close"
            aria-label="Close color picker"
            onclick={() => (open = false)}
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
              <path d="M3 3l6 6M9 3l-6 6"/>
            </svg>
          </button>
        </div>
      </div>

      <!-- Modes — phase 1: Solid 고정, 나머지 disabled (P2). -->
      <div class="cp-modes">
        <button type="button" class="cp-mode active" title="Solid">
          <svg width="12" height="12" viewBox="0 0 12 12" fill="currentColor" aria-hidden="true"><circle cx="6" cy="6" r="4"/></svg>
        </button>
        <button type="button" class="cp-mode" title="Linear (P2)" disabled aria-disabled="true">
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><rect x="2" y="2" width="8" height="8" rx="1" fill="currentColor" opacity=".3"/></svg>
        </button>
        <button type="button" class="cp-mode" title="Radial (P2)" disabled aria-disabled="true">
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><circle cx="6" cy="6" r="4" fill="currentColor" opacity=".3"/></svg>
        </button>
        <button type="button" class="cp-mode" title="Angular (P2)" disabled aria-disabled="true">
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1" aria-hidden="true"><circle cx="6" cy="6" r="4"/></svg>
        </button>
        <button type="button" class="cp-mode" title="Image (P2)" disabled aria-disabled="true">
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1" aria-hidden="true"><rect x="1.5" y="2" width="9" height="8" rx="1"/></svg>
        </button>
      </div>

      <!-- SV square — phase 1: static handle, drag in phase 2. -->
      <div class="cp-sv">
        <div class="sv-handle" style:left="{hsv.s}%" style:top="{100 - hsv.v}%"></div>
      </div>

      <!-- Sliders — phase 1: static handles, drag in phase 2. -->
      <div class="cp-sliders">
        <button type="button" class="cp-eye" title="Eyedropper (P2)" disabled aria-disabled="true">
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M9 1.5l3.5 3.5-1.5 1.5L8 4z"/>
            <path d="M8 4 3.2 8.8a1.4 1.4 0 0 0 0 2L4 11.6l-.6.6a.7.7 0 0 1-1-1l.6-.6.8-.8L8.8 4.8"/>
          </svg>
        </button>
        <div class="cp-slider hue">
          <div class="sl-handle" style:left="{(hsv.h / 360) * 100}%"></div>
        </div>
        <div class="cp-slider alpha">
          <div class="sl-handle" style:left="{alphaPercent}%"></div>
        </div>
      </div>

      <!-- Value row — format pill + hex input + alpha input. -->
      <div class="cp-value">
        <div class="cp-input cp-format" title="Format (HEX only — P2)">
          HEX <span class="caret" aria-hidden="true">▾</span>
        </div>
        <div class="cp-input">
          <input
            type="text"
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
            aria-label="Color hex value"
          />
        </div>
        <div class="cp-input cp-alpha">
          {#if allowAlpha}
            <input
              type="number"
              min="0"
              max="100"
              step="1"
              bind:value={alphaInput}
              {disabled}
              onfocus={() => (editingAlpha = true)}
              onblur={() => {
                editingAlpha = false;
                onAlphaChange();
              }}
              onkeydown={(e) => {
                if (e.key === 'Enter') (e.currentTarget as HTMLInputElement).blur();
              }}
              aria-label="Alpha percent"
            />
          {:else}
            <span>{alphaPercent}</span>
          {/if}
          <span class="pct">%</span>
        </div>
      </div>

      <!-- Swatches — Document + Recent. phase 1: hardcoded. P2: token-aware. -->
      <div class="cp-swatches">
        <div class="grp">
          <div class="lbl">
            <span>Document</span>
          </div>
          <div class="grid">
            {#each documentSwatches as sw}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <span
                class="sw"
                class:selected={resolvedHex !== null && hexRgb(resolvedHex) === sw}
                style:--c={sw}
                role="button"
                tabindex="0"
                aria-label={`Set color ${sw}`}
                onclick={() => onSwatchClick(sw)}
              ></span>
            {/each}
            {#if allowTransparent && !allowAlpha}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <span
                class="sw transparent"
                class:selected={isTransparent}
                role="button"
                tabindex="0"
                aria-label="Transparent"
                onclick={toggleTransparent}
              ></span>
            {/if}
          </div>
        </div>
        {#if recentSwatches.length > 0}
          <div class="grp">
            <div class="lbl"><span>Recent</span></div>
            <div class="grid">
              {#each recentSwatches as sw}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <span
                  class="sw"
                  class:selected={resolvedHex !== null && hexRgb(resolvedHex) === sw}
                  style:--c={sw}
                  role="button"
                  tabindex="0"
                  aria-label={`Set color ${sw}`}
                  onclick={() => onSwatchClick(sw)}
                ></span>
              {/each}
            </div>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  /* ── Trigger swatch (inspector 안의 색 box) ─────────────────────── */
  .color-picker {
    position: relative;
    display: inline-flex;
    align-items: center;
    height: 22px;
  }
  .color-picker.disabled { opacity: 0.55; }

  .swatch-trigger {
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: pointer;
    display: block;
  }
  .swatch-trigger:focus-visible {
    outline: 1px dashed var(--color-accent);
    outline-offset: 2px;
  }

  .swatch {
    display: block;
    width: 22px;
    height: 22px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--color-border);
    background-clip: padding-box;
  }
  .color-picker.mixed .swatch { background: var(--color-surface-2); }
  .swatch.checker {
    background-image: conic-gradient(
      var(--color-surface-2) 0 25%,
      var(--color-surface) 0 50%,
      var(--color-surface-2) 0 75%,
      var(--color-surface) 0 100%
    );
    background-size: 8px 8px;
    background-color: transparent;
  }

  /* ── Popover (v4 §.shape-colorpicker spec) ───────────────────── */
  .shape-colorpicker {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: var(--z-popover, 100);
    width: 240px;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    color: var(--color-fg);
    font-family: var(--font-sans);
    user-select: none;
  }
  .shape-colorpicker .cp-sv { overflow: hidden; }

  .cp-head {
    display: flex;
    align-items: center;
    height: 32px;
    padding: 0 4px 0 12px;
    border-bottom: 1px solid var(--color-border);
  }
  .cp-title {
    font-size: 12px;
    font-weight: var(--weight-medium);
    letter-spacing: -0.1px;
  }
  .cp-actions {
    margin-left: auto;
    display: flex;
    gap: 0;
  }
  .cp-btn {
    width: 24px;
    height: 24px;
    border: none;
    background: transparent;
    border-radius: var(--radius-sm);
    display: grid;
    place-items: center;
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    transition: background var(--motion-fast) var(--motion-easing), color var(--motion-fast) var(--motion-easing);
  }
  .cp-btn:hover { background: var(--color-glass-2); color: var(--color-fg); }
  .cp-btn:focus-visible { outline: 1px dashed var(--color-accent); outline-offset: 1px; }

  .cp-modes {
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    gap: 2px;
    padding: 4px;
    background: var(--color-surface-2);
    border-bottom: 1px solid var(--color-border);
  }
  .cp-mode {
    height: 22px;
    display: grid;
    place-items: center;
    border: none;
    background: transparent;
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    transition: background var(--motion-fast) var(--motion-easing), color var(--motion-fast) var(--motion-easing);
  }
  .cp-mode:hover:not(:disabled) { color: var(--color-fg); }
  .cp-mode:disabled { cursor: not-allowed; opacity: 0.4; }
  .cp-mode.active {
    background: var(--color-surface);
    color: var(--color-fg);
    box-shadow: 0 1px 2px rgba(0,0,0,.08), 0 0 0 0.5px rgba(0,0,0,.06);
  }
  :global(:root.dark) .cp-mode.active {
    box-shadow: 0 1px 2px rgba(0,0,0,.5), 0 0 0 0.5px rgba(255,255,255,.08);
  }

  .cp-sv {
    position: relative;
    width: 100%;
    height: 156px;
    cursor: not-allowed; /* P2: crosshair + drag */
    background:
      linear-gradient(to top, #000 0%, transparent 100%),
      linear-gradient(to right, #fff 0%, var(--cp-hue, #0d99ff) 100%);
  }
  .sv-handle {
    position: absolute;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 1.5px solid #fff;
    box-shadow: 0 0 0 1px rgba(0,0,0,.55), 0 1px 2px rgba(0,0,0,.4);
    transform: translate(-50%, -50%);
    pointer-events: none;
  }

  .cp-sliders {
    padding: 12px 12px 0;
    display: grid;
    grid-template-columns: 22px 1fr;
    column-gap: 10px;
    row-gap: 10px;
    align-items: center;
  }
  .cp-eye {
    grid-row: 1 / span 2;
    width: 22px;
    height: 22px;
    border: none;
    background: transparent;
    border-radius: var(--radius-sm);
    display: grid;
    place-items: center;
    color: var(--color-fg);
    cursor: not-allowed;
    padding: 0;
    opacity: 0.4;
  }
  .cp-slider {
    position: relative;
    height: 10px;
    border-radius: 50px;
    cursor: not-allowed; /* P2: pointer */
  }
  .cp-slider.hue {
    background: linear-gradient(
      to right,
      #ff0000 0%, #ffff00 17%, #00ff00 33%, #00ffff 50%,
      #0000ff 67%, #ff00ff 83%, #ff0000 100%
    );
  }
  .cp-slider.alpha {
    background:
      linear-gradient(to right, transparent, var(--cp-hue, #0d99ff)),
      repeating-conic-gradient(#d4d4d4 0% 25%, #ffffff 0% 50%) 0 0 / 8px 8px;
  }
  .sl-handle {
    position: absolute;
    top: 50%;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: #fff;
    border: 1px solid rgba(0,0,0,.25);
    box-shadow: 0 1px 3px rgba(0,0,0,.35);
    transform: translate(-50%, -50%);
    pointer-events: none;
  }

  .cp-value {
    display: grid;
    grid-template-columns: 56px 1fr 52px;
    gap: 6px;
    padding: 12px;
  }
  .cp-input {
    height: 28px;
    background: var(--color-surface-2);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    padding: 0 8px;
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0;
    color: var(--color-fg);
    transition: border-color var(--motion-fast) var(--motion-easing);
  }
  .cp-input:hover { border-color: var(--color-border); }
  .cp-input:focus-within { border-color: var(--color-accent); }
  .cp-input input {
    width: 100%;
    background: transparent;
    border: 0;
    outline: 0;
    color: inherit;
    font: inherit;
    padding: 0;
    text-transform: lowercase;
  }
  .cp-input input::placeholder {
    color: var(--color-fg-subtle);
    font-style: italic;
  }
  .cp-input.cp-format {
    cursor: not-allowed; /* P2: format toggle */
    justify-content: space-between;
    gap: 4px;
    color: var(--color-fg);
  }
  .cp-input.cp-format .caret { opacity: 0.55; }
  .cp-input.cp-alpha {
    justify-content: flex-end;
  }
  .cp-input.cp-alpha input {
    width: 32px;
    text-align: right;
    -moz-appearance: textfield;
    appearance: textfield;
  }
  .cp-input.cp-alpha input::-webkit-outer-spin-button,
  .cp-input.cp-alpha input::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
  .cp-input.cp-alpha .pct {
    color: var(--color-fg-muted);
    margin-left: 2px;
  }

  .cp-swatches {
    padding: 8px 12px 12px;
    border-top: 1px solid var(--color-border);
    display: grid;
    gap: 10px;
  }
  .cp-swatches .grp { display: grid; gap: 6px; }
  .cp-swatches .lbl {
    display: flex;
    align-items: center;
    gap: 6px;
    font-family: var(--font-mono);
    font-size: 9.5px;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
  }
  .cp-swatches .grid {
    display: grid;
    grid-template-columns: repeat(10, 1fr);
    gap: 4px;
  }
  .cp-swatches .sw {
    aspect-ratio: 1 / 1;
    border-radius: 3px;
    border: 1px solid var(--color-border);
    cursor: pointer;
    position: relative;
    background-image: repeating-conic-gradient(#d4d4d4 0% 25%, #ffffff 0% 50%);
    background-size: 6px 6px;
  }
  .cp-swatches .sw::after {
    content: '';
    position: absolute;
    inset: 0;
    background: var(--c, transparent);
    border-radius: 2px;
  }
  .cp-swatches .sw.selected {
    outline: 1.5px solid var(--color-accent);
    outline-offset: 1px;
    border-color: transparent;
  }
  .cp-swatches .sw.transparent::after {
    background: linear-gradient(
      45deg,
      transparent calc(50% - 0.5px),
      var(--color-danger) calc(50% - 0.5px),
      var(--color-danger) calc(50% + 0.5px),
      transparent calc(50% + 0.5px)
    );
  }
  .cp-swatches .sw:focus-visible {
    outline: 1px dashed var(--color-accent);
    outline-offset: 2px;
  }
</style>
