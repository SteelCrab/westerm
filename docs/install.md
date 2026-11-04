# Install

## Install script

macOS and Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/SteelCrab/westerm/main/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/SteelCrab/westerm/main/install.ps1 | iex
```

- The script picks the build for your OS and CPU, checks its SHA-256, and installs it to `~/.local/bin` (macOS, Linux) or `%LOCALAPPDATA%\Programs\westerm` (Windows, added to your user PATH).
- Choose a version with `WESTERM_VERSION=v0.1.0` and a folder with `WESTERM_INSTALL_DIR`.
- To update, use the update button in the dashboard, run `westerm update`, or run the script again. Open shells come back once as "restored".

## Archives

To install by hand, download an archive from [Releases](https://github.com/SteelCrab/westerm/releases/latest):

| System | File |
|---|---|
| macOS, Apple silicon | `westerm-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `westerm-x86_64-apple-darwin.tar.gz` |
| Linux x64 (static) | `westerm-x86_64-unknown-linux-musl.tar.gz` |
| Linux arm64 (static) | `westerm-aarch64-unknown-linux-musl.tar.gz` |
| Windows x64 and arm64 | `westerm-x86_64-pc-windows-msvc.zip` |

macOS blocks a file downloaded in a browser once. Allow it with `xattr -d com.apple.quarantine westerm`.

## macOS app

Download `westerm-macos-app.zip` (Apple silicon and Intel) from [Releases](https://github.com/SteelCrab/westerm/releases/latest), unzip it, and move **westerm.app** to Applications. It opens the same dashboard in its own window.

- If a westerm already runs on port 4317 (for example from a terminal), the app opens that one, so both show the same sessions. Otherwise it starts its own.
- Quitting the app keeps your shells running, as with the CLI. To save and stop them, run `/Applications/westerm.app/Contents/MacOS/westerm stop`.
- The app is not notarized by Apple, so macOS blocks the first open. Allow it once with `xattr -dr com.apple.quarantine /Applications/westerm.app`, or in System Settings ▸ Privacy & Security ▸ Open Anyway.
- To build it yourself: `npm run build:app` (creates `dist-app/westerm.app`).

## Build from source

Building needs Node.js 24 or later and Rust (cargo).

```bash
npm install
npm run build
npm start          # http://127.0.0.1:4317
```

- The build is a single executable: `native/target/release/westerm` (`westerm.exe` on Windows). The web UI is embedded, so running it does not need Node.
- Change the port with `westerm --port N` or the `PORT` environment variable.

## Platforms

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
