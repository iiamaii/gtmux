import { generateUuidV4 } from '$lib/uuid';

const STORAGE_KEY = 'gtmux_webpage_id';

function mintWebpageId(): string {
  return generateUuidV4();
}

export function getWebpageId(): string {
  if (typeof sessionStorage === 'undefined') return mintWebpageId();
  const id = mintWebpageId();
  try {
    const existing = sessionStorage.getItem(STORAGE_KEY);
    if (existing !== null && existing.length > 0) return existing;
    sessionStorage.setItem(STORAGE_KEY, id);
  } catch {
    return id;
  }
  return id;
}

export function webpageHeaders(): Record<string, string> {
  return { 'X-Gtmux-Webpage-Id': getWebpageId() };
}
