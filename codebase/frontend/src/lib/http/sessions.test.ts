import { describe, expect, it } from 'vitest';

import { parseLayoutBody } from './sessions';
import type { CanvasLayout } from '$lib/types/canvas';

function layout(workspaceRoot?: string): CanvasLayout {
  return {
    schema_version: 2,
    ...(workspaceRoot === undefined ? {} : { workspace_root: workspaceRoot }),
    groups: [],
    items: [],
    viewport: { x: 0, y: 0, zoom: 1 },
  };
}

describe('session layout response parsing', () => {
  it('reads workspace_root from a raw layout body', () => {
    const parsed = parseLayoutBody(layout('/srv/project'));
    expect(parsed.workspace_root).toBe('/srv/project');
    expect(parsed.layout.workspace_root).toBe('/srv/project');
  });

  it('prefers the envelope workspace_root when present', () => {
    const parsed = parseLayoutBody({
      layout: layout('/srv/project-from-layout'),
      workspace_root: '/srv/project-from-envelope',
    });
    expect(parsed.workspace_root).toBe('/srv/project-from-envelope');
  });
});
