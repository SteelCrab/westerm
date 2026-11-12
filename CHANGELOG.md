# Changelog

All notable changes to westerm are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Git sidebar: inspect the current repository, review diffs and recent commits (full messages, changed files, and per-file patches), stage or unstage files, and commit staged changes.
- Cargo packaging for `westerm`: the crate includes the built web UI and license, and can be installed without Node.js. A separate CI check installs the packaged crate and verifies its embedded web files on macOS, Linux, and Windows. Windows Cargo installs use the system ConPTY; the bundled ConPTY files remain available through npm and the GitHub releases.
- Tunnel: the cloud button in the activity bar starts a Cloudflare Quick Tunnel and shows its `https://<name>.trycloudflare.com` address, a QR code, and a 6-digit access code. Another device's browser opens the address, enters the code, and uses the same workspaces, tabs, and shells. Wrong codes lock logins and 20 stop the tunnel; **Stop tunnel** ends every device's access. The computer stays awake while the tunnel is on, and a tunnel with no device for 12 hours stops by itself. It needs `cloudflared`; the panel shows the install command when it is missing.

### Fixed

- Windows: the agents sidebar lists each shell under its folder and shows the AI agent it runs. Before, every shell was under "Unknown folder" and no agent was found. Agents on Windows now also show their status on tabs and panes, and resume after a restart, as on macOS and Linux. PowerShell does not move its process on `cd`, so a shell with no agent stays under the folder it started in.

## [0.4.3] - 2026-11-10

### Changed

- The focused pane is easier to see. It now has a 2px accent line with a soft halo, instead of a faint 1px border. The terminal keeps its size.

### Fixed

- The window could extend below the browser's bottom edge, cutting off the lowest panes, their prompts, and the activity bar's bottom buttons. This happened when the sidebar list (folders, agents, past conversations) was taller than the window. The list now scrolls inside the sidebar.

## [0.4.2] - 2026-11-10

### Added

- Install with `npm install -g @pistacrab/westerm` or run `npx @pistacrab/westerm` (Node.js 22 or later). The npm package includes the verified release executables for macOS, Linux, and Windows, with no install scripts or compiler required.
- Settings ▸ Terminal ▸ Font picks from the fonts installed on this computer. The fonts form an ordered list (up to 8): search to add one, drag or Alt+↑/↓ to reorder, and see a preview in the stack the terminals use. Fonts with your language's script or Nerd Font icons are tagged, and names that are not installed can still be typed in.

### Changed

- The Agents button moves to the top of the activity bar, below the launchers, so the agents panel is one glance away. The command palette now leads the bottom group.

## [0.4.1] - 2026-11-09

### Added

- Windows: terminal-browser and terminal-code run in WSL. They have Linux builds only, so a pane starts them with `wsl.exe` through your login shell in the default distribution. When one is missing there, the pane shows the PowerShell command that installs it in WSL. URLs with `& | < > ^ % !` now open too.
- Windows: the release archive, the install script, the update, and the desktop app put Microsoft's ConPTY (`conpty.dll`, `OpenConsole.exe`) next to `westerm.exe`. The ConPTY built into Windows drops Kitty graphics; this one passes them through.

### Changed

- This version changes the PTY host, so open shells restart once when it starts (their screens are restored).

### Fixed

- Wide gaps between letters in the terminal when no font is set, as on Windows without a Ghostty config: Latin text took a Korean font (Malgun Gothic), which is proportional. Monospace fonts now come before the Korean, Japanese, and Chinese fonts.

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

[Unreleased]: https://github.com/SteelCrab/westerm/compare/v0.4.3...HEAD
[0.4.3]: https://github.com/SteelCrab/westerm/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/SteelCrab/westerm/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/SteelCrab/westerm/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/SteelCrab/westerm/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/SteelCrab/westerm/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/SteelCrab/westerm/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/SteelCrab/westerm/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/SteelCrab/westerm/releases/tag/v0.1.0
