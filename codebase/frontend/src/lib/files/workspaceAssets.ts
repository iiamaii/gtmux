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
