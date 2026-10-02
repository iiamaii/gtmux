# Interface conventions

Use the existing compact workspace controls, system font stack, spacing and
light/dark color tokens. Settings should explain the outcome in user language.

## Boolean settings

Use a slide switch for persistent on/off preferences. The visual track is
28 × 16 px with a 12 px thumb; use the existing switch component in the workspace.
The manager's equivalent uses `role="switch"`, an accessible label, a visible
keyboard focus ring and reduced-motion support. A native checkbox may be the
underlying input, but must not appear as an unstyled square checkbox.

## State and confirmation

Show starting, running, stopping and failed states explicitly. Disable conflicting
actions while a request is pending. Port conflicts and write failures must show
the concrete cause without silently overwriting another configuration.

Stopping or restarting a server ends terminal programs. Explain this beside the
confirmation, preserve saved layouts, and keep credentials out of logs. Background
execution is opt-in and must leave an accessible tray control.

## Review

Check light and dark appearance, keyboard navigation, narrow windows, long paths,
errors and empty states. Windows filesystem paths must remain valid when selected,
joined, copied or converted into workspace-relative references.

The manager imports the canonical workspace tokens; packaged resources include the same CSS. Runtime interface text is English. Only documentation is translated at this stage. Browser regression checks exercise slide switches, a 700px manager window, both themes, keyboard resizing and Activity width restoration.

The desktop window, tray, executable and installer use the existing gtmux brand mark. The package icon source is `codebase/launcher/ui/icon.png`, copied unchanged from the workspace `brand.png`; `tray.png` comes from its 32px favicon. When making an unsigned Windows review build, disable signing with `signExecutable: false`; do not disable executable resource editing with `signAndEditExecutable: false`, which leaves the Electron icon in place.

The desktop first-run screen follows SettingsOverlay geometry, not only its palette: 16px section titles, 12px labels, 11px descriptions, 32px inputs/buttons, and settings rows with 14px vertical padding and a 28px gap. Labels and controls align in two columns; paths and narrow screens use a stacked layout. The workspace switch component and manager load the same `settings-controls.css` instead of separate switch implementations.
