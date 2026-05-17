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

  /** Full HSV → hex (sat/val 0~100). */
  function hsvToHex(h: number, s: number, v: number): string {
    const sat = Math.max(0, Math.min(100, s)) / 100;
    const val = Math.max(0, Math.min(100, v)) / 100;
    const c = val * sat;
    const hh = ((h % 360) + 360) % 360;
    const x = c * (1 - Math.abs(((hh / 60) % 2) - 1));
    const m = val - c;
    let r = 0, g = 0, b = 0;
    if (hh < 60) { r = c; g = x; }
    else if (hh < 120) { r = x; g = c; }
    else if (hh < 180) { g = c; b = x; }
    else if (hh < 240) { g = x; b = c; }
    else if (hh < 300) { r = x; b = c; }
    else { r = c; b = x; }
    return '#' + [r + m, g + m, b + m]
      .map((v2) => Math.round(v2 * 255).toString(16).padStart(2, '0'))
      .join('');
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
  let popoverEl: HTMLDivElement | undefined = $state();
  let svEl: HTMLDivElement | undefined = $state();
  let hueEl: HTMLDivElement | undefined = $state();
  let alphaEl: HTMLDivElement | undefined = $state();

  /**
   * Drag preview — drag 중 picker 내부 visual 만 update (swatch/handles/hex).
   * drag end (pointerup) 에 1회 commit → ADR-0028 history 1 entry.
   * null = drag 비활성 (resolvedHex / hsv / alphaPercent 사용).
   */
  let draftHsv = $state<{ h: number; s: number; v: number } | null>(null);
  let draftAlpha = $state<number | null>(null);

  const effectiveHsv = $derived(draftHsv ?? hsv);
  const effectiveAlpha = $derived(draftAlpha ?? alphaPercent);
  const effectiveHex = $derived.by(() => {
    if (draftHsv === null) return resolvedHex !== null ? hexRgb(resolvedHex) : '#000000';
    return hsvToHex(draftHsv.h, draftHsv.s, draftHsv.v);
  });
  const effectiveHueColor = $derived(hueToHex(effectiveHsv.h));
  /** Popover 위치 — viewport (fixed) 기준 clamp. open 시 measure + scroll/resize 갱신. */
  let popoverPos = $state<{ top: number; left: number }>({ top: 0, left: 0 });

  /**
   * Trigger rect + popover rect + viewport 로 위치 계산.
   * 기본 = trigger 아래 + 좌측 정렬. 우측 overflow 시 우측 정렬. 아래 overflow 시
   * trigger 위로 flip. 양쪽 모두 안 들어가면 viewport 안 clamp.
   */
  function updatePopoverPos(): void {
    if (containerEl === undefined || popoverEl === undefined) return;
    const tRect = containerEl.getBoundingClientRect();
    const pRect = popoverEl.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const margin = 8;
    const gap = 6;
    let left = tRect.left;
    if (left + pRect.width > vw - margin) left = vw - pRect.width - margin;
    if (left < margin) left = margin;
    let top = tRect.bottom + gap;
    if (top + pRect.height > vh - margin) {
      // 아래 overflow — trigger 위로 flip 시도
      const flipped = tRect.top - pRect.height - gap;
      top = flipped >= margin ? flipped : Math.max(margin, vh - pRect.height - margin);
    }
    popoverPos = { top, left };
  }

  $effect(() => {
    if (!editing) textInput = displayValue;
  });
  $effect(() => {
    if (!editingAlpha) alphaInput = String(alphaPercent);
  });

  // ── Click-outside close + popover position tracking ─────────────
  $effect(() => {
    if (!open) return;
    function onDocPointerDown(e: PointerEvent): void {
      // trigger 와 popover 둘 다 inside 로 인정 (popover 는 fixed 라
      // containerEl.contains 가 false — popoverEl 별도 검사).
      const target = e.target as Node;
      if (containerEl?.contains(target)) return;
      if (popoverEl?.contains(target)) return;
      open = false;
    }
    const onReflow = () => updatePopoverPos();
    queueMicrotask(() => {
      document.addEventListener('pointerdown', onDocPointerDown, true);
      // mount 직후 첫 measure — popoverEl 의 rect 가 reliable 한 시점.
      updatePopoverPos();
    });
    window.addEventListener('resize', onReflow);
    // capture phase scroll — popover 안 element scroll 은 무시 (popover 자체
    // 가 scroll 안 함 + 외부 scroll 만 reposition trigger).
    window.addEventListener('scroll', onReflow, true);
    return () => {
      document.removeEventListener('pointerdown', onDocPointerDown, true);
      window.removeEventListener('resize', onReflow);
      window.removeEventListener('scroll', onReflow, true);
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

  // ── Drag handlers (SV / hue / alpha) ─────────────────────────────
  /**
   * Pattern: pointerdown 에서 draft state 초기화 + setPointerCapture 으로 element
   * 외 drag 도 추적. pointermove 마다 draft 만 update (picker 내부 visual).
   * pointerup 시 draft 의 최종값을 1회 commit 후 draft = null.
   *
   * commit 시점: ADR-0028 정합 — drag 중 매 move commit 하면 history 가 dirty.
   * drag end 1회 commit 으로 history 1 entry 유지. drag 중 visual preview 는
   * draftHsv / draftAlpha 가 effective* 파생에 흘러가 swatch / handles / hex
   * 모두 reactive update.
   */
  function clamp01(v: number): number {
    return Math.max(0, Math.min(1, v));
  }

  function commitDraft(): void {
    const h = draftHsv;
    const a = draftAlpha;
    if (h === null && a === null) return;
    const baseRgb = h !== null
      ? hsvToHex(h.h, h.s, h.v)
      : (resolvedHex !== null ? hexRgb(resolvedHex) : '#000000');
    const aPct = a !== null ? a : alphaPercent;
    if (allowAlpha) {
      commitColor(combineHexAlpha(baseRgb, aPct));
    } else {
      commitColor(baseRgb);
    }
  }

  function onSvDown(e: PointerEvent): void {
    if (disabled || svEl === undefined) return;
    e.preventDefault();
    svEl.setPointerCapture(e.pointerId);
    const update = (ev: PointerEvent): void => {
      if (svEl === undefined) return;
      const r = svEl.getBoundingClientRect();
      const sx = clamp01((ev.clientX - r.left) / r.width);
      const sy = clamp01((ev.clientY - r.top) / r.height);
      const h = (draftHsv ?? hsv).h;
      draftHsv = { h, s: sx * 100, v: (1 - sy) * 100 };
    };
    update(e);
    const onMove = (ev: PointerEvent): void => update(ev);
    const onUp = (ev: PointerEvent): void => {
      svEl?.removeEventListener('pointermove', onMove);
      svEl?.removeEventListener('pointerup', onUp);
      svEl?.removeEventListener('pointercancel', onUp);
      commitDraft();
      draftHsv = null;
      draftAlpha = null;
      if (svEl?.hasPointerCapture(ev.pointerId)) svEl.releasePointerCapture(ev.pointerId);
    };
    svEl.addEventListener('pointermove', onMove);
    svEl.addEventListener('pointerup', onUp);
    svEl.addEventListener('pointercancel', onUp);
  }

  function onHueDown(e: PointerEvent): void {
    if (disabled || hueEl === undefined) return;
    e.preventDefault();
    hueEl.setPointerCapture(e.pointerId);
    const update = (ev: PointerEvent): void => {
      if (hueEl === undefined) return;
      const r = hueEl.getBoundingClientRect();
      const t = clamp01((ev.clientX - r.left) / r.width);
      const cur = draftHsv ?? hsv;
      draftHsv = { h: t * 360, s: cur.s, v: cur.v };
    };
    update(e);
    const onMove = (ev: PointerEvent): void => update(ev);
    const onUp = (ev: PointerEvent): void => {
      hueEl?.removeEventListener('pointermove', onMove);
      hueEl?.removeEventListener('pointerup', onUp);
      hueEl?.removeEventListener('pointercancel', onUp);
      commitDraft();
      draftHsv = null;
      draftAlpha = null;
      if (hueEl?.hasPointerCapture(ev.pointerId)) hueEl.releasePointerCapture(ev.pointerId);
    };
    hueEl.addEventListener('pointermove', onMove);
    hueEl.addEventListener('pointerup', onUp);
    hueEl.addEventListener('pointercancel', onUp);
  }

  function onAlphaDown(e: PointerEvent): void {
    if (disabled || !allowAlpha || alphaEl === undefined) return;
    e.preventDefault();
    alphaEl.setPointerCapture(e.pointerId);
    const update = (ev: PointerEvent): void => {
      if (alphaEl === undefined) return;
      const r = alphaEl.getBoundingClientRect();
      const t = clamp01((ev.clientX - r.left) / r.width);
      draftAlpha = Math.round(t * 100);
    };
    update(e);
    const onMove = (ev: PointerEvent): void => update(ev);
    const onUp = (ev: PointerEvent): void => {
      alphaEl?.removeEventListener('pointermove', onMove);
      alphaEl?.removeEventListener('pointerup', onUp);
      alphaEl?.removeEventListener('pointercancel', onUp);
      commitDraft();
      draftHsv = null;
      draftAlpha = null;
      if (alphaEl?.hasPointerCapture(ev.pointerId)) alphaEl.releasePointerCapture(ev.pointerId);
    };
    alphaEl.addEventListener('pointermove', onMove);
    alphaEl.addEventListener('pointerup', onUp);
    alphaEl.addEventListener('pointercancel', onUp);
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
      class:checker={isTransparent || (allowAlpha && effectiveAlpha < 100)}
      style:background={mixed || isTransparent ? undefined : (allowAlpha && effectiveAlpha < 100 ? combineHexAlpha(effectiveHex, effectiveAlpha) : effectiveHex)}
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
      bind:this={popoverEl}
      style="--cp-hue: {effectiveHueColor}; top: {popoverPos.top}px; left: {popoverPos.left}px;"
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

      <!-- SV square — drag in saturation/value 2D plane. -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="cp-sv"
        bind:this={svEl}
        onpointerdown={onSvDown}
        aria-label={`Saturation ${Math.round(effectiveHsv.s)}, Value ${Math.round(effectiveHsv.v)}`}
      >
        <div class="sv-handle" style:left="{effectiveHsv.s}%" style:top="{100 - effectiveHsv.v}%"></div>
      </div>

      <!-- Sliders — hue + alpha. -->
      <div class="cp-sliders">
        <button type="button" class="cp-eye" title="Eyedropper (P3+)" disabled aria-disabled="true">
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M9 1.5l3.5 3.5-1.5 1.5L8 4z"/>
            <path d="M8 4 3.2 8.8a1.4 1.4 0 0 0 0 2L4 11.6l-.6.6a.7.7 0 0 1-1-1l.6-.6.8-.8L8.8 4.8"/>
          </svg>
        </button>
        <div
          class="cp-slider hue"
          bind:this={hueEl}
          onpointerdown={onHueDown}
          role="slider"
          aria-label="Hue"
          aria-valuemin="0"
          aria-valuemax="360"
          aria-valuenow={Math.round(effectiveHsv.h)}
          tabindex="-1"
        >
          <div class="sl-handle" style:left="{(effectiveHsv.h / 360) * 100}%"></div>
        </div>
        <div
          class="cp-slider alpha"
          class:disabled={!allowAlpha}
          bind:this={alphaEl}
          onpointerdown={onAlphaDown}
          role="slider"
          aria-label="Alpha"
          aria-valuemin="0"
          aria-valuemax="100"
          aria-valuenow={Math.round(effectiveAlpha)}
          tabindex="-1"
        >
          <div class="sl-handle" style:left="{effectiveAlpha}%"></div>
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
            value={editing ? textInput : (draftHsv !== null ? effectiveHex : textInput)}
            oninput={(e) => {
              if (editing) textInput = (e.currentTarget as HTMLInputElement).value;
            }}
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
              value={editingAlpha ? alphaInput : String(Math.round(effectiveAlpha))}
              oninput={(e) => {
                if (editingAlpha) alphaInput = (e.currentTarget as HTMLInputElement).value;
              }}
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
            <span>{Math.round(effectiveAlpha)}</span>
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
    /* fixed — parent overflow 와 무관하게 viewport 기준 위치. open 시 JS 로
       trigger rect + viewport clamp 으로 top/left inline style 부여. */
    position: fixed;
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
    cursor: crosshair;
    touch-action: none;
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
    cursor: pointer;
    touch-action: none;
  }
  .cp-slider.disabled {
    cursor: not-allowed;
    opacity: 0.4;
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
