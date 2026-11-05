# Changelog

All notable changes to westerm are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Docs pane: a new pane that lists a folder's Markdown files as a tree and shows them in View, Edit, or Split mode. It opens in the focused shell's folder and can follow that shell's `cd`. ⌘S saves; a file changed on disk since editing started asks before it is overwritten. Unsaved changes are kept per file across reloads. In a remote window it shows the server's files.

## [0.2.1] - 2026-11-05

### Added

- Phone-wide windows (640px or less): the activity bar moves to the bottom, the sidebar opens as a drawer, and panes stack top to bottom. The split layout is kept for wider windows.
- Settings ▸ Appearance ▸ Color theme: Default, One, GitHub, Solarized, Gruvbox, Catppuccin, Tokyo Night, Rosé Pine, Everforest, or Kanagawa. It colors the window and the terminals, and each theme has a light and a dark set that follow Settings ▸ Appearance ▸ Mode.
- Settings ▸ General ▸ Workspaces and tabs picks the layout. Top: a workspace menu at the left of the tab bar, and no sidebar. Sidebar: the workspace menu at the top of the sidebar, the tabs below it, and no tab bar. Each workspace has a color and an icon, picked in the menu and saved with the layout; the menu button shows the ones of the workspace in view. A new workspace takes a color that no other workspace has, and the tabs show the color of the workspace in view.
- A terminal-browser or terminal-code pane that ends with "command not found" (exit 127) shows the official install command with a Copy button, in a local window ("on this computer") and in a remote window ("on this server"). A program that is installed but fails to start asks you to read its own message. westerm never installs anything.
- Remote windows: open a Linux server's westerm in its own window, from the activity bar or the command palette (a host from `~/.ssh/config`, or `user@host`). The window shows the server's tabs, workspaces, and shells, saved on the server. Closing the window or losing the network ends nothing, and another computer sees the same tabs. The header shows `QUIC` or `SSH tunnel`; a "Reconnecting…" banner and a "Try again" button cover link loss and permanent failures. In the macOS app, each server gets an app window that remembers its size and position.
- A remote window that comes back after sleep or a lost network reads the server's layout before it saves. If another computer changed the layout meanwhile, the window takes that layout instead of replacing it with its old one. Its disconnected panes reconnect as soon as that read is done.
- Programs in a remote window (terminal-code, terminal-browser) are found on the PATH of the server user's interactive login shell, such as folders that `~/.zshrc` adds. This applies once westerm next starts the server's host.
- `westerm remote-dashboard`, the server side of a remote window. It listens only on a Unix socket (mode 0600) and opens no TCP port. `westerm remote-setup` starts it.

### Changed

- Settings has pages, picked from a menu on the left: General, Appearance, Terminal, and Updates. Theme is now Mode, under Appearance.
- The default layout is Top, so the sidebar is hidden. Pick Sidebar in Settings to get it back.
- The gate passes each authenticated stream to the server's remote dashboard. It no longer reaches the PTY host itself. A server set up by v0.2.0 gets this version at its next connection; `westerm remote-setup` there now reports in a new format (version 2).
- When the gate stops, connected windows learn it at once and reconnect, instead of freezing for about 15 seconds.
- `westerm stop` also stops the remote dashboard on a server. It stops the gate and the remote dashboard before the host, so a window cannot start a new host in between.
- terminal-browser and terminal-code send only the parts of the picture that change, not the whole picture for every change: each pane gets `TERMINAL_BROWSER_PRESENT=patched`. In a 1350 × 780 pane, a blinking caret went from 4.4 Mbit/s to almost nothing, and a small spinner from 24 Mbit/s at 10 frames a second to 2.6 Mbit/s at about 100. Scrolling and video change the whole picture and cost the same as before. terminal-browser 0.13.4 and terminal-code 0.4.3 read the variable; terminal-code 0.2.0 does not. To send whole pictures again, run `terminal-browser config set render.presenter full`, or set `TERMINAL_BROWSER_PRESENT=full` before westerm starts.
- After this update, open local shells come back once as "restored".

### Removed

- The sidebar's list of workspaces, with dragging workspaces to reorder them and dropping tabs or panes on a workspace. The menu replaces the list; the palette's "Move tab to workspace" moves a tab.
- The remote westerm pane of v0.2.0. A saved layout drops remote panes when it loads. Their shells keep running on the server until you run `westerm stop` there.

### Fixed

- The palette's rename commands (tab, session, workspace) no longer close the name field at once.

## [0.2.0] - 2026-11-05

### Added

- A disk image for the macOS app, `westerm-macos-app.dmg`. Open it and drag westerm onto the Applications folder to install. `westerm-macos-app.zip` stays for the app's updater.
- Workspaces: separate sets of tabs, for example one per project. The sidebar lists them above the tabs; only the workspace in view shows its tabs, and the others keep running. Drop a tab or a pane on a workspace to move it there. The command palette creates, switches, renames, and closes workspaces. A layout saved before workspaces opens as one workspace.
- Remote westerm: a shell on a Linux server that keeps running when the network drops. westerm installs itself on the server over SSH (the server needs no internet access) and connects over QUIC, or through an SSH tunnel when UDP is blocked. Pick a host, then `westerm <host>`. New commands for the server side: `westerm gate` and `westerm remote-setup`.

### Changed

- The macOS app explains how to update when it runs from the disk image: move it to Applications first.
- The PTY host build id no longer depends on line ends, so the same source gives the same id on every platform. After this update, open shells come back once as "restored".
- `westerm stop` also stops a remote westerm gate.

### Fixed

- The macOS app and a browser tab no longer take single sessions from each other. One window at a time has every session; the others show "In use in another window" and take the sessions back when you switch to them or click them, with the latest saved layout.

## [0.1.0] - 2026-11-05

The first release.

### Added

- A web terminal dashboard drawn with [wterm](https://github.com/vercel-labs/wterm) and the Ghostty WASM core, served by one Rust executable with the UI inside.
- Sessions that survive a closed tab, a dashboard restart, and a reboot. A background PTY host keeps shells running and saves their screens and folders; after a reboot they come back as "restored".
- Tabs and split panes. Drag a pane onto another pane's edge to split it, onto its center to swap, or onto a tab to move it there. Drag a divider to resize. Layouts and names are saved.
- A sidebar of tabs and sessions that shrinks to icons when dragged narrow.
- Launchers for a shell, terminal-browser, terminal-code (tode), and SSH hosts from `~/.ssh/config`, including terminal-browser through an SSH host.
- A command palette: ⌘K on macOS, Ctrl+Shift+K on Linux and Windows.
- The UI in English, Korean, Japanese, and Simplified Chinese, picked from the browser language. The restore notice and host errors follow the UI language.
- Light and dark themes that follow the system, and the Ghostty font settings with Nerd Font and CJK fallbacks.
- macOS, Linux, and Windows. On Windows the dashboard talks to the PTY host over a named pipe, and the shell is PowerShell.
- A macOS app, `westerm.app` (Apple silicon and Intel), that opens the dashboard in its own window.
- Updates: `westerm update` on the command line, an update button in the dashboard, and Check for Updates in the macOS app. Downloads are checked against the release's SHA-256 sums and versions.
- Settings: language, theme (system, light, dark), terminal font and size, and automatic update checks.
- Keyboard switching between tabs and panes: Ctrl+Shift with 1–9, ] [, and . , in the browser and the app, plus ⌘1–9, ⌘⇧] ⌘⇧[, ⌘] ⌘[, and ⌥⌘ arrow keys in the macOS app.
- `westerm stop` to save every session and stop the PTY host, and `--port 0` for any free port.
- Install scripts with SHA-256 checks: `install.sh` for macOS and Linux, `install.ps1` for Windows.

### Security

- The dashboard listens on 127.0.0.1 only. Its WebSocket and layout writes accept only the dashboard's own origin.

### Known issues

- Any program on the same computer can connect to the dashboard port. The origin check stops web pages, not local programs. Do not run westerm on a computer shared with other users until access tokens arrive in a later release.
- terminal-browser and terminal-code do not work on Windows yet: Windows ConPTY does not pass their Kitty graphics through.
- Video in terminal-browser is choppy, because frames travel through the PTY as base64 text.
- The macOS app is not notarized, so macOS asks for approval on first open.

[Unreleased]: https://github.com/SteelCrab/westerm/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/SteelCrab/westerm/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/SteelCrab/westerm/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/SteelCrab/westerm/releases/tag/v0.1.0
