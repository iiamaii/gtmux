import { afterEach, describe, expect, it, vi } from 'vitest';
import { readExpandedTreeState, writeExpandedTreeState } from './treeExpansionState';

class MemoryStorage implements Storage {
  #values = new Map<string, string>();

  get length(): number {
    return this.#values.size;
  }

  clear(): void {
    this.#values.clear();
  }

  getItem(key: string): string | null {
    return this.#values.get(key) ?? null;
  }

  key(index: number): string | null {
    return [...this.#values.keys()][index] ?? null;
  }

  removeItem(key: string): void {
    this.#values.delete(key);
  }

  setItem(key: string, value: string): void {
    this.#values.set(key, value);
  }
}

const STORAGE_KEY = 'gtmux:test-tree-expanded';

describe('treeExpansionState', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('persists expansion values by state key', () => {
    vi.stubGlobal('localStorage', new MemoryStorage());

    writeExpandedTreeState(STORAGE_KEY, 'session-a:/workspace', ['/a', '/b'], 20);

    expect(readExpandedTreeState(STORAGE_KEY, 'session-a:/workspace')).toEqual(new Set(['/a', '/b']));
    expect(readExpandedTreeState(STORAGE_KEY, 'session-b:/workspace')).toEqual(new Set());
  });

  it('deduplicates values and applies the max value cap', () => {
    vi.stubGlobal('localStorage', new MemoryStorage());

    writeExpandedTreeState(STORAGE_KEY, 'session-a:/workspace', ['/a', '/a', '/b', '/c'], 2);

    expect([...readExpandedTreeState(STORAGE_KEY, 'session-a:/workspace')]).toEqual(['/a', '/b']);
  });

  it('removes an empty bucket without touching other buckets', () => {
    vi.stubGlobal('localStorage', new MemoryStorage());

    writeExpandedTreeState(STORAGE_KEY, 'session-a:/workspace', ['/a'], 20);
    writeExpandedTreeState(STORAGE_KEY, 'session-b:/workspace', ['/b'], 20);
    writeExpandedTreeState(STORAGE_KEY, 'session-a:/workspace', [], 20);

    expect(readExpandedTreeState(STORAGE_KEY, 'session-a:/workspace')).toEqual(new Set());
    expect(readExpandedTreeState(STORAGE_KEY, 'session-b:/workspace')).toEqual(new Set(['/b']));
  });
});
