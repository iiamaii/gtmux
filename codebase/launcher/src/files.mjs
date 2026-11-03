import { mkdir, readFile, rename, open, unlink } from 'node:fs/promises';
import { dirname } from 'node:path';
import { randomUUID, createHash } from 'node:crypto';
export const revision = text => createHash('sha256').update(text).digest('hex');
export async function readOptional(path) {
  try { return await readFile(path, 'utf8'); } catch (e) { if (e.code === 'ENOENT') return null; throw e; }
}
export async function atomicWrite(path, text) {
  await mkdir(dirname(path), { recursive: true, mode: 0o700 });
  const temporary = `${path}.${randomUUID()}.tmp`;
  const file = await open(temporary, 'wx', 0o600);
  try { await file.writeFile(text); await file.sync(); await file.close(); await rename(temporary, path); }
  finally { await file.close().catch(() => {}); await unlink(temporary).catch(() => {}); }
}
