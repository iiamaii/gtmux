const STORAGE_KEY = 'gtmux_webpage_id';

function mintWebpageId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return `webpage-${Math.random().toString(36).slice(2)}-${Date.now().toString(36)}`;
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
