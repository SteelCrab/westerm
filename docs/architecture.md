# Architecture

## Components

```
Browser or westerm.app (React + wterm, Ghostty WASM)
   │  WebSocket /ws/session, HTTP /api/*
   ▼
Dashboard server   westerm            127.0.0.1:4317, UI embedded
   │  WebSocket over a Unix socket (Windows: named pipe)
   ▼
PTY host           westerm host       host.sock / host.pipe in the state folder
   │  PTY (Windows: ConPTY)
   ▼
shell, ssh, terminal-browser, tode
```

| Path | Contents |
|---|---|
| `native/src/server/` | Dashboard server (axum): serves the UI and APIs, relays each browser WebSocket to the host, installs updates (`updates.rs`) |
| `native/src/host/` | PTY host: runs and keeps processes (`portable-pty`), screen copies (`vt100`), saving and restoring |
| `native/src/protocol.rs` | Message format and validation: session id, size, URL, SSH target, language |
| `native/src/ipc.rs` | Dashboard ↔ host channel: Unix socket, or a named pipe on Windows |
| `native/src/spawn.rs` | What a session runs: a login shell on macOS and Linux; PowerShell and argument lists on Windows |
| `native/src/update.rs` | `westerm update`: finds, downloads, checks, and installs releases |
| `native/src/i18n.rs` | Text the host writes into terminals (restore notice, errors) in four languages |
| `macos/` | The macOS app: a WKWebView window (Swift), its updater, icon, and build script. It uses a westerm on port 4317 or starts its own. |
| `src/` | UI: React + wterm (Ghostty WASM) |
| `src/i18n/` | UI text in four languages, and language detection |
| `shared/` | Protocol types, the layout tree, saved-file validation |

## Security

- The server listens on `127.0.0.1` only.
- WebSocket connections, layout writes (PUT), and update installs (POST) accept only the dashboard's own Origin. Other sites cannot open shells, change the layout, or trigger an update.
- The server and the host each validate the session id, kind, size, URL, SSH target, and language.
- SSH targets are passed as arguments, never inside a shell string. Values that start with `-` or contain spaces or shell characters are refused.
- On Windows, programs installed as `.cmd` shims pass their arguments through cmd.exe, so a URL for terminal-browser that contains `" & | < > ^ % !` is refused.
- The Windows host channel replaces Unix socket permissions with:
  - a random 128-bit pipe name, written only to `host.pipe` in the user's private state folder;
  - a first instance created with `first_pipe_instance`, so the host does not start when someone else created the name first;
  - refusing remote clients;
  - the dashboard checking that the pipe's server process id matches `host.pipe`.
- Updates check each download against the SHA-256 sum published with it, and run the new executable once to confirm its version before it replaces the old one.
- The layout file is checked by the server only for size (256 KiB) and basic shape; the UI validates the rest when it loads it.
- **Known gap**: any program on the same computer can reach port 4317 and send the dashboard's own Origin; the Origin check stops web pages, not local programs. Access tokens are planned. Until then, do not run westerm on a computer shared with other users.

## wterm and terminal-browser patches

- **Alternate-screen images** (`src/terminal/alt-screen-images.ts`)
  - terminal-browser sends each frame as an unsized Kitty image (`a=T,...,C=1`).
  - `@wterm/dom` 0.5.9 treats it as an image in shell output and pushes the lines below it down.
  - The patch turns that off on the alternate screen only.
- **Cmd keys** (`src/terminal/command-keys.ts`)
  - `@wterm/dom` leaves every Cmd+key to the browser.
  - When a program turns on Kitty "report all keys" (flag 8), Cmd combinations go to the program instead.
  - Cmd+V stays with the browser for pasting. Cmd+Q, Cmd+H, and Cmd+M stay with the macOS app. Chrome keeps Cmd+T, Cmd+W, Cmd+N, and Cmd+Q for itself.
- Both use wterm internals. If those change, the patches turn themselves off and log a warning in the console.

## Known limits

- **Choppy video**: browsers cannot read shared memory, so terminal-browser sends frames as base64 text through the PTY. Constantly changing pictures stutter.
- **terminal-browser and terminal-code on Windows**: both draw with Kitty graphics, which Windows ConPTY does not pass through. Shells and SSH work.
- **Korean in the terminal-browser address bar**: terminal-browser's own UI font (JetBrains Mono) has no Hangul, so it shows boxes. Page content is fine.

## Tests

```bash
npm test           # UI logic (node:test) + Rust unit and integration tests (cargo test)
npm run type-check
```

CI (`.github/workflows/ci.yml`) runs these on macOS, Linux, and Windows.

The Rust integration tests (`native/tests/integration.rs`) start the real executable with a temporary state folder. On Windows they run the same scenarios with PowerShell and a named pipe.

- A shell survives a dropped connection; after a host restart it comes back from the saved screen; closing it deletes the saved files.
- When a second window opens a session, the first gets close code 4001.
- When a pane from another window attaches, every pane of the first window gets close code 4001, also in sessions the new window did not open. The shells keep running.
- The dashboard accepts WebSockets only from its own origin.
- A second host refuses to start while one is running.
- Processes a test starts are cleaned up even when the test fails.
