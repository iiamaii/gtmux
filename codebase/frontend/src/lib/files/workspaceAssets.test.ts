import { describe, expect, it } from 'vitest';

import {
  formatPathsForTerminalInput,
  materializationTypeForPath,
  parseWorkspaceFileDragPayload,
  posixQuotePath,
  previewMetaForPath,
  resolveWorkspacePath,
  shikiLangForPath,
  workspaceRelativePath,
} from './workspaceAssets';

describe('workspaceAssets', () => {
  it('classifies workspace file materialization by extension', () => {
    expect(materializationTypeForPath('/workspace/image.PNG')).toBe('image');
    expect(materializationTypeForPath('/workspace/report.html')).toBe('document');
    expect(materializationTypeForPath('/workspace/bin/app')).toBe('file_path');
  });

  it('classifies preview surfaces and shiki languages from one map', () => {
    expect(previewMetaForPath('/workspace/photo.avif')).toMatchObject({
      kind: 'image',
      chipClass: 'img',
    });
    expect(previewMetaForPath('/workspace/readme.md')).toMatchObject({
      kind: 'markdown',
      shikiLang: 'markdown',
      fileTypeLabel: 'markdown',
    });
    expect(previewMetaForPath('/workspace/src/main.rs')).toMatchObject({
      kind: 'text',
      shikiLang: 'rust',
      chipClass: 'code',
    });
    expect(previewMetaForPath('/workspace/Dockerfile')).toMatchObject({
      kind: 'text',
      shikiLang: 'dockerfile',
    });
    expect(previewMetaForPath('/workspace/bin/tool')).toMatchObject({
      kind: 'text',
      shikiLang: 'text',
    });
    expect(shikiLangForPath('/workspace/app.py')).toBe('python');
  });

  it('creates B-relative paths only for files below the workspace root', () => {
    expect(workspaceRelativePath('/srv/project', '/srv/project/docs/readme.md')).toBe(
      'docs/readme.md',
    );
    expect(workspaceRelativePath('/srv/project/', '/srv/project/docs/readme.md')).toBe(
      'docs/readme.md',
    );
    expect(workspaceRelativePath('/srv/project', '/srv/project')).toBeNull();
    expect(workspaceRelativePath('/srv/project', '/srv/project-other/file.txt')).toBeNull();
  });

  it('resolves safe relative paths under the workspace root', () => {
    expect(resolveWorkspacePath('/srv/project', 'docs/readme.md')).toBe(
      '/srv/project/docs/readme.md',
    );
    expect(resolveWorkspacePath('/srv/project/', 'docs/readme.md')).toBe(
      '/srv/project/docs/readme.md',
    );
    expect(resolveWorkspacePath('/srv/project', '../escape.md')).toBeNull();
    expect(resolveWorkspacePath('/srv/project', '/absolute.md')).toBeNull();
    expect(resolveWorkspacePath('/srv/project', 'docs//readme.md')).toBeNull();
  });

  it('parses drag payloads defensively', () => {
    const parsed = parseWorkspaceFileDragPayload(JSON.stringify({
      files: [{ path: '/srv/project/a.txt', rootPath: '/srv/project', name: 'a.txt', sizeBytes: 12 }],
    }));
    expect(parsed?.files[0]?.path).toBe('/srv/project/a.txt');
    expect(parsed?.files[0]?.kind).toBe('file');
    const directory = parseWorkspaceFileDragPayload(JSON.stringify({
      files: [{ path: '/srv/project/docs', rootPath: '/srv/project', name: 'docs', kind: 'directory' }],
    }));
    expect(directory?.files[0]?.kind).toBe('directory');
    expect(parseWorkspaceFileDragPayload('{')).toBeNull();
    expect(parseWorkspaceFileDragPayload(JSON.stringify({ files: [{ path: 1 }] }))).toBeNull();
  });
});

describe('posixQuotePath (ADR-0047 D4 amend — terminal path injection)', () => {
  it('passes plain paths through unquoted', () => {
    expect(posixQuotePath('/srv/project/src/main.rs')).toBe('/srv/project/src/main.rs');
    expect(posixQuotePath('/srv/project/docs')).toBe('/srv/project/docs');
  });

  it('single-quotes paths containing a space', () => {
    expect(posixQuotePath('/srv/project/My Notes.md')).toBe("'/srv/project/My Notes.md'");
  });

  it('escapes an embedded single quote as \'\\\'\'', () => {
    expect(posixQuotePath("/srv/project/it's a file.txt")).toBe(
      "'/srv/project/it'\\''s a file.txt'",
    );
  });

  it('quotes other shell-special characters', () => {
    expect(posixQuotePath('/srv/project/$(whoami).log')).toBe("'/srv/project/$(whoami).log'");
    expect(posixQuotePath('/srv/project/file*.txt')).toBe("'/srv/project/file*.txt'");
    expect(posixQuotePath('')).toBe("''");
  });
});

describe('formatPathsForTerminalInput', () => {
  it('joins with a single space and appends one trailing space, no newline', () => {
    expect(
      formatPathsForTerminalInput(['/srv/project/a.txt', '/srv/project/b c.txt']),
    ).toBe("/srv/project/a.txt '/srv/project/b c.txt' ");
  });

  it('returns empty string for no paths', () => {
    expect(formatPathsForTerminalInput([])).toBe('');
  });

  it('appends a trailing space to a single plain path', () => {
    expect(formatPathsForTerminalInput(['/srv/project/a.txt'])).toBe('/srv/project/a.txt ');
  });
});
