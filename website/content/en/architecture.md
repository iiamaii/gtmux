# Architecture and contracts

## Process boundaries

The Rust backend owns PTYs, session layouts and attach locks. The Svelte application renders the workspace and stores browser appearance preferences. The optional manager owns a single server child, a separate local control listener and, when selected, a Caddy child. Electron provides desktop windows and tray lifetime; it does not give workspace web content Node.js access.

A managed server has its own TOML, Store and XDG directories. Loopback proxy deployment is explicit through `public_origin`; it activates cloud auth policy without binding the backend to an external interface.

## Server API

| Endpoint | Purpose |
| --- | --- |
| `GET /api/config` | Read file text, revision, saved/running configuration and restart status |
| `POST /api/config/preview` | Validate common-field edits without saving |
| `PUT /api/config` | Reauthenticated, revision-checked atomic save |
| `GET /api/server/status` | Lifecycle state and server-wide terminal/session counts |
| `POST /api/shutdown` | Reauthenticated host-owned stop request |

These routes use the existing server authentication boundary. They do not install software. The local manager API uses a distinct bootstrap cookie and same-origin requests, and is never exposed by the proxy.

## Attach ownership

The operating-system file lock remains authoritative. Lease text is diagnostic; expiry cannot override another process's lock. Within a server, owner maps, holder maps and connection registration have a fixed locking order. Last-connection generation checks and guard release share one fence. Disconnected orphan ownership is reclaimed after its monotonic timeout; active sockets are protected.

## Embedding

The HTTP library does not exit its host. Hosts explicitly supply configuration-file editing and a shutdown signal receiver. They are responsible for draining connections, shutting down the PTY backend and releasing attach guards. The standalone CLI, desktop app and third-party hosts can share the backend without sharing process ownership.
