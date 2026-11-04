# westerm

[![CI](https://github.com/SteelCrab/westerm/actions/workflows/ci.yml/badge.svg)](https://github.com/SteelCrab/westerm/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/SteelCrab/westerm?sort=semver)](https://github.com/SteelCrab/westerm/releases/latest)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue)](https://github.com/SteelCrab/westerm/releases/latest)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)](native)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

**We**b **S**ession **Term**inal. A web terminal whose sessions survive closing the window, restarting the program, and rebooting.

**English** · [한국어](README.ko.md) · [日本語](README.ja.md) · [中文](README.zh-CN.md)

![westerm: terminal-browser, a shell, and terminal-code split in one tab](docs/screenshot.png)

Shells, terminal-browser, terminal-code, and SSH sessions sit in tabs and split panes.
The screen is drawn by [wterm](https://github.com/vercel-labs/wterm) and the Ghostty WASM core. westerm is not an official wterm project.

Usage and design notes are in the [docs](docs/README.md) (Korean).

## Run

### Install a release

macOS and Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/SteelCrab/westerm/main/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/SteelCrab/westerm/main/install.ps1 | iex
```

Then run `westerm` and open http://127.0.0.1:4317.

- The script picks the build for your OS and CPU, checks its SHA-256, and installs it to `~/.local/bin` (macOS, Linux) or `%LOCALAPPDATA%\Programs\westerm` (Windows, added to your user PATH).
- Choose a version with `WESTERM_VERSION=v0.1.0` and a folder with `WESTERM_INSTALL_DIR`.
- Run the script again to update, then restart westerm. Open shells may come back once as "restored".
- To install by hand, download an archive from [Releases](https://github.com/SteelCrab/westerm/releases/latest):

| System | File |
|---|---|
| macOS, Apple silicon | `westerm-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `westerm-x86_64-apple-darwin.tar.gz` |
| Linux x64 (static) | `westerm-x86_64-unknown-linux-musl.tar.gz` |
| Linux arm64 (static) | `westerm-aarch64-unknown-linux-musl.tar.gz` |
| Windows x64 and arm64 | `westerm-x86_64-pc-windows-msvc.zip` |

  macOS blocks a file downloaded in a browser once. Allow it with `xattr -d com.apple.quarantine westerm`.

### macOS app

Download `westerm-macos-app.zip` (Apple silicon and Intel) from [Releases](https://github.com/SteelCrab/westerm/releases/latest), unzip it, and move **westerm.app** to Applications. It opens the same dashboard in its own window.

- If a westerm already runs on port 4317 (for example from a terminal), the app opens that one, so both show the same sessions. Otherwise it starts its own.
- Quitting the app keeps your shells running, as with the CLI. To save and stop them, run `/Applications/westerm.app/Contents/MacOS/westerm stop`.
- The app is not notarized by Apple, so macOS blocks the first open. Allow it once with `xattr -dr com.apple.quarantine /Applications/westerm.app`, or in System Settings ▸ Privacy & Security ▸ Open Anyway.
- To build it yourself: `npm run build:app` (creates `dist-app/westerm.app`).

### Build from source

Building needs Node.js 24 or later and Rust (cargo).

```bash
npm install
npm run build
npm start          # http://127.0.0.1:4317
```

- The build is a single executable: `native/target/release/westerm` (`westerm.exe` on Windows). The web UI is embedded, so **running it does not need Node.**
- Change the port with `westerm --port N` or the `PORT` environment variable.
- `westerm stop` asks the background PTY host to save every session and stop.

### Platforms

| | macOS, Linux | Windows |
|---|---|---|
| Shell | your login shell (`$SHELL`) | PowerShell 7 (`pwsh`), else Windows PowerShell |
| SSH | `ssh` and `~/.ssh/config` | the built-in OpenSSH client and `%USERPROFILE%\.ssh\config` |
| terminal-browser, terminal-code | when installed (see below) | not yet: Windows ConPTY does not pass their Kitty graphics through |

The launcher programs must be on your PATH (on macOS and Linux, your login shell's PATH):

| Launcher | Install |
|---|---|
| terminal-browser | `brew install --cask terminal-browser` |
| terminal-code | `curl -fsSL https://tode.sh/install \| bash` (the command is `tode`) |

## Develop

```bash
npm run dev:server   # Rust server (:4317), allows the Vite origin
npm run dev          # Vite HMR (:5173), proxies /api and /ws to :4317

npm test             # UI tests (node:test) + Rust tests (cargo test)
npm run type-check
```

| Path | Contents |
|---|---|
| `src/` | UI (React + wterm) |
| `src/i18n/` | UI text (한국어, English, 日本語, 中文) |
| `shared/` | Protocol types, layout tree, saved-file format |
| `native/src/server/` | Dashboard server (axum) |
| `native/src/host/` | PTY host: keeps shells alive, saves and restores screens |
| `native/src/ipc.rs` | Dashboard ↔ host channel: Unix socket, or a named pipe on Windows |
| `macos/` | macOS app: Swift window (WKWebView), icon, build script |

CI tests every pull request on macOS, Linux, and Windows. Changing PTY host code restarts running shells once, as "restored". See [session persistence](docs/sessions.md) (Korean).

## Contribute

- Report bugs and ideas in [Issues](https://github.com/SteelCrab/westerm/issues).
- Before a pull request, make `npm test` and `npm run type-check` pass.
- Read [CONTRIBUTING.md](CONTRIBUTING.md) for setup, rules, translations, and releases.
- Report security issues through [private vulnerability reporting](https://github.com/SteelCrab/westerm/security/advisories/new), not public issues.

westerm is released under the [MIT License](LICENSE). Contributions are accepted under the same license.
