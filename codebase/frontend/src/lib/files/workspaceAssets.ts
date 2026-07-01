export const WORKSPACE_FILE_DRAG_MIME = 'application/x-gtmux-workspace-file';

export const IMAGE_EXTENSIONS = [
  '.png',
  '.jpg',
  '.jpeg',
  '.gif',
  '.webp',
  '.svg',
  '.bmp',
  '.ico',
  '.avif',
] as const;

export const DOCUMENT_EXTENSIONS = [
  '.md',
  '.markdown',
  '.pdf',
  '.txt',
  '.text',
  '.log',
  '.json',
  '.html',
  '.htm',
  '.css',
  '.js',
  '.jsx',
  '.ts',
  '.tsx',
] as const;

export type WorkspaceMaterializationType = 'image' | 'document' | 'file_path';
export type WorkspaceEntryKind = 'file' | 'directory';
export type WorkspacePreviewKind = 'empty' | 'image' | 'pdf' | 'markdown' | 'html' | 'text';

export interface WorkspaceFilePreviewMeta {
  kind: WorkspacePreviewKind;
  shikiLang: string;
  fileTypeLabel: string;
  chipClass: 'img' | 'pdf' | 'md' | 'code' | 'file';
}

export interface WorkspaceFileDragPayload {
  files: WorkspaceDraggedFile[];
}

export interface WorkspaceDraggedFile {
  path: string;
  rootPath: string;
  name: string;
  kind: WorkspaceEntryKind;
  sizeBytes: number | null;
}

export function basename(path: string): string {
  return path.split('/').filter(Boolean).pop() ?? path;
}

export function extension(path: string): string {
  const name = basename(path).toLowerCase();
  const dot = name.lastIndexOf('.');
  return dot < 0 ? '' : name.slice(dot);
}

export function fileStem(path: string): string {
  const name = basename(path);
  const dot = name.lastIndexOf('.');
  return dot <= 0 ? name : name.slice(0, dot);
}

export function isImagePath(path: string): boolean {
  const ext = extension(path);
  return IMAGE_EXTENSIONS.includes(ext as (typeof IMAGE_EXTENSIONS)[number]);
}

export function isDocumentPath(path: string): boolean {
  const ext = extension(path);
  return DOCUMENT_EXTENSIONS.includes(ext as (typeof DOCUMENT_EXTENSIONS)[number]);
}

export function materializationTypeForPath(path: string): WorkspaceMaterializationType {
  if (isImagePath(path)) return 'image';
  if (isDocumentPath(path)) return 'document';
  return 'file_path';
}

export function previewMetaForPath(path: string): WorkspaceFilePreviewMeta {
  if (path.length === 0) {
    return { kind: 'empty', shikiLang: 'text', fileTypeLabel: 'file', chipClass: 'file' };
  }
  const name = basename(path).toLowerCase();
  const exact = PREVIEW_EXACT_NAME_META[name];
  if (exact !== undefined) return exact;
  const ext = extension(path);
  return PREVIEW_EXTENSION_META[ext] ?? {
    kind: 'text',
    shikiLang: 'text',
    fileTypeLabel: ext.length > 1 ? ext.slice(1) : 'text',
    chipClass: 'file',
  };
}

export function shikiLangForPath(path: string): string {
  return previewMetaForPath(path).shikiLang;
}

export function fileTypeLabelForPath(path: string, mime?: string | null): string {
  const meta = previewMetaForPath(path);
  if (meta.fileTypeLabel !== 'file' && meta.fileTypeLabel !== 'text') return meta.fileTypeLabel;
  const cleanMime = (mime ?? '').toLowerCase();
  if (cleanMime === 'application/json') return 'json';
  if (cleanMime === 'application/pdf') return 'pdf';
  if (cleanMime.startsWith('text/html')) return 'html';
  if (cleanMime.startsWith('text/markdown')) return 'markdown';
  if (cleanMime.startsWith('text/')) return 'text';
  return meta.fileTypeLabel;
}

/**
 * Resolve the Shiki language for a document's `source` view. Shared by
 * `DocumentNode` and `MaximizedItemModal` so the two source surfaces stay in
 * sync (single source — ADR-0037 D7 / ADR-0018 D10 amend ④ drift guard).
 *
 * - inline documents default to markdown (html when the label says so).
 * - workspace-file documents resolve from the path's extension; when that is
 *   plain `text`, fall back to the human file-type label for html/markdown/json.
 */
export function sourceLangForDocument(fileName: string, label: string, inline: boolean): string {
  if (inline) return label === 'html' ? 'html' : 'markdown';
  const mapped = shikiLangForPath(fileName);
  if (mapped !== 'text') return mapped;
  if (label === 'html') return 'html';
  if (label === 'markdown') return 'markdown';
  if (label === 'json') return 'json';
  return mapped;
}

export function guessMimeFromPath(path: string): string {
  switch (extension(path)) {
    case '.png':
      return 'image/png';
    case '.jpg':
    case '.jpeg':
      return 'image/jpeg';
    case '.gif':
      return 'image/gif';
    case '.webp':
      return 'image/webp';
    case '.svg':
      return 'image/svg+xml';
    case '.bmp':
      return 'image/bmp';
    case '.ico':
      return 'image/x-icon';
    case '.avif':
      return 'image/avif';
    case '.pdf':
      return 'application/pdf';
    case '.md':
    case '.markdown':
      return 'text/markdown';
    case '.html':
    case '.htm':
      return 'text/html';
    case '.json':
      return 'application/json';
    case '.css':
      return 'text/css';
    case '.js':
    case '.jsx':
      return 'text/javascript';
    case '.ts':
    case '.tsx':
      return 'text/typescript';
    case '.txt':
    case '.text':
    case '.log':
      return 'text/plain';
    default:
      return '';
  }
}

const PREVIEW_EXTENSION_META: Record<string, WorkspaceFilePreviewMeta> = {
  '.png': imageMeta(),
  '.jpg': imageMeta(),
  '.jpeg': imageMeta(),
  '.gif': imageMeta(),
  '.webp': imageMeta(),
  '.svg': imageMeta(),
  '.bmp': imageMeta(),
  '.ico': imageMeta(),
  '.avif': imageMeta(),
  '.pdf': { kind: 'pdf', shikiLang: 'text', fileTypeLabel: 'pdf', chipClass: 'pdf' },
  '.md': { kind: 'markdown', shikiLang: 'markdown', fileTypeLabel: 'markdown', chipClass: 'md' },
  '.markdown': { kind: 'markdown', shikiLang: 'markdown', fileTypeLabel: 'markdown', chipClass: 'md' },
  '.html': { kind: 'html', shikiLang: 'html', fileTypeLabel: 'html', chipClass: 'code' },
  '.htm': { kind: 'html', shikiLang: 'html', fileTypeLabel: 'html', chipClass: 'code' },
  '.txt': textMeta('text'),
  '.text': textMeta('text'),
  '.log': textMeta('log'),
  '.json': codeMeta('json', 'json'),
  '.jsonc': codeMeta('jsonc', 'json'),
  '.css': codeMeta('css', 'css'),
  '.scss': codeMeta('scss', 'scss'),
  '.sass': codeMeta('sass', 'sass'),
  '.less': codeMeta('less', 'less'),
  '.js': codeMeta('javascript', 'javascript'),
  '.jsx': codeMeta('jsx', 'javascript'),
  '.mjs': codeMeta('javascript', 'javascript'),
  '.cjs': codeMeta('javascript', 'javascript'),
  '.ts': codeMeta('typescript', 'typescript'),
  '.tsx': codeMeta('tsx', 'typescript'),
  '.svelte': codeMeta('svelte', 'svelte'),
  '.vue': codeMeta('vue', 'vue'),
  '.rs': codeMeta('rust', 'rust'),
  '.toml': codeMeta('toml', 'toml'),
  '.yaml': codeMeta('yaml', 'yaml'),
  '.yml': codeMeta('yaml', 'yaml'),
  '.py': codeMeta('python', 'python'),
  '.pyw': codeMeta('python', 'python'),
  '.go': codeMeta('go', 'go'),
  '.java': codeMeta('java', 'java'),
  '.kt': codeMeta('kotlin', 'kotlin'),
  '.kts': codeMeta('kotlin', 'kotlin'),
  '.swift': codeMeta('swift', 'swift'),
  '.c': codeMeta('c', 'c'),
  '.h': codeMeta('c', 'c'),
  '.cpp': codeMeta('cpp', 'cpp'),
  '.cc': codeMeta('cpp', 'cpp'),
  '.cxx': codeMeta('cpp', 'cpp'),
  '.hpp': codeMeta('cpp', 'cpp'),
  '.hh': codeMeta('cpp', 'cpp'),
  '.rb': codeMeta('ruby', 'ruby'),
  '.php': codeMeta('php', 'php'),
  '.sh': codeMeta('bash', 'shell'),
  '.bash': codeMeta('bash', 'shell'),
  '.zsh': codeMeta('zsh', 'shell'),
  '.fish': codeMeta('fish', 'shell'),
  '.sql': codeMeta('sql', 'sql'),
  '.xml': codeMeta('xml', 'xml'),
  '.ini': codeMeta('ini', 'ini'),
  '.conf': codeMeta('ini', 'conf'),
  '.env': codeMeta('dotenv', 'env'),
  '.dockerfile': codeMeta('dockerfile', 'dockerfile'),
  '.diff': codeMeta('diff', 'diff'),
  '.patch': codeMeta('diff', 'diff'),
  '.lua': codeMeta('lua', 'lua'),
  '.r': codeMeta('r', 'r'),
  '.dart': codeMeta('dart', 'dart'),
  '.ex': codeMeta('elixir', 'elixir'),
  '.exs': codeMeta('elixir', 'elixir'),
  '.erl': codeMeta('erlang', 'erlang'),
  '.hrl': codeMeta('erlang', 'erlang'),
  '.cs': codeMeta('csharp', 'csharp'),
};

const PREVIEW_EXACT_NAME_META: Record<string, WorkspaceFilePreviewMeta> = {
  dockerfile: codeMeta('dockerfile', 'dockerfile'),
  makefile: codeMeta('make', 'makefile'),
  'cmakelists.txt': codeMeta('cmake', 'cmake'),
  gemfile: codeMeta('ruby', 'ruby'),
  rakefile: codeMeta('ruby', 'ruby'),
};

function imageMeta(): WorkspaceFilePreviewMeta {
  return { kind: 'image', shikiLang: 'text', fileTypeLabel: 'image', chipClass: 'img' };
}

function textMeta(label: string): WorkspaceFilePreviewMeta {
  return { kind: 'text', shikiLang: 'text', fileTypeLabel: label, chipClass: 'file' };
}

function codeMeta(shikiLang: string, label: string): WorkspaceFilePreviewMeta {
  return { kind: 'text', shikiLang, fileTypeLabel: label, chipClass: 'code' };
}

export function isPathWithinRoot(path: string, root: string): boolean {
  const cleanRoot = normalizeWorkspaceRoot(root);
  if (cleanRoot === null) return false;
  if (cleanRoot === '/') return path.startsWith('/');
  return path === cleanRoot || path.startsWith(`${cleanRoot}/`);
}

export function workspaceRelativePath(root: string, absolutePath: string): string | null {
  if (!isPathWithinRoot(absolutePath, root)) return null;
  const cleanRoot = normalizeWorkspaceRoot(root);
  if (cleanRoot === null || absolutePath === cleanRoot) return null;
  const prefix = cleanRoot === '/' ? '/' : `${cleanRoot}/`;
  const relative = absolutePath.slice(prefix.length);
  return isSafeWorkspaceRelativePath(relative) ? relative : null;
}

export function resolveWorkspacePath(root: string, relativePath: string): string | null {
  const cleanRoot = normalizeWorkspaceRoot(root);
  if (cleanRoot === null || !isSafeWorkspaceRelativePath(relativePath)) return null;
  return cleanRoot === '/' ? `/${relativePath}` : `${cleanRoot}/${relativePath}`;
}

function normalizeWorkspaceRoot(root: string): string | null {
  if (root.length === 0) return null;
  return root.replace(/\/+$/, '') || '/';
}

export function isSafeWorkspaceRelativePath(path: string): boolean {
  if (path.length === 0 || path.startsWith('/') || path.includes('\0')) return false;
  return path.split('/').every((segment) =>
    segment.length > 0 && segment !== '.' && segment !== '..',
  );
}

/**
 * Characters that make a path unsafe to paste raw into a POSIX shell — a space
 * or any shell-special glob/quoting/expansion metacharacter (ADR-0047 D4 amend
 * 2026-06-30). When a path contains any of these we wrap it in single quotes so
 * the terminal receives one inert word (no word-splitting / glob / expansion).
 * Includes whitespace (space, tab, newline) plus the shell metacharacter set.
 */
// eslint-disable-next-line no-control-regex
const SHELL_SPECIAL_RE = /[\s'"\\$(){}\[\]*?;&|<>`!~# ]/;

/**
 * Quote a single path for safe insertion as terminal *input* (text only — never
 * executed by us). Plain paths pass through unchanged; paths containing a space
 * or any shell-special character are wrapped in POSIX single quotes with the
 * standard `'\''` escape for embedded single quotes. Mirrors macOS Terminal's
 * "drag file into terminal" behaviour (ADR-0047 D4 amend 2026-06-30).
 */
export function posixQuotePath(path: string): string {
  if (path.length === 0) return "''";
  if (!SHELL_SPECIAL_RE.test(path)) return path;
  return `'${path.replace(/'/g, "'\\''")}'`;
}

/**
 * Format one or more dragged file paths into the text to inject into a
 * terminal's PTY: each path quoted per `posixQuotePath`, joined by a single
 * space, with one trailing space appended (and NO trailing newline — the user
 * reviews before running). ADR-0047 D4 amend 2026-06-30.
 */
export function formatPathsForTerminalInput(paths: readonly string[]): string {
  if (paths.length === 0) return '';
  return `${paths.map(posixQuotePath).join(' ')} `;
}

export function encodeWorkspaceFileDragPayload(
  files: readonly WorkspaceDraggedFile[],
): string {
  return JSON.stringify({ files });
}

export function parseWorkspaceFileDragPayload(raw: string): WorkspaceFileDragPayload | null {
  try {
    const parsed = JSON.parse(raw) as Partial<WorkspaceFileDragPayload>;
    if (!Array.isArray(parsed.files)) return null;
    const files: WorkspaceDraggedFile[] = [];
    for (const file of parsed.files) {
      if (
        typeof file?.path !== 'string' ||
        typeof file.rootPath !== 'string' ||
        typeof file.name !== 'string'
      ) {
        return null;
      }
      files.push({
        path: file.path,
        rootPath: file.rootPath,
        name: file.name,
        kind: file.kind === 'directory' ? 'directory' : 'file',
        sizeBytes: typeof file.sizeBytes === 'number' ? file.sizeBytes : null,
      });
    }
    return { files };
  } catch {
    return null;
  }
}
