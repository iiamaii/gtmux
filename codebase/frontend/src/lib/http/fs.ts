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
