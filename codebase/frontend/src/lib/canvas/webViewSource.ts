// webViewSource — pure address classification + input normalization for the
// Web View node (ADR-0059 D2 render matrix + D8 scheme auto-fix).
//
// 정본:
// - ADR-0059 D2 — 주소 종류 → 렌더 표면 매핑 (remote / local-html / local-md /
//   local-image / unsupported / invalid).
// - ADR-0059 D8 — change 모달의 스킴-없는 도메인 입력 자동 보정 (FE UX 계층만;
//   BE 검증은 엄격 유지).
//
// 순수 함수 — DOM/네트워크 접근 없음. WebViewNode / ChangeWebViewModal /
// MaximizedItemModal 이 공유하는 단일 분류 소스 (drift 방지).

import { extension, isSafeWorkspaceRelativePath } from '$lib/files/workspaceAssets';

/** ADR-0059 D2 render surface kind for a web_view `url`. */
export type WebViewSourceKind =
  | 'remote' // http(s):// absolute URL → <iframe src>
  | 'local-html' // workspace .html/.htm → fetch → srcdoc sandbox
  | 'local-md' // workspace .md/.markdown → markdown render pipeline
  | 'local-image' // workspace image → <img>
  | 'unsupported' // safe relative path but not a renderable type
  | 'invalid'; // empty / traversal / bad scheme / malformed

export interface WebViewSource {
  kind: WebViewSourceKind;
}

const REMOTE_RE = /^https?:\/\//i;

/** Local file extensions the web_view can render (case-insensitive). */
const LOCAL_HTML_EXTS = ['.html', '.htm'];
const LOCAL_MD_EXTS = ['.md', '.markdown'];
const LOCAL_IMAGE_EXTS = [
  '.png', '.jpg', '.jpeg', '.gif', '.webp', '.svg', '.bmp', '.ico', '.avif',
];
/** All extensions that mark an input as an existing-file-looking path (D8 —
 *  blocks bare-domain auto-fix so `pic.png` is never turned into a URL). */
const KNOWN_LOCAL_EXTS = new Set([
  ...LOCAL_HTML_EXTS,
  ...LOCAL_MD_EXTS,
  ...LOCAL_IMAGE_EXTS,
]);

/**
 * Classify a stored web_view `url` into its render surface (ADR-0059 D2).
 *
 * - `http(s)://…`               → remote
 * - clean workspace-relative .html/.htm   → local-html
 * - clean workspace-relative .md/.markdown → local-md
 * - clean workspace-relative image         → local-image
 * - clean workspace-relative other         → unsupported
 * - empty / `..` traversal / abs / bad     → invalid
 *
 * The stored value is BE-validated (scheme allowlist + clean-relative guard),
 * so `invalid` here is a defensive surface for legacy/edge records.
 */
export function classifyWebViewSource(rawUrl: string): WebViewSource {
  const url = rawUrl.trim();
  if (url.length === 0) return { kind: 'invalid' };
  if (REMOTE_RE.test(url)) return { kind: 'remote' };
  // Any other scheme (javascript:/file:/data:/scheme-relative //…) is rejected.
  if (/^[a-z][a-z0-9+.-]*:/i.test(url) || url.startsWith('//')) {
    return { kind: 'invalid' };
  }
  // Treat as a workspace(B)-relative file path — form guard first.
  if (!isSafeWorkspaceRelativePath(url)) return { kind: 'invalid' };
  const ext = extension(url);
  if (LOCAL_HTML_EXTS.includes(ext)) return { kind: 'local-html' };
  if (LOCAL_MD_EXTS.includes(ext)) return { kind: 'local-md' };
  if (LOCAL_IMAGE_EXTS.includes(ext)) return { kind: 'local-image' };
  return { kind: 'unsupported' };
}

/**
 * Loopback host forms (with an optional `:port`) that a dev server binds to.
 * These are auto-prefixed with **`http://`** (not https) because local dev
 * servers speak plain http — an https:// prefix yields an SSL/connection-error
 * page that reads as "blocked". Matches:
 *  - `localhost`
 *  - `127.0.0.0/8` dotted quads (`127.x.x.x`)
 *  - `[::1]` (IPv6 loopback, bracketed)
 * The whole host must match (anchored) so `127.0.0.1.evil.com` / `localhostx`
 * fall through to the ordinary bare-domain https rule.
 */
const LOOPBACK_HOST_RE = /^(localhost|127(?:\.\d{1,3}){3}|\[::1\])(?::\d+)?$/i;

/**
 * Normalize a change-modal input before commit (ADR-0059 D8).
 *
 * Conservative scheme auto-fix. A bare domain like `example.com[/path]` gets
 * `https://` prepended; a loopback host like `127.0.0.1:5173`, `localhost:5173`,
 * or `[::1]:8080` gets **`http://`** prepended (local dev servers are http, not
 * https — see LOOPBACK_HOST_RE). Left untouched:
 *  - anything that already carries a scheme (`http(s)://`, `javascript:`, …) —
 *    an explicit scheme always wins; BE validation stays the authority.
 *  - whitespace-bearing input (never a bare domain).
 *  - an absolute path (`/…`) or scheme-relative (`//…`).
 *  - a file-looking relative path with a known local extension (`notes/x.md`,
 *    `pic.png`) — that is a workspace file reference, not a domain.
 *  - dot-less, non-loopback input (`README`) — not a domain by this heuristic.
 *
 * Returns the (possibly rewritten) value; the caller shows the normalized form
 * so the user sees exactly what will be committed.
 */
export function normalizeWebViewInput(raw: string): string {
  const url = raw.trim();
  if (url.length === 0) return '';
  // Already has a scheme (`scheme://…`) or a non-network scheme (`javascript:`,
  // `data:`, `mailto:`, `file:`…) — leave verbatim (BE validates/rejects).
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(url)) return url;
  if (/^(javascript|data|file|mailto|tel|blob|about|vbscript|ftp|ws|wss):/i.test(url)) {
    return url;
  }
  // Path-ish or whitespace-bearing → not a bare domain.
  if (url.startsWith('/') || url.startsWith('//') || /\s/.test(url)) return url;
  // Host part = everything before the first `/`.
  const host = url.split('/')[0] ?? url;
  // Loopback dev-server host → http:// (local servers are plain http). Checked
  // BEFORE the workspace-file heuristic so `127.0.0.1:5173/app.html` is treated
  // as a dev URL, not a workspace file — a loopback host is never a local path.
  if (LOOPBACK_HOST_RE.test(host)) return `http://${url}`;
  // File-looking relative path with a known local extension → workspace file.
  if (KNOWN_LOCAL_EXTS.has(extension(url))) return url;
  // Bare domain heuristic: has a dot in the host part.
  if (host.includes('.')) return `https://${url}`;
  return url;
}
