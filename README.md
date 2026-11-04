# westerm

**We**b **S**ession **Term**inal. A web terminal whose sessions survive closing the window, restarting the program, and rebooting.

**English** · [한국어](README.ko.md) · [日本語](README.ja.md) · [中文](README.zh-CN.md)

![westerm: terminal-browser, a shell, and terminal-code split in one tab](docs/screenshot.png)

Shells, terminal-browser, terminal-code, and SSH sessions sit in tabs and split panes.
The screen is drawn by [wterm](https://github.com/vercel-labs/wterm) and the Ghostty WASM core. westerm is not an official wterm project.

Usage and design notes are in the [docs](docs/README.md) (Korean).

## Run

Building needs Node.js 24 or later and Rust (cargo). westerm is developed and tested on macOS.

```bash
npm install
npm run build
npm start          # http://127.0.0.1:4317
```

- The build is a single executable: `native/target/release/westerm`. The web UI is embedded, so **running it does not need Node.**
- Change the port with `westerm --port N` or the `PORT` environment variable.
- Each launcher's program must be on your login shell's PATH.

| Launcher | Install |
|---|---|
| terminal-browser | `brew install --cask terminal-browser` |
| terminal-code | `curl -fsSL https://tode.sh/install \| bash` (the command is `tode`) |
| SSH | the system `ssh` and `~/.ssh/config` |

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

Changing PTY host code restarts running shells once, as "restored". See [session persistence](docs/sessions.md) (Korean).

## Contribute

- Report bugs and ideas in [Issues](https://github.com/SteelCrab/westerm/issues).
- Before a pull request, make `npm test` and `npm run type-check` pass.
- Read [CONTRIBUTING.md](CONTRIBUTING.md) for setup, rules, and translations.
- Report security issues through [private vulnerability reporting](https://github.com/SteelCrab/westerm/security/advisories/new), not public issues.
