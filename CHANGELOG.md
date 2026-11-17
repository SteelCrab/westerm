# Changelog

All notable changes to westerm are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/). Each entry is one short line.

## [Unreleased]

### Added

- Release files have build attestations; check one with `gh attestation verify <file> --repo SteelCrab/westerm`.

### Changed

- The plugin list (**Browse**) comes from [`westerm-dev/plugins`](https://github.com/westerm-dev/plugins).

## [0.5.0] - 2026-11-13

### Added

- Git plugin: view diffs and commits, stage files, and commit, from a built-in plugin.
- Install with `cargo install westerm --locked`, without Node.js.
- Tunnel: open westerm on another device through a Cloudflare Quick Tunnel and a 6-digit code.
- Remote windows: connect over QUIC by UDP hole punching when the router does not forward UDP.
- Agents sidebar: each folder shows its pull request and checks, and a repository's worktrees are grouped.
- Plugin bar and plugin manager in the activity bar.
- Docker plugin: list containers by compose project, follow logs, open a shell, and start or stop containers.
- External plugins: install sandboxed plugins from GitHub, an `https://` link, or a folder. See [Writing plugins](docs/plugins.md).
- Plugin API v1 (`/plugin-sdk/v1.js`), its routes under `/api/plugins/`, and the `/ws/session` keys `plugin`, `action`, and `target`.
- Databases plugin: browse PostgreSQL, MySQL/MariaDB, and SQLite, and run SQL in a table pane.
- Plugin panes: a plugin can open a pane without a terminal.

### Changed

- Desktop app: navigation to an outside address is refused instead of opening the default browser.
- Commands that westerm types into a shell are quoted for that shell (sh family or fish).
- `GET /api/plugins` lists the built-in plugins of the server's build (`builtin`).
- The PTY host changes, so open shells restart once (their screens are restored).

### Fixed

- Windows: the agents sidebar finds each shell's folder and agent.

## [0.4.5] - 2026-11-11

### Fixed

- Windows: `westerm.exe` no longer needs the Visual C++ runtime ("VCRUNTIME140.dll was not found").

## [0.4.4] - 2026-11-11

### Added

- Paste or drop images on a shell pane to attach them to Claude Code, Codex, and other agent CLIs.

## [0.4.3] - 2026-11-10

### Changed

- The focused pane has a thicker accent ring.

### Fixed

- A tall sidebar list no longer pushes the window below the browser's bottom edge.

## [0.4.2] - 2026-11-10

### Added

- Install with `npm install -g @pistacrab/westerm` or `npx @pistacrab/westerm`.
- Settings ▸ Terminal ▸ Font picks an ordered list from the installed fonts.

### Changed

- The Agents button moves to the top of the activity bar.

## [0.4.1] - 2026-11-09

### Added

- Windows: terminal-browser and terminal-code run in WSL.
- Windows: builds include Microsoft's ConPTY, which passes Kitty graphics through.

### Changed

- The PTY host changes, so open shells restart once (their screens are restored).

### Fixed

- Wide letter gaps when no font is set: monospace fonts now come before CJK fonts.

## [0.4.0] - 2026-11-08

### Added

- Agents sidebar: shells grouped by folder, with agent state, past conversations, **Start here**, and **Resume**.
- Gemini CLI: detected and resumed like Claude Code and Codex.

### Changed

- The PTY host changes, so open shells restart once (their screens are restored).

## [0.3.0] - 2026-11-07

### Added

- Desktop app for Windows and Linux (Tauri 2), from the same code as the macOS app.
- Docs pane: browse, view, and edit a folder's Markdown files.
- AI agents in shells: an AI mark shows the state of Claude Code or Codex in the pane header and the sidebar.
- A restored shell resumes the agent it ran (`claude --resume <id>` or `codex resume --last`).
- The AI mark opens the agent's card with its state, conversation id, and resume command.

### Changed

- The macOS app is now the Tauri app; the Swift app is gone.
- `GET /api/update` reports the channel `app` instead of `mac-app`.
- ⌘ terminal keys work in the macOS app only; Windows and Linux use the Ctrl+Shift keys.
- terminal-browser and terminal-code panes are much faster: the PTY host sends only changed rectangles through shared memory.
- The PTY host changes, so open shells restart once (their screens are restored).

### Fixed

- A changing Kitty image no longer repaints every other image on screen.

## [0.2.1] - 2026-11-05

### Added

- Phone-wide windows: a bottom activity bar, a drawer sidebar, and stacked panes.
- Settings ▸ Appearance ▸ Color theme: 10 themes, each with a light and a dark set.
- Settings ▸ General ▸ Workspaces and tabs: a Top or Sidebar layout, with a color and an icon per workspace.
- A terminal-browser or terminal-code pane that ends with "command not found" shows the install command.
- Remote windows: open a Linux server's westerm in its own window.
- A remote window that comes back reads the server's layout before it saves.
- Remote windows find programs on the PATH of the server user's login shell.
- `westerm remote-dashboard`, the server side of a remote window, on a private Unix socket.

### Changed

- Settings has pages: General, Appearance, Terminal, and Updates.
- The default layout is Top, so the sidebar is hidden.
- The gate passes each stream to the server's remote dashboard, not to the PTY host.
- When the gate stops, windows learn it at once and reconnect.
- `westerm stop` also stops the remote dashboard on a server.
- terminal-browser and terminal-code send only the changed parts of the picture (`TERMINAL_BROWSER_PRESENT=patched`).
- After this update, open local shells come back once as "restored".

### Removed

- The sidebar's list of workspaces; the workspace menu replaces it.
- The remote westerm pane of v0.2.0; remote windows replace it.

### Fixed

- The palette's rename commands no longer close the name field at once.

## [0.2.0] - 2026-11-05

### Added

- A disk image for the macOS app, `westerm-macos-app.dmg`.
- Workspaces: separate sets of tabs, for example one per project.
- Remote westerm: a shell on a Linux server that survives network loss, over QUIC or an SSH tunnel.

### Changed

- The macOS app explains how to update when it runs from the disk image.
- The PTY host build id is the same on every platform, so open shells come back once as "restored".
- `westerm stop` also stops a remote westerm gate.

### Fixed

- The macOS app and a browser tab no longer take sessions from each other.

## [0.1.0] - 2026-11-05

The first release.

### Added

- A web terminal dashboard drawn with [wterm](https://github.com/vercel-labs/wterm) and the Ghostty WASM core, in one Rust executable.
- Sessions that survive a closed tab, a dashboard restart, and a reboot.
- Tabs and split panes: drag to split, swap, move, and resize.
- A sidebar of tabs and sessions that shrinks to icons.
- Launchers for a shell, terminal-browser, terminal-code, and SSH hosts from `~/.ssh/config`.
- A command palette: ⌘K on macOS, Ctrl+Shift+K on Linux and Windows.
- The UI in English, Korean, Japanese, and Simplified Chinese.
- Light and dark themes, and the Ghostty font settings.
- macOS, Linux, and Windows.
- A macOS app, `westerm.app`.
- Updates from the command line, the dashboard, and the macOS app, checked against SHA-256 sums.
- Settings: language, theme, terminal font and size, and update checks.
- Keyboard switching between tabs and panes.
- `westerm stop` to save every session and stop the PTY host, and `--port 0` for any free port.
- Install scripts with SHA-256 checks: `install.sh` and `install.ps1`.

### Security

- The dashboard listens on 127.0.0.1 only and accepts only its own origin.

### Known issues

- Any program on the same computer can connect to the dashboard port; do not use westerm on a shared computer yet.
- terminal-browser and terminal-code do not work on Windows yet.
- Video in terminal-browser is choppy.
- The macOS app is not notarized, so macOS asks for approval on first open.

[Unreleased]: https://github.com/SteelCrab/westerm/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/SteelCrab/westerm/compare/v0.4.5...v0.5.0
[0.4.5]: https://github.com/SteelCrab/westerm/compare/v0.4.4...v0.4.5
[0.4.4]: https://github.com/SteelCrab/westerm/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/SteelCrab/westerm/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/SteelCrab/westerm/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/SteelCrab/westerm/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/SteelCrab/westerm/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/SteelCrab/westerm/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/SteelCrab/westerm/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/SteelCrab/westerm/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/SteelCrab/westerm/releases/tag/v0.1.0
