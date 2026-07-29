import { describe, it, expect } from 'vitest';
import {
  classifyWebViewSource,
  normalizeWebViewInput,
  type WebViewSourceKind,
} from './webViewSource';

describe('classifyWebViewSource', () => {
  const cases: Array<[string, WebViewSourceKind]> = [
    // remote
    ['https://example.com', 'remote'],
    ['http://localhost:5173/preview', 'remote'],
    ['HTTPS://EXAMPLE.COM', 'remote'],
    // local by extension (case-insensitive)
    ['index.html', 'local-html'],
    ['sub/dir/page.HTM', 'local-html'],
    ['notes/readme.md', 'local-md'],
    ['CHANGELOG.MARKDOWN', 'local-md'],
    ['assets/pic.png', 'local-image'],
    ['a/b/photo.JPEG', 'local-image'],
    ['icons/logo.svg', 'local-image'],
    // safe relative but not renderable
    ['data/report.csv', 'unsupported'],
    ['bin/tool', 'unsupported'],
    // invalid
    ['', 'invalid'],
    ['   ', 'invalid'],
    ['../secret.md', 'invalid'],
    ['/etc/passwd', 'invalid'],
    ['javascript:alert(1)', 'invalid'],
    ['file:///etc/passwd', 'invalid'],
    ['data:text/html,<h1>x', 'invalid'],
    ['//evil.com/x', 'invalid'],
    ['a/./b.md', 'invalid'],
  ];
  for (const [url, kind] of cases) {
    it(`classifies ${JSON.stringify(url)} → ${kind}`, () => {
      expect(classifyWebViewSource(url).kind).toBe(kind);
    });
  }
});

describe('normalizeWebViewInput', () => {
  const cases: Array<[string, string]> = [
    // bare domain → https://
    ['example.com', 'https://example.com'],
    ['example.com/path', 'https://example.com/path'],
    ['example.com:8080', 'https://example.com:8080'],
    ['sub.domain.io/a/b', 'https://sub.domain.io/a/b'],
    ['  example.com  ', 'https://example.com'],
    // loopback dev-server host → http:// (not https)
    ['localhost:5173', 'http://localhost:5173'],
    ['127.0.0.1:5173', 'http://127.0.0.1:5173'],
    ['[::1]:8080', 'http://[::1]:8080'],
    ['localhost', 'http://localhost'],
    ['127.0.0.1', 'http://127.0.0.1'],
    ['localhost:5173/app', 'http://localhost:5173/app'],
    // loopback host wins over the .html/.md/img workspace-file heuristic
    ['127.0.0.1:9251/page.html', 'http://127.0.0.1:9251/page.html'],
    ['localhost:5173/index.html', 'http://localhost:5173/index.html'],
    ['[::1]:8080/a.md', 'http://[::1]:8080/a.md'],
    // loopback look-alikes still fall through to https bare-domain rule
    ['127.0.0.1.evil.com', 'https://127.0.0.1.evil.com'],
    // already schemed → untouched (explicit scheme always wins)
    ['https://example.com', 'https://example.com'],
    ['http://localhost:5173', 'http://localhost:5173'],
    ['https://localhost:5173', 'https://localhost:5173'],
    ['javascript:alert(1)', 'javascript:alert(1)'],
    ['ftp://host/file', 'ftp://host/file'],
    // file-looking relative path with known ext → untouched
    ['notes/readme.md', 'notes/readme.md'],
    ['pic.png', 'pic.png'],
    ['sub/index.html', 'sub/index.html'],
    // path-ish / whitespace / dot-less → untouched
    ['/abs/path', '/abs/path'],
    ['//scheme-relative', '//scheme-relative'],
    ['my file.md', 'my file.md'],
    ['some text with spaces', 'some text with spaces'],
    ['README', 'README'],
    ['', ''],
  ];
  for (const [raw, out] of cases) {
    it(`normalizes ${JSON.stringify(raw)} → ${JSON.stringify(out)}`, () => {
      expect(normalizeWebViewInput(raw)).toBe(out);
    });
  }
});
