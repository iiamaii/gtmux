# gtmux Usage Guide — After Sign-in

> [English] · [한국어](USAGE.ko.md)
>
> What every part of the canvas does once you've signed in and picked a
> workspace session. Companion to [`QUICKSTART.md`](QUICKSTART.md)
> (install / config / first sign-in).

---

## Table of contents

1. [Session management](#1-session-management)
2. [Architecture: server · terminal server · web app — and Terminal vs Terminal panel](#2-architecture)
3. [Toolbar — every tool in detail](#3-toolbar--every-tool-in-detail)
4. [Group feature](#4-group-feature)
5. [Files tab — browse, preview, edit](#5-files-tab--browse-preview-edit)
6. [In-document find (Cmd/Ctrl+F)](#6-in-document-find)
7. [Agent / CLI control (`gtmux` CLI)](#7-agent--cli-control)

Appendix:
- [A. Keyboard shortcuts](#a-keyboard-shortcuts)
- [B. Inspector, layer tree, context menu, viewport controls](#b-other-ui-surfaces)

---

## 1) Session management

A **session** here is one named persistent workspace: one Canvas layout
file + the Terminals you've spawned inside it + the visual items (notes,
shapes, documents, …) you've placed on the canvas. One gtmux **Server
Instance** (one running server, named by `--name`) hosts as many sessions
as you want, but only **one is active per browser tab**.

> Terminology: a **Server Instance** is one running gtmux server process
> (started with `gtmux start --name <instance>`). A **session** is a saved
> workspace/layout record you pick inside the UI. They are different
> concepts — don't conflate them. (See also **Pane** = the real tmux/PTY
> unit vs. **Panel** = the canvas visual object, §2.4.)

### 1.1 Where state lives

| Artifact | Path |
|---|---|
| Canvas layout (per session) | `${XDG_DATA_HOME:-~/.local/share}/gtmux/store/<instance>/<session>.json` (session record, schema v2: `groups[]` + `items[]` + `viewport`) |
| Server token | `${XDG_STATE_HOME}/gtmux/<session>.token` (mode 0600) |
| Password hash (when set) | `${XDG_STATE_HOME}/gtmux/password.argon2` (mode 0600; Argon2id PHC, per Server Instance — **not** per session) |
| Pidfile | `${XDG_STATE_HOME}/gtmux/<session>.pid` |
| Asset uploads | `<workspace>/.assets/<sha256>` (content-addressed) |
| Server config | `${XDG_CONFIG_HOME:-~/.config}/gtmux/<session>.config.toml` |

The layout file is rewritten on every meaningful mutation (drag-commit,
inspector edit, paste, …) with a 300 ms debounce. Terminal output is
not persisted — only the panel positioning and metadata.

### 1.2 Signing in (the `/auth` page)

Sign-in itself happens on the `/auth` page, **before** the workspace
loads. gtmux uses a single union sign-in: you always have a one-time
**token** link, and you may additionally set a **password** — both are
valid at once, so you can log in with either. There is no auth "mode".
The `/auth` page shows both a token field and a password field; the
password field is only active when a password is set (the page gates it
via `GET /auth/methods`). A magic-link `?t=<token>` URL auto-submits and
then strips the token from the address bar. See
[`QUICKSTART.md`](QUICKSTART.md) for the full sign-in walkthrough.

### 1.3 The session picker (first thing after sign-in)

Once you're signed in, the **session picker** (`AuthDialog`, titled
*"Choose a workspace session"*) appears whenever no session is bound to
your browser tab yet — the first time in, after **Switch session…**, or
after the server restarted with a new identity. It is a simple
switchboard with two choices:

- **New session** — *"Start with an empty canvas."* Opens
  `NewSessionModal`; name it → the server creates the layout file and
  you drop into an empty canvas.
- **Open existing** — *"Pick from saved workspaces."* Opens
  `SessionListModal`; pick from the saved sessions.

When no session is active the picker is non-dismissable (no Esc /
backdrop / Cancel) — you must choose New or Open to proceed.

### 1.4 Active session dropdown (toolbar, top-left)

The button next to the toolbar's tool groups shows the active session
name. Click it to open `SessionListModal` directly — pick another
session and the canvas hot-swaps to it. By default the swap happens
**without a page reload** (terminals from the old session are detached
but kept alive in the server's terminal pool, so re-attaching later
restores their live state). If you turn on **Settings → Behavior →
Reload page on session switch**, the switch instead does a full page
reload to reset caches, WS state, and attach state.

### 1.5 Session menu (titlebar kebab)

The kebab `⋮` button in the titlebar opens `SessionMenu`. It has exactly
**four items** — everything auth/lifecycle-related lives in Settings (see
§1.6), not here:

| Item | Effect |
|---|---|
| **Switch session…** | Open the session picker (`SessionListModal`) to attach a different session. |
| **Import layout** | Open `ImportSessionModal` — pick a `.json` file. The server validates the schema and writes a new session file. Name conflict → 409; rename and retry. |
| **Export layout** | Open `ExportSessionModal` — downloads the current session as a JSON file. Includes layout (positions, labels, notes, references) but **not** terminal output and **not** the uploaded asset bytes. (Disabled when no session is active.) |
| **Settings…** | Open `SettingsOverlay`. |

Destructive actions (Shutdown, Close panel, Delete group with children)
always show a confirm modal; Shutdown and Rotate token additionally
require a step-up re-auth (§1.6).

### 1.6 Settings → Auth & lifecycle

`SettingsOverlay` is reached from the kebab's **Settings…**. Its
left-rail sections are **Storage · Behavior · Appearance · Components ·
Keyboard · Auth · About**. The auth/account and lifecycle actions —
which used to be guessed at as kebab items — actually live here:

**Settings → Auth** (account + credentials):

| Action | Effect |
|---|---|
| **Sign out** | Clears the auth cookie and returns to `/auth`. Flow: clear the local reconnect hint → `POST /auth/logout` → navigate to `/auth`. |
| **Rotate token** | Reissues the **server token** and signs out **every** session, including this one (BE `revoke_all` + active WebSockets closed with close code 4001). The old token link stops working; you get a fresh login link (copied to clipboard, also shown in the toast). Requires re-entering your current credential first (step-up: password if one is set, else token). Your password is unchanged. |
| **Set / Change password** | Set a password (≥ 8 chars, a letter, and a digit) to sign in with, or change the existing one (enter the current password to authorise). Password sign-in becomes active immediately — no restart, and the token still works too. |
| **Delete password** | Reverts to token-only sign-in. Confirmed with a union step-up — your **token OR** current **password** (lost-password recovery path). The cookie/session is unchanged; you can set a new password later. |
| **Status** | Read-out rows: **Token** (Present / Missing) and **Password** (Set / Not set). |

**Settings → About → Danger zone**:

| Action | Effect |
|---|---|
| **Shutdown server** | Confirm modal → step-up re-auth → the server reaps every live pane, preserves the canvas layout on disk, emits a `SERVER_SHUTDOWN` frame over WS, and exits with **code 6**. Re-enter with `gtmux start --name <instance>`. |

Step-up re-auth (ADR-0020 D16): both **Rotate token** and **Shutdown**
open a re-auth modal that re-collects your current credential (password
if set, else token) and verifies it inline before the action runs.

### 1.7 Import / Export — what's in and what's out

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

### 1.8 Attach recovery & reconnect

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
 │  · http-api crate      │  --name/--port pair (Server Instance).
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

- One **Server Instance** identity, set by `--name` and immutable for
  the process lifetime.
- HTTP API: layout GET/PUT, `GET /api/sessions`, `POST
  /api/sessions/import`, terminal pool (`GET /api/terminals`, `POST
  /api/terminals/<id>/{kill,respawn}`), asset upload (`POST
  /api/assets/upload`), file picker stat endpoint (`GET
  /api/files/stat`), auth (`POST /auth/login`, `POST /auth/logout`,
  `POST /auth/rotate`).
- WebSocket: bidirectional control + data frames. Multiplexes
  per-terminal output and notification frames, and is the channel a new
  terminal is spawned over (see §2.4).
- Cookie auth gate on every HTTP and WS handshake.
- Per-session canvas layout persistence to the session record
  `${XDG_DATA_HOME:-~/.local/share}/gtmux/store/<instance>/<session>.json`.

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
- Sidebars (left: Layers / Terminals / Files tabs + a unified search
  footer; right: inspector).
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
| Lifecycle | Spawned over the WebSocket control channel (the server fans out a `pane-spawned` frame to subscribers); managed afterward via `GET /api/terminals` and `POST /api/terminals/<id>/{kill,respawn}`. Killed only by explicit close (or supervisor reap on crash). Survives WS disconnect, browser reload, session switch. | Spawned by the Terminal tool (or paste / duplicate). Disappears on panel close (which by default also closes the terminal). |
| Persisted? | Live in-memory only. Not in layout JSON. Server restart = terminal gone. | Yes, position + label + the referenced `terminal_id` are written to the layout JSON. |
| Output stream | One `tokio::broadcast` channel per terminal | Each panel is one subscriber on the channel |
| Input stream | One master fd writer | Whichever panel is the input target (I) writes |

#### Mirror behavior

Because each Terminal is one broadcast channel, two Terminal panels
that reference the **same** `terminal_id` are *mirrors*: identical
output in both, input from either reaches the same shell. This is
useful when you want the same long-running process visible across
multiple groups or sessions on the same server.

To point a panel at an existing terminal today: right-click a Terminal
panel and choose **Change terminal…** (`changeTerminalDialog`) to attach
it to another live `terminal_id` — two panels on the same `terminal_id`
are mirrors. Plain copy/paste (Cmd/Ctrl+C / Cmd/Ctrl+V) **clones** — a
fresh terminal is spawned for the pasted panel. Cloning by default keeps
you from accidentally creating multi-input panels.

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
- **`gtmux teardown --name X`**: layout file + token + state all
  removed.

---

## 3) Toolbar — every tool in detail

The toolbar lives at the top of the workspace. From left to right:

```
[Active session dropdown] | [Select · Hand] | [Terminal] |
                            [Rect · Ellipse · Line · Free draw · Text] |
                            [Note · Snippets · Document · Image · File path · Web view] |
                                            [Undo · Redo · Lock indicator]
```

13 canvas tools live in the centre, divided into four semantic groups.
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
- Backend: the spawn request goes over the WebSocket control channel —
  the server opens a PTY pair, spawns the user's `$SHELL` as the child,
  and broadcasts a `pane-spawned` frame (carrying the fresh
  `terminal_id`) to subscribers. (There is no `POST /api/pane/new`
  endpoint; terminal-pool HTTP is `GET /api/terminals` and `POST
  /api/terminals/<id>/{kill,respawn}`.)
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
- A segmented **[Rendered | Source]** control on the node header (aria
  *Document view mode*) toggles between:
  - **Rendered**: Markdown → HTML, sanitised through DOMPurify. HTML
    documents render inside a sandboxed, origin-isolated iframe.
  - **Source**: raw text.
- Header also carries **Find in document** (§6), **Copy path**, **Change
  document**, and minimize / maximize / close. Double-click the title to
  rename; double-click the body to edit inline.

#### Web view (W)
- Drag-spawn a live view of a **web address or a workspace file**. The
  Web-view tool opens a **New web view** modal first (the node isn't
  created until you commit a valid address).
- **Address rules** — the `url` accepts exactly two forms:
  - an **`http(s)://` absolute URL**, or
  - a **workspace-relative path** to an HTML / Markdown / image file.
  - `javascript:` / `file:` / `data:` / absolute local paths / `..`
    traversal are rejected, as is the app's own origin (recursive
    embed). A loopback dev server on another port
    (`http://localhost:5173`) *is* allowed. 4 KiB cap.
  - In the **change modal** a scheme-less entry is auto-fixed: a bare
    host like `example.com` becomes `https://…`, while a loopback host
    (`localhost`, `127.0.0.1`, `[::1]`, with optional port) becomes
    `http://…`. The modal previews the normalized value ("Will be saved
    as …") before you commit. (The **CLI requires an explicit scheme** —
    see §7.)
- **Render matrix**:

  | Address | Rendered as |
  |---|---|
  | `http(s)://` remote | `<iframe src>` (browser SOP-isolated, no sandbox) |
  | local `.html` | `<iframe srcdoc sandbox="allow-scripts">` — scripts run in an opaque origin (no app-origin access, no sanitize) |
  | local `.md` | the shared Markdown render pipeline (same output as a Document node) |
  | local image | inline `<img>`, contain-fit |
  | anything else | an error state (Invalid address / Unsupported file type / Preview unavailable) |

- **Header**: globe glyph + address label · **Reload** · **Copy URL** ·
  **Open in browser** · **Change address** · minimize · maximize ·
  close. web_view supports **minimize** and participates in
  **edge-dock** with the other box-type items.
- **Open-in-browser fallback**: some sites refuse to be embedded
  (`X-Frame-Options` / `frame-ancestors`) and show a blank frame —
  cross-origin means the failure can't be detected. The node shows the
  hint *"If the site refuses to load here, use 'Open in browser'."* and
  the header's **Open in browser** button always opens the URL in a new
  tab.
- Known limits: text can blur at zoom ≠ 100 % (CSS scale); an https-served
  app blocks `http://` URLs as mixed content; maximize loads a fresh
  iframe (canvas-side scroll/SPA state does not follow).

#### Image (I)
- Drag-spawn the placeholder, then pick or upload an image via the picker.
- Picking an **existing workspace file** references it by path (no copy —
  the node follows the file). Uploading **external bytes** goes through
  the assets endpoint — server SHA256-hashes them, stores at
  `<workspace>/.assets/<sha256>`, returns `asset_id`. An image item
  carries exactly one of the two.
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

`SettingsOverlay → Keyboard` lets you reassign single-key bindings.
Overrides are saved to `localStorage`, scoped to your browser. Default
bindings:

| Group | Default keys |
|---|---|
| Mode | V (Select), H (Hand) |
| Terminal | T |
| Figures | R (Rect), O (Ellipse), L (Line), P (Free draw), T (Text) |
| Content | N (Note), D (Document), I (Image), F (File path), W (Web view) |
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

### 4.1 Data model (in the session record `<session>.json`)

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

The left sidebar has **three tabs — Layers, Terminals, Files — plus a
unified search footer** pinned at the bottom. The **Layers** tab is the
canonical Group manager.

- **Tree mode** (default): nested Groups and Panels, drag to reparent
  or reorder. Per-row icons: visibility toggle, lock toggle, type icon
  (terminal / shape / text / note / etc.), context menu (`⋮`).
- **Z mode** (toggle button on the sidebar header): flat list sorted
  by z-index descending. Read-only — no drag. Each row shows the z
  badge. Use this to debug "what's in front?" or to spot accidentally
  buried items.

The **Terminals** tab flattens out all Terminal items in the active
session (regardless of which group they sit in) for quick selection,
label rename, and the *Change terminal* flow (`ChangeTerminalModal`).
The **Files** tab browses the workspace file tree. The search footer
filters whichever tab is active.

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

Terminal item paste **clones** (a fresh terminal is spawned for each
pasted panel). To make a panel mirror an existing terminal instead, use
the panel's right-click **Change terminal…** action to attach it to a
live `terminal_id` — two panels on the same `terminal_id` are mirrors.

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

## 5) Files tab — browse, preview, edit

The left sidebar's **Files** tab browses the session's workspace tree.
Beyond navigation it is the host for reading and editing workspace
files.

### 5.1 Selecting and previewing

- **Single-click** a file selects it and shows a read-only preview in
  the right panel's **Preview** tab (text / code, Markdown, HTML, image,
  PDF). Selecting does **not** force a folded right panel open.
- **Double-click** a file explicitly reveals the preview: it expands a
  folded right panel onto the Preview tab. Double-clicking a **directory**
  toggles its expand / collapse instead.
- The preview toolbar (both inline and maximized) is, in order: the
  **[Viewer | Edit]** mode control · **Find in file** · **Download** ·
  **Copy path** · **Maximize / Restore**.

### 5.2 Files context menu

Right-click a row:

- **File**: Open in Preview · Insert as image · Insert as document ·
  Insert as file path · Copy path · Copy · **Download** · Rename ·
  Remove.
- **Directory**: Select folder · Upload here… · Paste here · Insert as
  file path · New folder · Rename · Copy path · Copy · Remove.
- **Multi-select**: Insert selected · Insert as file paths · Copy · Copy
  paths · Remove selected.

**Drag a file onto a live terminal panel** to type its path into that
shell — the path is sent as PTY *input text only* (POSIX-quoted if it
contains shell-special characters, no trailing Enter, so nothing runs
until you press Return). Dropping onto empty canvas materializes canvas
items instead.

### 5.3 In-place editing (text / Markdown / HTML)

The Preview surface can edit workspace **text, Markdown, and HTML**
files in place (image / PDF / directory are view-only).

- Switch the header's mode control from **Viewer** to **Edit** (the Edit
  segment is tinted purple — the app-wide "edit mode" signal). Markdown
  and HTML open on their **source** text and re-render on save.
- A second header row appears in edit mode: **Save** · **Undo** ·
  **Redo**, with a **●** dirty indicator on the right.
- Saving is **explicit** — there is no autosave. Save with the **Save**
  button or **Cmd/Ctrl+S** (the shortcut is intercepted, so it never
  opens the browser's save dialog).
- **Conflict-safe writes.** Editing captures the file's version
  (`If-Match`). If the file changed on disk since you started (another
  program or an agent wrote it), Save opens a **"File changed on disk"**
  dialog with **Reload (drop my changes)** or **Overwrite** — and lets
  you *copy your draft to the clipboard* first.
- **Undo / Redo** in edit mode operate on the native text-editor stack
  (Cmd/Ctrl+Z, Shift+Cmd/Ctrl+Z pass through to the textarea, not the
  canvas history stack).
- **Dirty guard**: changing the selected file, switching session /
  workspace, or leaving the browser while unsaved prompts a
  *"Discard unsaved changes?"* confirm. Drafts live in memory only.
- Under the hood, saving goes through `PUT /api/fs/file` (UTF-8 text
  only, mandatory `If-Match`; `412` on conflict, `428` if the version
  header is missing).

### 5.4 View-state persistence (silent)

Document nodes remember their scroll position and Rendered/Source mode
across canvas ↔ maximize and across a page reload — it just works, no
control to manage. (Remote web views cannot restore scroll and persist
only their URL.)

---

## 6) In-document find

`Cmd/Ctrl+F` searches **inside** the document or preview you're looking
at, instead of the left-panel search. A floating **find bar** appears in
the top-right of the surface:

- Input placeholder **Find**; matches are **literal substring,
  case-insensitive**.
- Match counter shows **current / total** (e.g. `3/12`); `0/0` in a
  warning tone when nothing matches; very large results cap at
  `5000+`.
- **Enter** = next match, **Shift+Enter** = previous (also the `↑`/`↓`
  buttons); **Esc** closes the bar.
- Highlighting uses the CSS Custom Highlight API, so the rendered DOM is
  never rewritten.

**Where Cmd/Ctrl+F routes** (first match wins):

1. Focus is in a terminal → left-panel search (terminal special-case,
   unchanged).
2. A maximized, searchable document → that document's find bar.
3. A text selection inside a document / preview → that surface's find
   bar, pre-filled with the selection.
4. A single selected document item, or the Preview tab on a searchable
   file → that surface's find bar.
5. Otherwise → the existing left-panel search behavior.

Coverage (v1): Markdown *rendered* and *source / text* views. HTML
*rendered* (sandbox iframe), PDF, and image views don't expose Find —
toggle HTML to Source to search it. Find is available while editing a
preview (it searches the draft).

---

## 7) Agent / CLI control

Everything you can do on the canvas by hand, a terminal command — or an
AI agent running inside a pane — can do through the **`gtmux` CLI**. The
CLI drives the **live** server over its HTTP API (web state only; tmux
state is still owned by tmux), so changes appear on the canvas in real
time. Full contract: `gtmux skill` (or install it for agents with
`gtmux skill install`).

### 7.1 Layout control (`gtmux layout`)

Targets are an **item UUID or an exact label**. Inside a
gtmux-spawned terminal, `--session` defaults to `$GTMUX_CANVAS_SESSION`.

```bash
gtmux layout list                      # every item: id/type/label/geometry/visibility/locked/z
gtmux layout get <target>              # one item in detail (--json for full payload)
gtmux layout connections <target>      # path connections touching an item

gtmux layout move <target>   --x <f> --y <f>
gtmux layout resize <target> --w <f> --h <f>
gtmux layout show|hide|minimize|restore <target>   # minimize: terminal/note/document/snippets/web_view
gtmux layout label <target> <text> | --clear
gtmux layout raise|raise-top|lower|lower-bottom <target>
gtmux layout edit <target> --set k=v … | --json '<partial payload>'
gtmux layout create <type> [--x --y --w --h] [--set …|--json …]
gtmux layout delete <target> [--kill-terminal] [--force]
gtmux layout group create <t1> <t2> … [--label <s>] | ungroup <g> | reparent <t> --parent <g|root>
gtmux layout align <mode> <t1> <t2> …   # left/right/top/bottom/center-h/center-v/distribute-h/distribute-v
gtmux layout batch --json '<ops[]>'     # atomic multi-op
```

`create <type>` accepts
`text|note|rect|ellipse|line|free_draw|image|document|web_view|file_path|path|snippets`
(terminals are not created here — use `terminal spawn`). Creating a
**web_view** requires an **explicit scheme** — write
`gtmux layout create web_view --set url=http://localhost:5173`, not
`localhost:5173` (the browser modal's scheme auto-fix does not apply on
the CLI). Same URL rules as §3.4.

Locked items are refused (409) unless you pass `--force`; z-order is the
four raise/lower actions only (no arbitrary z); `maximized` / viewport /
selection are not CLI-controllable.

### 7.2 Terminals (`gtmux terminal`)

```bash
gtmux terminal spawn [--x --y --w --h]     # new PTY terminal + panel
gtmux terminal mount <uuid> [--x --y]      # re-mount a pool terminal onto the canvas
gtmux terminal unmount <target>            # remove the panel, keep the PTY in the pool
gtmux terminal kill <target>               # SIGTERM the pool terminal
gtmux terminal ls                          # the live terminal pool
gtmux terminal read <target> [--tail N] [--raw]     # read recent output (ANSI-stripped by default)
gtmux terminal send <target> <text> [--no-enter]    # inject input; submits via a separate delayed CR (Enter) write — reliable for agent TUIs. --no-enter = keystrokes only
gtmux terminal send <target> --bytes <hex>          # control bytes (03 = Ctrl-C, 1b5b41 = Up)
```

`read` returns a snapshot of a lossy 128 KiB ring — long output loses
its head, and full-screen TUIs (vim, interactive `claude`/`codex`) are
near-useless to scrape; run those headless (NDJSON) if you need complete
capture. Never `send` to your own `$GTMUX_TERMINAL_ID` (self-injection).

### 7.3 Workspace, session, files, skill

```bash
gtmux workspace get --session <name>                # print session Workspace(B) root
gtmux workspace set <path> --session <name>         # re-point it
gtmux session ls | export | import | create | delete  # session records (create/delete gate on auth)
gtmux fs upload <src> --dir <ws-relative> --session <name>   # import a local file into the workspace
gtmux skill [--section <n>]                # print the embedded agent skill
gtmux skill install [--claude] [--codex] [--force]   # install it for AI agents
```

### 7.4 Letting an agent build your canvas (self-building layouts)

The intended way to use §7.1–7.3 is usually **not** typing them yourself
— it is handing them to an AI agent running *inside* your session:

1. **One-time setup**: `gtmux skill install --claude` (and/or `--codex`).
   This installs the CLI contract as an agent skill, so the agent knows
   every command, target rule, and gate without you pasting docs.
   Restart the agent CLI once so it picks the skill up.
2. **Spawn a terminal on the canvas** (toolbar Terminal tool or
   `gtmux terminal spawn`) and start your agent in it (`claude`,
   `codex`, …). Because the pane is a gtmux-spawned PTY,
   `$GTMUX_CANVAS_SESSION` and `$GTMUX_TERMINAL_ID` are pre-set — the
   agent's `gtmux` calls target **this session** automatically, and you
   watch the canvas assemble itself live around the agent's own
   terminal.
3. **Prompt it.** The agent reads current state with
   `gtmux layout list` and mutates from there. Example prompts:

   > *"Use the gtmux skill. Build a research board in this session:
   > two terminals on the left (one running the test watcher), README.md
   > as a document in the middle, and a web_view of
   > `http://localhost:5173` on the right. Label, group and align
   > everything, then leave a note with what you did."*

   > *"Tidy up this canvas: align the overlapping panels into a grid,
   > group the terminals, minimize anything I haven't touched, and label
   > the groups."*

   > *"Watch the build terminal (`gtmux terminal read`), and when it
   > fails, open the failing file as a document next to it."*

   Practical guardrails the skill already teaches the agent: locked
   items are refused without `--force`; it must never `send` to its own
   `$GTMUX_TERMINAL_ID`; viewport/selection stay user-owned.

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
| W | Web view |

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
| Shift + `1` | Fit all items in viewport (panel-aware — fits into the canvas region left visible by the open side panels, not the raw window) |

Zoom ranges from **5 % to 300 %** (`Cmd/Ctrl + Scroll`, or the viewport
pill).

### Document & preview
| Key | Action |
|---|---|
| Cmd/Ctrl + F | Open in-document find on the focused document / preview (§6); elsewhere routes to left-panel search |
| Cmd/Ctrl + S | Save (only while editing a file preview, §5.3) — does not trigger the browser save dialog |
| Enter / Shift+Enter | Next / previous find match |
| Esc | Close the find bar (first), then exit edit mode |

All single-key bindings are reassignable in **Settings → Keyboard**.

---

## B. Other UI surfaces

### Titlebar (44 px, top)

- Left: Session menu (kebab `⋮`) + brand mark + "gtmux".
- Centre: `<host>:<port> · Local` — the page host (from
  `window.location.host`) and the run mode, currently always the literal
  `Local`. No `<auth-mode>`, no session name, no `gtmux ·` prefix.
- Right: a single **Refresh page** button (full reload). Theme lives in
  **Settings → Appearance**, not the titlebar.

### Left sidebar (248 px, dockable)

- Three tabs: **Layers** (Groups + Items, tree mode / z mode),
  **Terminals** (flat per-session list of Terminal items), and **Files**
  (workspace file tree).
- A unified **search footer** pinned at the bottom filters the active
  tab.

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

- Zoom −, zoom %, zoom + (range **5 %–300 %**).
- Reset to 100%.
- **Fit all panels** — panel-aware: fits into the canvas area left
  visible by the open side panels, not the raw window.
- Selection count badge.

### Panel width (double-click the resize handle)

Double-clicking a left- or right-panel resize handle (the `ew-resize`
border) toggles the panel between its **minimum width** and a
**content-fit width** — a one-gesture way to peek at a narrow panel and
snap back, without dragging. This is distinct from folding the panel
(the fold button / collapsed rail).

### Context menu (right-click)

Adapts to whether you clicked an item, a group, or the canvas:

- **Item / canvas**: Copy / Cut / Paste / Duplicate, Group / Ungroup,
  Align …, Distribute …, Delete, Bring to front / Send to back / Bring
  forward / Send backward, Lock / Unlock, Show / Hide,
  Minimize / Maximize.
- **Group**: above + Change color, Rename, Ungroup.
- **Terminal panel**: above + **Change terminal…** (attach the panel to
  another live terminal).

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
