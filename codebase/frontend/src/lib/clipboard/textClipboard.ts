export type TextClipboardMethod = 'async-clipboard' | 'exec-command';

export interface TextClipboardResult {
  ok: boolean;
  method?: TextClipboardMethod;
  reason?: string;
}

export function canUseAsyncClipboard(): boolean {
  return (
    typeof window !== 'undefined' &&
    window.isSecureContext === true &&
    typeof navigator !== 'undefined' &&
    typeof navigator.clipboard?.writeText === 'function'
  );
}

export async function copyTextToSystemClipboard(text: string): Promise<TextClipboardResult> {
  if (canUseAsyncClipboard()) {
    try {
      await navigator.clipboard.writeText(text);
      return { ok: true, method: 'async-clipboard' };
    } catch (err) {
      return {
        ok: false,
        reason: errorMessage(err) ?? 'Async clipboard write was blocked.',
      };
    }
  }

  if (typeof window !== 'undefined' && window.isSecureContext === true) {
    return {
      ok: false,
      reason: 'Async clipboard API is unavailable in this secure context.',
    };
  }

  return copyViaExecCommand(text);
}

function copyViaExecCommand(text: string): TextClipboardResult {
  if (typeof document === 'undefined' || document.body === null) {
    return { ok: false, reason: 'Document is unavailable.' };
  }

  const activeElement = document.activeElement;
  const selection = document.getSelection();
  const savedRanges: Range[] = [];
  if (selection !== null) {
    for (let i = 0; i < selection.rangeCount; i += 1) {
      savedRanges.push(selection.getRangeAt(i).cloneRange());
    }
  }

  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.setAttribute('readonly', 'true');
  textarea.setAttribute('aria-hidden', 'true');
  textarea.style.position = 'fixed';
  textarea.style.left = '-9999px';
  textarea.style.top = '0';
  textarea.style.width = '1px';
  textarea.style.height = '1px';
  textarea.style.opacity = '0';

  try {
    document.body.appendChild(textarea);
    textarea.focus({ preventScroll: true });
    textarea.select();
    textarea.setSelectionRange(0, textarea.value.length);
    const copied = document.execCommand('copy');
    return copied
      ? { ok: true, method: 'exec-command' }
      : { ok: false, reason: 'Legacy copy command was rejected.' };
  } catch (err) {
    return {
      ok: false,
      reason: errorMessage(err) ?? 'Legacy copy command failed.',
    };
  } finally {
    textarea.remove();
    restoreSelection(selection, savedRanges);
    restoreFocus(activeElement);
  }
}

function restoreSelection(selection: Selection | null, ranges: Range[]): void {
  if (selection === null) return;
  try {
    selection.removeAllRanges();
    for (const range of ranges) selection.addRange(range);
  } catch {
    // Restoring selection is best-effort; copy success is more important than
    // preserving a stale range whose nodes may have unmounted.
  }
}

function restoreFocus(activeElement: Element | null): void {
  if (!(activeElement instanceof HTMLElement)) return;
  if (!document.contains(activeElement)) return;
  try {
    activeElement.focus({ preventScroll: true });
  } catch {
    // Focus restoration is best-effort for the same reason selection
    // restoration is: the prior focus target may have disappeared.
  }
}

function errorMessage(err: unknown): string | undefined {
  if (err instanceof Error && err.message.length > 0) return err.message;
  if (typeof err === 'string' && err.length > 0) return err;
  return undefined;
}
