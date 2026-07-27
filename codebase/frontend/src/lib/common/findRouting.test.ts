// findRouting — ADR-0058 D5 priority chain (0)–(5), pure part.

import { describe, expect, it } from 'vitest';

import { resolveDocumentFindRoute, type FindRouteContext } from './findRouting';

function ctx(overrides: Partial<FindRouteContext> = {}): FindRouteContext {
  return {
    xtermFocused: false,
    findBarFocused: false,
    leftPanelSearchFocused: false,
    maximizedItemId: null,
    selectionText: '',
    selectionSurfaceKey: null,
    singleSelectedItemId: null,
    previewTabActive: false,
    hasSurface: () => false,
    ...overrides,
  };
}

const surfaces = (...keys: string[]) => (key: string) => keys.includes(key);

describe('resolveDocumentFindRoute', () => {
  it('(0) xterm focus always falls through to the ADR-0052 chain', () => {
    expect(
      resolveDocumentFindRoute(
        ctx({
          xtermFocused: true,
          maximizedItemId: 'doc1',
          previewTabActive: true,
          hasSurface: surfaces('max:doc1', 'preview'),
        }),
      ),
    ).toBeNull();
  });

  it('re-invoke while a FindBar input is focused → select-all', () => {
    expect(resolveDocumentFindRoute(ctx({ findBarFocused: true }))).toEqual({
      kind: 'select-all-find-input',
    });
  });

  it('left-panel search focus falls through despite an active searchable preview', () => {
    // ADR-0052 D2 (2) guarantee: Cmd/Ctrl+F while typing in the footer search
    // input select-alls THAT input — D5 branch (4) must not hijack it.
    expect(
      resolveDocumentFindRoute(
        ctx({
          leftPanelSearchFocused: true,
          previewTabActive: true,
          hasSurface: surfaces('preview'),
        }),
      ),
    ).toBeNull();
  });

  it('left-panel search focus falls through despite a single selected document', () => {
    // Same guarantee vs D5 branch (3).
    expect(
      resolveDocumentFindRoute(
        ctx({
          leftPanelSearchFocused: true,
          singleSelectedItemId: 'doc1',
          hasSurface: surfaces('node:doc1'),
        }),
      ),
    ).toBeNull();
  });

  it('FindBar focus takes precedence over the left-panel fall-through', () => {
    // The two inputs can never hold focus at once; assert the order anyway so
    // the precedence stays deterministic.
    expect(
      resolveDocumentFindRoute(
        ctx({ findBarFocused: true, leftPanelSearchFocused: true }),
      ),
    ).toEqual({ kind: 'select-all-find-input' });
  });

  it('(1) maximized searchable document wins over everything below', () => {
    expect(
      resolveDocumentFindRoute(
        ctx({
          maximizedItemId: 'doc1',
          singleSelectedItemId: 'doc2',
          previewTabActive: true,
          hasSurface: surfaces('max:doc1', 'node:doc2', 'preview'),
        }),
      ),
    ).toEqual({ kind: 'open-surface', key: 'max:doc1', prefill: undefined });
  });

  it('(1) prefills only when the selection lives in the maximized surface', () => {
    const base = {
      maximizedItemId: 'doc1',
      selectionText: 'needle',
      hasSurface: surfaces('max:doc1'),
    };
    expect(
      resolveDocumentFindRoute(ctx({ ...base, selectionSurfaceKey: 'max:doc1' })),
    ).toEqual({ kind: 'open-surface', key: 'max:doc1', prefill: 'needle' });
    expect(
      resolveDocumentFindRoute(ctx({ ...base, selectionSurfaceKey: null })),
    ).toEqual({ kind: 'open-surface', key: 'max:doc1', prefill: undefined });
  });

  it('(1) maximized but NOT searchable (e.g. html rendered) falls to later branches', () => {
    expect(
      resolveDocumentFindRoute(
        ctx({
          maximizedItemId: 'doc1',
          previewTabActive: true,
          hasSurface: surfaces('preview'),
        }),
      ),
    ).toEqual({ kind: 'open-surface', key: 'preview' });
  });

  it('(2) selection anchored in a searchable surface opens it with prefill', () => {
    expect(
      resolveDocumentFindRoute(
        ctx({
          selectionText: 'phrase',
          selectionSurfaceKey: 'node:doc3',
          singleSelectedItemId: 'doc9',
          hasSurface: surfaces('node:doc3', 'node:doc9'),
        }),
      ),
    ).toEqual({ kind: 'open-surface', key: 'node:doc3', prefill: 'phrase' });
  });

  it('(2) selection outside any surface does not open one', () => {
    expect(
      resolveDocumentFindRoute(
        ctx({ selectionText: 'phrase', selectionSurfaceKey: null }),
      ),
    ).toBeNull();
  });

  it('(3) single selected searchable document opens without prefill', () => {
    expect(
      resolveDocumentFindRoute(
        ctx({
          singleSelectedItemId: 'doc4',
          previewTabActive: true,
          hasSurface: surfaces('node:doc4', 'preview'),
        }),
      ),
    ).toEqual({ kind: 'open-surface', key: 'node:doc4' });
  });

  it('(3) single selected non-searchable item (terminal etc.) falls through', () => {
    expect(
      resolveDocumentFindRoute(
        ctx({ singleSelectedItemId: 'term1', hasSurface: surfaces('preview') }),
      ),
    ).toBeNull();
  });

  it('(4) preview tab + searchable kind opens the preview find', () => {
    expect(
      resolveDocumentFindRoute(
        ctx({ previewTabActive: true, hasSurface: surfaces('preview') }),
      ),
    ).toEqual({ kind: 'open-surface', key: 'preview' });
  });

  it('(4) preview tab with unsearchable kind (image/pdf/directory) falls through', () => {
    expect(
      resolveDocumentFindRoute(ctx({ previewTabActive: true })),
    ).toBeNull();
  });

  it('(5) nothing applicable → null (existing ADR-0052 chain unchanged)', () => {
    expect(resolveDocumentFindRoute(ctx())).toBeNull();
  });
});
