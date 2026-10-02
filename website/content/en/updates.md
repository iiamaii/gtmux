# Application updates

Open **Updates** in the desktop manager. **Check for updates** finds a newer stable release; **Download update** downloads and verifies it. **Automatic checks and downloads** is off by default. When enabled, the app checks at startup and every six hours. Installing is always explicit: **Install and restart** asks for confirmation and the current server password when required. Running terminal programs end; saved layouts remain. A rejected shutdown leaves the update ready without installing.

The packaged app uses electron-updater and verifies the SHA512 from release metadata. Hash verification does not replace trusted release hosting or platform code signing. The application never silently installs on ordinary exit or downgrades to an older version.

## Release preparation

Build with `GTMUX_RELEASE_REPOSITORY=owner/repo` to include the GitHub update feed. The artifact workflow supplies the current repository, derives the application version from a `vMAJOR.MINOR.PATCH` tag, and builds installer files plus update YAML and blockmaps. It only uploads workflow artifacts: it does not publish a GitHub release or deploy anything. A maintainer must review/sign packages and publish the corresponding files and metadata to the configured release feed. Keep architecture-specific metadata correct when combining matrix outputs; do not overwrite one architecture's latest YAML with another without merging its files.

Windows NSIS, macOS applications and Linux AppImage are the update targets. macOS distribution requires appropriate signing/notarization for reliable installation and updates. Unpacked builds, web-only managers, Linux archives and local packages without a configured feed show why automatic updates are unavailable. Stop an archive installation's server before replacing its binary and frontend together. Do not delete its configuration or Store.

A loopback feed test exercises the actual updater SDK's metadata lookup, download and SHA512 rejection. That checksum test does not install an update. A separate opt-in Windows test has verified real NSIS installation from 0.1.0 to 0.1.1, server shutdown, automatic relaunch, and preservation of TOML and a Store marker, using a separate test-app identity and loopback feed. No public release was needed. macOS/ARM runner results and end-to-end signed release updates remain release acceptance checks.

Installing the Windows desktop app and registering Start-menu shortcuts does not require GitHub Releases. Automatic updates do require a configured feed with a published newer version. A feed-configured preview can be installed before the first release, but checking for updates may fail until that release exists. Stable update builds keep the same application ID and publish a higher version with its installer, `latest.yml`, and blockmap to the same trusted repository; a git push alone does not publish an update.

## Reproduce the Windows installation test

Use Windows Node and an interactive Windows desktop. Stage the native Windows server, its runtime DLLs and frontend in the launcher `resources` directory first, or set `GTMUX_TEST_SERVER_RESOURCES` to that directory. From `codebase/launcher`, run:

```powershell
node test/build-windows-update-fixtures.mjs
node test/windows-update-install-smoke.mjs
```

The fixture builder creates unsigned versions 0.1.0 and 0.1.1 under a separate app ID. The test installs **gtmux Update Test**, serves updates on loopback port 39241, operates only that installer's wizard, verifies the new executable version and preserved configuration, then uninstalls the test app. It refuses an existing test profile/cache. This verifies the local Windows installation pipeline, not GitHub hosting or signed release distribution.
