import { UnauthorizedError } from './sessions';

export interface UploadedAsset {
  asset_id: string;
  mime: string;
  file_name: string;
  size_bytes: number;
  original_w?: number;
  original_h?: number;
}

export class AssetUploadUnavailableError extends Error {
  constructor(message = 'Asset upload API is not available yet.') {
    super(message);
    this.name = 'AssetUploadUnavailableError';
  }
}

export async function uploadAsset(file: File, kind: 'image' | 'document'): Promise<UploadedAsset> {
  const form = new FormData();
  form.set('file', file, file.name);
  form.set('kind', kind);

  const res = await fetch('/api/assets', {
    method: 'POST',
    credentials: 'include',
    body: form,
  });

  if (res.status === 401) throw new UnauthorizedError();
  if (res.status === 404 || res.status === 405) throw new AssetUploadUnavailableError();
  if (!res.ok) throw new Error(`POST /api/assets returned ${res.status}`);

  const body = await res.json() as Partial<UploadedAsset>;
  if (typeof body.asset_id !== 'string' || body.asset_id.length === 0) {
    throw new Error('POST /api/assets response missing asset_id');
  }

  return {
    asset_id: body.asset_id,
    mime: body.mime ?? file.type,
    file_name: body.file_name ?? file.name,
    size_bytes: body.size_bytes ?? file.size,
    original_w: body.original_w,
    original_h: body.original_h,
  };
}
