# Contributing to westerm

**English** · [한국어](CONTRIBUTING.ko.md)

Thank you for helping. This guide covers how to set up, what to check before a pull request, and the rules that keep the project working.

westerm is under the [MIT License](LICENSE). By sending a contribution, you agree to license it under the same terms.

## Ways to help

- **Report a bug**: open an [issue](https://github.com/SteelCrab/westerm/issues) with the bug report form. See [Reporting a bug](#reporting-a-bug).
- **Suggest an idea**: open an issue with the feature request form first, especially for a large change. Agree on the direction before you write code. See [Suggesting a feature](#suggesting-a-feature).
- **Fix or build something**: send a pull request.
- **Translate**: improve a UI language, or add one. See [UI text and languages](#ui-text-and-languages).

## Set up

Requirements:

- Node.js 24 or later
- Rust (stable, with cargo)
- macOS, Linux, or Windows. CI runs the tests on all three.
- Optional, for the launchers: `terminal-browser` (`brew install --cask terminal-browser`) and `tode` (`curl -fsSL https://tode.sh/install | bash`)

```bash
git clone https://github.com/SteelCrab/westerm.git
cd westerm
npm install
npm run build      # UI (Vite) → Rust executable with the UI embedded
npm start          # http://127.0.0.1:4317
```

### Development servers

```bash
npm run dev:server   # Rust server on :4317, allows the Vite origin
npm run dev          # Vite with HMR on :5173, forwards /api and /ws to :4317
```

Open http://localhost:5173. UI changes reload at once. Rust changes need a restart of `dev:server`.

### Keep your own sessions safe

westerm keeps shells in a background **PTY host**. By default the host and its state live in `~/.local/state/westerm`. If you also use westerm every day, a development build can touch those sessions:

- A dev server with changed host code **replaces your running host**. Your shells then restart as "restored".
- A second browser window on the same sessions takes them over from the first.

Use a separate state folder and port while you develop:

```bash
WESTERM_STATE_DIR=/tmp/westerm-dev ./native/target/release/westerm --port 4318
```

```powershell
$env:WESTERM_STATE_DIR = "$env:TEMP\westerm-dev"; .\native\target\release\westerm.exe --port 4318
```

- On macOS and Linux keep the path short. The host socket path must fit in 104 bytes (a Unix socket limit).
- `npm run dev` always forwards to :4317. Stop your everyday dashboard first (Ctrl+C). Your shells stay alive in the host.
- Stop a test host with `westerm stop`, run with the same `WESTERM_STATE_DIR`. Then delete the folder.

### Try remote windows with a development build

A source build has no release with its code, so a remote window installs a Linux build you make on the server. Build both from the same source: westerm compares their version lines.

```bash
brew install zig && cargo install cargo-zigbuild   # once (zig links the Linux build)
npx vite build
cargo build --release --manifest-path native/Cargo.toml
cargo zigbuild --release --target x86_64-unknown-linux-musl --manifest-path native/Cargo.toml
mkdir -p /tmp/wrb
cp native/target/x86_64-unknown-linux-musl/release/westerm /tmp/wrb/westerm-x86_64-unknown-linux-musl
WESTERM_STATE_DIR=/tmp/westerm-dev WESTERM_REMOTE_BINARY_DIR=/tmp/wrb ./native/target/release/westerm --port 4318
```

- For an Arm server, use `aarch64-unknown-linux-musl`.
- `WESTERM_REMOTE_TRANSPORT=tunnel` tries the SSH tunnel path.
- `native/tests/remote.rs` runs the whole path against a fake `ssh`, without a server.

## Project layout

| Path | Contents |
|---|---|
| `src/` | UI: React + wterm (Ghostty WASM) |
| `src/i18n/` | UI text in all languages, language detection |
| `src/terminal/` | Terminal view, WebSocket connection, fonts, wterm patches |
| `shared/` | Protocol types, layout tree, saved-file validation |
| `native/src/server/` | Dashboard server (axum): serves the UI, API, relays sockets to the host |
| `native/src/host/` | PTY host: runs and keeps processes, screen copy (`vt100`), save and restore |
| `native/src/protocol.rs` | Message format and validation |
| `native/src/i18n.rs` | Text the host writes into terminals, in all languages |
| `native/src/ipc.rs` | Dashboard ↔ host channel: Unix socket, or a named pipe on Windows |
| `native/src/remote/` | Remote windows on the server: `westerm gate`, `westerm remote-setup`, `westerm remote-dashboard`, certificates, QUIC settings |
| `native/src/remote/daemon.rs` | What the gate and the server dashboard share: one running copy, its record, and stopping it |
| `native/src/remote/dashboard.rs` | `westerm remote-dashboard`: the server's dashboard, on a private Unix socket only |
| `native/src/server/remote/` | Remote windows in the local dashboard: ssh setup, install, QUIC and tunnel links |
| `native/src/server/remote_window.rs` | `/remote/<target>/…`: a remote window's requests, sent over the link to the server's dashboard |
| `native/src/server/http1.rs` | One HTTP/1.1 request over any byte stream: to the PTY host, or over a link to a server |
| `src/remoteWindow.ts` | A remote window's address (`/?remote=<target>`), its server paths, opening one |
| `src/useRemoteLink.ts` | A remote window's link state from the local dashboard's status stream |
| `src/components/RemoteConnect.tsx` | The connecting screen and the banner while a remote window's link is down |
| `tests/` | UI tests (`node:test`) |
| `native/tests/` | Rust integration tests against the real executable |
| `macos/` | macOS app: Swift window (`app/main.swift`), icon, `build-app.sh` |
| `scripts/start.mjs` | `npm start` on every OS |
| `docs/` | Usage, sessions, architecture |

Read [docs/architecture.md](docs/architecture.md) for how the parts fit together.

## Issues

Every issue starts from a form: **Bug report** or **Feature request**. Blank issues are turned off.

- Search the open and closed issues first. If one exists, add to it instead of opening a copy.
- One problem or idea per issue.
- Give the issue a short title that says what is wrong or what you want: `Restored pane loses its folder on Windows`, not `Bug` or `Help`.
- Never report a security issue in a public issue. See [Security issues](#security-issues).

### Reporting a bug

The form asks for:

- `westerm --version` output (it includes the host build id)
- How you run westerm: install script, release archive, macOS app, or a source build
- OS and browser, with versions
- Steps to reproduce, what you expected, and what happened
- Relevant lines from the host log and the browser console. The log is `~/.local/state/westerm/host.log` on macOS and Linux, and `%LOCALAPPDATA%\westerm\host.log` on Windows.

Remove private data first: host names, paths, and screen contents.

### Suggesting a feature

- Describe the problem first, then your proposal. The solution can change in the discussion.
- For a large change, agree on the direction in the issue before you write code.

## Pull requests

- Open an issue first for anything larger than a small fix, and agree on the direction.
- Branch from `main`. Keep one topic per pull request.
- Write the title as `<type>(<scope>): <subject>`, about 72 characters or less in all.
  - `type`: `feat` (new behavior), `fix` (a bug fix), `docs`, `refactor`, `test`, or `chore` (build, CI, releases).
  - `scope`: the part the change touches, such as `remote`, `host`, `ui`, `i18n`, or `macos`.
  - `subject`: imperative mood, starting in lowercase, with no period at the end.
  - Examples: `feat(remote): install westerm over SSH and connect over QUIC`, `fix(host): keep the folder of a restored shell`.
- Fill in the template: a summary of at most 5 bullets, one line each, and the commands a reviewer runs to test. Link the issue with `Closes #123`, so the issue closes on merge.
- Open a draft while you still work on it. Mark it ready when the checks in [Before you open a pull request](#before-you-open-a-pull-request) pass.
- CI must pass on macOS, Linux, and Windows.
- Answer review comments with new commits. Do not force-push during a review, so reviewers can see what changed.
- A maintainer merges after review.

## Before you open a pull request

```bash
npm test             # UI tests + cargo test (unit and integration)
npm run type-check
npm run build        # the release build must succeed
```

All three must pass. Then check:

- [ ] One topic per pull request. Split unrelated changes.
- [ ] New behavior has tests. A bug fix has a test that fails without the fix.
- [ ] UI text is in `src/i18n/messages.ts`, in all four languages.
- [ ] For a UI change, the pull request has a screenshot. Try light and dark themes.
- [ ] Docs match the change: `docs/` for behavior, the READMEs for run or develop steps.
- [ ] `CHANGELOG.md` has a line under `[Unreleased]` for a change that users notice.

## Rules

### UI text and languages

- Put every user-visible string in `src/i18n/messages.ts`. Do not write text inline in components.
- `en` is the reference. Each language is typed as `Messages`, so a missing key fails the type check. `tests/i18n.test.ts` also compares every language with `en`.
- Text with values is a function, for example `sessionCount: (count) => ...`. Do not build sentences by joining pieces; word order differs between languages.
- Command palette entries keep English `keywords`, so users can search in English from any language.
- Text that the host writes into a terminal (restore notice, errors) is in `native/src/i18n.rs`. Keep it in step with the UI languages.
- If you are not sure of a translation, say so in the pull request. A reviewer who reads the language can check it.

To add a language:

1. `src/i18n/messages.ts`: add it to `Locale`, `LOCALES`, `LOCALE_NAMES`, and `HTML_LANG`, write its catalog, and add it to `MESSAGES`.
2. `npm run type-check` then shows the other places to update: palette language keywords and the CJK font order (`src/terminal/fonts.ts`).
3. `native/src/i18n.rs`: add a `Lang` variant, then fill in each `match`. The compiler lists them.
4. Optional: a UI font list in `src/styles.css` (`:root:lang(xx)`) and a translated README.

### Browser ↔ server protocol

- `shared/protocol.ts` (browser) and `native/src/protocol.rs` (server and host) describe the same messages. Change them together.
- The server and the host both validate every attach request: session id, kind, size, URL, SSH target, language. Keep validation on both sides.

### PTY host

- The host build id is a hash of the files in `HOST_SOURCES` (`native/build.rs`). When it changes, the dashboard replaces the running host, and every shell restarts once as "restored".
- Add new host files to `HOST_SOURCES`. Change host code only when you need to; UI and dashboard server changes do not restart shells.

### Security

- The server listens on `127.0.0.1` only. WebSocket and layout writes accept only the dashboard's own Origin. Do not weaken these checks.
- Never pass user input through a shell string. Pass it as an argument, as `spawn.rs` does for SSH targets and URLs.
- State files and the host socket are private to the user (folder 0700, files 0600).

### Platforms

westerm runs on macOS, Linux, and Windows. CI tests every pull request on all three.

- Put OS-specific code behind `#[cfg(unix)]` and `#[cfg(windows)]`, and give every OS a path. Keep the differences in small modules: `ipc.rs` (host channel), `spawn.rs` (what a session runs), `state_dir.rs` (folders and file permissions), `cwd.rs`, `time.rs`.
- macOS and Linux start sessions through the login shell. Windows starts programs directly with an argument list, so check arguments that a `.cmd` shim would pass through cmd.exe.
- Check Windows from macOS or Linux before you push:

  ```bash
  rustup target add x86_64-pc-windows-msvc
  cargo check --target x86_64-pc-windows-msvc --all-targets --manifest-path native/Cargo.toml
  ```

- Run the Linux tests from macOS with Docker:

  ```bash
  docker run --rm -v "$PWD":/src:ro -w /src/native -e CARGO_TARGET_DIR=/tmp/target rust:1 cargo test --locked
  ```

### wterm patches

`src/terminal/alt-screen-images.ts` and `src/terminal/command-keys.ts` use wterm internals. If wterm changes, they must turn themselves off and log a warning, never break the terminal. Note the wterm version you tested in the pull request.

### Code style

There is no formatter configuration yet. Match the code around your change.

- TypeScript: strict mode, 2 spaces, double quotes, semicolons, trailing commas, lines up to about 120 characters.
- React: function components. The app does not use `StrictMode`, because a double-mounted effect would start and kill an extra shell for every pane.
- Rust: 4 spaces, lines up to about 120 characters. Return errors instead of panicking in the server and the host.
- Comments explain why, not what.

### Commits

- Subject in the imperative mood, about 72 characters or less: `Add Japanese restore notice`, not `added ...`.
- The body explains why the change is needed, when it is not obvious.
- Pull request titles have their own format: see [Pull requests](#pull-requests).

## Releasing

For maintainers. A version tag builds and publishes everything.

1. Set the same version in `package.json` and `native/Cargo.toml`:

   ```bash
   npm version 0.2.0 --no-git-tag-version     # package.json and package-lock.json
   # edit `version` in native/Cargo.toml, then update Cargo.lock:
   cargo check --manifest-path native/Cargo.toml
   ```

2. Commit, push, and wait for CI to pass on all three systems.
3. Tag and push the tag:

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

4. The [Release workflow](.github/workflows/release.yml) checks that the tag matches both versions, tests and builds macOS (Apple silicon, Intel), Linux (x64, arm64, static), and Windows, bundles the universal macOS app, and publishes a GitHub release with the archives and SHA-256 sums. A last job installs the release with `install.sh` and `install.ps1` on every system. `install.sh` and `install.ps1` install it as the latest release.

A new version with changed PTY host code restarts users' shells once, as "restored". Mention it in the release notes.

## Security issues

Do not open a public issue. Use [private vulnerability reporting](https://github.com/SteelCrab/westerm/security/advisories/new).
