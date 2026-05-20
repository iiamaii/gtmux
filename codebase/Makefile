# gtmux top-level Makefile (Task C3, docs/plans/0002-work-dispatch.md §3).
#
# Drives the codegen handoff between the Rust backend (utoipa-derived
# OpenAPI 3.1) and the Svelte frontend (openapi-typescript). Also wraps
# the per-side build/test commands so a single `make` cycle exercises
# both halves.
#
# Layout decision references: ADR-0011 D5 (utoipa), ADR-0012 D7
# (utoipa -> openapi-typescript single path, A4 §A2 unified codegen).

# -- Paths ---------------------------------------------------------------------

ROOT          := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
BACKEND       := $(ROOT)/backend
FRONTEND      := $(ROOT)/frontend
SHARED        := $(ROOT)/shared
OPENAPI_FILE  := $(SHARED)/openapi.yaml
TS_TYPES_FILE := $(FRONTEND)/src/lib/types/api.d.ts
SMOKE_SCRIPT  := $(ROOT)/smoke/01_engine_connect.sh

# -- Tooling -------------------------------------------------------------------

CARGO ?= cargo
NPM   ?= npm

# -- Phony declarations --------------------------------------------------------

.PHONY: help build build-backend build-frontend test test-backend test-frontend \
        codegen codegen-backend codegen-frontend smoke clean clean-backend \
        clean-frontend clean-codegen

# -- Default -------------------------------------------------------------------

.DEFAULT_GOAL := help

help: ## List available targets.
	@echo "gtmux Makefile targets:"
	@echo ""
	@echo "  make build     Build backend (cargo) then frontend (vite)."
	@echo "  make test      Run backend tests (cargo) then frontend type-check (svelte-check)."
	@echo "  make codegen   Run backend gen-openapi -> shared/openapi.yaml -> frontend api.d.ts."
	@echo "  make smoke     Run the C4 smoke script (placeholder until C4 lands)."
	@echo "  make clean     Remove target/, node_modules/, dist/, and generated codegen artifacts."
	@echo "  make help      Show this list."

# -- Build ---------------------------------------------------------------------

build: build-backend build-frontend ## Build backend and frontend sequentially.

build-backend:
	cd $(BACKEND) && $(CARGO) build --workspace

build-frontend:
	cd $(FRONTEND) && $(NPM) run build

# -- Test ----------------------------------------------------------------------

test: test-backend test-frontend ## Run backend tests and frontend type-check.

test-backend:
	cd $(BACKEND) && $(CARGO) test --workspace

test-frontend:
	cd $(FRONTEND) && $(NPM) run check

# -- Codegen pipeline ----------------------------------------------------------
# Single cycle: Rust struct (utoipa) -> OpenAPI 3.1 YAML -> TS types.

codegen: codegen-backend codegen-frontend ## Run the full codegen cycle.

codegen-backend:
	@mkdir -p $(SHARED)
	cd $(BACKEND) && $(CARGO) run --quiet --package gen-openapi --bin gen-openapi > $(OPENAPI_FILE)
	@echo "wrote $(OPENAPI_FILE)"

codegen-frontend:
	@test -f $(OPENAPI_FILE) || { echo "missing $(OPENAPI_FILE) -- run 'make codegen-backend' first"; exit 1; }
	@mkdir -p $(dir $(TS_TYPES_FILE))
	cd $(FRONTEND) && ./codegen/run.sh

# -- Smoke (placeholder; C4 will fill it) --------------------------------------

smoke: ## Run integration smoke test (C4 task).
	@if [ -x "$(SMOKE_SCRIPT)" ]; then \
	  "$(SMOKE_SCRIPT)"; \
	else \
	  echo "Smoke test not yet implemented (C4 task)"; \
	fi

# -- Clean ---------------------------------------------------------------------

clean: clean-backend clean-frontend clean-codegen ## Remove all generated artifacts.

clean-backend:
	rm -rf $(BACKEND)/target

clean-frontend:
	rm -rf $(FRONTEND)/node_modules $(FRONTEND)/dist $(FRONTEND)/.svelte-kit

clean-codegen:
	rm -f $(OPENAPI_FILE) $(TS_TYPES_FILE)
