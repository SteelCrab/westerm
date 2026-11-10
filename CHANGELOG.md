# Changelog

All notable changes to westerm are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Plugin bar: a group of buttons in the activity bar between the launchers and the bottom group. Each installed plugin has a button, which can show a number badge and a red dot, and opens the plugin's panel in the sidebar's place. The puzzle-piece button (also **Plugins…** in the command palette) opens the plugin manager, where you pick a plugin and press **Install** or **Uninstall**. The installed list is kept on the server (`plugins.json` in the state folder) and shared by all windows; a window reads it again when it gets focus. A remote window has no plugin bar. With nothing installed, the page makes no plugin request except `GET /api/plugins`, and westerm runs no `docker` process.
- Docker plugin: the first plugin. It lists the computer's containers by compose project, with a filter, a running count, and a chip for each published port. A row click opens `docker logs -f` in a shell pane, and the row's buttons and menu open a container shell, open a port in terminal-browser, and start, stop, restart, or remove (stopped containers only, after one confirmation) a container; a group row has Start all and Stop all. The Docker button shows the number of running containers and a red dot when a running container exits with a code other than 0 or becomes unhealthy. When Docker is missing or off, the panel says so (with the install command and a Copy button when the CLI is missing, and Retry when the engine is off) and recovers by itself. The server finds `docker` the way your login shell does, so a desktop app started from Finder sees the same Docker as your shells.
- New session query keys `plugin`, `action`, and `target` for `/ws/session`. They start a plugin's command in a new shell. The browser sends only the key and the container id, and the host builds the command.
- External plugins: install plugins that other people wrote, from the plugin list (**Browse**; the list will be `index.json` in `SteelCrab/westerm-plugins`, a repository that does not exist yet, so Browse shows "Could not load the plugin list" until it does, and URL and folder installs work regardless), a GitHub repository (its latest release's `westerm-plugin.tar.gz`), an `https://` link to a `.tar.gz`, or a folder on your computer (for developing, with **Reload**). A plugin is a folder with a `plugin.json` and a panel page. The panel runs in a sandboxed frame (`sandbox="allow-scripts"`, an opaque origin, no direct network requests) and talks to westerm only through messages, which the server checks against the permissions you confirmed: `run` (named programs, with any arguments, as you), `panes`, and `background`. Plugins show a badge (`built-in`, `verified`, `unverified`, `local`); an unverified or local plugin needs an "I trust …" check before **Install**. westerm looks for new versions when you open the plugin manager and once a day, never updates by itself, and asks again when an update asks for more. Package checks: `https://` only, 5 MiB packed, 10 MiB unpacked, 300 entries, regular files and folders only. A plugin that navigates away from its page is stopped. See [Writing plugins](docs/plugins.md).
- Plugin API v1 (`/plugin-sdk/v1.js`): `run`, `openShell`, `openBrowser`, `storage`, `setStatus`, and the events `ready`, `theme`, `locale`, and `panel`. `run` takes an argument array (no shell) and has limits of 10 seconds, 1 MiB per output stream, and 4 at once for a plugin, with the server's environment plus your whole login-shell environment. `openShell` (macOS and Linux; sh family and fish) types a fully quoted `env -- program args…` line into a new shell. `openBrowser` refuses westerm's own addresses in all their spellings.
- New server routes `/api/plugins/catalog`, `fetch`, `install`, `<id>/update`, `<id>/run`, `<id>/storage`, `<id>/browser`, `DELETE /api/plugins/<id>`, `/plugin-assets/<id>/…`, and `/plugin-sdk/v1.js`. The page sends `Content-Security-Policy: frame-src 'self'` (the Vite dev server too), and every plugin file is served with `Content-Security-Policy: sandbox allow-scripts; …`. Files and folders whose names start with `.` are never served, and a file gets `Access-Control-Allow-Origin: *` only when the browser loads it as a script, style, image, or font, so another website cannot read a plugin's files. **Reinstall to update** replaces the installed version in one step (`replace: true`) and keeps the plugin's saved data. `GET /api/plugins` says whether the login shell can take plugin panes (`shellPanes`); when it cannot, `openShell` fails and opens no pane. A `run` answers when the program exits, and on macOS and Linux kills the background jobs it left.
- The session query `plugin=<id>&action=run&target=<JSON array>` starts an external plugin's program in a new shell. The server checks the plugin's `run` permission, and the host checks the array again.

### Changed

- Desktop app: a window no longer opens an outside address in the default browser when the page or a frame in it navigates there. It refuses the navigation. Links that open a new window (`target="_blank"`) still open in the default browser.
- Shell commands that westerm types for you (resuming an AI agent, a plugin's command) are now quoted for your shell: single quotes for the sh family, and fish's own rules for fish. Before, a fish shell could read a quoted `\'` differently.

## [0.4.0] - 2026-11-08

### Added

- Agents sidebar: the Agents button in the activity bar opens a list of the workspace's shells grouped by the folder they run in, with the git branch, each agent's state and time, and past conversations. **Start here** opens a new shell in that folder and starts Claude Code, Codex, or Gemini CLI; **Resume** continues a past conversation. It opens as a drawer on phone-wide windows, and in a remote window it shows the server's shells.
- Gemini CLI: detected like Claude Code and Codex (AI mark, state, card), and resumed after a restore with `gemini --resume <id>` or `gemini --resume latest`. Its past conversations show in the agents sidebar.

### Changed

- This version changes the PTY host, so open shells restart once when it starts (their screens are restored).

## [0.3.0] - 2026-11-07

### Added

- Desktop app for Windows and Linux, from the same code as the macOS app (Tauri 2, in `desktop/`). Windows: `westerm-windows-app-setup.exe`. Linux: `.deb` and `.rpm` for x64 and arm64. Like the macOS app, it opens the westerm on port 4317 or starts the one it ships, and quitting it keeps your shells running. A second launch brings the open window forward. On Linux an update opens the release page; install the new package from there.
- Docs pane: a new pane that lists a folder's Markdown files as a tree and shows them in View, Edit, or Split mode. It opens in the focused shell's folder and can follow that shell's `cd`. ⌘S saves; a file changed on disk since editing started asks before it is overwritten. Unsaved changes are kept per file across reloads. In a remote window it shows the server's files.
- AI agents in shells: while Claude Code or Codex runs, the pane header and the sidebar show an AI mark with a ring for its state (working, needs input, done out of view, idle), and the tab shows a dot. macOS and Linux.
- A shell that comes back as "restored" (a reboot, a logout, `westerm stop`, a host update) resumes the agent it ran: `claude --resume <id>` with the flags it had, or `codex resume --last`. Settings ▸ Terminal ▸ Resume AI agents turns it off.
- The AI mark in a pane header opens the agent's card: its state and for how long, the conversation id, the folder, the model, the command a reboot runs (with Copy buttons), and a choice to keep this pane from resuming.

### Changed

- The macOS app is now the same Tauri app as on Windows and Linux; the Swift app (`macos/`) is gone. It keeps the window, the remote windows, the menu, and the update from `westerm-macos-app.zip`, so the old app updates to it as before. Its log moves to `~/Library/Logs/io.github.steelcrab.westerm/westerm.log`. Check for Updates now opens the dashboard's update panel.
- `GET /api/update` reports the channel `app` instead of `mac-app` for a westerm inside the desktop app.
- ⌘ terminal keys (⌘1–9, ⌘] and so on) work in the macOS app only. On Windows and Linux the app uses the Ctrl+Shift keys.
- terminal-browser and terminal-code panes are much faster. The PTY host now takes their pictures through shared memory instead of base64 text, keeps them, and sends the page only the changed rectangles, which a Worker draws off the main thread. In a 1700 × 1000 pane, scrolling went from 8 to 107 frames a second and video from 5 to 107, and the page's main thread no longer stalls (long tasks: 160–220 ms a second → 0). A remote window gets the rectangles compressed. Set `localStorage["westerm.pixels"] = "off"` in the page to get the old way back. This changes the PTY host, so shells restart once when this version starts (their screens are restored).

### Fixed

- A Kitty image that changes no longer makes wterm repaint every other image on screen. In a terminal-browser pane on the old path, a blinking caret repainted 1.8 million pixels; now it repaints the 40 that changed.

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

[Unreleased]: https://github.com/SteelCrab/westerm/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/SteelCrab/westerm/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/SteelCrab/westerm/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/SteelCrab/westerm/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/SteelCrab/westerm/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/SteelCrab/westerm/releases/tag/v0.1.0
