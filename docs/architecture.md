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
| `native/src/server/plugins/` | Plugin routes: `GET` and `PUT /api/plugins` with the installed list in `plugins.json` (`mod.rs`), and the Docker plugin's `GET /api/plugins/docker/state` and `POST /api/plugins/docker/containers/<id>/<action>` (`docker.rs`) |
| `native/src/plugins/` | What the server and the host share about plugins: the ids, the session query `plugin=&action=&target=`, and for Docker the container id check and the log and shell commands (`docker.rs`). `external.rs` is the same for external plugins: the id and program-name rules, the argument array check (at most 32 words of 1024 bytes, no control characters), and the line the host types (`'env' '--' '<program>' '<arg>'…`). The host types the command into a new shell. |
| `native/src/quote.rs` | Shell quoting for typed commands: sh family (`sh bash zsh dash ksh`) and fish, each word single-quoted by that shell's rules; a control character is refused; any other shell is unsupported. The agent resume commands use it too. |
| `native/src/server/plugins/external/` | External plugins: the manifest check (`manifest.rs`), packages (download, list check, unpack; `package.rs`) and staging (`stage.rs`), the records in `plugins.json` (`store.rs`), the plugin list and update checks (`catalog.rs`), the routes (`routes.rs`), `/plugin-assets` and `/plugin-sdk/v1.js` (`assets.rs`, `sdk_v1.js`), `run` (`run.rs`), and the browser-pane address check (`browser.rs`) |
| `native/src/server/plugins/db/` | The Databases plugin (cargo feature `plugin-db`, on by default): saved connections in `db-connections.json` (`store.rs`), passwords in the keychain with a memory fallback (`secrets.rs`), sqlx pools and queries (`driver.rs`), the tree (`tree.rs`), consoles and their secret files (`console.rs`), SSH tunnels (`tunnel.rs`), found connections (`detect.rs`), and the `/api/plugins/db/…` routes (`mod.rs`) |
| `native/src/server/plugins/login_env.rs` | The login shell's whole environment, read once (`$SHELL -i -l -c env`) and shared: Docker keeps six keys of it, `run` of an external plugin uses all of it |
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
| `src/plugins/` | The plugin bar and manager, the installed list, and the built-in plugins. `registry.ts` is the only core file that imports a plugin's folder. `src/plugins/docker/` is the Docker plugin: panel, polling, and text. |
| `src/plugins/db/` | The Databases plugin: panel (`DbPanel.tsx`), connection dialog, table pane (`TablePane.tsx`), requests, model, and text |
| `src/pluginPanes.ts`, `src/plugins/PluginPaneHost.tsx` | Plugin panes: panes without a terminal that a plugin draws (`PluginContext.openPane`, `PluginInstance.PaneView`). They are saved in the layout's `pluginPanes` and each tab's `pluginIds`, beside the docs panes, so an older version still reads the file. |
| `src/plugins/external/` | External plugins on the page: the `WestermPlugin` built from a plugin's info (`plugin.tsx`), its iframe (`frame.ts`), the message bridge (`bridge.ts`), the routes (`api.ts`), and the info and permission text (`info.ts`) |
| `src/i18n/` | UI text in four languages, and language detection |
| `shared/` | Protocol types, the layout tree, saved-file validation |

A Cloudflare tunnel adds a second listener that exists only while the tunnel is on:

```
Another device's browser
   │  HTTPS  https://<name>.trycloudflare.com
   ▼
Cloudflare edge                       TLS ends here
   │  the outbound connection cloudflared made
   ▼
cloudflared        westerm tunnel-guard -- cloudflared tunnel --url …
   │  HTTP + WebSocket
   ▼
Tunnel listener    in the dashboard   127.0.0.1:<any free port>; Host, path list, cookie
   │  the dashboard's own handlers
   ▼
PTY host           as above
```

- The dashboard starts `westerm tunnel-guard`, which runs cloudflared. The dashboard holds the guard's stdin open and never writes to it; when the dashboard ends, however it ends, the OS closes that pipe and the guard ends cloudflared.
- cloudflared runs with `--no-autoupdate`, `--metrics 127.0.0.1:<port>`, and an empty `--config`, so a user's `~/.cloudflared/config.yml` does not apply. The dashboard reads the address from the metrics route `/quicktunnel` and the edge connection from `/ready` every 2 seconds.
- The tunnel's controls are on 4317 only: `GET /api/tunnel/status` (a WebSocket with the status, now and on every change) and `POST /api/tunnel/start`, `/api/tunnel/stop`, `/api/tunnel/check`.
- Both listeners answer `GET /api/tunnel-auth`. The page asks it first: 4317 says `{"tunnel":false}`; the tunnel listener says whether the cookie is valid and whether logins are locked.

## Security

- The server listens on `127.0.0.1` only.
- WebSocket connections, layout writes (PUT), and update installs (POST) accept only the dashboard's own Origin. Other sites cannot open shells, change the layout, or trigger an update.
- The server and the host each validate the session id, kind, size, URL, SSH target, and language.
- SSH targets are passed as arguments, never inside a shell string. Values that start with `-` or contain spaces or shell characters are refused.
- On Windows, terminal-browser and terminal-code run in WSL. `wsl.exe --exec` hands every argument over unchanged, and the login shell there gets the URL and SSH target as positional parameters, as on macOS and Linux. cmd.exe is never involved.
- The Windows host channel replaces Unix socket permissions with:
  - a random 128-bit pipe name, written only to `host.pipe` in the user's private state folder;
  - a first instance created with `first_pipe_instance`, so the host does not start when someone else created the name first;
  - refusing remote clients;
  - the dashboard checking that the pipe's server process id matches `host.pipe`.
- Updates check each download against the SHA-256 sum published with it, and run the new executable once to confirm its version before it replaces the old one. The desktop app checks its download the same way, and on macOS also the new app's version.
- `GET /api/agents/history?dir=<folder>` lists a folder's past conversations. It is GET only, and checks the Host header and `Sec-Fetch-Site` like the docs routes. `dir` must be an absolute path, else 400. It returns ids and titles only (a title may be the first or last prompt), never the conversation. Reads are bounded: 16 MiB per file, 50 Claude Code files (also those of another folder with the same encoded name), 500 Codex files, 1 MiB for a Codex first line, 4 MiB from the end of Codex's `session_index.jsonl`, 256 Gemini `.project_root` folders, 10 results, and titles of 120 characters. Gemini `.jsonl` chats are read one line at a time. The UI shows 5 of the results, after it drops the conversations that run now.
- A session opened from the agents sidebar adds `cwd`, `agent`, and `conversation` to `/ws/session`. `cwd` must be an absolute path (no NUL, 4096 bytes at most). `agent` is `claude`, `codex`, or `gemini`, and only for a shell. `conversation` must look like an id, and needs `agent`. The browser never sends a command: the host builds it from the agent kind and the checked id, and types it only into a brand-new shell.
- **Plugins**
  - `PUT /api/plugins` takes the same checks as the layout PUT: the dashboard's own Origin, a JSON content type, and at most 4096 bytes. It changes only the built-in ids. Unknown ids and external ids in the body are dropped, and the `external` records and external ids in `plugins.json` stay as they are.
  - `GET /api/plugins` answers `installed`, `external`, and `shellPanes` (true on macOS and Linux when the login shell is of the sh family or fish, so a plugin's `openShell` can open a pane).
  - `GET /api/plugins` and `GET /api/plugins/docker/state` are GET only, and check the Host header and `Sec-Fetch-Site` like the docs routes. Both answer 403 to a cross-site request.
  - `POST /api/plugins/docker/containers/<id>/<action>` checks the Origin (403 otherwise). `id` must be 12 to 64 lowercase hex characters, and `action` one of `start`, `stop`, `restart`, `remove`. Anything else is 400. `remove` never uses `-f`. The routes of a plugin that is not installed answer 404.
  - A session adds `plugin`, `action`, and `target` to `/ws/session`. `plugin` must be a known id, and be installed (else the server refuses). For Docker, `action` is `logs` or `shell` and `target` follows the same id rule. They are for a new shell only, and not together with `agent`. The browser never sends a command: the host builds it from fixed strings and the checked id (`docker logs -f --tail 200 <id>`, `docker exec -it <id> sh -c '…'`) and types it into a new shell only.
  - The server calls `docker` with its arguments as an array, never in a shell string. Its one shell run is the fixed command `$SHELL -i -l -c env`, with no browser value in it, to read the login shell's environment once. Only `PATH`, `DOCKER_HOST`, `DOCKER_CONTEXT`, `DOCKER_CONFIG`, `DOCKER_CERT_PATH`, and `DOCKER_TLS_VERIFY` are kept, and `docker` gets no other variable from the login shell (it inherits the server's other variables). If the output has no `PATH`, the server's own PATH is kept. On Unix the probe runs in its own session (no controlling terminal, so it cannot take over the terminal that runs westerm) and ends when the shell exits, even if a background job still holds its output; then its whole process group is killed. The probe and each `docker` read are limited to 5 seconds, and a container action to 30 seconds. A read lists the ids with `docker ps -a -q --no-trunc` and runs one `docker inspect --type container`.
  - With nothing installed, the server runs no `docker` and the page asks for no plugin route except `GET /api/plugins`.
  - `GET /api/plugins` also names the built-in plugins of the build (`builtin`); the page hides the others.
- **Databases plugin**
  - Every `/api/plugins/db/…` route answers 404 while the plugin is not installed. GET routes check the Host header and `Sec-Fetch-Site`; the others check the Origin (and the JSON type for a body). A connection body is at most 16 KiB, a query body 256 KiB. Connection ids are 16 lowercase hex characters, query ids too, console tokens 32.
  - No route answers a password except `GET /detect`, which hands the page the passwords it found in `.env` files and container environments (the page can read those files through a shell anyway).
  - Read-only is enforced by the database (session settings, or SQLite's read-only open), never by reading SQL. Each table-pane Run uses its own connection and closes it, so a Run cannot change the session of another. SQL goes to the database only, never into a shell.
  - Consoles: the page asks `POST /connections/<id>/console`; the server writes the console's program and arguments to `<state>/db-consoles/<token>.json` and the password to `<state>/db-secrets/<id>.pgpass` or `.cnf` (both 0600). The session query carries `plugin=db&action=console&target=<token>`; the server allows it only while the spec waits, and the host reads the spec once, deletes it, and types `'env' '--' 'psql' '…'`. Hosts, users, databases, and paths are checked for control characters when saved and quoted when typed. Spec and secret files are removed when the server starts, a password changes, a connection is deleted, or the plugin is uninstalled.
  - Logs never carry SQL, URLs, or passwords.
- **External plugins** (see [Writing plugins](plugins.md))
  - **Records.** `plugins.json` has `installed` (built-in ids and external ids) and `external`: for each plugin its id, version, source (`index`, `url`, or `folder`), SHA-256, and `granted` (`run`, `panes`, `background`). `granted` is what the user confirmed. The server checks every request against `granted`, never against a later `plugin.json`, so a changed file cannot add a permission. The old `{"installed":["docker"]}` form is still read. Packages live in `<state>/plugins/<id>/<version>/` (a folder source has none: it is used in place), and the storage in `<state>/plugins/<id>/data.json`.
  - **Routes.** "Read check" means the Host header and `Sec-Fetch-Site`, like the docs routes (403 otherwise). "JSON check" means the dashboard's own Origin and a JSON content type (400 otherwise). A body is at most 4096 bytes, and 64 KiB for `run` and `browser`.

    | Route | Checks | What it does |
    |---|---|---|
    | `GET /api/plugins/catalog` | read check | The plugin list (`null` when it cannot be loaded), the installed plugins, and the newer versions found |
    | `POST /api/plugins/fetch` | JSON check | Downloads (`https://` only) or reads a folder (an absolute path), checks it, and keeps it for 15 minutes under a token (at most 4 waiting) |
    | `POST /api/plugins/install` | JSON check | Installs a fetched package (needs `trusted: true`) or a plugin-list entry, with the permissions the user confirmed. The permissions must equal what the package asks for (409 with the real ones otherwise). 409 when it is already installed, unless a fetched package comes with `replace: true` (a reinstall): then it replaces the installed plugin like an update, keeping its storage and deleting the old version's folder. |
    | `POST /api/plugins/<id>/update` | JSON check | Updates, or reloads a folder. More permissions: 409 with what is added, then a second call with the token. 409 "reinstall to update" for a `.tar.gz` link. |
    | `DELETE /api/plugins/<id>` | Origin | Deletes the package, the storage, and the record. A folder source's own folder stays. |
    | `POST /api/plugins/<id>/run` | JSON check, installed (404), `granted.run` has the program (403), `cwd` an absolute folder (400) | Runs the program. 429 above 4 runs at once for the plugin, 504 on the time limit, 502 when the program is not found. |
    | `GET /api/plugins/<id>/storage` | read check, installed (404) | The stored value, or `null` |
    | `PUT /api/plugins/<id>/storage` | JSON check, 64 KiB (413), installed (404) | Replaces the value |
    | `POST /api/plugins/<id>/browser` | JSON check, installed (404), `granted.panes` (403) | Says whether the address may open in a browser pane: 204, 403 for westerm's own address, 400 for a bad address. The page opens the pane only after a 204. |
    | `GET /plugin-assets/<id>/<path>` | Host header (403) | A file of an installed plugin, else 404 |
    | `GET /plugin-sdk/v1.js` | Host header (403) | The SDK |

  - **Assets.** `/plugin-assets` and the SDK are static files, so they check only the Host header: a sandboxed frame has an opaque origin, and its requests are cross-site. A path must be a package path (no empty part, no part that starts with `.`, so no `..` and no dot file such as a folder source's `.env` or `.git/config`; no `\`, NUL, or `:`) and must resolve, links followed, to a regular file inside the package. Every answer, 403 and 404 and the SDK too, carries `Content-Security-Policy: sandbox allow-scripts; …` and `X-Content-Type-Options: nosniff`. A plugin file's policy also sets `default-src 'none'`, scripts from the plugin's folder and the SDK (no inline scripts), styles from the folder and inline, images from the folder, `connect-src`, `frame-src`, `form-action`, `base-uri`, and `object-src` all `'none'`, and `frame-ancestors` to the dashboard's origins (`--allow-origin` too). So a plugin page opened outside its frame still gets an opaque origin, never the dashboard's. A file carries `Access-Control-Allow-Origin: *` only when `Sec-Fetch-Dest` is `script`, `style`, `image`, or `font`, so module scripts load in the opaque-origin frame while a `fetch()` from another website (`empty`) cannot read the file.
  - **The frame.** `<iframe sandbox="allow-scripts">` and nothing else: never `allow-same-origin`, `allow-popups`, `allow-forms`, `allow-top-navigation`, or `allow-modals`. It is inserted once into a layer under `<body>` and is never moved in the DOM, because a browser reloads a moved iframe. The dashboard page sends `Content-Security-Policy: frame-src 'self'`, so a frame that navigates to another site gets a blocked page, whose `load` stops the plugin. The page counts `load` events: a second one removes the frame, stops the bridge, and shows "Plugin stopped: it tried to leave its page". Icons are shown only through `<img>`.
  - **The bridge.** The page reads a message only when `event.source` is that plugin's own frame window and the message is a string of at most 256 KiB. It checks each call against `granted`, then the server checks it again. At most 8 calls per frame wait for the server; more fail at once. A 404 from the server ends the frame and reads the installed list again. Replies and events use JSON strings with `"westerm": 1` (format in the header of `sdk_v1.js`).
  - **Programs.** `run` starts the program with an argument array, never in a shell string, with no stdin. The environment is the server's own with the whole login-shell environment on top (the login value wins). Limits: 10 seconds, 1 MiB for each of stdout and stderr, 4 at once for a plugin. A program that runs too long is killed. The answer comes when the program exits (plus 200 ms for its last output), not when its pipes close. On Unix the program runs in a session and process group of its own, and the whole group is killed after the exit, on the time limit, and when the request goes away, so a background job cannot keep the answer waiting or outlive the call. The login shell's own `PWD`, `OLDPWD`, `SHLVL`, and `_` are not passed on. `run` lists only plain program names (`[A-Za-z0-9][A-Za-z0-9._-]*`, at most 8). Shells, interpreters, and wrappers are refused at install (case-blind, with a version suffix ignored). That catches mistakes. It is not a boundary, because a granted program can start any other program, so `run` is "trust this plugin" and the UI says so.
  - **Shell panes.** The session query is `plugin=<id>&action=run&target=<JSON array of strings>`. The server checks that the plugin is installed, that `granted.panes` is set, and that `granted.run` has the first word. The host checks the array again (1 to 32 words, each at most 1024 bytes, no NUL, no control character in C0, DEL, or C1, the first a plain program name), asks which shell the new pane runs, and types `'env' '--' '<program>' '<arg>'…` with every word single-quoted by that shell's rules (sh family: `'` becomes `'\''`; fish: `\` becomes `\\` and `'` becomes `\'`). `env --` keeps a builtin, alias, or function from taking the program's place. Only the sh family (`sh bash zsh dash ksh`) and fish are supported, on macOS and Linux. Any other shell (nu, pwsh, xonsh, elvish, and also ash, mksh, yash) is unsupported: the page's `openShell` call fails with `failed` and "Your shell is not supported for plugin panes", and no pane opens: the page reads `shellPanes` from `GET /api/plugins` (the host also refuses such a session, as a second check). Windows is unsupported the same way.
  - **Browser panes.** `openBrowser` takes `http` and `https` only. A browser pane's requests carry the dashboard's origin, so the server refuses an address that is the dashboard: it reads the host like a browser does (so `127.1`, `0x7f.0.0.1`, `2130706433`, `LOCALHOST.`, `[::ffff:127.0.0.1]`, and `0.0.0.0` count as the local computer) and refuses it on the port of any of the dashboard's loopback origins, and refuses any address whose origin is one of the dashboard's allowed origins. A host name it cannot fold to ASCII (IDNA: `ⅾash.example`, a soft hyphen) is refused on a loopback origin's port and on the scheme and port of any allowed origin, because a browser may map it to one of them. Other local ports open.
  - **Packages.** Downloads use `https://` only, with `curl`, a time limit (60 seconds; 20 seconds for the plugin list and the GitHub API) and a size limit (5 MiB). The tar listing is read before anything is unpacked, and refused for `..`, an absolute path, or an entry that is not a regular file or a folder; the limits are 10 MiB unpacked, 300 entries, and 11 MiB of tar stream. The package unpacks into an empty staging folder and is checked again. A package from the plugin list (a repository that does not exist yet) must match the SHA-256 in the list. A failed install removes the half-written folder.
  - **Updates.** The server looks for newer versions when `GET /api/plugins/catalog` runs and once a day (the first time 10 minutes after it starts); a server with no plugin from the list or from GitHub makes no request. An update that asks for more than `granted` waits for a second confirmation, so a version cannot add a permission silently.
  - **Nothing installed, nothing asked.** With no external plugin installed, the page makes no new request, and the plugin list is not downloaded before the manager opens.
  - **Known limits.** (1) `run: ["kubectl"]` can do anything kubectl can do as the user. The UI marks it high and asks for a confirmation. (2) A bad plugin can still send out data it was given, once: the policy blocks direct requests, and a navigation is detected and stopped, but its first request and WebRTC cannot be blocked.
- **Desktop app**
  - Each window shows only the app's own start page and the dashboard on its port. A link that opens a new window (`target="_blank"`, `window.open`) to any other `http`, `https`, or `mailto` address opens in the default browser. Any other navigation away from the dashboard is refused and opens nothing: the webview reports frame navigations too without saying which frame, so a plugin's iframe could otherwise open any address in the user's browser. The dashboard never navigates its own page to an outside address.
  - The page asks the app for remote windows and updates through one Tauri command, `app_request`. A capability (`desktop/capabilities/dashboard.json`) allows it only for `http://127.0.0.1:*`, and the app also checks that the page is the dashboard on its port. The page gets no other Tauri API.
  - The app passes a remote window's target to the dashboard only when it is a valid ssh destination, as the dashboard checks it.
- The layout file is checked by the server only for size (256 KiB) and basic shape; the UI validates the rest when it loads it.
- **Remote windows** open a UDP port on the server: the gate. It is the only port westerm opens to a network.
- **Cloudflare tunnel** opens no port: cloudflared makes outbound connections only, and the tunnel listener is on `127.0.0.1` like the dashboard.
  - The tunnel listener exists only while a tunnel is on. Every request on it is a tunnel request, so no header can skip its checks: the Host must be the tunnel's own name (it has none before cloudflared gives the address), then the path must be on a fixed list, then the API paths need the cookie. Static page files and the login are public; `/api/terminal-config`, `/api/workspace`, `/api/ssh-hosts`, `/ws/session`, `/api/docs/…`, `/api/agents/history`, and `/api/git/…` need the cookie; every other `/api/…`, `/ws/…`, and `/remote/…` path is 404, so a new route is closed to the tunnel until it is listed.
  - The dashboard's own Origin checks hold there with the origin `https://<name>.trycloudflare.com`.
  - Plugins stay on 4317: `/api/plugins…` is 404 on the tunnel, and `/plugin-assets/…` and `/plugin-sdk/v1.js` get the page instead of plugin files. A pane that a plugin started opens through the tunnel's `/ws/session` with the same check as on 4317: the plugin must be installed, and an external plugin's program must be granted. Both listeners share one plugin state.
  - A 6-digit access code from the OS random source, new for each tunnel, compared in constant time. A right code gives a 256-bit random token as a cookie (`HttpOnly; Secure; SameSite=Strict`). The dashboard keeps only the tokens' SHA-256, in memory. Wrong codes lock logins for the whole tunnel (5 in a row: 30 s, doubling to 15 min); 20 stop the tunnel.
  - Stopping a tunnel drops every token first, then closes the tunnel's terminal sockets with close code 4010 (the page stops reconnecting), the listener, and cloudflared. Shells only detach.
  - Every answer on the tunnel listener has `Referrer-Policy: no-referrer` and `X-Frame-Options: DENY`. Terminal sockets get a ping every 30 seconds, because Cloudflare closes quiet WebSockets.
  - The status holds the access code, so its stream and the controls check the Host and the Origin, and the POST routes take JSON only. The code and the token are never logged.
  - TLS ends at Cloudflare's edge: Cloudflare can see the terminal's traffic.
  - TLS 1.3 with a certificate on both ends. Each side pins the other's SHA-256 fingerprint, exchanged over SSH; there is no certificate authority.
  - The gate accepts only clients listed in `remote/authorized_clients` in the server's state folder. Only `westerm remote-setup`, which runs over SSH, adds lines, so only someone who can log in to the server can connect.
  - QUIC Retry checks a client's address before the gate does any work for it.
  - The gate joins each authenticated stream to the server dashboard and does not read the request. It has no direct way to the host.
  - The server dashboard listens only on the Unix socket `dashboard.sock` (mode 0600) in the server's private state folder. It opens no TCP port. It answers only `GET` and `PUT /api/workspace`, `GET /api/ssh-hosts`, and `GET /ws/session`; everything else is 404. It does not check Origin, because a socket carries no browser.
  - On the local dashboard, the `/remote/…` routes and the status stream (`/api/remote/link`) check the Host header (against DNS rebinding) and the Origin. GET routes also refuse `Sec-Fetch-Site: cross-site` and `same-site`. JSON APIs check the content type. A request alone never starts SSH: it gets 503 until a window keeps the status stream open.
  - Remote commands are fixed strings that mean the same in sh, bash, zsh, and fish. The destination is validated and comes after `--`; data (a fingerprint, the executable) goes on stdin.
  - The executable sent to a server is the release archive of this version, checked against its published SHA-256 sum. A source build sends a file the developer names (`WESTERM_REMOTE_BINARY_DIR`).
- **Known gap**: any program on the same computer can reach port 4317 and send the dashboard's own Origin; the Origin check stops web pages, not local programs. Through the `/remote/…` routes such a program can also open shells on the servers this dashboard has a link to. Such a program can also start a Cloudflare tunnel and read its access code; it could open shells through 4317 anyway. If the Docker plugin is installed, such a program can also stop or remove containers through the Docker routes. The same holds for external plugins: such a program can install one, or call its `run`, and that too is no new power, because it can already open shells. With the Databases plugin installed, such a program can run SQL on the saved connections, and read a console's password file in `<state>/db-secrets/` while westerm runs; again, it could already open a shell as you. Access tokens are planned. Until then, do not run westerm on a computer shared with other users.

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
- **terminal-browser and terminal-code on Windows** run in WSL. Their Kitty graphics need Microsoft's ConPTY next to `westerm.exe` (the ConPTY built into Windows drops them), and WSL and Windows share no memory, so every frame travels inline as base64.
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
