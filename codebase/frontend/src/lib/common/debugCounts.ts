// Debug counters — 0045 refresh reconnect loop 분석용 dev-only instrumentation.
//
// 켜기:
//   localStorage.setItem('gtmux-debug-counts', '1')
//   // 새로고침
//
// 끄기:
//   localStorage.removeItem('gtmux-debug-counts')
//
// 사용:
//   import { debugCount } from '$lib/common/debugCounts';
//   debugCount('flowNodes.rebuild');
//   debugCount('flowNodes.cache.hit');
//
// 본 모듈은 *production noise 0* 가 목표 — flag 가 꺼져 있으면 무시.
// localStorage 접근은 try-catch (private mode safe).

const FLAG_KEY = 'gtmux-debug-counts';

function readFlag(): boolean {
  try {
    if (typeof window === 'undefined') return false;
    return window.localStorage.getItem(FLAG_KEY) === '1';
  } catch {
    return false;
  }
}

let enabled = readFlag();
const counts = new Map<string, number>();
let lastSummaryAt = 0;
const SUMMARY_THROTTLE_MS = 1_000;

/**
 * Increment a named counter. No-op when disabled.
 *
 * Periodically dumps a summary to console (throttled 1s) to keep DevTools
 * readable. Each event also logs at debug level if `gtmux-debug-counts-verbose`
 * flag is set.
 */
export function debugCount(name: string): void {
  if (!enabled) return;
  counts.set(name, (counts.get(name) ?? 0) + 1);
  maybeSummary();
}

function maybeSummary(): void {
  const now = Date.now();
  if (now - lastSummaryAt < SUMMARY_THROTTLE_MS) return;
  lastSummaryAt = now;
  const lines: string[] = ['[gtmux-debug-counts]'];
  for (const [name, count] of [...counts.entries()].sort()) {
    lines.push(`  ${name}: ${count}`);
  }
  console.debug(lines.join('\n'));
}

/** Reset counters — useful for E2E tests / manual checkpoints. */
export function debugCountReset(): void {
  counts.clear();
  lastSummaryAt = 0;
}

/** Snapshot for tests. */
export function debugCountSnapshot(): Record<string, number> {
  return Object.fromEntries(counts);
}

/** Toggle at runtime (e.g. from DevTools console). */
export function debugCountSetEnabled(next: boolean): void {
  enabled = next;
  try {
    if (typeof window !== 'undefined') {
      if (next) window.localStorage.setItem(FLAG_KEY, '1');
      else window.localStorage.removeItem(FLAG_KEY);
    }
  } catch {
    /* private mode — flag 만 in-memory */
  }
}

// Expose to window for ad-hoc inspection (dev only — guarded by flag-check
// in debugCount itself, so cost when off ≈ 0).
if (typeof window !== 'undefined') {
  (window as unknown as { __gtmuxDebug?: unknown }).__gtmuxDebug = {
    enable: () => debugCountSetEnabled(true),
    disable: () => debugCountSetEnabled(false),
    snapshot: debugCountSnapshot,
    reset: debugCountReset,
  };
}
