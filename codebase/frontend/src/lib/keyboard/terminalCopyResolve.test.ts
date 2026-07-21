import { describe, expect, it, vi } from 'vitest';
import { resolveCopyDecision, resolveTerminalCopyText } from './terminalCopyResolve';

describe('resolveTerminalCopyText (ADR-0049 D7)', () => {
  it('copies a real selection and does NOT consult the buffer', () => {
    const take = vi.fn(() => 'buffered');
    expect(resolveTerminalCopyText('selected', take)).toBe('selected');
    // Selection wins — the OSC 52 buffer must not be drained by an unrelated copy.
    expect(take).not.toHaveBeenCalled();
  });

  it('falls back to a fresh buffer when the selection is empty', () => {
    const take = vi.fn(() => 'from-osc52');
    expect(resolveTerminalCopyText('', take)).toBe('from-osc52');
    expect(take).toHaveBeenCalledTimes(1);
  });

  it('returns null when selection is empty AND the buffer is empty (no-op)', () => {
    const take = vi.fn(() => null);
    expect(resolveTerminalCopyText('', take)).toBeNull();
    expect(take).toHaveBeenCalledTimes(1);
  });

  it('treats an empty-string buffer value as a no-op', () => {
    const take = vi.fn(() => '');
    expect(resolveTerminalCopyText('', take)).toBeNull();
  });
});

describe('resolveCopyDecision (ADR-0052 copy-source parity)', () => {
  const base = {
    terminalHasFocus: false,
    focusedTerminalSelection: '',
    lingeringTerminalSelection: '',
    domSelection: '',
  };

  it('terminal focused with a selection → copies it, buffer untouched', () => {
    const take = vi.fn(() => 'buffered');
    expect(
      resolveCopyDecision({
        ...base,
        terminalHasFocus: true,
        focusedTerminalSelection: 'term-sel',
        takeBuffer: take,
      }),
    ).toEqual({ kind: 'terminal', text: 'term-sel' });
    expect(take).not.toHaveBeenCalled();
  });

  it('terminal focused, no selection → drains fresh OSC 52 buffer', () => {
    const take = vi.fn(() => 'from-osc52');
    expect(
      resolveCopyDecision({ ...base, terminalHasFocus: true, takeBuffer: take }),
    ).toEqual({ kind: 'terminal', text: 'from-osc52' });
    expect(take).toHaveBeenCalledTimes(1);
  });

  it('terminal focused, no selection, empty buffer → none', () => {
    const take = vi.fn(() => null);
    expect(
      resolveCopyDecision({ ...base, terminalHasFocus: true, takeBuffer: take }),
    ).toEqual({ kind: 'none' });
  });

  // Repro core: after copying in the terminal (stale xterm selection lingers) the
  // user drags a note body. Focus is off the terminal, so the live DOM selection
  // must win and the stale terminal selection must NOT shadow it.
  it('terminal NOT focused, note DOM selection present, stale terminal selection lingers → dom wins, buffer untouched', () => {
    const take = vi.fn(() => null);
    expect(
      resolveCopyDecision({
        ...base,
        terminalHasFocus: false,
        lingeringTerminalSelection: 'stale-terminal-text',
        domSelection: 'note text',
        takeBuffer: take,
      }),
    ).toEqual({ kind: 'dom' });
    // The one-shot OSC 52 buffer is left for a later terminal Cmd+C.
    expect(take).not.toHaveBeenCalled();
  });

  // Repro core (mouse-mode TUI variant): a fresh OSC 52 buffer from a terminal
  // drag must not shadow a subsequent note DOM selection either.
  it('terminal NOT focused, note DOM selection present, fresh OSC 52 buffer → dom wins, buffer NOT drained', () => {
    const take = vi.fn(() => 'osc52-highlight');
    expect(
      resolveCopyDecision({
        ...base,
        domSelection: 'note text',
        takeBuffer: take,
      }),
    ).toEqual({ kind: 'dom' });
    expect(take).not.toHaveBeenCalled();
  });

  // Reverse-order guard: a stale DOM selection must not shadow a fresh terminal
  // selection when the terminal is the focused (active) surface.
  it('terminal focused with a selection AND a stale DOM selection present → terminal wins', () => {
    const take = vi.fn(() => null);
    expect(
      resolveCopyDecision({
        ...base,
        terminalHasFocus: true,
        focusedTerminalSelection: 'term-sel',
        domSelection: 'stale note text',
        takeBuffer: take,
      }),
    ).toEqual({ kind: 'terminal', text: 'term-sel' });
  });

  it('terminal NOT focused, no DOM selection, lingering terminal selection → copies it', () => {
    const take = vi.fn(() => null);
    expect(
      resolveCopyDecision({
        ...base,
        lingeringTerminalSelection: 'visible-terminal-sel',
        takeBuffer: take,
      }),
    ).toEqual({ kind: 'terminal', text: 'visible-terminal-sel' });
    expect(take).not.toHaveBeenCalled();
  });

  it('terminal NOT focused, no DOM selection, no lingering selection, fresh buffer → drains buffer', () => {
    const take = vi.fn(() => 'from-osc52');
    expect(resolveCopyDecision({ ...base, takeBuffer: take })).toEqual({
      kind: 'terminal',
      text: 'from-osc52',
    });
    expect(take).toHaveBeenCalledTimes(1);
  });

  it('nothing focused, nothing selected, empty buffer → none', () => {
    const take = vi.fn(() => null);
    expect(resolveCopyDecision({ ...base, takeBuffer: take })).toEqual({ kind: 'none' });
  });
});
