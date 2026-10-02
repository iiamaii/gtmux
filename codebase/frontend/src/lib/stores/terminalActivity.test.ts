import { describe, expect, it } from 'vitest';
import { activityKey, activityLabel, activityNotice, activityTitle, DEFAULT_ACTIVITY_PREFERENCES,
  parseActivityPreferences, parseActivitySnapshot, formatActivityTitle, validateActivityFormat, type ActivityRow } from './terminalActivity';
const prefs = { ...DEFAULT_ACTIVITY_PREFERENCES, enabled: true };
const row: ActivityRow = { id: 'a', pane_id: 1,
  activity: { state: 'completed', source: 'report', output_seq: 4, state_seq: 3 } };
const ack = { output: 2, state: 1 };
describe('terminal activity', () => {
  it('defaults off and tolerates corrupt or future preferences', () => {
    for (const raw of [null, '{', 'null', '{"version":2,"enabled":true}']) {
      expect(parseActivityPreferences(raw)).toEqual(DEFAULT_ACTIVITY_PREFERENCES);
    }
    expect(parseActivityPreferences('{"version":1,"enabled":true,"tab":false,"unread":"false"}'))
      .toEqual({ ...prefs, tab: false });
  });
  it('suppresses old output until a baseline is available', () => {
    expect(activityNotice(row, undefined, prefs)).toEqual({ completed: false, needs_input: false, unread: false });
  });
  it('completion and unread are independently configurable', () => {
    expect(activityNotice(row, ack, { ...prefs, unread: false })).toEqual({ completed: true, needs_input: false, unread: false });
    expect(activityNotice(row, ack, { ...prefs, completed: false })).toEqual({ completed: false, needs_input: false, unread: true });
  });
  it('acknowledgement clears pending notices but preserves current state', () => {
    expect(activityNotice(row, { output: 4, state: 3 }, prefs)).toEqual({ completed: false, needs_input: false, unread: false });
    expect(activityLabel(row, prefs)).toBe('Completed');
  });
  it('repeated completion without additional output can notify again', () => {
    expect(activityNotice({ ...row, activity: { ...row.activity, state_seq: 4 } }, { output: 4, state: 3 }, prefs).completed).toBe(true);
  });
  it('input notices can be disabled separately', () => {
    const waiting: ActivityRow = { ...row, activity: { ...row.activity, state: 'needs_input' } };
    expect(activityNotice(waiting, ack, prefs).needs_input).toBe(true);
    expect(activityNotice(waiting, ack, { ...prefs, needs_input: false }).needs_input).toBe(false);
  });
  it('silence is not completion and estimated states are labelled', () => {
    expect(activityLabel({ ...row, activity: { ...row.activity, state: 'quiet' } }, prefs)).toBe('Output quiet');
    expect(activityLabel({ ...row, activity: { ...row.activity, source: 'heuristic' } }, prefs)).toBe('Completed (estimated)');
  });
  it('read markers cannot leak across server restarts or respawned panes', () => {
    expect(activityKey('boot1', row)).not.toBe(activityKey('boot2', row));
    expect(activityKey('boot1', row)).not.toBe(activityKey('boot1', { ...row, pane_id: 2 }));
  });
  it('tab counts are session-scoped and count a terminal only once', () => {
    const other = { ...row, id: 'b' };
    const acks = { [activityKey('s', row)]: ack, [activityKey('s', other)]: ack };
    expect(activityTitle('gtmux - one', new Set(['a']), [row, other], 's', acks, prefs)).toBe('[1 done] gtmux - one');
    expect(activityTitle('gtmux - empty', new Set(), [row, other], 's', acks, prefs)).toBe('gtmux - empty');
    for (const patch of [{ enabled: false }, { tab: false }]) {
      expect(activityTitle('gtmux', new Set(['a']), [row], 's', acks, { ...prefs, ...patch })).toBe('gtmux');
    }
    expect(activityTitle('gtmux', new Set(['a']), [row], 's', acks, { ...prefs, completed: false, tabUnread: true })).toBe('[1 unread] gtmux');
  });
  it('rejects malformed snapshots rather than showing inaccurate badges', () => {
    expect(parseActivitySnapshot({ server_id: 's', terminals: [row] }).terminals).toEqual([row]);
    for (const terminals of [[row, row], [{ ...row, pane_id: -1 }], [{ ...row, activity: { ...row.activity, output_seq: -1 } }],
      [{ ...row, activity: { ...row.activity, state: 'finished' } }], [null]]) {
      expect(() => parseActivitySnapshot({ server_id: 's', terminals })).toThrow('Invalid activity response');
    }
  });
});

describe('activity title formats', () => {
  it('migrates old boolean-only preferences with default formats', () => {
    expect(parseActivityPreferences('{"version":1,"enabled":true}')).toEqual(prefs);
  });
  it('preserves custom Unicode labels and composes the existing title', () => {
    const custom = { ...prefs, titleFormat: '{title} | {activity}', completedFormat: '✓ {count}', inputFormat: '입력 {count}', unreadFormat: '● {count}' };
    expect(formatActivityTitle('gtmux - 개발', { waiting: 1, done: 2, unread: 0 }, custom)).toBe('gtmux - 개발 | 입력 1, ✓ 2');
    expect(formatActivityTitle('gtmux', { waiting: 0, done: 0, unread: 0 }, custom)).toBe('gtmux');
    expect(parseActivityPreferences(JSON.stringify({ version: 1, ...custom }))).toEqual(custom);
  });
  it('does not interpret braces embedded in session names', () => {
    expect(formatActivityTitle('gtmux - {activity}', { waiting: 0, done: 1, unread: 0 }, prefs)).toBe('[1 done] gtmux - {activity}');
  });
  it('allows a static symbol but requires both placeholders in the full title', () => {
    expect(validateActivityFormat('completedFormat', '✓')).toBeNull();
    expect(validateActivityFormat('titleFormat', '{title}')).not.toBeNull();
    expect(validateActivityFormat('titleFormat', '{activity}')).not.toBeNull();
    for (const format of ['', '{unknown}', 'a'.repeat(121), 'line\nline', '{count']) {
      expect(validateActivityFormat('completedFormat', format)).not.toBeNull();
    }
  });
  it('falls back safely when a saved format is malformed', () => {
    expect(parseActivityPreferences('{"version":1,"enabled":true,"titleFormat":"{bad}"}')).toEqual(prefs);
  });
});

 describe('unread tab opt-in', () => {
  it('keeps list notices while hiding unread title counts by default, including older preferences', () => {
    const p = parseActivityPreferences('{"version":1,"enabled":true,"completed":false,"unread":true}');
    const acks = { [activityKey('s', row)]: ack };
    expect(activityNotice(row, ack, p).unread).toBe(true);
    expect(activityTitle('gtmux', new Set(['a']), [row], 's', acks, p)).toBe('gtmux');
    const optedIn = parseActivityPreferences(JSON.stringify({ version: 1, ...p, tabUnread: true }));
    expect(activityTitle('gtmux', new Set(['a']), [row], 's', acks, optedIn)).toBe('[1 unread] gtmux');
    expect(activityTitle('gtmux', new Set(['a']), [row], 's', acks, { ...optedIn, tabUnread: false })).toBe('gtmux');
    expect(formatActivityTitle('gtmux', { waiting: 0, done: 0, unread: 1 }, p)).toBe('gtmux');
  });
});
