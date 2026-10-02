export type ActivityState = 'unknown' | 'working' | 'quiet' | 'completed' | 'needs_input';
export type ActivitySource = 'output' | 'heuristic' | 'shell' | 'report';
export interface ActivityRow {
  id: string;
  pane_id: number;
  activity: { state: ActivityState; source: ActivitySource; output_seq: number; state_seq: number };
}
export interface ActivitySnapshot { server_id: string; terminals: ActivityRow[] }
export interface ActivityPreferences {
  enabled: boolean;
  completed: boolean;
  needs_input: boolean;
  unread: boolean;
  tabUnread: boolean;
  list: boolean;
  tab: boolean;
  titleFormat: string;
  completedFormat: string;
  inputFormat: string;
  unreadFormat: string;
}
export type ActivityToggle = 'enabled' | 'completed' | 'needs_input' | 'unread' | 'list' | 'tab' | 'tabUnread';
export type ActivityFormat = 'titleFormat' | 'completedFormat' | 'inputFormat' | 'unreadFormat';
export const ACTIVITY_FORMAT_MAX_LENGTH = 120;
export function validateActivityFormat(key: ActivityFormat, value: string): string | null {
  if (!value.trim()) return 'Enter a format.';
  if (value.length > ACTIVITY_FORMAT_MAX_LENGTH) return 'Use at most 120 characters.';
  if (/[\r\n\x00-\x1f]/.test(value)) return 'Use a single line without control characters.';
  const allowed = key === 'titleFormat' ? ['title', 'activity'] : ['count'];
  const remainder = value.replace(/\{([^{}]+)\}/g, (match, name) => allowed.includes(name) ? '' : match);
  if (/[{}]/.test(remainder)) return `Available placeholders: ${allowed.map((name) => `{${name}}`).join(', ')}.`;
  if (key === 'titleFormat' && (!value.includes('{title}') || !value.includes('{activity}'))) {
    return 'Include both {title} and {activity}.';
  }
  return null;
}
export const ACTIVITY_STORAGE_KEY = 'gtmux-terminal-activity:v1';
export const DEFAULT_ACTIVITY_PREFERENCES: Readonly<ActivityPreferences> = {
  enabled: false, completed: true, needs_input: true, unread: true, list: true, tab: true, tabUnread: false,
  titleFormat: '[{activity}] {title}', completedFormat: '{count} done',
  inputFormat: '{count} input', unreadFormat: '{count} unread',
};
export interface ActivityAck { output: number; state: number }

export function parseActivityPreferences(raw: string | null): ActivityPreferences {
  const defaults = { ...DEFAULT_ACTIVITY_PREFERENCES };
  try {
    const value: unknown = JSON.parse(raw ?? 'null');
    if (!value || typeof value !== 'object' || !('version' in value) || value.version !== 1) return defaults;
    for (const key of ['enabled', 'completed', 'needs_input', 'unread', 'list', 'tab', 'tabUnread'] as ActivityToggle[]) {
      if (key in value && typeof value[key as keyof typeof value] === 'boolean') {
        defaults[key] = value[key as keyof typeof value] as boolean;
      }
    }
    for (const key of ['titleFormat', 'completedFormat', 'inputFormat', 'unreadFormat'] as ActivityFormat[]) {
      const format = (value as Record<string, unknown>)[key];
      if (typeof format === 'string' && !validateActivityFormat(key, format)) defaults[key] = format;
    }
  } catch { /* Corrupt preferences must not prevent app startup. */ }
  return defaults;
}
export function activityKey(serverId: string, row: ActivityRow): string {
  return `${serverId}:${row.id}:${row.pane_id}`;
}
export function activityNotice(row: ActivityRow, ack: ActivityAck | undefined, prefs: ActivityPreferences) {
  if (!prefs.enabled || !ack) return { completed: false, needs_input: false, unread: false };
  return {
    completed: prefs.completed && row.activity.state === 'completed' && row.activity.state_seq > ack.state,
    needs_input: prefs.needs_input && row.activity.state === 'needs_input' && row.activity.state_seq > ack.state,
    unread: prefs.unread && row.activity.output_seq > ack.output,
  };
}
export function activityLabel(row: ActivityRow, prefs: ActivityPreferences): string {
  const estimated = row.activity.source === 'heuristic' ? ' (estimated)' : '';
  switch (row.activity.state) {
    case 'completed': return prefs.completed ? `Completed${estimated}` : 'Monitoring';
    case 'needs_input': return prefs.needs_input ? `Needs input${estimated}` : 'Monitoring';
    case 'working': return row.activity.source === 'output' ? 'Active' : 'Working';
    case 'quiet': return 'Output quiet';
    default: return 'No activity yet';
  }
}
export function parseActivitySnapshot(value: unknown): ActivitySnapshot {
  if (!value || typeof value !== 'object') throw new Error('Invalid activity response');
  const data = value as Partial<ActivitySnapshot>;
  if (typeof data.server_id !== 'string' || !Array.isArray(data.terminals)) throw new Error('Invalid activity response');
  const states = ['unknown', 'working', 'quiet', 'completed', 'needs_input'];
  const sources = ['output', 'heuristic', 'shell', 'report'];
  const ids = new Set<string>();
  for (const row of data.terminals) {
    if (!row || typeof row.id !== 'string' || ids.has(row.id) || (!Number.isSafeInteger(row.pane_id) || row.pane_id < 0)
      || !row.activity || !states.includes(row.activity.state) || !sources.includes(row.activity.source)
      || !Number.isSafeInteger(row.activity.output_seq) || row.activity.output_seq < 0
      || !Number.isSafeInteger(row.activity.state_seq) || row.activity.state_seq < 0) {
      throw new Error('Invalid activity response');
    }
    ids.add(row.id);
  }
  return data as ActivitySnapshot;
}

/** Count each terminal once, only within the session represented by this tab. */
export function activityTitle(base: string, terminalIds: Set<string>, rows: ActivityRow[], serverId: string,
  acks: Record<string, ActivityAck>, prefs: ActivityPreferences): string {
  if (!prefs.enabled || !prefs.tab) return base;
  let waiting = 0, done = 0, unread = 0;
  for (const row of rows) {
    if (!terminalIds.has(row.id)) continue;
    const notice = activityNotice(row, acks[activityKey(serverId, row)], prefs);
    if (notice.needs_input) waiting++;
    else if (notice.completed) done++;
    else if (notice.unread && prefs.tabUnread) unread++;
  }
  return formatActivityTitle(base, { waiting, done, unread }, prefs);
}

export function formatActivityTitle(base: string, counts: { waiting: number; done: number; unread: number }, prefs: ActivityPreferences): string {
  const formatCount = (format: string, count: number) => format.replaceAll('{count}', String(count));
  const parts = [counts.waiting ? formatCount(prefs.inputFormat, counts.waiting) : '',
    counts.done ? formatCount(prefs.completedFormat, counts.done) : '',
    prefs.tabUnread && counts.unread ? formatCount(prefs.unreadFormat, counts.unread) : ''].filter(Boolean);
  if (!parts.length) return base;
  // One pass: braces in session names/custom text must remain literal.
  return prefs.titleFormat.replace(/\{(title|activity)\}/g, (_, key) => key === 'title' ? base : parts.join(', '));
}
