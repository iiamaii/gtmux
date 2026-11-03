# gtmux

gtmux keeps terminals, files and notes together on a spatial canvas. Sessions preserve a workspace layout; terminals are live processes managed by the server.

## Choose your way in

- [Install](install.html) a server archive or the desktop app.
- [Manage your server](manager.html), choose its port and open it in the app, browser, or both.
- [Prepare external HTTPS access](external-access.html) with a managed proxy.
- [Contribute](contributing.html) or explore the [architecture](architecture.html).

## What is included

Session-aware browser titles, configurable terminal activity indicators, persistent TOML settings, reconnect-safe attach ownership, a desktop server manager, and bilingual documentation.

Activity distinguishes observed output and reported agent state. Output becoming quiet does not reliably prove that an agent finished or needs input. Disable the indicators you do not use in Settings.

## Preview status

Linux local tests cover the server and manager. Windows builds use a native ConPTY server; WSL is not required. Windows installer upgrade and relaunch have also been tested with an isolated local feed. The release workflow builds platform packages; macOS and ARM runtime validation, signing and public release delivery remain release checks. Download availability depends on the repository publishing a release. Unsigned packages can trigger OS trust prompts.

## Project homepage

The site root is the English project homepage; the header brand returns to the homepage in the current language. Documentation remains under `/en/` and `/ko/`. Screenshots show an isolated demo workspace with synthetic agent hook events, not private user sessions or live agent runs.
