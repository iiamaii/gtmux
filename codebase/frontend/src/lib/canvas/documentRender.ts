// documentRender — Document item 의 markdown / html rendering helper.
//
// 정본:
// - ADR-0018 D10 amend ③ (2026-05-21) — marked + DOMPurify 도입.
// - ADR-0018 D10 amend ④ (2026-05-21) — HTML source/rendered toggle + helper 외부화.
//
// 분기:
// - file type = markdown → marked.parse(raw) → DOMPurify.sanitize → HTML string.
// - file type = html     → DOMPurify.sanitize(raw) → HTML string (raw 가 이미 HTML).
// - file type = 기타     → 단순 escape 된 source 그대로.
//
// DocumentNode (normal) 와 MaximizedItemModal (maximize) 양쪽이 같은 helper 사용
// — rendering 동기화 보장. 양쪽 코드 중복 방지.

import { marked } from 'marked';
import DOMPurify from 'dompurify';

marked.setOptions({ gfm: true, breaks: false });

const PURIFY_OPTIONS = { USE_PROFILES: { html: true } } as const;

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

/** Document view mode for HTML / markdown source vs rendered toggle. */
export type DocumentViewMode = 'rendered' | 'source';

/** File type label → whether source/rendered toggle 가 의미 있는지. */
export function isToggleableFileType(label: string): boolean {
  return label === 'markdown' || label === 'html';
}
