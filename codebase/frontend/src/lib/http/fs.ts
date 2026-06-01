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

export interface ListDirOptions {
  /** ADR-0035 D7 — per-request override of `Settings.picker_show_hidden`.
   * `undefined` → use Settings default; `true`/`false` → override. */
  showHidden?: boolean;
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
