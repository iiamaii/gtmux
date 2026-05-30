import { describe, it, expect } from 'vitest';
import sample from '../../../../shared/contract/canvas-layout-contract.sample.json';
import { isText, isRect, isEllipse, type CanvasItem } from '$lib/types/canvas';

// ADR-0042 / plan-0017 Phase 4 — FE-side cross-language contract guard.
//
// The same golden fixture the BE test deserializes
// (`schema.rs::contract_sample_layout_deserializes_validates_and_round_trips`)
// is asserted here against the FE-facing types. This is the FE half of the
// shared-anchor drift guard: the fixture is the agreed BE↔FE contract, and
// both sides validate the *same* file. Enum drift is additionally caught at
// compile time via the generated-schema aliases in `canvas.ts`.

describe('canvas layout contract (FE side)', () => {
  const items = sample.items as unknown as CanvasItem[];

  it('fixture envelope matches CanvasLayout shape', () => {
    expect(sample.schema_version).toBe(2);
    expect(Array.isArray(sample.groups)).toBe(true);
    expect(sample.items.length).toBeGreaterThan(0);
  });

  it('carries the feature surface (box-on-text, embedded text, font, label_auto)', () => {
    const text = items.find(isText);
    expect(text).toBeDefined();
    // box-on-text (ADR-0040) + font (ADR-0041) + label_auto (ADR-0040 D9).
    expect(text?.stroke_enabled).toBe(true);
    expect(text?.fill_enabled).toBe(true);
    expect(text?.corner_rounded).toBe(true);
    expect(text?.font_family).toBe('serif');
    expect(text?.label_auto).toBe(true);

    const rect = items.find(isRect);
    expect(rect?.text).toBe('Label inside rect'); // text-on-figure (ADR-0040)
    expect(rect?.font_family).toBe('mono');
    expect(rect?.label_auto).toBe(false);

    const ellipse = items.find(isEllipse);
    expect(ellipse?.text).toBe('Circle text');
  });

  it('every item type narrows to a known CanvasItem variant', () => {
    const known = new Set([
      'terminal', 'text', 'note', 'rect', 'ellipse', 'line',
      'free_draw', 'image', 'document', 'file_path', 'snippets', 'path',
    ]);
    for (const it of items) {
      expect(known.has(it.type)).toBe(true);
    }
  });
});
