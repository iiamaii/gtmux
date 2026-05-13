#!/usr/bin/env bash
# frontend codegen entrypoint (Task C3).
#
# Consumes the OpenAPI 3.1 YAML produced by the Rust `gen-openapi`
# binary and emits TypeScript types into the frontend tree.
#
# Invariant: this script is the *only* writer of `src/lib/types/api.d.ts`.
# Hand edits will be clobbered on the next `make codegen`.
#
# Layout decision references: ADR-0012 D7 (utoipa -> openapi-typescript
# single path), ADR-0011 D5.

set -euo pipefail

cd "$(dirname "$0")/.."

INPUT="../shared/openapi.yaml"
OUTPUT="src/lib/types/api.d.ts"

if [[ ! -f "${INPUT}" ]]; then
  echo "error: ${INPUT} not found -- run 'make codegen-backend' first" >&2
  exit 1
fi

mkdir -p "$(dirname "${OUTPUT}")"
npx --no-install openapi-typescript "${INPUT}" --output "${OUTPUT}"
echo "codegen complete: ${OUTPUT}"
