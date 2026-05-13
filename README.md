# codebase/ — gtmux source tree

This directory holds all gtmux source code. Documentation, ADRs, plans, and
reports live one level up under `../docs/`.

## Layout

    codebase/
      backend/    Rust workspace (axum + tokio + tokio-tungstenite).
                  Seven crates per ADR-0011 D10 + two binaries:
                    crates/{mux-router, ws-server, http-api,
                            lifecycle, config, auth}
                    bin/{gtmux-cli, gen-openapi}
      frontend/   Svelte 5 + Vite + TypeScript app (ADR-0012).
                  Codegen entrypoint: codegen/run.sh.
      shared/     Machine-only handoff between backend and frontend.
                  Currently holds the generated openapi.yaml. See
                  shared/README.md.
      smoke/      Integration smoke scripts (populated by Task C4).
      Makefile    Top-level orchestrator.

## Bootstrap

The top-level `Makefile` is the single entrypoint. From this directory:

    make help       List targets.
    make codegen    Rust utoipa -> shared/openapi.yaml -> TS types.
    make build      cargo build --workspace, then vite build.
    make test       cargo test --workspace, then svelte-check.
    make smoke      Run the C4 integration smoke (placeholder until C4).
    make clean      Remove target/, node_modules/, dist/, codegen outputs.

Run `make codegen` before `make build` on a fresh clone so the generated
TypeScript types under `frontend/src/lib/types/api.d.ts` exist.

## Codegen path

A single direction, per ADR-0012 D7 (and ADR-0011 D5):

    Rust struct + utoipa derive
      -> cargo run -p gen-openapi
      -> shared/openapi.yaml          (committed)
      -> openapi-typescript
      -> frontend/src/lib/types/api.d.ts  (committed)

Both endpoints are committed; CI's `codegen-verify` job
(`.github/workflows/ci.yml`) rejects PRs that change the source but
forget to regenerate.

## Layout decision references

- `docs/adr/0011-backend-stack-rust.md` D5, D10 — Rust crates and utoipa.
- `docs/adr/0012-frontend-stack-svelte.md` D7 — openapi-typescript pairing.
- `docs/sketch.md` §10.1 — backend module map.
- `docs/sketch.md` §10.2 — frontend module map.
- `docs/ssot/canvas-layout-schema.md` — eventual schema target for codegen.
- `docs/ssot/wire-protocol.md` — note: binary WS envelope, not part of the
  OpenAPI surface.
- `docs/ssot/security-defaults.md` — note: configuration, not an API surface.

## Bootstrap state (2026-05-14)

This tree is in C1/C2/C3 bootstrap. The `gen-openapi` binary emits a
stub schema (`Group { id }`, `Panel { id }`) covering only enough to
prove the codegen pipeline. Real surface lands incrementally with the
`http-api` crate.
