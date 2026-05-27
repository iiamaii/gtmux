# gtmux Usage Guide — After Sign-in

> [English] · [한국어](USAGE.ko.md)
>
> What every part of the canvas does once you're past the auth dialog
> and looking at the workspace. Companion to [`QUICKSTART.md`](QUICKSTART.md)
> (install / config / first sign-in).

---

## Table of contents

1. [Session management](#1-session-management)
2. [Architecture: server · terminal server · web app — and Terminal vs Terminal panel](#2-architecture)
3. [Toolbar — every tool in detail](#3-toolbar--every-tool-in-detail)
4. [Group feature](#4-group-feature)

Appendix:
- [A. Keyboard shortcuts](#a-keyboard-shortcuts)
- [B. Inspector, layer tree, context menu, viewport controls](#b-other-ui-surfaces)

---

## 1) Session management

A **session** is one named persistent workspace: one Canvas layout file
+ the Terminals you've spawned inside it + the visual items (notes,
shapes, documents, …) you've placed on the canvas. One gtmux server
hosts as many sessions as you want, but only **one is active per
browser tab**.

### 1.1 Where session state lives

| Artifact | Path |
|---|---|
| Canvas layout (per session) | `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.json` (schema v2: `groups[]` + `items[]` + `viewport`) |
| Auth token | `${XDG_STATE_HOME}/gtmux/<session>.token` (mode 0600) |
| Pidfile | `${XDG_STATE_HOME}/gtmux/<session>.pid` |
| Asset uploads | `<workspace>/.assets/<sha256>` (content-addressed) |
| Server config | `${XDG_CONFIG_HOME:-~/.config}/gtmux/<session>.config.toml` |

The layout file is rewritten on every meaningful mutation (drag-commit,
inspector edit, paste, …) with a 300 ms debounce. Terminal output is
not persisted — only the panel positioning and metadata.

### 1.2 The Auth dialog (first thing you see)

Triggered when the server has no active session bound to your browser
tab yet — typically the very first sign-in, after `Switch session`, or
after the server was restarted with a new identity.

Choices in the dialog:

- **[Existing session]** — pick from the list. Selecting prompts an
  *Attach confirm modal* if the layout has a recent edit conflict.
- **[New session]** — name → server creates the layout file and you
  drop into an empty canvas.

### 1.3 Active session dropdown (toolbar, top-left)

The button next to the toolbar's tool groups shows the active session
name. Click it to open `SessionListModal` directly — pick another
session and the canvas hot-swaps without a page reload (terminals from
the old session are detached but kept alive in the server's terminal
pool, so re-attaching later restores their live state).

### 1.4 Session menu (titlebar kebab)

The kebab `⋮` button in the titlebar opens `SessionMenu`. Items:

| Item | Effect |
|---|---|
| **New session** | Open `NewSessionModal`, name + create. |
| **Session list** | Open `SessionListModal` (same as toolbar dropdown). |
| **Import session** | Open `ImportSessionModal` — pick a `.json` file. Server validates schema and writes a new session file. Name conflict → 409, you can rename and retry. |
| **Export session** | Open `ExportSessionModal` — downloads the current session as a JSON file. Includes layout (positions, labels, notes, references) but **not** terminal output and **not** the uploaded asset bytes. |
| **Rotate token** | Token mode only. Issues a new token, reissues the cookie. |
| **Settings** | Open `SettingsOverlay`. Tabs cover behavior, auth, keyboard shortcuts, assets storage. |
| **Shutdown** | Confirm modal → server kills every PTY in this session, writes the layout, exits with code 0. |
| **Logout** | Three-step flow: clear local reconnect hint → `POST /auth/logout` → full reload to `/auth`. Cookie is revoked server-side. |

Destructive actions (Delete session, Shutdown, Close panel, Delete
group with children) always show a confirm modal.

### 1.5 Import / Export — what's in and what's out

`Export` writes a single session `.json`:

- ✅ Groups (tree, label, color, visibility, lock).
- ✅ Items (positions, sizes, z, visibility, lock, per-type payload —
  text content, note body, shape stroke/fill, snippet entries, document
  rendered content if inline, file path references, image asset IDs).
- ✅ Viewport (current pan / zoom).
- ❌ Live terminal stream — Terminal items carry their `terminal_id`
  (UUID) and label; on import that UUID is unknown to the pool, so the
  item ends up *dangling* (rendered with a placeholder; double-click to
  spawn a fresh shell that takes its place).
- ❌ Uploaded asset bytes (images, embedded documents) — the IDs are
  preserved but the bytes live at `<workspace>/.assets/`. Hand-copy them
  across machines if you need them.

This is fine for layout backup, sharing a canvas template, or moving a
session between machines that share an asset store. It is not a full
workspace archive.

### 1.6 Attach recovery & reconnect

When the WebSocket drops (laptop sleep, server restart, network blip):

- A **reconnect banner** appears at the top of the page.
- 1-second grace, then exponential backoff (cap 30 s), retried
  indefinitely.
- 10 consecutive failures → a `Server stopped` banner prompts you to
  restart or attach a different session.
- When the WS comes back, the FE re-attaches every Terminal panel by
  UUID — if the terminal is still alive in the pool, you get the
  live stream + scrollback; if it's gone (kill, server-restart) the
  panel shows a *dangling* badge so you can decide whether to close
  it or spawn a fresh shell in its place.

---

## 2) Architecture

Three logical tiers, all running inside one `gtmux` Rust binary plus
the browser tab.

```
 ┌────────────────────────┐
 │  Web app (browser)     │  Svelte 5 + xterm.js — owns canvas,
 │  · canvas              │  panel layout, viewport, selection,
 │  · panels              │  inspector, layer tree, clipboard.
 │  · sidebars            │
 └────────────┬───────────┘
              │ HTTP (REST · layout PUT/GET)  +  WebSocket (live)
              │
 ┌────────────▼───────────┐
 │  gtmux server          │  Rust (axum 0.8 + tokio). One process per
 │  · http-api crate      │  --session/--port pair.
 │  · ws-server crate     │  Origin / Host / CSRF middleware. Auth.
 │  · auth crate          │  Layout persistence (HTTP PUT, 300 ms debounce).
 │  · config crate        │
 └────────────┬───────────┘
              │ in-process channels
              │
 ┌────────────▼───────────┐
 │  Terminal server       │  pty-backend crate.
 │  (PTY supervisor)      │  Per Terminal: 1 PTY pair + 1 child shell.
 │  · portable-pty        │  Output → tokio::broadcast → all subscribers.
 │  · ring buffer (128KB) │  Input → master fd writer. SIGWINCH on resize.
 │  · child reaper        │  SIGTERM → 200ms → SIGKILL on close.
 └────────────────────────┘
```

### 2.1 gtmux server — what it owns

- One **session binding**, set by `--session` and immutable for the
  process lifetime.
- HTTP API: `GET/PUT /api/layout`, `GET /api/sessions`, `POST
  /api/sessions/import`, asset upload (`POST /api/assets/upload`),
  file picker stat endpoint (`GET /api/files/stat`),
  auth (`POST /auth/login`, `POST /auth/logout`).
- WebSocket: bidirectional control + data frames. Multiplexes
  per-terminal output and notification frames.
- Cookie auth gate on every HTTP and WS handshake.
- Per-session canvas layout persistence to
  `${XDG_STATE_HOME}/gtmux/<session>.json`.

### 2.2 Terminal server (`pty-backend` crate) — what it owns

- For every Terminal: a PTY master/slave pair (`portable_pty`) plus the
  child shell process.
- Output loop: master fd is read on a dedicated std::thread, bytes are
  fanned out via `tokio::broadcast<Bytes>` to every subscribed WS
  connection. A per-terminal ring buffer (128 KiB by default,
  configurable) replays history when a fresh subscriber attaches.
- Input loop: WS frames from the foreground panel write straight to
  master fd.
- Lifecycle: tokio child-watcher reaps on exit; explicit close sends
  SIGTERM → 200 ms grace → SIGKILL.
- Resize: WS `PANE_RESIZE` → `MasterPty::resize()` → TIOCSWINSZ →
  SIGWINCH to the child.

The PTY supervisor lives **inside** the gtmux process, so there is no
separate terminal daemon to bring up.

### 2.3 Web app — what it owns

Everything **visual** and **layout-related**:

- Canvas: an infinite, pan/zoom workspace rendered with custom
  SvelteFlow integration.
- Panels: positions, sizes, z-index, lock, visibility, minimized/
  maximized state, label, color, notes.
- Selection model: **M** (manipulation selection — items you're about
  to act on) is orthogonal to **I** (input target — the one Terminal
  panel that receives keystrokes).
- Sidebars (left: layer tree + terminal list; right: inspector).
- Toolbar, viewport controls, command palette, context menu.
- Local FE clipboard (page lifetime), undo/redo stack (50 entries,
  in-memory).
- Custom keyboard shortcut overrides via `localStorage`.

### 2.4 Terminal vs Terminal panel — the most important distinction

This separation is load-bearing:

|  | **Terminal** (backend) | **Terminal panel** (frontend) |
|---|---|---|
| What it is | 1 PTY + 1 child shell process | 1 canvas item rendering one Terminal |
| Identifier | `terminal_id` (UUID, server-issued) | item `id` (UUID, FE-issued) + reference to a `terminal_id` |
| Owner | gtmux server (`pty-backend`) | Canvas layout (`items[]`) |
| Cardinality | 1 process at a time | N panels can reference the *same* terminal |
| Lifecycle | Spawned on `POST /api/pane/new`. Killed only by explicit close (or supervisor reap on crash). Survives WS disconnect, browser reload, session switch. | Spawned by the Terminal tool (or paste / duplicate). Disappears on panel close (which by default also closes the terminal). |
| Persisted? | Live in-memory only. Not in layout JSON. Server restart = terminal gone. | Yes, position + label + the referenced `terminal_id` are written to the layout JSON. |
| Output stream | One `tokio::broadcast` channel per terminal | Each panel is one subscriber on the channel |
| Input stream | One master fd writer | Whichever panel is the input target (I) writes |

#### Mirror behavior

Because each Terminal is one broadcast channel, two Terminal panels
that reference the **same** `terminal_id` are *mirrors*: identical
output in both, input from either reaches the same shell. This is
useful when you want the same long-running process visible across
multiple groups or sessions on the same server.

To create a mirror today: copy a Terminal panel with **Cmd/Ctrl+C** and
paste as mirror via the context menu (`Paste as mirror`, when
available). Plain paste (Cmd/Ctrl+V) **clones** — a fresh terminal is
spawned. The default keeps newcomers from accidentally creating
multi-input panels.

#### What survives what

- **WS disconnect**: terminal stays alive, ring buffer keeps producing.
  Reconnect replays buffer + resumes live stream.
- **Browser tab close**: same as WS disconnect (terminal survives).
- **Session switch in the same tab**: terminal stays alive in the
  server's pool even though no panel currently references it. Switch
  back → panel re-attaches.
- **Server restart (`gtmux stop` + `start`)**: terminals do **not**
  survive. Layout JSON is replayed on next attach but
  every panel becomes *dangling*; double-click to spawn a fresh shell
  into the same panel slot.
- **`gtmux teardown --session X`**: layout file + token + state all
  removed.

---

## 3) Toolbar — every tool in detail

The toolbar lives at the top of the workspace. From left to right:

```
[Active session dropdown] | [Select · Hand] | [Terminal] |
                            [Rect · Ellipse · Line · Free draw · Text] |
                            [Note · Snippets · Document · Image · File path] |
                                            [Undo · Redo · Lock indicator]
```

12 canvas tools live in the centre, divided into four semantic groups.
A divider sits between groups. Tools are **one-shot** by default — you
spawn one item and the toolbar bounces back to Select. Hold the lock
(press **Q** while a tool is active) to keep spawning the same type
until you press **Esc**.

All canvas tools require an active session. With no session attached
the icons are disabled.

### 3.1 Mode group

#### Select (V)
- Default mode. Click selects, Shift-click adds, Cmd/Ctrl-click toggles,
  drag-on-empty starts a lasso.
- The set of selected items is the **M** (manipulation selection).
- The Inspector (right panel) populates from M.
- Click on a Terminal panel's terminal area separately sets the
  **I** (input target) — only that one panel receives keystrokes,
  no matter how many panels are in M.

#### Hand (H)
- Click-drag pans the canvas. M is not cleared.
- Hold **Space** in any tool for a temporary Hand (release returns
  to the prior tool).

### 3.2 Terminal group

#### Terminal (T)
- Click on the canvas to spawn a Terminal panel.
- Backend: `POST /api/pane/new` opens a PTY pair, spawns the user's
  `$SHELL` as the child, returns a fresh `terminal_id`.
- Frontend: a panel mounts at the click position with default size
  (~640 × 360), attaches to the broadcast channel, and grabs the input
  target.
- Auto-mount cascade: if you spawn several terminals in quick
  succession the FE offsets each by ~40 px so they don't stack
  perfectly.
- Output is rendered with **xterm.js** (256-colour, true-colour, full
  scrollback up to ring buffer size). Input goes straight to the
  child shell.
- Close: clicking the panel **×** opens `PanelCloseConfirmModal`. Confirm
  → SIGTERM → 200 ms grace → SIGKILL → panel removed + layout saved.
- Closing the panel by **Delete** key does the same.
- Resize: drag the panel handle → SIGWINCH propagates to the shell, so
  full-screen TUIs (vim, htop, …) re-flow correctly.

### 3.3 Figure group

Vector primitives. All drag-spawn: click-drag from the starting corner
to the opposite corner.

#### Rectangle (R)
- Filled / stroked / both. Inspector fields: stroke, stroke width
  (1–32), stroke dash, fill, fill enabled, stroke enabled, corner
  radius.

#### Ellipse (O)
- Same as Rectangle minus corner radius.

#### Line (L)
- Two-click: click start, click end. Inspector fields: stroke,
  stroke width, stroke dash (solid / dash / dot).

#### Free draw (P)
- Click-drag a freehand stroke. Point array stored verbatim (no
  simplification yet). Stroke + width editable in the Inspector after
  the fact.

#### Text (T)
- Drag-spawn the text box, then auto-enter edit mode (cursor in the
  box). **Esc** or click-out commits.
- Inspector fields: content, font size (1–96), text align (left /
  centre / right), vertical align (top / middle / bottom), colour,
  font weight (light / normal / bold), italic, underline, strikethrough.
- ⚠ The shortcut **T** is shared between Terminal and Text — the
  toolbar resolves it based on the current focus group (the Text tool
  is reachable from inside the Content group by re-pressing **T** when
  Text was last used).

### 3.4 Content group

Items that carry user content (notes, snippets, documents, images,
file paths). All drag-spawn.

#### Note (N)
- Drag-spawn a sticky note. Editable title + multiline body inline.
- Inspector: title, body, colour (hex picker).

#### Snippets
- Drag-spawn a snippet collection. Each entry is `{ key, body }`.
- Each key shows as a chip on the node — clicking the chip copies the
  body to your clipboard.
- Edit via the per-snippet edit panel (`SnippetEditPanel`). Delete
  via per-snippet trash icon → `SnippetDeleteConfirmModal`.

#### Document (D)
- Drag-spawn a document viewer. Two content modes:
  - Inline: paste / type Markdown directly (≤ 64 KB, stored in the
    layout JSON).
  - Asset-backed: upload a file → server stores it under
    `<workspace>/.assets/<sha256>` and the item carries the
    `asset_id`.
- Three render modes cycle through a button on the node:
  - **Rendered**: Markdown → HTML, sanitised through DOMPurify.
  - **Interactive**: sandboxed iframe — full HTML/JS but isolated
    origin.
  - **Source**: raw text.

#### Image (I)
- Drag-spawn the placeholder, then upload an image via the picker.
- `POST /api/assets/upload` — server SHA256-hashes the bytes, stores
  at `<workspace>/.assets/<sha256>`, returns `asset_id`.
- Supported: PNG, JPEG, WebP, GIF.
- Max size: `[assets].max_size_bytes` (default 50 MiB, sample sets
  100 MiB).

#### File path (F)
- Drag-spawn the node; the **File picker modal** auto-opens.
- The picker traverses allowed roots only. Defaults to your home
  directory and is configurable.
- Server-side `GET /api/files/stat` returns file metadata.
  No file contents are uploaded — the node is a *reference* to a path.
- Double-click the node to reopen the picker.

### 3.5 History group (right side of toolbar)

#### Undo (Cmd/Ctrl+Z)
- Reverts the last layout mutation: drag commit, inspector edit,
  align, paste, delete, group/ungroup, z-order swap.
- Out of scope: terminal input, viewport pan/zoom, session lifecycle.
- Stack is in-memory per session (50 entries). Reload loses the stack.
- Terminal item resurrection: if undo would bring back a panel whose
  `terminal_id` was already removed from the pool, the action is
  refused with a toast (preserves the rest of the stack).

#### Redo (Shift+Cmd/Ctrl+Z)
- Inverse of Undo.

### 3.6 Lock indicator

A small **Q** chip appears on the right side of the toolbar whenever
the active tool is Q-locked (sticky). Press **Q** again or **Esc** to
release.

### 3.7 Customising shortcuts

`SettingsOverlay → Shortcuts` lets you reassign single-key bindings.
Overrides are saved to `localStorage`, scoped to your browser. Default
bindings:

| Group | Default keys |
|---|---|
| Mode | V (Select), H (Hand) |
| Terminal | T |
| Figures | R (Rect), O (Ellipse), L (Line), P (Free draw), T (Text) |
| Content | N (Note), D (Document), I (Image), F (File path) |
| History | Cmd/Ctrl+Z (Undo), Shift+Cmd/Ctrl+Z (Redo) |
| Tool lock | Q |
| Cancel | Esc |

See [Appendix A](#a-keyboard-shortcuts) for the full canvas-shortcut
matrix.

---

## 4) Group feature

A **Group** is a web-only hierarchical container for organizing related
canvas items. Groups have label, colour, visibility, lock, and an
ordered list of children. Groups do
**not** carry geometry of their own: the group's visual bounds are
derived from its children every render.

### 4.1 Data model (in `<session>.json`)

```
groups[ {id, parent_id, label, color, visibility, locked, order} ]
items[  {id, type, parent_id, x, y, w, h, z, visibility, locked, label, ...} ]
```

Both `Group.parent_id` and `Item.parent_id` are `null` for canvas-root
children. Tree depth has no explicit cap; the schema is recursive.

### 4.2 Creating and removing groups

| Action | Trigger | Result |
|---|---|---|
| **Group** | Multi-select M ≥ 1 → context menu **Group** | New Group node wraps all M children. Group inherits no geometry. |
| **Ungroup** | Group selected → context menu **Ungroup** | Children promoted to `Group.parent_id`. Group node deleted. Non-destructive. |
| **Delete group** | Group selected → Delete / Backspace | `GroupCloseConfirmModal`. Confirm → all descendant Terminal items also kill their PTYs (destructive). |
| **Move into group** | Drag a row in the layer tree onto a Group header | Reparent. Group bounds re-derived. |
| **Move between groups** | Drag a row to a different Group in the layer tree | Same. Canvas-drag reparenting is P1+ — for now, the layer tree is the way. |

### 4.3 Properties

| Property | Behaviour |
|---|---|
| **Label** | Free-text. Edit inline in the layer tree (double-click). Empty label → "(unnamed)" placeholder. |
| **Colour** | Optional. Shown as the group's accent band in the layer tree and as a thin tint band on each descendant panel's header. Picker via context menu → **Change color**. |
| **Visibility** | Eye icon in the layer tree row. Effective visibility = `self AND all ancestors`. Hidden groups dim and stop rendering descendant panels. |
| **Lock** | Padlock icon. Effective lock = `self OR any ancestor`. Locked items can't be dragged, deleted, or hit by alignment. |
| **Order** | Sibling rank in the layer tree only (no z impact — see §4.5). Drag rows in tree mode to reorder. |
| **Geometry / resize** | Not first-class state in MVP. Bounds derived from children. Group resize is P1+ (Group spatial frame). |

### 4.4 Layer tree sidebar

The left sidebar's **Layer tree** is the canonical Group manager.

- **Tree mode** (default): nested Groups and Panels, drag to reparent
  or reorder. Per-row icons: visibility toggle, lock toggle, type icon
  (terminal / shape / text / note / etc.), context menu (`⋮`).
- **Z mode** (toggle button on the sidebar header): flat list sorted
  by z-index descending. Read-only — no drag. Each row shows the z
  badge. Use this to debug "what's in front?" or to spot accidentally
  buried items.

The sibling sidebar tab — **Terminal list view** — flattens out all
Terminal items in the active session (regardless of which group they
sit in) for quick selection, label rename, and the *Connect to existing
terminal* flow (`ChangeTerminalModal`).

### 4.5 Z-index and Groups

z-index and the tree are **independent**. Reordering rows in
the layer tree changes only `order` (organisational); z stays put.
Conversely, z-order changes (Bring to front, Send backward, …) do not
touch the tree.

z lives on items only — Groups have no z. All items share one global
z-space, so a child of Group A can be in front of a child of Group B
without violating the tree.

Mutations:

| Action | Key | Effect |
|---|---|---|
| Bring forward | `]` | z++ swap with the next-higher item |
| Send backward | `[` | z-- swap with the next-lower item |
| Bring to front | `Shift + ]` | z = global max + 1 |
| Send to back | `Shift + [` | z = global min − 1 |

Newly spawned items get `z = global_max + 1` → they always appear in
front.

### 4.6 Clipboard with sub-trees (Cmd/Ctrl+C / X / V / D)

The FE clipboard (page-lifetime, in-memory) understands Groups:

- **Copy** a Group → the entire sub-tree (Group + every descendant
  Group + every descendant Item) is serialised into the clipboard.
- **Cut** a Group → copy + delete in one history entry.
- **Paste** → the sub-tree is re-instantiated with **fresh UUIDs** for
  every Group and every Item. Positions are offset by 24 px from the
  source bounding-box. Relative positions inside the sub-tree are
  preserved.
- **Duplicate** = paste without touching the clipboard.

Terminal item paste defaults to **Clone** (a fresh terminal is spawned
for each pasted panel). The context menu offers **Paste as mirror**
when the source terminal still exists in the pool — the pasted panel
shares the *same* `terminal_id` as the original.

### 4.7 Multi-select and bulk operations

When M ≥ 2:

- Alignment: left / centre / right / top / middle / bottom + Distribute
  H / Distribute V — buttons in the Inspector and in the context menu.
- Bulk visibility / lock toggle from the layer tree (toggle group
  ancestor → AND/OR propagation handles the rest).
- Bulk z-order: applies to the M set, preserving relative z order
  among them.
- Bulk delete: one confirm modal listing the doomed Terminal IDs.

---

## A. Keyboard shortcuts

Canvas focus required (the canvas-root element, not a terminal panel
text area).

### Tools
| Key | Tool |
|---|---|
| V | Select |
| H | Hand |
| T | Terminal (or Text — last-used wins when both are reachable) |
| R | Rectangle |
| O | Ellipse |
| L | Line |
| P | Free draw |
| N | Note |
| D | Document |
| I | Image |
| F | File path |

### Tool modifiers
| Key | Action |
|---|---|
| Q | Toggle lock on the active tool (sticky) |
| Esc | Release lock and return to Select; also exit text-edit / picker modals |
| Space (hold) | Temporary Hand pan |
| Cmd/Ctrl + Scroll | Zoom canvas in / out |

### Selection
| Key | Action |
|---|---|
| Click | Select item (also sets I if it's a Terminal panel) |
| Shift + Click | Add to selection |
| Cmd/Ctrl + Click | Toggle in selection |
| Lasso drag (Select tool, empty canvas) | Rectangular multi-select |
| Cmd/Ctrl + A | Select all visible items in current scope |
| Esc | Clear selection (after exiting tool lock) |

### Move (nudge)
| Key | Distance |
|---|---|
| Arrow keys | 1 px |
| Shift + Arrow | 8 px |
| Cmd/Ctrl + Arrow | 64 px |

### Clipboard
| Key | Action |
|---|---|
| Cmd/Ctrl + C | Copy selection (Groups bring their sub-tree) |
| Cmd/Ctrl + X | Cut |
| Cmd/Ctrl + V | Paste (24 px offset) |
| Cmd/Ctrl + D | Duplicate |

### Z-order
| Key | Action |
|---|---|
| `]` | Bring forward |
| `[` | Send backward |
| Shift + `]` | Bring to front |
| Shift + `[` | Send to back |

### History
| Key | Action |
|---|---|
| Cmd/Ctrl + Z | Undo |
| Shift + Cmd/Ctrl + Z | Redo |

### Lifecycle
| Key | Action |
|---|---|
| Delete / Backspace | Delete selection (Group with children → confirm) |

### Viewport
| Key | Action |
|---|---|
| `0` | Reset zoom to 100% |
| Shift + `1` | Fit all items in viewport |

All single-key bindings are reassignable in **Settings → Shortcuts**.

---

## B. Other UI surfaces

### Titlebar (44 px, top)

- Left: Session menu (kebab `⋮`).
- Centre: `gtmux · <session> · <host>:<port> · <auth-mode>`.
- Right: Theme toggle (light / dark), Focus mode toggle (P1+).

### Left sidebar (248 px, dockable)

- Tab 1: **Layer tree** (Groups + Items, tree mode / z mode).
- Tab 2: **Terminal list view** (flat per-session list of Terminal items).

### Right panel — Inspector

- Shown when M ≥ 1.
- Common section: x, y, w, h, z, visibility, locked, label.
- Type-specific section: differs per item type (text styles, stroke /
  fill for shapes, snippet entries, document content, image asset,
  file path, etc.).
- Mixed values (M items with different values) show a placeholder
  rather than blanking the field.
- Alignment buttons appear when M ≥ 2.

### Viewport controls (bottom centre pill)

- Zoom −, zoom %, zoom +.
- Reset to 100%.
- Fit all.
- Selection count badge.

### Context menu (right-click)

Adapts to whether you clicked an item, a group, or the canvas:

- **Item / canvas**: Copy / Cut / Paste / Duplicate, Group / Ungroup,
  Align …, Distribute …, Delete, Bring to front / Send to back / Bring
  forward / Send backward, Lock / Unlock, Show / Hide,
  Minimize / Maximize.
- **Group**: above + Change color, Rename, Ungroup.
- **Terminal panel**: above + Connect to existing terminal (`Change
  terminal`).

### Reconnect banner (32 px, conditional)

Auto-shows when the WS drops. Includes retry countdown and a manual
retry button. Auto-hides when the connection comes back.

### Command palette (Cmd/Ctrl + K, when bound)

Lists every named action — tool changes, alignment, z-order,
shortcuts, group ops. Useful when you've forgotten a shortcut.

---

## See also

- [`QUICKSTART.md`](QUICKSTART.md) — install, config, auth, session
  creation.
- [`README.md`](README.md) — project overview.
