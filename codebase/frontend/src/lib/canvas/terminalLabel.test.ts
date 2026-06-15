import { describe, it, expect } from 'vitest';
import {
  renameItemLabel,
  shortTerminalId,
  terminalHeaderLabel,
  terminalPoolDisplayName,
} from '$lib/canvas/terminalLabel';
import type { NoteItem, TerminalItem } from '$lib/types/canvas';

// ADR-0050 — terminal panel label derives from the persisted layout
// item.label, never from the in-memory terminal_meta (PATCH /api/terminals).

const UUID = '12345678-90ab-cdef-1234-567890abcdef';

function terminalItem(label?: string): TerminalItem {
  return {
    id: UUID,
    type: 'terminal',
    parent_id: null,
    x: 0,
    y: 0,
    w: 320,
    h: 200,
    z: 1,
    visibility: 'visible',
    locked: false,
    minimized: false,
    ...(label === undefined ? {} : { label }),
  };
}

function noteItem(title: string): NoteItem {
  return {
    id: 'note-1',
    type: 'note',
    parent_id: null,
    x: 0,
    y: 0,
    w: 200,
    h: 120,
    z: 1,
    visibility: 'visible',
    locked: false,
    minimized: false,
    title,
    body: '',
    color: '#fff',
  };
}

describe('shortTerminalId', () => {
  it('drops dashes and takes the first 8 chars', () => {
    expect(shortTerminalId(UUID)).toBe('12345678');
  });
});

describe('terminalHeaderLabel', () => {
  it('prefers a non-empty layout item.label', () => {
    expect(terminalHeaderLabel('build watch', '%3', UUID)).toBe('build watch');
  });
  it('trims the label before using it', () => {
    expect(terminalHeaderLabel('  spaced  ', '%3', UUID)).toBe('spaced');
  });
  it('falls back to pane_id when label is empty', () => {
    expect(terminalHeaderLabel('', '%3', UUID)).toBe('%3');
  });
  it('falls back to pane_id when label is whitespace-only', () => {
    expect(terminalHeaderLabel('   ', '%3', UUID)).toBe('%3');
  });
  it('falls back to pane_id when label is null/undefined', () => {
    expect(terminalHeaderLabel(null, '%3', UUID)).toBe('%3');
    expect(terminalHeaderLabel(undefined, '%3', UUID)).toBe('%3');
  });
  it('falls back to id when both label and pane_id are absent', () => {
    expect(terminalHeaderLabel(null, undefined, UUID)).toBe(UUID);
  });
  it('does not consult any terminal_meta source — only the passed args', () => {
    // Regression guard for ADR-0050: a blank persisted label must NOT resolve
    // to some external (server-wide) label; it falls straight through to the
    // pane_id/id fallback.
    expect(terminalHeaderLabel('', undefined, UUID)).toBe(UUID);
  });
});

describe('terminalPoolDisplayName', () => {
  it('uses the current-session layout item.label when present', () => {
    expect(terminalPoolDisplayName('build watch', UUID)).toBe('build watch');
  });
  it('trims the session label', () => {
    expect(terminalPoolDisplayName('  api server  ', UUID)).toBe('api server');
  });
  it('falls back to a short id when no session label exists', () => {
    expect(terminalPoolDisplayName(undefined, UUID)).toBe('t12345678');
    expect(terminalPoolDisplayName(null, UUID)).toBe('t12345678');
  });
  it('falls back to a short id when the session label is whitespace-only', () => {
    expect(terminalPoolDisplayName('   ', UUID)).toBe('t12345678');
  });
});

describe('renameItemLabel (shared layout-persist transform — ADR-0050 D2)', () => {
  it('writes a terminal rename into the persisted item.label', () => {
    const next = renameItemLabel(terminalItem(), 'build watch');
    expect(next.type).toBe('terminal');
    expect(next.label).toBe('build watch');
  });
  it('overwrites an existing terminal label', () => {
    const next = renameItemLabel(terminalItem('old'), 'new');
    expect(next.label).toBe('new');
  });
  it('does not mutate the input item (returns a fresh object)', () => {
    const original = terminalItem('old');
    const next = renameItemLabel(original, 'new');
    expect(original.label).toBe('old');
    expect(next).not.toBe(original);
  });
  it('writes a note rename into title, leaving label untouched (regression)', () => {
    const next = renameItemLabel(noteItem('First'), 'Second');
    expect(next.type).toBe('note');
    if (next.type === 'note') expect(next.title).toBe('Second');
    expect(next.label).toBeUndefined();
  });
});
