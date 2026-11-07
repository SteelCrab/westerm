# Architecture

## Components

```
Browser or the desktop app (React + wterm, Ghostty WASM)
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

A remote window takes another path. The window is a second dashboard page; its requests go to `/remote/<target>/…` on the local dashboard and travel to the server:

```
Remote window (the same UI, opened with ?remote=<target>)
   │  HTTP + WebSocket /remote/<target>/…, and a status stream /api/remote/link
   ▼
Local dashboard    westerm            keeps one link per server
   │  setup:  ssh <host> 'westerm remote-setup'  (install, start, exchange certificates)
   │  link:   QUIC, UDP 4317 (or an ssh -L tunnel when UDP is blocked); one stream per request
   ▼
Remote gate        westerm gate       on the Linux server; checks the client's certificate
   │  every authenticated stream is joined to the Unix socket dashboard.sock
   ▼
Server dashboard   westerm remote-dashboard   no TCP port; the layout, ssh-hosts, and sessions only
   │  the same host protocol as above, over the server's host.sock
   ▼
PTY host on the server ─ shells, terminal-browser, terminal-code
```

| Path | Contents |
|---|---|
| `native/src/server/` | Dashboard server (axum): serves the UI and APIs, relays each browser WebSocket to the host, installs updates (`updates.rs`) |
| `native/src/host/` | PTY host: runs and keeps processes (`portable-pty`), screen copies (`vt100`), saving and restoring; AI agents in shells and their resume commands (`agent.rs`), and each shell's folder and git branch (`place`) |
| `native/src/git.rs` | The git branch of a folder, read from the files in `.git` (also linked worktrees; a detached HEAD gives 7 characters of the commit). It never runs `git`. |
| `native/src/server/agents.rs` | `GET /api/agents/history`: a folder's past Claude Code, Codex, and Gemini CLI conversations (ids and titles only) |
| `native/src/process.rs` | Command line of another process: `KERN_PROCARGS2` on macOS, `/proc` on Linux |
| `native/src/protocol.rs` | Message format and validation: session id, size, URL, SSH target, language, start folder, agent kind, conversation id |
| `native/src/ipc.rs` | Dashboard ↔ host channel: Unix socket, or a named pipe on Windows |
| `native/src/pixels/` | Pixel updates: the host takes Kitty graphics out of a pixel app's output, keeps the picture, and sends the changed rectangles (see below) |
| `native/src/spawn.rs` | What a session runs: a login shell on macOS and Linux; PowerShell and argument lists on Windows |
| `native/src/update.rs` | `westerm update`: finds, downloads, checks, and installs releases |
| `native/src/i18n.rs` | Text the host writes into terminals (restore notice, errors) in four languages |
| `native/src/remote/` | Remote window parts both ends share: certificates, the authorized client list, QUIC settings; and the server side: `westerm gate` (`gate.rs`), `westerm remote-dashboard` (`dashboard.rs`), `westerm remote-setup` (`setup.rs`), and the lock, record, and stop code the gate and the remote dashboard share (`daemon.rs`) |
| `native/src/server/remote/` | The local dashboard's side of remote windows: ssh commands, the Linux build to install, saved connections, QUIC and tunnel links, and the per-server link driver (`manager.rs`) |
| `native/src/server/remote_window.rs` | The `/remote/<target>/…` routes: they pass the layout, ssh-hosts, and session requests over the server link |
| `native/src/server/http1.rs` | Sends and reads one HTTP/1.1 request over a stream; the remote routes and the server dashboard use it |
| `src/remoteWindow.ts`, `src/useRemoteLink.ts`, `src/components/RemoteConnect.tsx` | The window's side: the target and its URLs, the status stream, and the connecting screen |
| `desktop/` | The desktop app (Tauri 2) for macOS, Windows, and Linux: a window around the dashboard, a window for each remote server, the macOS menu bar, and its updater. It ships the westerm executable next to its own (`westerm-app`) and uses a westerm on port 4317 or starts that one. |
| `src/appBridge.ts` | The page's side of the desktop app: `window.westermApp.request` and the menu's page events |
| `scripts/desktop.mjs` | `npm run app:dev` and `npm run build:app`: builds westerm for the app and bundles it |
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
- Updates check each download against the SHA-256 sum published with it, and run the new executable once to confirm its version before it replaces the old one. The desktop app checks its download the same way, and on macOS also the new app's version.
- `GET /api/agents/history?dir=<folder>` lists a folder's past conversations. It is GET only, and checks the Host header and `Sec-Fetch-Site` like the docs routes. `dir` must be an absolute path, else 400. It returns ids and titles only (a title may be the first or last prompt), never the conversation. Reads are bounded: 16 MiB per file, 50 Claude Code files (also those of another folder with the same encoded name), 500 Codex files, 1 MiB for a Codex first line, 4 MiB from the end of Codex's `session_index.jsonl`, 256 Gemini `.project_root` folders, 10 results, and titles of 120 characters. Gemini `.jsonl` chats are read one line at a time. The UI shows 5 of the results, after it drops the conversations that run now.
- A session opened from the agents sidebar adds `cwd`, `agent`, and `conversation` to `/ws/session`. `cwd` must be an absolute path (no NUL, 4096 bytes at most). `agent` is `claude`, `codex`, or `gemini`, and only for a shell. `conversation` must look like an id, and needs `agent`. The browser never sends a command: the host builds it from the agent kind and the checked id, and types it only into a brand-new shell.
- **Desktop app**
  - Each window shows only the app's own start page and the dashboard on its port. Any other address opens in the default browser.
  - The page asks the app for remote windows and updates through one Tauri command, `app_request`. A capability (`desktop/capabilities/dashboard.json`) allows it only for `http://127.0.0.1:*`, and the app also checks that the page is the dashboard on its port. The page gets no other Tauri API.
  - The app passes a remote window's target to the dashboard only when it is a valid ssh destination, as the dashboard checks it.
- The layout file is checked by the server only for size (256 KiB) and basic shape; the UI validates the rest when it loads it.
- **Remote windows** open a UDP port on the server: the gate. It is the only port westerm opens to a network.
  - TLS 1.3 with a certificate on both ends. Each side pins the other's SHA-256 fingerprint, exchanged over SSH; there is no certificate authority.
  - The gate accepts only clients listed in `remote/authorized_clients` in the server's state folder. Only `westerm remote-setup`, which runs over SSH, adds lines, so only someone who can log in to the server can connect.
  - QUIC Retry checks a client's address before the gate does any work for it.
  - The gate joins each authenticated stream to the server dashboard and does not read the request. It has no direct way to the host.
  - The server dashboard listens only on the Unix socket `dashboard.sock` (mode 0600) in the server's private state folder. It opens no TCP port. It answers only `GET` and `PUT /api/workspace`, `GET /api/ssh-hosts`, and `GET /ws/session`; everything else is 404. It does not check Origin, because a socket carries no browser.
  - On the local dashboard, the `/remote/…` routes and the status stream (`/api/remote/link`) check the Host header (against DNS rebinding) and the Origin. GET routes also refuse `Sec-Fetch-Site: cross-site` and `same-site`. JSON APIs check the content type. A request alone never starts SSH: it gets 503 until a window keeps the status stream open.
  - Remote commands are fixed strings that mean the same in sh, bash, zsh, and fish. The destination is validated and comes after `--`; data (a fingerprint, the executable) goes on stdin.
  - The executable sent to a server is the release archive of this version, checked against its published SHA-256 sum. A source build sends a file the developer names (`WESTERM_REMOTE_BINARY_DIR`).
- **Known gap**: any program on the same computer can reach port 4317 and send the dashboard's own Origin; the Origin check stops web pages, not local programs. Through the `/remote/…` routes such a program can also open shells on the servers this dashboard has a link to. Access tokens are planned. Until then, do not run westerm on a computer shared with other users.

## Pixel updates

terminal-browser and terminal-code (pixel apps) draw everything as Kitty images. A browser page cannot read shared memory, so before this they sent every frame as zlib and base64 text through the PTY, and the pane decoded it with the Ghostty WASM core on the page's main thread.

```
pixel app ── shared memory (t=s), only the command goes through the PTY
   │
PTY host   native/src/pixels/: splits the output, answers queries, keeps
   │       images and placements, composes changed areas
   │       binary WebSocket messages: kind byte 0 = terminal output, 1 = pixel update
Dashboard  relays the messages unchanged
   │
Page       src/terminal/pixel-surface.ts: a Worker draws the rectangles on an
           OffscreenCanvas over the terminal grid
```

- **Asking for it.** A browser or editor pane opens its socket with `?pixels=raw` (a remote window: `pixels=deflate`). The host answers `"ready"` with `pixels: true`. A host or page without this keeps the old path, so either side can be older.
- **Shared memory.** The host answers the app's `a=q,t=s` probe with OK only after it has read and unlinked the probe's segment, as Kitty does. The app then sends each image as a segment name. The host maps the segment, copies it, and unlinks it. File transfer (`t=f`) is refused. Inline images (`t=d`, with or without zlib) still work.
- **Frames.** Commands inside a synchronized update (`CSI ? 2026 h` … `l`) belong to one frame. At its end the changed areas are composed, so an update never shows half a frame. An update that stays open longer than one second is shown anyway.
- **Kitty rules.** A placement sits at the cursor cell plus `X`/`Y` pixels. A higher `z` covers a lower one; among equal `z` the newer one is on top. Sending an image again under the same id deletes the old one and its placements. `a=d` with `d=a/A/i/I` deletes. Leaving the alternate screen clears the picture. Images are kept up to 96 MiB per session; the oldest images that nothing shows go first.
- **Update format** (little endian): kind `1`, flags (bit 0: deflate), frame width and height (u16), rectangle count (u16); then per rectangle x, y, w, h (u16), byte length (u32), and RGBA rows (raw deflate with the flag). At most 16 rectangles; more are merged.
- **The canvas stays.** wterm empties `.term-grid` whenever its rows or columns change and puts back only its own layers, so the pixel canvas puts itself back (a `MutationObserver`). Without that, a pane or window resize leaves the pane blank.
- **Flow control.** The page answers `{"type":"painted"}` after drawing an update. The host keeps at most 2 updates unanswered; later frames merge into the next one, so a slow page gets fewer, newer frames and the PTY never waits. A hidden pane holds its answers until it is shown again.
- **Socket buffers** (`native/src/ipc.rs`). macOS gives a Unix socket 8 KiB. tungstenite moves the rest of its buffer forward after every partial write, so a 7 MB update cost quadratic copying, a whole CPU core in the host. Both ends of the dashboard ↔ host socket ask for 4 MiB.
- `localStorage["westerm.pixels"] = "off"` in the page brings back the Kitty images, for comparison (`bench/pixel/`).

## wterm and terminal-browser patches

- **Alternate-screen images** (`src/terminal/alt-screen-images.ts`)
  - terminal-browser sends each frame as an unsized Kitty image (`a=T,...,C=1`).
  - `@wterm/dom` 0.5.9 treats it as an image in shell output and pushes the lines below it down.
  - The patch turns that off on the alternate screen only.
- **Frame patches** (`native/src/spawn.rs`)
  - terminal-browser and terminal-code draw with the pixel library. In a terminal that pixel does not know, such as westerm, it sends the whole picture for every change, even a blinking caret.
  - Every pane gets `TERMINAL_BROWSER_PRESENT=patched`. pixel then sends only the changed tiles, as small Kitty images over the last whole picture, and a whole picture again when the layout changes.
  - A value set before westerm starts or in a shell startup file wins, and so does terminal-browser's own `render.presenter` setting.
  - With pixel updates the host keeps these images, so the browser never sees them.
- **Image repaints** (`src/terminal/image-repaint.ts`)
  - `@wterm/dom` 0.5.9 repaints every visible image canvas when any image changes. With patches, one blinking caret repainted the whole frame and every tile: 1.8 million pixels for 40 that changed.
  - A canvas's key already names its image and version, so the patch paints a canvas again only when its size or source area changes, and otherwise only moves it.
  - This matters where wterm still draws Kitty images: `kitten icat` in a shell, or a pixel app on a host without pixel updates.
- **Cmd keys** (`src/terminal/command-keys.ts`)
  - `@wterm/dom` leaves every Cmd+key to the browser.
  - When a program turns on Kitty "report all keys" (flag 8), Cmd combinations go to the program instead.
  - Cmd+V stays with the browser for pasting. Cmd+Q, Cmd+H, and Cmd+M stay with the macOS app. Chrome keeps Cmd+T, Cmd+W, Cmd+N, and Cmd+Q for itself.
- These patches use wterm internals. If those change, the patches turn themselves off and log a warning in the console.

## Known limits

- **Pixel updates are large.** A changed rectangle travels as raw RGBA to the local page: scrolling or video in a 1700 × 1000 pane moves 0.4 to 0.7 GB/s through local sockets. A remote window gets deflated rectangles, which cost the server's host CPU (zlib level 1).
- **Retina sharpness**: a pixel app draws one pixel per CSS pixel, as before, so its picture is not sharp on a high-density screen.
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
- A pixel app (a fake `tode`) reaches a pane that asks for pixels as a pixel update, with no Kitty image in the terminal output; a pane that does not ask still gets the Kitty images (macOS and Linux).
- Processes a test starts are cleaned up even when the test fails.
- A fake `claude` in a shell is found, its conversation and flags are saved, and after a host restart the shell resumes it; with `resume=off` it does not (macOS and Linux).

Pixel updates have unit tests in `native/src/pixels/` (splitting, Kitty rules, shared memory, frames, encoding) and `tests/pixels.test.ts` (the page side). `tests/pixel-surface.test.ts` runs wterm's real renderer in happy-dom and checks that the pixel canvas survives the renderer emptying the grid on a resize. `bench/pixel/README.md` describes how to measure them.

`native/tests/remote.rs` (macOS and Linux) covers remote windows with the real executable:

- The server dashboard: it answers only its own routes on a private socket (0600) and listens on no TCP port.
- `remote-setup` starts a host, a gate, and a server dashboard, and twice in a row keeps one of each. A QUIC client reaches the dashboard through the gate. Unknown clients are refused. A stopping gate tells its clients at once, and logs a stream the dashboard refuses. `westerm stop` ends the gate, the dashboard, and the host.
- Through the local dashboard, with a fake `ssh` that runs the remote commands on this machine:
  - the `/remote/…` routes: a layout round trip, a shell on the "server", and refusals for a wrong Origin, a wrong Host, and cross-site requests;
  - the status stream shows its stages until connected; requests get 503 without a watched link;
  - two windows opening at once set up the server once;
  - a permanent failure waits for "Try again" (`/api/remote/retry`); the link closes after the last window leaves;
  - a killed gate comes back and the shell is reattached; a pane closed while disconnected ends its shell after the reconnect;
  - an editor pane finds a program that only the server user's interactive shell puts on PATH.

`tests/remote.test.ts` covers the page side. The SSH tunnel path is checked by hand, because the fake `ssh` cannot forward ports.
