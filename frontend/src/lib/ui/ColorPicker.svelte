<script module lang="ts">
  /**
   * Module-scope recent-color history — singleton 공유 (모든 picker instance
   * 가 같은 LRU 본다). max 10, FIFO. Phase 4: localStorage 영속 (ADR-0016 D11).
   * private/incognito 등 storage 차단 시 in-memory fallback.
   */
  const STORAGE_KEY = 'gtmux:colorpicker:recent';
  const MAX = 10;

  function loadFromStorage(): string[] {
    if (typeof window === 'undefined') return [];
    try {
      const raw = window.localStorage.getItem(STORAGE_KEY);
      if (raw === null) return [];
      const parsed = JSON.parse(raw);
      if (!Array.isArray(parsed)) return [];
      return parsed
        .filter((v): v is string => typeof v === 'string' && /^#[0-9a-f]{6}([0-9a-f]{2})?$/i.test(v))
        .slice(0, MAX);
    } catch {
      return [];
    }
  }

  function saveToStorage(list: readonly string[]): void {
    if (typeof window === 'undefined') return;
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(list));
    } catch {
      // private/incognito or quota exceeded — silent.
    }
  }

  const _recent: { list: string[] } = $state({ list: loadFromStorage() });

  export function recentColorList(): readonly string[] {
    return _recent.list;
  }

  export function pushRecentColor(hex: string): void {
    const norm = hex.toLowerCase();
    if (!/^#[0-9a-f]{6}([0-9a-f]{2})?$/.test(norm)) return;
    const i = _recent.list.indexOf(norm);
    if (i !== -1) _recent.list.splice(i, 1);
    _recent.list.unshift(norm);
    if (_recent.list.length > MAX) _recent.list.length = MAX;
    saveToStorage(_recent.list);
  }
</script>

<script lang="ts">
  /**
   * ColorPicker — Figma-style popover (ref/frontend-design/components-v4.html
   * §.shape-colorpicker spec 정합).
   *
   * Phase 1: visual shell — trigger swatch + popover layout (v4 시안).
   * Phase 2: SV / hue / alpha drag — pointerdown→move→up, draft preview, 1회 commit.
   * Phase 3: Format toggle (Hex / RGB / HSL), Eyedropper, Recent (session-scope).
   * Phase 4 (현 commit):
   *   - OKLCH format 추가 (ADR-0016 D12).
   *   - Recent 의 localStorage 영속 (ADR-0016 D11, key `gtmux:colorpicker:recent`).
   *   - Token-aware Document palette (ADR-0016 D10) — 시안 hardcoded 색 →
   *     semantic color token 10 개의 resolved hex.
   *
   * 입력 (props):
   *   - value: string | null | undefined  — 현재 색 (#rrggbb / #rrggbbaa /
   *     'transparent' / var(--…) / named).
   *   - oncommit: (string) => void — 변경 commit. 출력은 hex normalize.
   *   - disabled / mixed / allowTransparent / allowAlpha — 기존 props 유지.
   *
   * 정본:
   *   - plan-0010 Task 3 (Q "OKLCH/HSL/hex" — phase 3: HSL 추가, OKLCH P4)
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

  // ── Helpers (parse / normalize) ─────────────────────────────────
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

  // ── HSV ↔ hex / hue 색 ──────────────────────────────────────────
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

  // ── Phase 3: hex ↔ RGB string / HSL string ──────────────────────
  function hexToRgbParts(hex: string): { r: number; g: number; b: number } {
    const norm = normalizeHex(hex) ?? '#000000';
    const m = norm.match(/^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i);
    if (m === null) return { r: 0, g: 0, b: 0 };
    return {
      r: Number.parseInt(m[1] ?? '0', 16),
      g: Number.parseInt(m[2] ?? '0', 16),
      b: Number.parseInt(m[3] ?? '0', 16),
    };
  }

  function hexToRgbString(hex: string): string {
    const { r, g, b } = hexToRgbParts(hex);
    return `${r}, ${g}, ${b}`;
  }

  function parseRgbString(s: string): string | null {
    const m = s.trim().match(/^(\d+)\s*[,\s]\s*(\d+)\s*[,\s]\s*(\d+)$/);
    if (m === null) return null;
    const r = Math.max(0, Math.min(255, Number.parseInt(m[1] ?? '0', 10)));
    const g = Math.max(0, Math.min(255, Number.parseInt(m[2] ?? '0', 10)));
    const b = Math.max(0, Math.min(255, Number.parseInt(m[3] ?? '0', 10)));
    return '#' + [r, g, b].map((n) => n.toString(16).padStart(2, '0')).join('');
  }

  /** hex → HSL (h:0-360, s:0-100, l:0-100). */
  function hexToHsl(hex: string): { h: number; s: number; l: number } {
    const { r, g, b } = hexToRgbParts(hex);
    const rr = r / 255, gg = g / 255, bb = b / 255;
    const max = Math.max(rr, gg, bb);
    const min = Math.min(rr, gg, bb);
    const l = (max + min) / 2;
    let h = 0, s = 0;
    const d = max - min;
    if (d !== 0) {
      s = d / (l < 0.5 ? max + min : 2 - max - min);
      if (max === rr) h = ((gg - bb) / d + (gg < bb ? 6 : 0)) % 6;
      else if (max === gg) h = (bb - rr) / d + 2;
      else h = (rr - gg) / d + 4;
      h = Math.round(h * 60);
      if (h < 0) h += 360;
    }
    return { h, s: Math.round(s * 100), l: Math.round(l * 100) };
  }

  function hexToHslString(hex: string): string {
    const { h, s, l } = hexToHsl(hex);
    return `${h}, ${s}%, ${l}%`;
  }

  /** HSL string → hex. */
  function parseHslString(s: string): string | null {
    const m = s.trim().match(/^(\d+)\s*[,\s]\s*(\d+)%?\s*[,\s]\s*(\d+)%?$/);
    if (m === null) return null;
    const h = ((Number.parseInt(m[1] ?? '0', 10) % 360) + 360) % 360;
    const sat = Math.max(0, Math.min(100, Number.parseInt(m[2] ?? '0', 10))) / 100;
    const l = Math.max(0, Math.min(100, Number.parseInt(m[3] ?? '0', 10))) / 100;
    const c = (1 - Math.abs(2 * l - 1)) * sat;
    const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
    const m2 = l - c / 2;
    let r = 0, g = 0, b = 0;
    if (h < 60) { r = c; g = x; }
    else if (h < 120) { r = x; g = c; }
    else if (h < 180) { g = c; b = x; }
    else if (h < 240) { g = x; b = c; }
    else if (h < 300) { r = x; b = c; }
    else { r = c; b = x; }
    return '#' + [r + m2, g + m2, b + m2]
      .map((v2) => Math.round(v2 * 255).toString(16).padStart(2, '0'))
      .join('');
  }

  // ── Phase 4: OKLCH 변환 (Björn Ottosson formula) ─────────────────
  // sRGB ↔ linear ↔ OKLab ↔ OKLCH. 입력은 sRGB hex.

  function srgbToLinear(c: number): number {
    return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  }

  function linearToSrgb(c: number): number {
    return c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055;
  }

  /** sRGB hex → OKLCH { L, C, H }. L: 0-1 도메인, C: 0~0.4 정도, H: 0-360 degree. */
  function hexToOklch(hex: string): { L: number; C: number; H: number } {
    const { r, g, b } = hexToRgbParts(hex);
    const rl = srgbToLinear(r / 255);
    const gl = srgbToLinear(g / 255);
    const bl = srgbToLinear(b / 255);
    // OKLab cone responses (Ottosson).
    const l = 0.4122214708 * rl + 0.5363325363 * gl + 0.0514459929 * bl;
    const m = 0.2119034982 * rl + 0.6806995451 * gl + 0.1073969566 * bl;
    const s = 0.0883024619 * rl + 0.2817188376 * gl + 0.6299787005 * bl;
    const l_ = Math.cbrt(l);
    const m_ = Math.cbrt(m);
    const s_ = Math.cbrt(s);
    const L = 0.2104542553 * l_ + 0.793617785 * m_ - 0.0040720468 * s_;
    const A = 1.9779984951 * l_ - 2.428592205 * m_ + 0.4505937099 * s_;
    const B = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.808675766 * s_;
    const C = Math.sqrt(A * A + B * B);
    let H = (Math.atan2(B, A) * 180) / Math.PI;
    if (H < 0) H += 360;
    return { L, C, H };
  }

  function hexToOklchString(hex: string): string {
    const { L, C, H } = hexToOklch(hex);
    return `${Math.round(L * 100)}%, ${C.toFixed(3)}, ${Math.round(H)}`;
  }

  /** OKLCH 좌표 → sRGB hex (gamut clip). */
  function oklchToHex(L: number, C: number, H: number): string {
    const hr = (H * Math.PI) / 180;
    const A = C * Math.cos(hr);
    const B = C * Math.sin(hr);
    const l_ = L + 0.3963377774 * A + 0.2158037573 * B;
    const m_ = L - 0.1055613458 * A - 0.0638541728 * B;
    const s_ = L - 0.0894841775 * A - 1.291485548 * B;
    const l = l_ * l_ * l_;
    const m = m_ * m_ * m_;
    const s = s_ * s_ * s_;
    const rl = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
    const gl = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
    const bl = -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s;
    const r = Math.max(0, Math.min(1, linearToSrgb(rl)));
    const g = Math.max(0, Math.min(1, linearToSrgb(gl)));
    const b = Math.max(0, Math.min(1, linearToSrgb(bl)));
    return '#' + [r, g, b]
      .map((v2) => Math.round(v2 * 255).toString(16).padStart(2, '0'))
      .join('');
  }

  /** "L%, C, H" → hex. */
  function parseOklchString(s: string): string | null {
    const m = s.trim().match(/^(\d+(?:\.\d+)?)%?\s*[,\s]\s*(\d+(?:\.\d+)?)\s*[,\s]\s*(\d+(?:\.\d+)?)$/);
    if (m === null) return null;
    const Lp = Number.parseFloat(m[1] ?? '0');
    const C = Number.parseFloat(m[2] ?? '0');
    const H = Number.parseFloat(m[3] ?? '0');
    const L = Math.max(0, Math.min(100, Lp)) / 100;
    return oklchToHex(L, Math.max(0, Math.min(0.4, C)), ((H % 360) + 360) % 360);
  }

  // ── 파생값 ────────────────────────────────────────────────────────
  const resolvedHex = $derived.by(() => {
    if (mixed) return null;
    if (typeof value !== 'string') return null;
    return resolveCssColor(value);
  });

  const isTransparent = $derived(typeof value === 'string' && isTransparentValue(value));

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

  /** Inline hex input — trigger 옆 항상 보이는 색상값 표시 + 명시적 입력.
   *  popover 의 cp-input 과 별도 state — outer 는 *항상 hex* 만 (format toggle
   *  적용 X), popover 는 formatMode 따라. alpha 는 alphaInput 공유. */
  let inlineHexInput = $state('');
  let inlineEditing = $state(false);

  type FormatMode = 'hex' | 'rgb' | 'hsl' | 'oklch';
  let formatMode = $state<FormatMode>('hex');
  let formatMenuOpen = $state(false);

  /** Drag preview — drag 중 picker 내부 visual 만 update. drag end 1회 commit. */
  let draftHsv = $state<{ h: number; s: number; v: number } | null>(null);
  let draftAlpha = $state<number | null>(null);

  const effectiveHsv = $derived(draftHsv ?? hsv);
  const effectiveAlpha = $derived(draftAlpha ?? alphaPercent);
  const effectiveHex = $derived.by(() => {
    if (draftHsv === null) return resolvedHex !== null ? hexRgb(resolvedHex) : '#000000';
    return hsvToHex(draftHsv.h, draftHsv.s, draftHsv.v);
  });
  const effectiveHueColor = $derived(hueToHex(effectiveHsv.h));

  /** Display value 의 format 별 정규화 — input 의 표시값. editing 중엔 textInput 그대로. */
  const displayInFormat = $derived.by(() => {
    if (mixed) return 'Mixed';
    if (isTransparent) return 'transparent';
    const baseHex = hexRgb(effectiveHex);
    if (formatMode === 'rgb') return hexToRgbString(baseHex);
    if (formatMode === 'hsl') return hexToHslString(baseHex);
    if (formatMode === 'oklch') return hexToOklchString(baseHex);
    return baseHex.replace('#', '').toUpperCase();
  });

  // ── Browser feature detect — EyeDropper Web API ───────────────
  const hasEyeDropper = $derived.by(() => {
    if (typeof window === 'undefined') return false;
    return typeof (window as unknown as { EyeDropper?: unknown }).EyeDropper === 'function';
  });

  /**
   * Trigger rect + popover rect + viewport 로 위치 계산.
   * 기본 = trigger 아래 + 좌측 정렬. 우측 overflow 시 우측 정렬. 아래 overflow 시
   * trigger 위로 flip. 양쪽 모두 안 들어가면 viewport 안 clamp.
   */
  let popoverPos = $state<{ top: number; left: number }>({ top: 0, left: 0 });

  /**
   * 위치 정책 (사용자 요구):
   * - Horizontal: RightPanel 의 *좌측 가장자리* anchor — popover 가 panel
   *   영역을 가리지 않도록 panel.left - popover.width - gap. RightPanel 가
   *   없는 경우 trigger.left - popover.width - gap (좌측 mount).
   * - Vertical: trigger 의 top 근처. popover.height 가 viewport 보다 크면
   *   viewport [margin, vh-margin] 안 clamp.
   * - Horizontal clamp: 좌측 < margin 이면 margin, 우측 > vw-margin 이면
   *   안쪽으로 밀어 넣음.
   */
  function updatePopoverPos(): void {
    if (containerEl === undefined || popoverEl === undefined) return;
    const tRect = containerEl.getBoundingClientRect();
    const pRect = popoverEl.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const margin = 8;
    const gap = 8;

    const rightPanel = document.querySelector('.right-panel') as HTMLElement | null;
    let left: number;
    if (rightPanel !== null) {
      const rRect = rightPanel.getBoundingClientRect();
      left = rRect.left - pRect.width - gap;
    } else {
      left = tRect.left - pRect.width - gap;
    }
    if (left < margin) left = margin;
    if (left + pRect.width > vw - margin) left = vw - pRect.width - margin;

    let top = tRect.top;
    if (top + pRect.height > vh - margin) top = vh - pRect.height - margin;
    if (top < margin) top = margin;

    popoverPos = { top, left };
  }

  $effect(() => {
    if (!editing) textInput = displayInFormat;
  });
  $effect(() => {
    if (!editingAlpha) alphaInput = String(Math.round(effectiveAlpha));
  });
  // Inline 의 표시값 — 항상 hex (#rrggbb), '#' 제거 + uppercase.
  $effect(() => {
    if (!inlineEditing) {
      if (mixed) inlineHexInput = '';
      else if (isTransparent) inlineHexInput = '';
      else inlineHexInput = hexRgb(effectiveHex).replace('#', '').toUpperCase();
    }
  });

  // ── Click-outside close + popover position tracking ─────────────
  $effect(() => {
    if (!open) return;
    function onDocPointerDown(e: PointerEvent): void {
      const target = e.target as Node;
      if (containerEl?.contains(target)) return;
      if (popoverEl?.contains(target)) return;
      formatMenuOpen = false;
      open = false;
    }
    const onReflow = () => updatePopoverPos();
    queueMicrotask(() => {
      document.addEventListener('pointerdown', onDocPointerDown, true);
      updatePopoverPos();
    });
    window.addEventListener('resize', onReflow);
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
    // Recent: 6자리 hex 정규화 후 prepend (alpha 8자리 도 OK, single token 으로 기억).
    const norm = normalizeHex(next);
    if (norm !== null) pushRecentColor(norm);
  }

  function onTextChange(): void {
    const trimmed = textInput.trim().toLowerCase();
    if (allowTransparent && !allowAlpha && isTransparentValue(trimmed)) {
      if (value !== 'transparent') oncommit('transparent');
      textInput = '';
      return;
    }
    // Format 별 parse.
    let normRgb: string | null = null;
    if (formatMode === 'rgb') normRgb = parseRgbString(textInput);
    else if (formatMode === 'hsl') normRgb = parseHslString(textInput);
    else if (formatMode === 'oklch') normRgb = parseOklchString(textInput);
    else normRgb = normalizeHex(textInput);

    if (normRgb === null) {
      // 잘못된 입력 — revert display.
      textInput = displayInFormat;
      return;
    }
    if (allowAlpha) {
      if (/^#[0-9a-f]{8}$/.test(normRgb)) {
        commitColor(normRgb);
      } else {
        const a = Math.max(0, Math.min(100, Number.parseInt(alphaInput || '100', 10)));
        commitColor(combineHexAlpha(normRgb, Number.isNaN(a) ? 100 : a));
      }
    } else {
      commitColor(normRgb);
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

  /** Inline hex input commit — outer 는 hex 만. transparent literal 도 허용. */
  function onInlineHexCommit(): void {
    const trimmed = inlineHexInput.trim().toLowerCase();
    if (allowTransparent && !allowAlpha && isTransparentValue(trimmed)) {
      if (value !== 'transparent') oncommit('transparent');
      inlineHexInput = '';
      return;
    }
    const norm = normalizeHex(inlineHexInput);
    if (norm === null) {
      // revert
      inlineHexInput = isTransparent || mixed ? '' : hexRgb(effectiveHex).replace('#', '').toUpperCase();
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

  // ── Drag handlers (SV / hue / alpha) ─────────────────────────────
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

  // ── Phase 3: Format toggle ───────────────────────────────────────
  function selectFormat(m: FormatMode): void {
    if (formatMode !== m) {
      formatMode = m;
      // editing 중이면 사용자가 본 format 의 새 표시값으로 동기 (editing 종료 후
      // $effect 의 textInput=displayInFormat 이 처리하지만, 즉시 보이게).
      if (!editing) textInput = displayInFormat;
    }
    formatMenuOpen = false;
  }

  function toggleFormatMenu(): void {
    if (disabled) return;
    formatMenuOpen = !formatMenuOpen;
  }

  // ── Phase 3: Eyedropper ─────────────────────────────────────────
  async function pickFromScreen(): Promise<void> {
    if (disabled || !hasEyeDropper) return;
    try {
      const W = window as unknown as {
        EyeDropper: new () => { open: () => Promise<{ sRGBHex: string }> };
      };
      const dropper = new W.EyeDropper();
      const result = await dropper.open();
      const norm = normalizeHex(result.sRGBHex);
      if (norm !== null) {
        if (allowAlpha) {
          const a = Math.max(0, Math.min(100, Number.parseInt(alphaInput || '100', 10)));
          commitColor(combineHexAlpha(norm, Number.isNaN(a) ? 100 : a));
        } else {
          commitColor(norm);
        }
      }
    } catch {
      // 사용자가 Esc 또는 cancel — silent.
    }
  }

  // ── Phase 4: Token-aware Document palette (ADR-0016 D10) ────────
  // Semantic color token 10 종을 *현재 theme 의 resolved hex* 로 변환. theme
  // 전환 시 reactive — themeStore 변경 → CSS computed value 도 변경 → 다음 read.
  // resolveCssColor 는 document 의존이라 SSR 환경에선 null fallback.
  const TOKEN_NAMES: readonly { token: string; label: string }[] = [
    { token: 'var(--color-fg)', label: 'Foreground' },
    { token: 'var(--color-fg-muted)', label: 'Muted' },
    { token: 'var(--color-fg-subtle)', label: 'Subtle' },
    { token: 'var(--color-bg)', label: 'Background' },
    { token: 'var(--color-surface-2)', label: 'Surface 2' },
    { token: 'var(--color-accent)', label: 'Accent' },
    { token: 'var(--color-success)', label: 'Success' },
    { token: 'var(--color-warning)', label: 'Warning' },
    { token: 'var(--color-danger)', label: 'Danger' },
    { token: 'var(--color-info)', label: 'Info' },
  ];
  const documentSwatches = $derived.by((): readonly { hex: string; label: string }[] => {
    if (!open) return []; // popover 열린 후 에야 measure
    return TOKEN_NAMES
      .map(({ token, label }) => {
        const hex = resolveCssColor(token);
        return hex !== null ? { hex: hexRgb(hex), label } : null;
      })
      .filter((v): v is { hex: string; label: string } => v !== null);
  });
  const recentSwatches = $derived(recentColorList());
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

  <!-- Inline hex value — trigger 옆 항상 표시 + 명시적 입력. InspectorField 와
       동일 패턴 (label wrapper + .k prefix + native input). -->
  <label class="inline-hex">
    <span class="k" aria-hidden="true">HEX</span>
    <input
      type="text"
      class="field"
      value={inlineHexInput}
      oninput={(e) => {
        inlineEditing = true;
        inlineHexInput = (e.currentTarget as HTMLInputElement).value;
      }}
      placeholder={mixed ? 'Mixed' : isTransparent ? 'transparent' : '000000'}
      {disabled}
      onfocus={() => (inlineEditing = true)}
      onblur={() => {
        inlineEditing = false;
        onInlineHexCommit();
      }}
      onkeydown={(e) => {
        if (e.key === 'Enter') (e.currentTarget as HTMLInputElement).blur();
      }}
      spellcheck="false"
      autocomplete="off"
      aria-label="Color hex"
    />
  </label>
  {#if allowAlpha}
    <label class="inline-alpha">
      <span class="k" aria-hidden="true">A</span>
      <input
        type="number"
        class="field"
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
      <span class="suf" aria-hidden="true">%</span>
    </label>
  {/if}

  {#if open}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="shape-colorpicker"
      bind:this={popoverEl}
      style="--cp-hue: {effectiveHueColor}; top: {popoverPos.top}px; left: {popoverPos.left}px;"
      role="dialog"
      aria-label="Color picker"
    >
      <!-- Head -->
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

      <!-- SV square -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="cp-sv"
        bind:this={svEl}
        onpointerdown={onSvDown}
        aria-label={`Saturation ${Math.round(effectiveHsv.s)}, Value ${Math.round(effectiveHsv.v)}`}
      >
        <div class="sv-handle" style:left="{effectiveHsv.s}%" style:top="{100 - effectiveHsv.v}%"></div>
      </div>

      <!-- Sliders -->
      <div class="cp-sliders">
        <button
          type="button"
          class="cp-eye"
          class:has-feature={hasEyeDropper}
          title={hasEyeDropper ? 'Pick from screen' : 'Eyedropper (browser unsupported)'}
          disabled={!hasEyeDropper || disabled}
          aria-disabled={!hasEyeDropper || disabled}
          onclick={() => void pickFromScreen()}
        >
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

      <!-- Value row -->
      <div class="cp-value">
        <div
          class="cp-input cp-format"
          class:is-open={formatMenuOpen}
          title="Color format"
          role="button"
          tabindex="0"
          onclick={toggleFormatMenu}
          onkeydown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault();
              toggleFormatMenu();
            }
          }}
        >
          {formatMode === 'hex' ? 'HEX' : formatMode === 'rgb' ? 'RGB' : formatMode === 'hsl' ? 'HSL' : 'OKLCH'}
          <span class="caret" aria-hidden="true">▾</span>
        </div>
        <div class="cp-input">
          <input
            type="text"
            value={editing ? textInput : displayInFormat}
            oninput={(e) => {
              if (editing) textInput = (e.currentTarget as HTMLInputElement).value;
            }}
            placeholder={mixed ? 'Mixed' : isTransparent ? 'transparent' : formatMode === 'hex' ? '000000' : formatMode === 'rgb' ? '0, 0, 0' : formatMode === 'hsl' ? '0, 0%, 0%' : '0%, 0, 0'}
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
            aria-label={`Color ${formatMode.toUpperCase()} value`}
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

        {#if formatMenuOpen}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="cp-format-menu is-open" role="menu" aria-label="Color format">
            {#each ['hex', 'rgb', 'hsl', 'oklch'] as const as m}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <div
                class="item"
                class:active={formatMode === m}
                role="menuitemradio"
                aria-checked={formatMode === m}
                tabindex="0"
                onclick={() => selectFormat(m)}
              >
                {m.toUpperCase()}
                {#if formatMode === m}
                  <span class="tick" aria-hidden="true">✓</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>

      <!-- Swatches: Document + Recent -->
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
                class:selected={resolvedHex !== null && hexRgb(resolvedHex) === sw.hex}
                style:--c={sw.hex}
                role="button"
                tabindex="0"
                aria-label={`${sw.label} (${sw.hex})`}
                title={`${sw.label} · ${sw.hex}`}
                onclick={() => onSwatchClick(sw.hex)}
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
                  class:selected={resolvedHex !== null && hexRgb(resolvedHex) === hexRgb(sw)}
                  style:--c={hexRgb(sw)}
                  role="button"
                  tabindex="0"
                  aria-label={`Set color ${sw}`}
                  onclick={() => onSwatchClick(hexRgb(sw))}
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
  /* ── Trigger swatch ──────────────────────────────────────────── */
  .color-picker {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: var(--space-4);
    height: 22px;
  }
  .color-picker.disabled { opacity: 0.55; }

  /* Inline hex / alpha — trigger 옆 항상 보이는 값 표시 + 직접 입력.
     InspectorField .inspector-input 과 동일 패턴 (label wrapper + .k prefix). */
  .inline-hex,
  .inline-alpha {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 6px;
    box-sizing: border-box;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-fg);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0;
    cursor: text;
    transition: border-color var(--motion-fast) var(--motion-easing);
  }
  .inline-hex {
    flex: 1 1 auto;
    min-width: 0;
    width: 76px;
  }
  .inline-alpha {
    width: 60px;
  }
  .inline-hex:hover,
  .inline-alpha:hover {
    border-color: var(--color-border-strong);
  }
  .inline-hex:focus-within,
  .inline-alpha:focus-within {
    border-color: var(--color-accent);
  }

  .inline-hex .k,
  .inline-alpha .k {
    flex: 0 0 auto;
    color: var(--color-fg-muted);
    text-transform: uppercase;
    font-size: 10px;
    letter-spacing: 0.4px;
    pointer-events: none;
  }
  .inline-alpha .suf {
    flex: 0 0 auto;
    color: var(--color-fg-subtle);
    font-size: 10px;
    pointer-events: none;
  }

  .inline-hex .field,
  .inline-alpha .field {
    flex: 1 1 auto;
    min-width: 0;
    width: 100%;
    background: transparent;
    border: 0;
    outline: 0;
    padding: 0;
    margin: 0;
    color: inherit;
    font: inherit;
    letter-spacing: inherit;
    text-transform: lowercase;
  }
  .inline-alpha .field {
    text-align: right;
    -moz-appearance: textfield;
    appearance: textfield;
  }
  .inline-alpha .field::-webkit-outer-spin-button,
  .inline-alpha .field::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
  .inline-hex .field::placeholder,
  .inline-alpha .field::placeholder {
    color: var(--color-fg-subtle);
    font-style: italic;
  }
  .inline-hex .field:disabled,
  .inline-alpha .field:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

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

  /* ── Popover ────────────────────────────────────────────────── */
  .shape-colorpicker {
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
    cursor: pointer;
    padding: 0;
    opacity: 0.4;
    transition: background var(--motion-fast) var(--motion-easing), opacity var(--motion-fast) var(--motion-easing);
  }
  .cp-eye.has-feature { opacity: 1; }
  .cp-eye.has-feature:hover { background: var(--color-glass-1); }
  .cp-eye:disabled { cursor: not-allowed; }
  .cp-eye:focus-visible { outline: 1px dashed var(--color-accent); outline-offset: 1px; }

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
    position: relative; /* format menu absolute anchor */
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
    cursor: pointer;
    justify-content: space-between;
    gap: 4px;
    color: var(--color-fg);
    user-select: none;
  }
  .cp-input.cp-format:hover { border-color: var(--color-border); }
  .cp-input.cp-format.is-open { border-color: var(--color-accent); }
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

  /* Format dropdown — anchored under format pill */
  .cp-format-menu {
    position: absolute;
    top: 44px;
    left: 12px;
    width: 96px;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    padding: 4px;
    z-index: 4;
    display: none;
  }
  .cp-format-menu.is-open { display: block; }
  .cp-format-menu .item {
    display: flex;
    align-items: center;
    height: 24px;
    padding: 0 8px;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--color-fg);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .cp-format-menu .item:hover { background: var(--color-surface-2); }
  .cp-format-menu .item.active { color: var(--color-accent); }
  .cp-format-menu .item .tick { margin-left: auto; }

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
