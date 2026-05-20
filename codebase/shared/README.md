# shared/ — codegen handoff directory

This directory is the *machine-only* meeting point between the Rust backend
and the Svelte frontend. It exists so neither side has to reach into the
other's tree to pick up generated schema.

## Contents

| File | Producer | Consumer |
| ---- | -------- | -------- |
| `openapi.yaml` | `cargo run -p gen-openapi` (utoipa 5.x, OpenAPI 3.1) | `frontend/codegen/run.sh` (`openapi-typescript`) |

`openapi.yaml` is **regenerated** by `make codegen` from the backend's
utoipa-derived types and **must not be hand-edited**. CI verifies that
`make codegen` produces a clean diff against the committed copy
(`.github/workflows/ci.yml` `codegen-verify` job).

## Why this lives in the repo

ADR-0011 D5 and ADR-0012 D7 fix a single codegen path:

    Rust struct + `utoipa` derive
      -> `gen-openapi` binary
      -> `shared/openapi.yaml`        <-- committed
      -> `openapi-typescript`
      -> `frontend/src/lib/types/api.d.ts`  <-- committed

Committing both endpoints (the YAML *and* the `.d.ts`) keeps PR diffs
truthful about schema changes and lets `codegen-verify` catch drift
without requiring contributors to run codegen locally before opening a PR.

## Bootstrap state (Task C3, 2026-05-14)

The current `openapi.yaml` covers only stub `Group { id }` and
`Panel { id }` schemas. Real surface (`GET/PUT /api/layout` with the
G-hybrid tree from ADR-0010 / `docs/ssot/canvas-layout-schema.md`) is
populated by the `http-api` crate as it lands.
