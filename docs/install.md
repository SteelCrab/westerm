# Install

The [desktop app](#desktop-app) is the recommended and easiest way to install. The other ways install the command-line `westerm`.

## npm

Requires Node.js 22 or later:

```bash
npm install -g @pistacrab/westerm
westerm
```

Or run `npx @pistacrab/westerm` without a global installation. The package includes the release executables for all supported platforms and needs no Rust compiler or install scripts. Windows on Arm uses the x64 build through emulation.

Update with `npm install -g @pistacrab/westerm@latest`, then restart westerm. The built-in updater replaces the bundled executable directly; npm updates keep the package version in sync.

## Bun

Bun installs the same npm package:

```bash
bun add -g @pistacrab/westerm
westerm
```

- The `westerm` command is a small Node.js script (`#!/usr/bin/env node`) that starts the bundled executable, so it needs Node.js 22 or later on your PATH.
- Without Node.js, run `bunx @pistacrab/westerm`. Bun then runs the script itself.
- Update with `bun add -g @pistacrab/westerm@latest`, then restart westerm.

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

## Desktop app

The desktop app opens the same dashboard in its own window, on macOS, Windows, and Linux. Download it from [Releases](https://github.com/SteelCrab/westerm/releases/latest):

| System | File | Install |
|---|---|---|
| macOS (Apple silicon and Intel) | `westerm-macos-app.dmg` | Open it, drag **westerm** onto the Applications folder, then eject the disk image. |
| Windows x64 | `westerm-windows-app-setup.exe` | Run it. It installs for your user only, in `%LOCALAPPDATA%\westerm`. Installing or removing it first stops the PTY host, which saves every session; the shells come back once as "restored". |
| Linux x64 | `westerm-linux-app-amd64.deb`, `westerm-linux-app-x86_64.rpm` | `sudo apt install ./westerm-linux-app-amd64.deb`, or `sudo dnf install ./westerm-linux-app-x86_64.rpm` |
| Linux arm64 | `westerm-linux-app-arm64.deb`, `westerm-linux-app-aarch64.rpm` | The same, with the arm64 file. |

- If a westerm already runs on port 4317 (for example from a terminal), the app opens that one, so both show the same sessions. Otherwise it starts the westerm that comes with the app.
- Quitting the app keeps your shells running, as with the CLI. To save and stop them, run the app's westerm with `stop`: `/Applications/westerm.app/Contents/MacOS/westerm stop` on macOS, `%LOCALAPPDATA%\westerm\westerm.exe stop` on Windows, `/usr/bin/westerm stop` on Linux.
- The app writes the output of the westerm it starts to `westerm.log` in its log folder: `~/Library/Logs/io.github.steelcrab.westerm` (macOS), `%LOCALAPPDATA%\io.github.steelcrab.westerm\logs` (Windows), `~/.local/share/io.github.steelcrab.westerm/logs` (Linux).
- Updates: see [Updates](usage.md#updates). On Linux the app opens the release page; install the new package from there.
- The app is not notarized by Apple, so macOS blocks the first open. Allow it once with `xattr -dr com.apple.quarantine /Applications/westerm.app`, or in System Settings ▸ Privacy & Security ▸ Open Anyway. Windows SmartScreen may also ask once: More info ▸ Run anyway.
- To build it yourself: `npm run build:app` for this computer's system and CPU (`-- --target universal-apple-darwin` for both Mac CPUs). The files go to `dist-app/`. Linux needs the WebKitGTK 4.1 development packages (on Ubuntu: `libwebkit2gtk-4.1-dev libxdo-dev librsvg2-dev libdbus-1-dev`). `npm run app:dev` runs a debug build.

## Check a download

Each release file (archives, desktop apps, and Linux packages) has a build attestation. It is a signed record, kept by GitHub, that this repository's Release workflow built the file from the tagged source. Check a file with the [GitHub CLI](https://cli.github.com/):

```bash
gh attestation verify westerm-linux-app-amd64.deb --repo SteelCrab/westerm
```

- Releases up to v0.5.0 have no attestations.
- The `.sha256` file next to each release file only catches a broken download. It does not show who built the file.
- The Linux packages have no GPG signature.

## Build from source

Building needs Node.js 24 or later and Rust (cargo).

```bash
npm install
npm run build
npm start          # http://127.0.0.1:4317
```

- The build is a single executable: `native/target/release/westerm` (`westerm.exe` on Windows). The web UI is embedded, so running it does not need Node.
- Change the port with `westerm --port N` or the `PORT` environment variable.

## Cargo

Once the crate has been published to crates.io, Rust users can install the CLI and web dashboard with:

```bash
cargo install westerm --locked
westerm
```

Use a current stable Rust toolchain and your platform's C compiler and linker. The crate includes the built web UI, so installing and running it does not need Node.js. It does not install the Tauri desktop app.

Update with `cargo install westerm --locked`. The built-in `westerm update` instead replaces the executable with a GitHub release build.

On Windows, Cargo installs the executable without Microsoft's `conpty.dll` and `OpenConsole.exe`. Windows' built-in ConPTY drops Kitty graphics. Use the desktop app, npm, or the GitHub release for the bundled Windows files.

## Platforms

| | macOS, Linux | Windows |
|---|---|---|
| Shell | your login shell (`$SHELL`) | PowerShell 7 (`pwsh`), else Windows PowerShell |
| SSH | `ssh` and `~/.ssh/config` | the built-in OpenSSH client and `%USERPROFILE%\.ssh\config` |
| terminal-browser, terminal-code | when installed (see below) | in WSL, when installed there (see below) |

The launcher programs must be on your PATH (on macOS and Linux, your login shell's PATH):

| Launcher | Install |
|---|---|
| terminal-browser | `brew install --cask terminal-browser` |
| terminal-code | `curl -fsSL https://tode.sh/install \| bash` (the command is `tode`) |

### terminal-browser and terminal-code on Windows

Both have Linux builds only, so westerm starts them in WSL: `wsl.exe` runs your login shell in the default distribution, and the shell runs the program. Install them inside WSL. From PowerShell:

```powershell
wsl --install    # once, when WSL is not set up; then restart Windows
wsl --exec bash -c "curl -fsSL https://terminal-browser.sh/install | bash"
wsl --exec bash -c "curl -fsSL https://tode.sh/install | bash"
```

When a program is missing, its pane shows the command for it.

The ConPTY built into Windows drops the Kitty graphics both programs draw with. The Windows release, the install script, the update, and the desktop app put Microsoft's ConPTY (`conpty.dll` and `OpenConsole.exe`, MIT) next to `westerm.exe`, and westerm uses it for every session. A source build needs the two files too:

```powershell
node scripts/conpty.mjs native/target/release
```
