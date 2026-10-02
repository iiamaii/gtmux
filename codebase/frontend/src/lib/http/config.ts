import { stepUpErrorFor } from './stepup';
export interface ServerConfig {
  server: { session: string; port: number; bind: string };
  server_workspace: string | null;
  default_session_workspace: string | null;
}
export interface ConfigSnapshot {
  available: boolean; reason?: string; path: string; contents: string; revision: string;
  restart_required: boolean; validation_error: string | null; saved: ServerConfig | null; running: ServerConfig;
}
export async function configRequest<T>(path: string, method = 'GET', body?: unknown): Promise<T> {
  const response = await fetch(path, { method, credentials: 'include', headers: { 'Content-Type': 'application/json' },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
  if (!response.ok) {
    const stepUp = await stepUpErrorFor(response.clone());
    if (stepUp) throw stepUp;
    const error = await response.json().catch(() => ({}));
    throw new Error(error.message ?? `Request failed (HTTP ${response.status})`);
  }
  return response.json() as Promise<T>;
}
