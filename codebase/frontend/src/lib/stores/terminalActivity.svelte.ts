import {
  ACTIVITY_STORAGE_KEY, DEFAULT_ACTIVITY_PREFERENCES,
  activityKey, activityNotice, activityTitle, validateActivityFormat, parseActivityPreferences, parseActivitySnapshot,
  type ActivityAck, type ActivityPreferences, type ActivityRow, type ActivityFormat,
} from './terminalActivity';

const ACK_KEY = 'gtmux-terminal-activity-read:v1';
function preferences(): ActivityPreferences {
  try { return parseActivityPreferences(localStorage.getItem(ACTIVITY_STORAGE_KEY)); }
  catch { return { ...DEFAULT_ACTIVITY_PREFERENCES }; }
}
function loadAcks(): Record<string, ActivityAck> {
  try {
    const value: unknown = JSON.parse(sessionStorage.getItem(ACK_KEY) ?? '{}');
    if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
    const result: Record<string, ActivityAck> = {};
    for (const [key, ack] of Object.entries(value).slice(-2000)) {
      if (ack && Number.isSafeInteger(ack.output) && ack.output >= 0
        && Number.isSafeInteger(ack.state) && ack.state >= 0) result[key] = ack;
    }
    return result;
  } catch { return {}; }
}

class TerminalActivityStore {
  preferences = $state<ActivityPreferences>(preferences());
  rows = $state<ActivityRow[]>([]);
  serverId = $state('');
  acks = $state<Record<string, ActivityAck>>(loadAcks());
  error = $state<string | null>(null);
  saveError = $state<string | null>(null);
  loading = $state(false);

  update(patch: Partial<ActivityPreferences>): boolean {
    const next = { ...this.preferences, ...patch };
    for (const key of ['titleFormat', 'completedFormat', 'inputFormat', 'unreadFormat'] as ActivityFormat[]) {
      const error = validateActivityFormat(key, next[key]);
      if (error) { this.saveError = error; return false; }
    }
    try {
      localStorage.setItem(ACTIVITY_STORAGE_KEY, JSON.stringify({ version: 1, ...next }));
      this.preferences = next; this.saveError = null;
      return true;
    } catch (error) {
      this.saveError = `Could not save activity preferences: ${error instanceof Error ? error.message : String(error)}`;
      return false;
    }
  }
  notice(row: ActivityRow) { return activityNotice(row, this.acks[activityKey(this.serverId, row)], this.preferences); }
  markRead(row: ActivityRow): void {
    if (!this.preferences.enabled) return;
    this.acks[activityKey(this.serverId, row)] = { output: row.activity.output_seq, state: row.activity.state_seq };
    this.persistAcks();
  }
  private persistAcks(): void {
    try { sessionStorage.setItem(ACK_KEY, JSON.stringify(this.acks)); }
    catch { /* Read acknowledgements still work for this page lifetime. */ }
  }
  private acknowledgeFocused(): void {
    if (document.visibilityState !== 'visible' || !document.hasFocus()) return;
    const host = document.activeElement?.closest<HTMLElement>('[data-activity-pane]');
    if (!host || host.getClientRects().length === 0) return;
    const rect = host.getBoundingClientRect();
    if (rect.bottom <= 0 || rect.top >= innerHeight || rect.right <= 0 || rect.left >= innerWidth) return;
    const row = this.rows.find((r) => String(r.pane_id) === host.dataset.activityPane);
    if (row) this.markRead(row);
  }
  title(base: string, terminalIds: Set<string>): string {
    return this.error ? base : activityTitle(base, terminalIds, this.rows, this.serverId, this.acks, this.preferences);
  }

  listen(): () => void {
    const storage = (event: StorageEvent) => {
      if (event.storageArea === localStorage && (event.key === null || event.key === ACTIVITY_STORAGE_KEY)) {
        this.preferences = parseActivityPreferences(event.newValue); this.saveError = null;
      }
    };
    window.addEventListener('storage', storage);
    return () => window.removeEventListener('storage', storage);
  }
  start(): () => void {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let request: AbortController | undefined;
    const focused = () => this.acknowledgeFocused();
    const poll = async () => {
      request = new AbortController();
      const timeout = setTimeout(() => request?.abort(), 8000);
      try {
        const response = await fetch('/api/terminals/activity', {
          credentials: 'include', signal: request.signal, headers: { Accept: 'application/json' },
        });
        if (!response.ok) throw new Error(response.status === 404
          ? 'This server does not support terminal activity. Update the backend.'
          : `Activity unavailable (HTTP ${response.status}).`);
        const snapshot = parseActivitySnapshot(await response.json());
        if (stopped) return;
        this.serverId = snapshot.server_id;
        // First observation is the baseline, not a notification for old scrollback.
        const acks: Record<string, ActivityAck> = {};
        for (const row of snapshot.terminals) {
          const key = activityKey(snapshot.server_id, row);
          acks[key] = this.acks[key] ?? { output: row.activity.output_seq, state: row.activity.state_seq };
        }
        this.acks = acks; this.rows = snapshot.terminals; this.error = null;
        this.acknowledgeFocused(); this.persistAcks();
      } catch (error) {
        if (!stopped) this.error = error instanceof Error ? error.message : String(error);
      } finally {
        clearTimeout(timeout);
        if (!stopped) { this.loading = false; timer = setTimeout(() => void poll(), 2000); }
      }
    };
    this.loading = true;
    document.addEventListener('focusin', focused);
    window.addEventListener('focus', focused);
    document.addEventListener('visibilitychange', focused);
    void poll();
    return () => {
      stopped = true; clearTimeout(timer); request?.abort();
      document.removeEventListener('focusin', focused);
      window.removeEventListener('focus', focused);
      document.removeEventListener('visibilitychange', focused);
      this.rows = []; this.error = null; this.loading = false;
    };
  }
}
export const terminalActivity = new TerminalActivityStore();
