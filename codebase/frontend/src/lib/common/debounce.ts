// Tiny trailing-edge debounce. Used by the left-panel search inputs (ADR-0052
// D2, ~150ms) and reusable elsewhere. No Node types — the timer handle uses
// `ReturnType<typeof setTimeout>` so it works in both browser and jsdom.

export type Debounced<T extends (...args: never[]) => void> = T & { cancel: () => void };

/**
 * Wrap `fn` so that calls are coalesced onto the trailing edge: the wrapped
 * function only fires `ms` after the last invocation. `.cancel()` clears any
 * pending call.
 */
export function debounce<T extends (...args: never[]) => void>(fn: T, ms = 150): Debounced<T> {
  let handle: ReturnType<typeof setTimeout> | undefined;

  const wrapped = (...args: Parameters<T>): void => {
    if (handle !== undefined) clearTimeout(handle);
    handle = setTimeout(() => {
      handle = undefined;
      fn(...args);
    }, ms);
  };

  (wrapped as Debounced<T>).cancel = (): void => {
    if (handle !== undefined) {
      clearTimeout(handle);
      handle = undefined;
    }
  };

  return wrapped as Debounced<T>;
}
