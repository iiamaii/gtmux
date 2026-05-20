# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Project status

This repository is in the **pre-implementation** phase. The only substantive content is the design spec at `docs/sketch.md` (written in Korean). There is no source code, no build system, no test suite, and no commits yet on `master`. Do not invent build/lint/test commands — none exist until the engine is bootstrapped. When implementation begins, code goes under `codebase/`, not the repo root.

Git note: the local branch is `master` with zero commits; the project's main branch for PRs is `main`. The first commit will need to either be made on `main` directly or `master` will need to be renamed/retargeted. Until that first commit lands, `git log` / `git diff HEAD~1` / `git blame` will error — don't rely on git history for context yet; rely on `docs/sketch.md` and `docs/plans/`.

## What this project is

**gtmux** is the canonical short name for this project — use it consistently in package names, binaries, config keys, and prose. (`docs/sketch.md` uses the longer Korean title "tmux-backed Web Canvas Workspace" as the descriptive form; `gtmux` is the name.)

gtmux is a single-user web app that uses **tmux as the backend execution engine** and renders sessions/windows/panes as draggable panels on an **infinite web canvas**. tmux owns process/session lifecycle; the web app owns visual layout and interaction state. The spec (`docs/sketch.md`) is the source of truth for scope and priorities — read it before making product decisions.

## Language conventions

- **Code** (identifiers, comments, log messages, commit messages, error strings): **English**.
- **Docs** (`docs/sketch.md`, new ADRs, plans, reports, SSoT files): **Korean**, matching the existing spec.
- README, AGENTS.md, and other repo-meta files: English (this file is the example).

When generating user-facing UI strings, treat that as a separate i18n question — don't assume either language until product decides.

## Directory layout (intended)

- `codebase/` — all source code (backend + frontend). Currently empty.
- `docs/sketch.md` — full design spec (KO). Scope, MVP, security model, priorities.
- `docs/ssot/` — single-source-of-truth documents (specs that code must conform to).
- `docs/adr/` — architecture decision records.
- `docs/plans/` — implementation plans.
- `docs/reports/` — investigation / status reports.
- `docs/src/` — source-document materials referenced by other docs.

The empty `docs/` subdirectories signal an intended documentation discipline. **ADR-before-code is a hard rule**: any non-trivial architectural choice (transport protocol, state store, auth model, persistence format, framework selection, tmux integration strategy, etc.) gets an ADR in `docs/adr/` *before* implementation lands in `codebase/`. Plans (`docs/plans/`) precede multi-step work; reports (`docs/reports/`) capture investigations. Trivial choices (naming, formatting, small refactors) don't need an ADR.

The active plan is the highest-numbered file in `docs/plans/`. Check it before starting work — it describes what phase the project is currently in.

## Architectural invariants (do not violate)

These come from `docs/sketch.md` §4, §8, §13 and are load-bearing. Future code must preserve them:

1. **Two state domains, kept separate.**
   - *tmux state*: sessions, windows, panes, pane metadata, active flags, output streams. Owned by tmux, mirrored from tmux, never authored by the web app.
   - *web state*: panel geometry, visibility, minimize/maximize, lock, z-index, labels, notes, focus mode, viewport, saved layouts. Owned by the web app, persisted by the web app, never sent to tmux.
   Mixing these two stores is the primary failure mode this project is designed to avoid.

2. **tmux-native vs. web-only feature split.** Create/close/select/rename for session/window/pane → tmux commands. Hide/minimize/lock/z-index/grouping/label/note → web-only state. When adding a feature, decide which side owns it before writing code.

3. **tmux layout ≠ canvas layout.** tmux's native split layout and the canvas's free-placement layout are different concepts and must not be conflated.

4. **Security defaults are not optional, even though this is single-user.** Default bind is `127.0.0.1` or a unix socket; external exposure requires explicit opt-in. WebSocket handshakes require an auth token and origin check. All user input (pane labels, notes, names, palette commands) is untrusted — escape on render, never `dangerouslySetInnerHTML`, never string-concat into a shell, route only allowlisted tmux commands with separated argv. See §13 of the spec for the full threat model.

5. **tmux integration uses control mode** (`tmux -C` / `control mode client`), not screen-scraping or repeated `tmux` shell-outs. This is stated in §10.1 and §11.2.A and is the basis for the streaming architecture.

## Priorities (from spec §12, §15)

When deciding what to build next, follow this order — don't pull P1/P2 work forward unless P0 is solid:

- **P0**: tmux control-mode connection; session/window/pane listing; pane terminal render + input; canvas panel placement; pane list; create/close/select; layout persistence.
- **P1**: search, custom labels, focus/highlight, auto-reconnect, fit-to-view, keyboard shortcuts, destructive-action confirms.
- **P2**: layout presets, mini-map, undo/redo, panel notes, snap-to-grid, advanced filtering.

Development stages (§15): (1) engine connection → (2) basic canvas UI → (3) persistence/reconnect → (4) UX polish → (5) hardening + security. Don't jump stages.

## Working with the spec

`docs/sketch.md` is in Korean. Key terminology to preserve when translating or referencing:
- **Pane** = real tmux execution unit
- **Panel** = the visual object on the canvas representing a pane
- **Canvas Layout** = web-side layout (panel position/size/visibility)
- **tmux Layout** = tmux's native split structure inside a window

The spec explicitly excludes from scope: multi-tenancy, accounts, team permissions, sharing, and mobile polish. Do not propose features in those areas.

## Frontend canvas UI discipline

Canvas item visuals have two different layers that must not be conflated:

- **SvelteFlow wrapper state**: hover/selected bounding boxes, resize handles, drag/selection behavior. Non-minimized items use this layer for selection. Do not convert non-minimized selection into component border color changes.
- **Component chrome**: the item’s own surface, idle border, header/footer/body, clipping, and content layout. This layer may change for component-specific states, but it must not override the shared wrapper bbox unless the item is explicitly minimized.

Rules for canvas component fixes:

1. Before editing bbox, resize handle, hover, selected, minimize, or clipping behavior, identify the exact rendered node type and DOM classes. For SvelteFlow custom nodes this means confirming the wrapper class, e.g. `svelte-flow__node-${type}`, not guessing from the Svelte component name.
2. Compare at least one sibling component that already behaves correctly. For example, if a component needs visible resize handles with clipped content, check the outer/inner split used by Image-style nodes: outer host keeps `overflow: visible`, inner card/clip owns `overflow: hidden`.
3. Treat component borders and selection bbox as separate systems. Non-minimized components keep their idle border color while selected; minimized components are the exception and may express selected state through their own border when the wrapper bbox is intentionally suppressed.
4. Do not patch a visual symptom with one-off CSS until all likely paint layers are accounted for: wrapper box-shadow/outline, component border, pseudo-elements, footer/header backgrounds, overflow, z-index, resize controls, and inherited SvelteFlow styles.
5. If hover and selected differ in shape, radius, or missing edges, diagnose both states separately. They may be produced by different CSS rules.
6. Avoid adding component-specific wrapper exceptions unless the component cannot follow the shared rule. If an exception is needed, document why the shared rule fails and verify it does not change the component’s own border semantics.
7. When the user’s expected visual behavior is ambiguous, ask before editing. Examples: whether a bbox should be rounded or square, whether selected state should color the wrapper or component border, and whether a minimized item should use wrapper bbox or internal border.
8. When changing visual structure, prefer a structural fix over stacked overlays. For border/overflow conflicts, split host and clipped card layers instead of adding pseudo-element borders over existing borders.

<!-- code-review-graph MCP tools -->
## MCP Tools: code-review-graph

**IMPORTANT: This project has a knowledge graph. ALWAYS use the
code-review-graph MCP tools BEFORE using Grep/Glob/Read to explore
the codebase.** The graph is faster, cheaper (fewer tokens), and gives
you structural context (callers, dependents, test coverage) that file
scanning cannot.

### When to use graph tools FIRST

- **Exploring code**: `semantic_search_nodes` or `query_graph` instead of Grep
- **Understanding impact**: `get_impact_radius` instead of manually tracing imports
- **Code review**: `detect_changes` + `get_review_context` instead of reading entire files
- **Finding relationships**: `query_graph` with callers_of/callees_of/imports_of/tests_for
- **Architecture questions**: `get_architecture_overview` + `list_communities`

Fall back to Grep/Glob/Read **only** when the graph doesn't cover what you need.

*Caveat for current phase:* `codebase/` is empty, so the graph is near-empty and queries will return little. Until implementation starts, `docs/sketch.md` is the primary source; graph-first becomes the rule once code exists.

### Key Tools

| Tool | Use when |
| ------ | ---------- |
| `detect_changes` | Reviewing code changes — gives risk-scored analysis |
| `get_review_context` | Need source snippets for review — token-efficient |
| `get_impact_radius` | Understanding blast radius of a change |
| `get_affected_flows` | Finding which execution paths are impacted |
| `query_graph` | Tracing callers, callees, imports, tests, dependencies |
| `semantic_search_nodes` | Finding functions/classes by name or keyword |
| `get_architecture_overview` | Understanding high-level codebase structure |
| `refactor_tool` | Planning renames, finding dead code |

### Workflow

1. The graph auto-updates on file changes (via hooks).
2. Use `detect_changes` for code review.
3. Use `get_affected_flows` to understand impact.
4. Use `query_graph` pattern="tests_for" to check coverage.

## Agent skills

### Issue tracker

Local markdown — issues live as files under `.scratch/<feature-slug>/`. No git remote configured yet. See `docs/agents/issue-tracker.md`.

### Triage labels

Canonical defaults (no overrides): `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout: `CONTEXT.md` (not yet created) + `docs/adr/` at repo root. See `docs/agents/domain.md`.
