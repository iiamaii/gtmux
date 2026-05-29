# Canvas layout contract fixtures

Cross-language contract anchors for the canvas layout schema (ADR-0018).

## Why this exists

The OpenAPI codegen chain (`gen-openapi` → `openapi.yaml` → `api.d.ts`) is a
bootstrap stub and does **not** carry the canvas `Item` schema (see
`.scratch/openapi-schema-contract/issues/01-*.md`, ADR-0042). The real
contract is hand-mirrored between BE `crates/http-api/src/schema.rs` and FE
`frontend/src/lib/types/canvas.ts`. There is no compile-time guard that the
two stay in sync.

These fixtures are an **interim drift guard**: a single golden sample that
both sides test against.

## Files

- `canvas-layout-contract.sample.json` — one representative item per
  recently-changed type, with **all** feature fields populated (box-on-text,
  text-on-figure, `font_family`, `label_auto`). Valid against the current
  `schema_version: 2`.

## Who tests it

- **BE** (live): `schema.rs::contract_sample_layout_deserializes_validates_and_round_trips`
  — asserts the sample deserializes, passes `validate()`, and round-trips stably.
- **FE** (mandated by plan-0017, once a FE test harness exists): a test that
  asserts the sample is assignable to `CanvasLayout` / `CanvasItem` types.

## Maintenance

When a canvas item field is added/changed, update **all three** in lockstep:
`schema.rs`, `canvas.ts`, and this fixture. The BE test fails loudly if the
fixture drifts from `schema.rs`. ADR-0042 replaces this manual anchor with a
generated contract.
