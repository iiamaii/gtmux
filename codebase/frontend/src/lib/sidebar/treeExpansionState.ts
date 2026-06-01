type ExpansionPayload = Record<string, string[]>;

const MAX_STATE_BUCKETS = 40;

export function readExpandedTreeState(storageKey: string, stateKey: string | null): Set<string> {
  if (stateKey === null) return new Set();
  const payload = readPayload(storageKey);
  return new Set(payload[stateKey] ?? []);
}

export function writeExpandedTreeState(
  storageKey: string,
  stateKey: string | null,
  values: Iterable<string>,
  maxValues: number,
): void {
  if (stateKey === null || typeof localStorage === 'undefined') return;
  try {
    const payload = readPayload(storageKey);
    const next = Array.from(new Set(values)).slice(0, maxValues);
    delete payload[stateKey];
    if (next.length > 0) payload[stateKey] = next;
    trimOldestBuckets(payload);
    localStorage.setItem(storageKey, JSON.stringify(payload));
  } catch (e) {
    console.debug('[gtmux] tree expansion persist failed', e);
  }
}

function readPayload(storageKey: string): ExpansionPayload {
  if (typeof localStorage === 'undefined') return {};
  try {
    const raw = localStorage.getItem(storageKey);
    if (raw === null) return {};
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) return {};
    const out: ExpansionPayload = {};
    for (const [key, value] of Object.entries(parsed)) {
      if (!Array.isArray(value)) continue;
      out[key] = value.filter((item): item is string => typeof item === 'string');
    }
    return out;
  } catch {
    return {};
  }
}

function trimOldestBuckets(payload: ExpansionPayload): void {
  const keys = Object.keys(payload);
  for (let i = 0; i < keys.length - MAX_STATE_BUCKETS; i += 1) {
    const key = keys[i];
    if (key !== undefined) delete payload[key];
  }
}
