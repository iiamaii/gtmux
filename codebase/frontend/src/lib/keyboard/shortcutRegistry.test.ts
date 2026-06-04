import { describe, expect, it } from 'vitest';
import { reservedReasonForBinding } from './shortcutReserved';

describe('shortcutRegistry reserved browser shortcuts', () => {
  it('rejects Cmd/Ctrl+N so the browser new-window shortcut is preserved', () => {
    expect(reservedReasonForBinding({ key: 'n', meta: true })).toBe(
      'This browser or OS standard shortcut is reserved.',
    );
    expect(reservedReasonForBinding({ key: 'n', ctrl: true })).toBe(
      'This browser or OS standard shortcut is reserved.',
    );
  });
});
