import { describe, it, expect } from 'vitest';
import { normalizeNativePath, isAbsoluteNativePath } from './nativePath';
import { basename, isPathWithinRoot, workspaceRelativePath, resolveWorkspacePath } from './workspaceAssets';
describe('native workspace paths', () => {
  it('normalizes Windows drive, verbatim and UNC paths', () => {
    expect(normalizeNativePath('C:\\work\\file.md')).toBe('C:/work/file.md');
    expect(normalizeNativePath('\\\\?\\C:\\work\\file.md')).toBe('C:/work/file.md');
    expect(normalizeNativePath('\\\\?\\UNC\\host\\share\\file.md')).toBe('//host/share/file.md');
    expect(normalizeNativePath('/work/back\\slash')).toBe('/work/back\\slash');
    expect(isAbsoluteNativePath('C:\\work')).toBe(true);
    expect(isAbsoluteNativePath('C:relative')).toBe(false);
  });
  it('resolves Windows workspace files and rejects traversal or sibling prefixes', () => {
    expect(basename('C:\\work\\file.md')).toBe('file.md');
    expect(isPathWithinRoot('\\\\?\\C:\\work\\file.md', 'C:/work')).toBe(true);
    expect(isPathWithinRoot('C:/workspace/file', 'C:/work')).toBe(false);
    expect(workspaceRelativePath('C:\\work', 'C:\\work\\docs\\file.md')).toBe('docs/file.md');
    expect(resolveWorkspacePath('C:\\work', 'docs/file.md')).toBe('C:/work/docs/file.md');
    expect(resolveWorkspacePath('C:/work', '..\\outside')).toBe(null);
    expect(resolveWorkspacePath('C:/work', 'D:/outside')).toBe(null);
  });
});
