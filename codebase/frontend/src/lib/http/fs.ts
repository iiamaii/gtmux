// File system picker — ADR-0035 / 0061.
//
// MVP scope: workspace root only. External roots (ADR-0035 D2.1 picker.roots)
// land in Stage 3 with the toml schema mutation.

import { UnauthorizedError } from './sessions';

export interface FsEntry {
  name: string;
  kind: 'file' | 'directory';
  size_bytes: number | null;
  mtime_unix: number | null;
}

export interface FsListResponse {
  dir: string;
  parent: string | null;
  entries: FsEntry[];
  total: number;
  truncated: boolean;
}

export class DirNotAllowedError extends Error {
  constructor(message = 'Directory is outside the workspace.') {
    super(message);
    this.name = 'DirNotAllowedError';
  }
}

export class DirNotFoundError extends Error {
  constructor(message = 'Directory not found.') {
    super(message);
    this.name = 'DirNotFoundError';
  }
}

export class FsApiUnavailableError extends Error {
  constructor(message = 'Workspace file operations are not available on this server.') {
    super(message);
    this.name = 'FsApiUnavailableError';
  }
}

export class DirAlreadyExistsError extends Error {
  constructor(message = 'Directory already exists.') {
    super(message);
    this.name = 'DirAlreadyExistsError';
  }
}

export class DirNotEmptyError extends Error {
  constructor(message = 'Directory is not empty.') {
    super(message);
    this.name = 'DirNotEmptyError';
  }
}

export class FsNameConflictError extends Error {
  constructor(message = 'A file with that name already exists.') {
    super(message);
    this.name = 'FsNameConflictError';
  }
}

export class FsPayloadTooLargeError extends Error {
  constructor(message = 'File is too large.') {
    super(message);
    this.name = 'FsPayloadTooLargeError';
  }
}

export class FsUnsupportedMimeError extends Error {
  constructor(message = 'File type is not supported.') {
    super(message);
    this.name = 'FsUnsupportedMimeError';
  }
}

export class FsAlreadyExistsError extends Error {
  constructor(message = 'A file or folder with that name already exists.') {
    super(message);
    this.name = 'FsAlreadyExistsError';
  }
}

export class FsNotFoundError extends Error {
  constructor(message = 'File or folder not found.') {
    super(message);
    this.name = 'FsNotFoundError';
  }
}

export class FsInvalidNameError extends Error {
  constructor(message = 'Name is not valid.') {
    super(message);
    this.name = 'FsInvalidNameError';
  }
}

export class FsInvalidRequestError extends Error {
  constructor(message = 'File operation request is not valid.') {
    super(message);
    this.name = 'FsInvalidRequestError';
  }
}

export class FsMoveCycleError extends Error {
  constructor(message = 'Cannot move a folder into itself.') {
    super(message);
    this.name = 'FsMoveCycleError';
  }
}

export interface ListDirOptions {
  /** ADR-0035 D7 — per-request override of `Settings.picker_show_hidden`.
   * `undefined` → use Settings default; `true`/`false` → override. */
  showHidden?: boolean;
}

export type UploadConflictPolicy = 'reject' | 'rename' | 'overwrite';

export interface UploadedFsFile {
  path: string;
  name: string;
  mime: string;
  size: number;
  conflict: boolean;
}

export interface UploadFsResponse {
  files: UploadedFsFile[];
}

export interface RenameFsResponse {
  path: string;
  name: string;
  kind: 'file' | 'directory';
}

export type CopyConflictPolicy = 'reject' | 'rename' | 'overwrite';

export interface CopyFsEntry {
  source: string;
  path: string;
  name: string;
  kind: 'file' | 'directory';
}

export interface CopyFsResponse {
  entries: CopyFsEntry[];
}

export type MoveConflictPolicy = 'reject' | 'rename';

export interface MoveFsEntry {
  source: string;
  path: string;
  name: string;
  kind: 'file' | 'directory';
}

export interface MoveFsResponse {
  entries: MoveFsEntry[];
}

/** `GET /api/fs/list?dir=<percent-encoded>`. Empty `dir` = workspace root. */
export async function listDir(dir: string, options: ListDirOptions = {}): Promise<FsListResponse> {
  const qs = new URLSearchParams({ dir });
  if (options.showHidden !== undefined) {
    qs.set('show_hidden', String(options.showHidden));
  }
  const res = await fetch(`/api/fs/list?${qs}`, {
    method: 'GET',
    headers: { Accept: 'application/json' },
    credentials: 'include',
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 403) throw new DirNotAllowedError();
  if (res.status === 404) throw new DirNotFoundError();
  if (!res.ok) throw new Error(`GET /api/fs/list returned ${res.status}`);
  return res.json() as Promise<FsListResponse>;
}

async function errorBodyCode(res: Response): Promise<string | null> {
  const body = await res.clone().json().catch(() => null) as { error?: unknown } | null;
  return typeof body?.error === 'string' ? body.error : null;
}

async function errorMessage(res: Response, prefix: string): Promise<string> {
  const text = await res.text().catch(() => '');
  if (text.trim().length === 0) return `${prefix} ${res.status}`;
  try {
    const body = JSON.parse(text) as { error?: unknown; message?: unknown; reason?: unknown };
    const code = typeof body.error === 'string' ? body.error : null;
    const message = typeof body.message === 'string'
      ? body.message
      : typeof body.reason === 'string'
        ? body.reason
        : null;
    if (code !== null && message !== null) return `${prefix} ${res.status}: ${code}: ${message}`;
    if (code !== null) return `${prefix} ${res.status}: ${code}`;
  } catch {
    // Fall through.
  }
  return `${prefix} ${res.status}: ${text.slice(0, 500)}`;
}

async function mutateDir(endpoint: 'mkdir' | 'rmdir', path: string): Promise<void> {
  const res = await fetch(`/api/fs/${endpoint}`, {
    method: 'POST',
    headers: {
      Accept: 'application/json',
      'Content-Type': 'application/json',
    },
    credentials: 'include',
    body: JSON.stringify({ path }),
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 403) throw new DirNotAllowedError();
  if (res.status === 404 || res.status === 405) throw new FsApiUnavailableError();
  if (res.status === 409) {
    const body = await res.json().catch(() => ({})) as { error?: string };
    if (body.error === 'dir_not_empty') throw new DirNotEmptyError();
    throw new DirAlreadyExistsError();
  }
  if (!res.ok) throw new Error(await errorMessage(res, `POST /api/fs/${endpoint} returned`));
}

export async function mkdirFs(path: string): Promise<void> {
  await mutateDir('mkdir', path);
}

export async function rmdirFs(path: string): Promise<void> {
  await mutateDir('rmdir', path);
}

export async function renameFs(path: string, newName: string): Promise<RenameFsResponse> {
  const res = await fetch('/api/fs/rename', {
    method: 'POST',
    headers: {
      Accept: 'application/json',
      'Content-Type': 'application/json',
    },
    credentials: 'include',
    body: JSON.stringify({ path, new_name: newName }),
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 400) {
    const code = await errorBodyCode(res);
    if (code === 'invalid_name') {
      throw new FsInvalidNameError(await errorMessage(res, 'Rename failed'));
    }
  }
  if (res.status === 403) throw new DirNotAllowedError();
  if (res.status === 404) {
    const code = await errorBodyCode(res);
    if (code === 'not_found') throw new FsNotFoundError();
    throw new FsApiUnavailableError();
  }
  if (res.status === 405) throw new FsApiUnavailableError();
  if (res.status === 409) {
    const code = await errorBodyCode(res);
    if (code === 'already_exists') {
      throw new FsAlreadyExistsError(await errorMessage(res, 'Rename conflict'));
    }
    throw new FsAlreadyExistsError();
  }
  if (!res.ok) throw new Error(await errorMessage(res, 'POST /api/fs/rename returned'));

  const body = await res.json() as Partial<RenameFsResponse>;
  if (
    typeof body.path !== 'string' ||
    typeof body.name !== 'string' ||
    (body.kind !== 'file' && body.kind !== 'directory')
  ) {
    throw new Error('POST /api/fs/rename response missing path/name/kind');
  }
  return { path: body.path, name: body.name, kind: body.kind };
}

export async function removeFs(path: string): Promise<void> {
  const res = await fetch('/api/fs/remove', {
    method: 'POST',
    headers: {
      Accept: 'application/json',
      'Content-Type': 'application/json',
    },
    credentials: 'include',
    body: JSON.stringify({ path }),
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 403) throw new DirNotAllowedError();
  if (res.status === 404) {
    const code = await errorBodyCode(res);
    if (code === 'not_found') throw new FsNotFoundError();
    throw new FsApiUnavailableError();
  }
  if (res.status === 405) throw new FsApiUnavailableError();
  if (res.status === 409) {
    const code = await errorBodyCode(res);
    if (code === 'dir_not_empty') throw new DirNotEmptyError();
  }
  if (!res.ok) throw new Error(await errorMessage(res, 'POST /api/fs/remove returned'));
}

export async function copyFs(
  sources: readonly string[],
  destDir: string,
  onConflict: CopyConflictPolicy = 'rename',
): Promise<CopyFsResponse> {
  if (sources.length === 0) return { entries: [] };
  const res = await fetch('/api/fs/copy', {
    method: 'POST',
    headers: {
      Accept: 'application/json',
      'Content-Type': 'application/json',
    },
    credentials: 'include',
    body: JSON.stringify({ sources, dest_dir: destDir, on_conflict: onConflict }),
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 400) {
    const code = await errorBodyCode(res);
    if (code === 'invalid_name' || code === 'invalid_request') {
      throw new FsInvalidNameError(await errorMessage(res, 'Copy failed'));
    }
  }
  if (res.status === 403) throw new DirNotAllowedError();
  if (res.status === 404) {
    const code = await errorBodyCode(res);
    if (code === 'not_found') throw new FsNotFoundError();
    throw new FsApiUnavailableError();
  }
  if (res.status === 405) throw new FsApiUnavailableError();
  if (res.status === 409) {
    const code = await errorBodyCode(res);
    if (code === 'already_exists' || code === 'name_conflict') {
      throw new FsNameConflictError(await errorMessage(res, 'Copy conflict'));
    }
    throw new FsAlreadyExistsError(await errorMessage(res, 'Copy conflict'));
  }
  if (!res.ok) throw new Error(await errorMessage(res, 'POST /api/fs/copy returned'));

  const body = await res.json() as Partial<CopyFsResponse>;
  if (!Array.isArray(body.entries)) {
    throw new Error('POST /api/fs/copy response missing entries');
  }
  return {
    entries: body.entries.map((entry) => {
      if (
        typeof entry.source !== 'string' ||
        typeof entry.path !== 'string' ||
        typeof entry.name !== 'string' ||
        (entry.kind !== 'file' && entry.kind !== 'directory')
      ) {
        throw new Error('POST /api/fs/copy response entry missing source/path/name/kind');
      }
      return {
        source: entry.source,
        path: entry.path,
        name: entry.name,
        kind: entry.kind,
      };
    }),
  };
}

export async function moveFs(
  sources: readonly string[],
  destDir: string,
  onConflict: MoveConflictPolicy = 'reject',
): Promise<MoveFsResponse> {
  if (sources.length === 0) return { entries: [] };
  const res = await fetch('/api/fs/move', {
    method: 'POST',
    headers: {
      Accept: 'application/json',
      'Content-Type': 'application/json',
    },
    credentials: 'include',
    body: JSON.stringify({ sources, dest_dir: destDir, on_conflict: onConflict }),
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 400) {
    const code = await errorBodyCode(res);
    if (code === 'invalid_request' || code === 'invalid_name') {
      throw new FsInvalidRequestError(await errorMessage(res, 'Move failed'));
    }
  }
  if (res.status === 403) throw new DirNotAllowedError();
  if (res.status === 404) {
    const code = await errorBodyCode(res);
    if (code === 'not_found') throw new FsNotFoundError();
    throw new FsApiUnavailableError();
  }
  if (res.status === 405) throw new FsApiUnavailableError();
  if (res.status === 409) {
    const code = await errorBodyCode(res);
    if (code === 'move_cycle') {
      throw new FsMoveCycleError(await errorMessage(res, 'Move cycle'));
    }
    if (code === 'already_exists' || code === 'name_conflict') {
      throw new FsNameConflictError(await errorMessage(res, 'Move conflict'));
    }
    throw new FsAlreadyExistsError(await errorMessage(res, 'Move conflict'));
  }
  if (!res.ok) throw new Error(await errorMessage(res, 'POST /api/fs/move returned'));

  const body = await res.json() as Partial<MoveFsResponse>;
  if (!Array.isArray(body.entries)) {
    throw new Error('POST /api/fs/move response missing entries');
  }
  return {
    entries: body.entries.map((entry) => {
      if (
        typeof entry.source !== 'string' ||
        typeof entry.path !== 'string' ||
        typeof entry.name !== 'string' ||
        (entry.kind !== 'file' && entry.kind !== 'directory')
      ) {
        throw new Error('POST /api/fs/move response entry missing source/path/name/kind');
      }
      return {
        source: entry.source,
        path: entry.path,
        name: entry.name,
        kind: entry.kind,
      };
    }),
  };
}

export function fsFileUrl(path: string): string {
  const qs = new URLSearchParams({ path });
  return `/api/fs/file?${qs}`;
}

export async function uploadFs(
  dir: string,
  files: readonly File[],
  onConflict: UploadConflictPolicy = 'reject',
): Promise<UploadFsResponse> {
  const form = new FormData();
  form.set('dir', dir);
  form.set('on_conflict', onConflict);
  for (const file of files) {
    form.append('file', file, file.name);
  }

  const res = await fetch('/api/fs/upload', {
    method: 'POST',
    credentials: 'include',
    body: form,
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 403) throw new DirNotAllowedError();
  if (res.status === 404 || res.status === 405) throw new FsApiUnavailableError();
  if (res.status === 409) throw new FsNameConflictError(await errorMessage(res, 'Upload conflict'));
  if (res.status === 413) throw new FsPayloadTooLargeError();
  if (res.status === 415) throw new FsUnsupportedMimeError();
  if (!res.ok) throw new Error(await errorMessage(res, 'POST /api/fs/upload returned'));

  const body = await res.json() as Partial<UploadFsResponse>;
  if (!Array.isArray(body.files)) {
    throw new Error('POST /api/fs/upload response missing files');
  }
  return {
    files: body.files.map((file) => ({
      path: file.path,
      name: file.name,
      mime: file.mime,
      size: file.size,
      conflict: file.conflict,
    })),
  };
}
