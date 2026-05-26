// documentRender — Document item 의 markdown / html rendering helper.
//
// 정본:
// - ADR-0018 D10 amend ③ (2026-05-21) — marked + DOMPurify 도입.
// - ADR-0018 D10 amend ④ (2026-05-21) — HTML source/rendered toggle + helper 외부화.
// - ADR-0037 (2026-05-21) — HTML rendered mode iframe isolation +
//   SVG/MathML/media allowlist 확장.
//
// 분기:
// - file type = markdown → marked.parse(raw) → DOMPurify.sanitize → HTML string.
// - file type = html     → sandboxed iframe srcdoc (CSS 격리 + MathJax 허용).
// - file type = 기타     → 단순 escape 된 source 그대로.
//
// DocumentNode (normal) 와 MaximizedItemModal (maximize) 양쪽이 같은 helper 사용
// — rendering 동기화 보장. 양쪽 코드 중복 방지 (amend ④ 의 drift-방지 invariant).

import { marked } from 'marked';
import DOMPurify from 'dompurify';

marked.setOptions({ gfm: true, breaks: false });

// ADR-0037 D2 — rendered mode 의 DOMPurify allowlist 확장:
// - USE_PROFILES.html : 기존 (block/inline tag)
// - USE_PROFILES.svg + svgFilters : <svg> 자식 (path/circle/g/filter 등) — diagram/icon
// - USE_PROFILES.mathMl : <math> + MathML — 수식
// - ADD_TAGS : HTML5 media (<video> / <audio> / <source> / <track> / <picture>)
// - ADD_ATTR : 위 element 의 attribute (controls/autoplay/srcset 등)
// 금지 유지 (DOMPurify default): <script>, on*, javascript:, <iframe>, <object>, <embed>,
// <link rel="stylesheet">, <style>@import.
const PURIFY_OPTIONS = {
  USE_PROFILES: { html: true, svg: true, svgFilters: true, mathMl: true },
  ADD_TAGS: ['video', 'audio', 'source', 'track', 'picture'],
  ADD_ATTR: [
    'controls', 'autoplay', 'loop', 'muted', 'playsinline',
    'preload', 'poster', 'srcset', 'sizes', 'crossorigin',
  ],
};

/** Markdown raw → sanitized HTML string. */
export function renderMarkdown(raw: string): string {
  if (raw.length === 0) return '';
  try {
    const html = marked.parse(raw, { async: false }) as string;
    return DOMPurify.sanitize(html, PURIFY_OPTIONS);
  } catch {
    return DOMPurify.sanitize(raw, PURIFY_OPTIONS);
  }
}

/** HTML raw → sanitized HTML string (no markdown processing). */
export function renderHtml(raw: string): string {
  if (raw.length === 0) return '';
  return DOMPurify.sanitize(raw, PURIFY_OPTIONS);
}

/** Document view mode. HTML/markdown both use a simple rendered ↔ source toggle. */
export type DocumentViewMode = 'rendered' | 'source';

/** File type label → whether viewMode toggle 가 의미 있는지. */
export function isToggleableFileType(label: string): boolean {
  return label === 'markdown' || label === 'html';
}

/**
 * View-mode transition helper. Toggleable file types use rendered ↔ source.
 *
 * DocumentNode + MaximizedItemModal 의 mode 전이 single source of truth —
 * 두 컴포넌트의 toggle 동작 drift 방지.
 */
export function getNextViewMode(current: DocumentViewMode, fileType: string): DocumentViewMode {
  void fileType;
  return current === 'source' ? 'rendered' : 'source';
}

/**
 * ADR-0037 D4 — 토글 button 의 next-mode 라벨 (tooltip / aria-label).
 * icon 분기와 짝 — 사용자가 클릭 시 어디로 가는지 예측 가능.
 */
export function getNextViewModeLabel(current: DocumentViewMode, fileType: string): string {
  const next = getNextViewMode(current, fileType);
  if (next === 'source') return 'Show source';
  return 'Show rendered';
}

/**
 * Rendered HTML is isolated from the parent DOM but still allows script-only
 * renderers such as MathJax to produce static output. No popups, forms,
 * top-navigation, downloads, or same-origin privileges are granted.
 */
export const RENDERED_HTML_IFRAME_SANDBOX = 'allow-scripts';

/**
 * Full HTML rendered mode 의 iframe srcdoc builder.
 *
 * rendered mode 는 sandbox iframe 으로 격리한다. 이유:
 * standalone HTML 의 <style> 은 root/body/universal/class selector 를
 * 포함할 수 있고, parent DOM 에 {@html} 로 직접 주입하면 gtmux app chrome 을
 * 오염시킨다. iframe 에서는 CSS 와 script-rendered static output(MathJax 등)
 * 은 보존하되 same-origin / popup / top-navigation 권한은 주지 않는다.
 *
 * <base target="_blank"> 는 주입하지 않는다. 문서 내부 TOC 의 href="#..."
 * routing 이 iframe 안에서 그대로 동작해야 하기 때문이다.
 */
export function buildRenderedHtmlSrcdoc(raw: string): string {
  return raw;
}
