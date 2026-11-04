# Changelog

All notable changes to westerm are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

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

[Unreleased]: https://github.com/SteelCrab/westerm/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/SteelCrab/westerm/releases/tag/v0.1.0
