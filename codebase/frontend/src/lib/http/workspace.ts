// HTTP client — workspace manifest organization.
//
// 정본:
// - ADR-0044 D-B2~B8 (workspace manifest)
// - docs/reports/2026-06-01-fe-handover-instance-and-session-org.md §1

import { UnauthorizedError, listSessions } from '$lib/http/sessions';
import type { ManifestPutBody, WorkspaceListResponse } from '$lib/types/sessions';

const JSON_HEADERS: Record<string, string> = {
  Accept: 'application/json',
  'Content-Type': 'application/json',
};

export class ManifestStaleError extends Error {
  constructor(message = 'Workspace manifest changed since it was loaded') {
    super(message);
    this.name = 'ManifestStaleError';
  }
}

async function json<T>(res: Response): Promise<T> {
  try {
    return (await res.json()) as T;
  } catch (e) {
    throw new Error(`response JSON parse failed: ${String(e)}`);
  }
}

async function responseErrorMessage(res: Response, prefix: string): Promise<string> {
  const text = await res.text().catch(() => '');
  if (text.trim().length === 0) return `${prefix} ${res.status}`;
  try {
    const body = JSON.parse(text) as { error?: unknown; message?: unknown };
    const code = typeof body.error === 'string' ? body.error : null;
    const message = typeof body.message === 'string' ? body.message : null;
    if (code !== null && message !== null) return `${prefix} ${res.status}: ${code}: ${message}`;
    if (message !== null) return `${prefix} ${res.status}: ${message}`;
  } catch {
    // Fall through to raw body snippet.
  }
  return `${prefix} ${res.status}: ${text.slice(0, 500)}`;
}

export async function getWorkspace(): Promise<WorkspaceListResponse> {
  return listSessions();
}

export async function putManifest(
  body: ManifestPutBody,
  etag: string,
): Promise<{ manifest_etag: string }> {
  const res = await fetch('/api/workspace/manifest', {
    method: 'PUT',
    headers: {
      ...JSON_HEADERS,
      'If-Match': `"${etag}"`,
    },
    credentials: 'include',
    body: JSON.stringify(body),
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 412) throw new ManifestStaleError();
  if (!res.ok) throw new Error(await responseErrorMessage(res, 'PUT manifest returned'));
  return json<{ manifest_etag: string }>(res);
}
