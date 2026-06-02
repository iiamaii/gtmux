import { describe, expect, it } from 'vitest';

import type { CanvasLayout, DocumentItem, FilePathItem, ImageItem, TextItem } from '$lib/types/canvas';

import { rebindCanvasLayoutPathsForMove } from './workspaceMoveRebind';

const common = {
  parent_id: null,
  x: 0,
  y: 0,
  z: 1,
  visibility: 'visible',
  locked: false,
  minimized: false,
} as const;

function layout(items: CanvasLayout['items']): CanvasLayout {
  return {
    schema_version: 2,
    workspace_root: '/srv/project',
    groups: [],
    items,
    viewport: { x: 0, y: 0, zoom: 1 },
  };
}

describe('workspaceMoveRebind', () => {
  it('rebinds workspace-relative image and document paths under a moved directory', () => {
    const image: ImageItem = {
      ...common,
      id: 'image-1',
      type: 'image',
      w: 320,
      h: 240,
      path: 'docs/a.png',
    };
    const document: DocumentItem = {
      ...common,
      id: 'doc-1',
      type: 'document',
      w: 360,
      h: 280,
      path: 'docs/note.md',
      file_name: 'note.md',
    };

    const result = rebindCanvasLayoutPathsForMove(
      layout([image, document]),
      [{ source: '/srv/project/docs', path: '/srv/project/archive/docs', name: 'docs', kind: 'directory' }],
      '/srv/project',
    );

    expect(result.changedItemCount).toBe(2);
    expect((result.layout.items[0] as ImageItem).path).toBe('archive/docs/a.png');
    expect((result.layout.items[1] as DocumentItem).path).toBe('archive/docs/note.md');
    expect((result.layout.items[1] as DocumentItem).file_name).toBe('note.md');
  });

  it('rebinds absolute file_path items and preserves child kind on directory moves', () => {
    const directoryPath: FilePathItem = {
      ...common,
      id: 'dir-ref',
      type: 'file_path',
      w: 320,
      h: 80,
      path: '/srv/project/docs',
      kind: 'directory',
    };
    const childPath: FilePathItem = {
      ...common,
      id: 'child-ref',
      type: 'file_path',
      w: 320,
      h: 80,
      path: '/srv/project/docs/a.ts',
      kind: 'file',
    };

    const result = rebindCanvasLayoutPathsForMove(
      layout([directoryPath, childPath]),
      [{ source: '/srv/project/docs', path: '/srv/project/archive/docs', name: 'docs', kind: 'directory' }],
      '/srv/project',
    );

    expect(result.changedItemCount).toBe(2);
    expect((result.layout.items[0] as FilePathItem).path).toBe('/srv/project/archive/docs');
    expect((result.layout.items[0] as FilePathItem).kind).toBe('directory');
    expect((result.layout.items[1] as FilePathItem).path).toBe('/srv/project/archive/docs/a.ts');
    expect((result.layout.items[1] as FilePathItem).kind).toBe('file');
  });

  it('updates document file_name when a file move resolves to a renamed target', () => {
    const document: DocumentItem = {
      ...common,
      id: 'doc-1',
      type: 'document',
      w: 360,
      h: 280,
      path: 'draft.md',
      file_name: 'draft.md',
    };

    const result = rebindCanvasLayoutPathsForMove(
      layout([document]),
      [{ source: '/srv/project/draft.md', path: '/srv/project/archive/draft (2).md', name: 'draft (2).md', kind: 'file' }],
      '/srv/project',
    );

    expect(result.changedItemCount).toBe(1);
    expect((result.layout.items[0] as DocumentItem).path).toBe('archive/draft (2).md');
    expect((result.layout.items[0] as DocumentItem).file_name).toBe('draft (2).md');
  });

  it('leaves non-path and legacy asset items unchanged', () => {
    const text: TextItem = {
      ...common,
      id: 'text-1',
      type: 'text',
      w: 160,
      h: 56,
      text: 'hello',
      font_size: 16,
      color: 'var(--color-fg)',
      fill_enabled: false,
      stroke_enabled: false,
      stroke: 'var(--color-fg)',
      fill: 'var(--color-surface)',
      stroke_width: 2,
    };
    const legacyImage: ImageItem = {
      ...common,
      id: 'image-legacy',
      type: 'image',
      w: 320,
      h: 240,
      asset_id: 'abc',
    };

    const input = layout([text, legacyImage]);
    const result = rebindCanvasLayoutPathsForMove(
      input,
      [{ source: '/srv/project/docs', path: '/srv/project/archive/docs', name: 'docs', kind: 'directory' }],
      '/srv/project',
    );

    expect(result.changedItemCount).toBe(0);
    expect(result.layout).toBe(input);
  });
});
