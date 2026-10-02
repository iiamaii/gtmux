# Contributing

## Development environment

Use the Rust toolchain pinned in `codebase/backend/rust-toolchain.toml` and Node.js 22 or newer. Install frontend and launcher dependencies with `npm ci` in their respective directories. Do not commit local investigation notes under `docs/`.

```sh
cd codebase/backend
cargo test --workspace --locked
cargo build -p gtmux-cli --locked
cd ../frontend
npm ci
npm run check
npm test
npm run build
cd ../launcher
npm ci
npm test
```

## Prepare a local desktop build

From the repository root, copy your backend binary and built frontend into the launcher resources:

```sh
node scripts/prepare-launcher.mjs codebase/backend/target/debug/gtmux codebase/frontend/dist
cd codebase/launcher
npm start
```

For browser-only management without Electron, run `npm run web -- --data-dir /absolute/temporary/manager-data`. It prints a management URL and waits for you to choose Start. Always use a separate data folder and unused ports for testing; do not reuse a real user's Store or settings.

## One contribution, reviewable commits

Keep feature commits separate on one integration branch. Rebase or merge deliberately after inspecting upstream changes, including `push/cloud-mode`. Do not infer that an unmerged branch reserves a feature. Explain overlap and preserve independently useful upstream behavior.

## Documentation checks

English is the source language. Every English page has a Korean counterpart with the same filename. CI requires code changes to include documentation updates and requires changed English pages to have paired Korean edits. This checks update coverage, not translation accuracy. Configuration samples remain generated inputs for reference material; do not fabricate automatic prose translation.

```sh
cd website
npm ci
npm run check
npm run build
```

The documentation workflow builds the custom static site on main. After maintainers select GitHub Actions as the Pages source and set the repository variable `GTMUX_PUBLISH_DOCS=true`, it also updates the generated `gh-pages` branch and deploys the Pages artifact. Pull requests only validate the site. Release workflows build server archives and desktop packages; secrets for signing are supplied by maintainers, never committed.

## Packaged application regression check

After `npm run pack`, set `GTMUX_TEST_APP` to the absolute unpacked executable path and run `npm run test:electron` in `codebase/launcher`. The test creates a temporary profile and unused port, starts a native server, opens a sandboxed workspace, stops the server, and closes the manager while the workspace remains open. It verifies the complete application exits successfully. It opens temporary desktop windows; do not point it at your normal application profile.
