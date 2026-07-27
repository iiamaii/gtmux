// fsWriteResult — transport-level write-result types + status classification for
// the `PUT /api/fs/file` text write (ADR-0057 D3/D4).
//
// Lives in the http layer (not chrome/) so `http/fs.ts` stays free of any
// `$lib/chrome` dependency — http/* modules import only siblings and
// `$lib/types/*`. Pure: no imports.

export type WriteErrorKind =
  | 'conflict' // 412 — ETag mismatch, offer reload/overwrite
  | 'precondition_required' // 428 — If-Match missing (should not happen from FE)
  | 'invalid' // 400 — invalid_path / not_a_file / not_utf8
  | 'forbidden' // 403 — path_not_allowed (denylist / outside A-cap)
  | 'not_found' // 404 — file gone
  | 'too_large' // 413 — payload over the size cap
  | 'unknown';

export interface WriteOk {
  ok: true;
  etag: string;
  sizeBytes: number;
}

export interface WriteErr {
  ok: false;
  kind: WriteErrorKind;
  status: number;
  message: string;
}

export type WriteResult = WriteOk | WriteErr;

/** Map an HTTP status (+ optional body error code) to a write error kind. */
export function classifyWriteStatus(status: number, bodyCode?: string | null): WriteErrorKind {
  switch (status) {
    case 412:
      return 'conflict';
    case 428:
      return 'precondition_required';
    case 400:
      return 'invalid';
    case 403:
      return 'forbidden';
    case 404:
      return 'not_found';
    case 413:
      return 'too_large';
    default:
      void bodyCode;
      return 'unknown';
  }
}

/** Human-facing message for the inline error surface (English — matches the
 *  existing preview UI strings). Includes the status code per the spec. */
export function writeErrorMessage(kind: WriteErrorKind, status: number, bodyCode?: string | null): string {
  const code = bodyCode !== null && bodyCode !== undefined && bodyCode.length > 0 ? ` (${bodyCode})` : '';
  switch (kind) {
    case 'conflict':
      return `The file changed on disk since you started editing (${status}).`;
    case 'precondition_required':
      return `Save failed: missing precondition (${status}).`;
    case 'invalid':
      return `Save failed: the file is not editable text${code} (${status}).`;
    case 'forbidden':
      return `Save failed: writing this path is not allowed (${status}).`;
    case 'not_found':
      return `Save failed: the file no longer exists (${status}).`;
    case 'too_large':
      return `Save failed: the file is too large to write (${status}).`;
    default:
      return `Save failed with status ${status}${code}.`;
  }
}
