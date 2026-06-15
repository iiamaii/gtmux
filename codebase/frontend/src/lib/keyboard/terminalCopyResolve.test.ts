import { describe, expect, it, vi } from 'vitest';
import { resolveTerminalCopyText } from './terminalCopyResolve';

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
