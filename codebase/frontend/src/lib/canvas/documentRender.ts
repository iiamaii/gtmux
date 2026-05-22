// documentRender — Document item 의 markdown / html rendering helper.
//
// 정본:
// - ADR-0018 D10 amend ③ (2026-05-21) — marked + DOMPurify 도입.
// - ADR-0018 D10 amend ④ (2026-05-21) — HTML source/rendered toggle + helper 외부화.
// - ADR-0037 (2026-05-21) — 3-mode viewMode (rendered/interactive/source) +
//   SVG/MathML/media allowlist 확장 + sandboxed iframe (interactive).
//
// 분기 (rendered mode):
// - file type = markdown → marked.parse(raw) → DOMPurify.sanitize → HTML string.
// - file type = html     → DOMPurify.sanitize(raw) → HTML string (raw 가 이미 HTML).
// - file type = 기타     → 단순 escape 된 source 그대로.
//
// 분기 (interactive mode, html 만):
// - 컴포넌트가 직접 <iframe srcdoc={raw} sandbox={INTERACTIVE_IFRAME_SANDBOX}> 렌더.
//   helper 함수 거치지 않음 — Svelte 의 attribute escaping 이 자동 처리.
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

/** Document view mode (ADR-0037 D1 — 3-mode 확장). */
export type DocumentViewMode = 'rendered' | 'interactive' | 'source';

/** File type label → whether viewMode toggle 가 의미 있는지. */
export function isToggleableFileType(label: string): boolean {
  return label === 'markdown' || label === 'html';
}

/**
 * ADR-0037 D1 — viewMode 전이 helper. file type 별 mode set 분기 통합.
 * - markdown : rendered ↔ source (2-mode binary).
 * - html     : rendered → interactive → source → rendered (3-mode cyclic).
 * - 그 외    : noop (호출 site 가 isToggleableFileType 으로 미리 가드).
 *
 * DocumentNode + MaximizedItemModal 의 mode 전이 single source of truth —
 * 두 컴포넌트의 toggle 동작 drift 방지.
 */
export function getNextViewMode(current: DocumentViewMode, fileType: string): DocumentViewMode {
  if (fileType === 'html') {
    if (current === 'rendered') return 'interactive';
    if (current === 'interactive') return 'source';
    return 'rendered';
  }
  // markdown + 기타 toggleable: rendered ↔ source 2-mode (interactive 사용 X).
  return current === 'source' ? 'rendered' : 'source';
}

/**
 * ADR-0037 D4 — 토글 button 의 next-mode 라벨 (tooltip / aria-label).
 * icon 분기와 짝 — 사용자가 클릭 시 어디로 가는지 예측 가능.
 */
export function getNextViewModeLabel(current: DocumentViewMode, fileType: string): string {
  const next = getNextViewMode(current, fileType);
  if (next === 'source') return 'Show source';
  if (next === 'interactive') return 'Run interactively';
  return 'Show rendered';
}

/**
 * ADR-0037 D3 — interactive mode 의 sandboxed iframe flag.
 * - allow-scripts : script 실행 (본 mode 의 핵심).
 * - allow-popups  : <a target="_blank"> 의 새 탭 link 동작.
 * 의도적으로 미포함:
 * - allow-same-origin : parent origin 의 cookie / localStorage / DOM 격리 (XSS 방어).
 * - allow-top-navigation : parent location 변경 차단 (clickjacking 방어).
 * - allow-forms / allow-modals / allow-downloads / allow-pointer-lock : 1단계 scope 밖.
 */
export const INTERACTIVE_IFRAME_SANDBOX = 'allow-scripts allow-popups';

/**
 * ADR-0037 R4 (2단계 land 2026-05-22) — iframe content height postMessage tag.
 * parent 가 본 tag 의 message 만 accept (다른 source 의 message 는 무시).
 */
export const IFRAME_HEIGHT_MESSAGE_TAG = '__gtmuxIframeHeight';

/**
 * ADR-0037 R4 — iframe 안에 inject 할 inline probe script.
 * ResizeObserver 로 content height 측정 + parent.postMessage 로 통보.
 * sandbox="allow-scripts" 안에서만 동작 (script 비활성 시 noop — fallback 100%).
 *
 * 보안: postMessage 의 target origin = '*' — sandbox 의 unique opaque origin
 * 이라 specific origin 명시 불가. parent 가 message source 검증 (e.source ===
 * iframeRef.contentWindow) 으로 다른 page 의 postMessage 차단.
 */
const IFRAME_HEIGHT_PROBE = `<script>(function(){
document.addEventListener('contextmenu',function(e){e.preventDefault();},{capture:true});
var post=function(){try{parent.postMessage({${JSON.stringify(IFRAME_HEIGHT_MESSAGE_TAG)}:document.documentElement.scrollHeight},'*');}catch(e){}};
if(typeof ResizeObserver!=='undefined'){new ResizeObserver(post).observe(document.documentElement);}
window.addEventListener('load',post);
document.addEventListener('DOMContentLoaded',post);
setTimeout(post,50);setTimeout(post,500);
})();</script>`;

/**
 * ADR-0037 R4 — interactive mode 의 iframe srcdoc 빌더.
 * raw HTML 의 </body> 직전 (또는 끝) 에 height probe inject.
 *
 * 주의: 사용자 HTML 안에 이미 <script> 가 있어도 probe 와 충돌 X (둘 다 같은
 * sandbox 에서 동작). 사용자 script 가 postMessage 의 IFRAME_HEIGHT_MESSAGE_TAG
 * 키를 *직접* 보내면 parent 가 잘못된 height 받음 — 그래도 sandbox 격리 안
 * 깨짐 (height 만 어색하게 변함). 의도된 사용자 input 의 자유.
 */
export function buildInteractiveSrcdoc(raw: string): string {
  if (raw.length === 0) return IFRAME_HEIGHT_PROBE;
  if (raw.includes('</body>')) return raw.replace('</body>', IFRAME_HEIGHT_PROBE + '</body>');
  if (raw.includes('</html>')) return raw.replace('</html>', IFRAME_HEIGHT_PROBE + '</html>');
  return raw + IFRAME_HEIGHT_PROBE;
}
