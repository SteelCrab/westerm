# Contributing to westerm

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
| `desktop/` | Desktop app (Tauri 2) for macOS, Windows, and Linux: `src/` (Rust), `tauri.conf.json`, icons |
| `scripts/desktop.mjs` | `npm run app:dev` and `npm run build:app` |
| `scripts/conpty.mjs` | Microsoft's ConPTY for Windows builds (Kitty graphics pass through it) |
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
- How you run westerm: install script, release archive, desktop app, or a source build
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
  - `scope`: the part the change touches, such as `remote`, `host`, `ui`, `i18n`, or `desktop`.
  - `subject`: imperative mood, starting in lowercase, with no period at the end.
  - Examples: `feat(remote): install westerm over SSH and connect over QUIC`, `fix(host): keep the folder of a restored shell`.
- Fill in the template: a summary of at most 5 bullets, one line each, and the commands a reviewer runs to test. Link the issue with `Closes #123`, so the issue closes on merge.
- Open a draft while you still work on it. Mark it ready when the checks in [Before you open a pull request](#before-you-open-a-pull-request) pass.
- CI must pass on macOS, Linux, and Windows. It runs the type check, the UI tests, and the UI build once; clippy and `cargo test` on each system, for westerm and for the desktop app; the release build on Linux; and shellcheck on the shell scripts.
- A pull request that changes the desktop app or its UI (`desktop/`, `src/`, `shared/`, and the build files) also bundles the app for macOS, Windows, and Linux ([Desktop app workflow](.github/workflows/app.yml)). To try the installers before a release, open the run and download them from Artifacts. They are kept for 14 days. Changes to `native/` alone do not start it; run it from the Actions tab (Run workflow) for those.
- Answer review comments with new commits. Do not force-push during a review, so reviewers can see what changed.
- A maintainer merges after review.

## Before you open a pull request

```bash
npm test             # UI tests + cargo test (unit and integration)
npm run type-check
npm run build        # the release build must succeed
cargo clippy --all-targets --manifest-path native/Cargo.toml -- -D warnings
```

All four must pass. Then check:

- [ ] One topic per pull request. Split unrelated changes.
- [ ] New behavior has tests. A bug fix has a test that fails without the fix.
- [ ] UI text is in `src/i18n/messages.ts`, in all four languages.
- [ ] For a UI change, the pull request has a screenshot. Try light and dark themes.
- [ ] Docs match the change: `docs/` for behavior, the READMEs for run or develop steps.
- [ ] `CHANGELOG.md` has one short line under `[Unreleased]` for a change that users notice. Details go in `docs/`.

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
- macOS and Linux start sessions through the login shell. Windows starts PowerShell and ssh directly with an argument list, and terminal-browser and terminal-code with `wsl.exe --exec` through the login shell in WSL.
- Check Windows from macOS or Linux before you push:

  ```bash
  rustup target add x86_64-pc-windows-msvc
  cargo check --target x86_64-pc-windows-msvc --all-targets --manifest-path native/Cargo.toml
  ```

  The desktop app's check also needs `llvm-rc` on PATH for the Windows resources (on macOS: `brew install llvm`, then `PATH="$(brew --prefix llvm)/bin:$PATH"`), a placeholder for the westerm it ships: `touch desktop/binaries/westerm-x86_64-pc-windows-msvc.exe`, and the ConPTY files: `node scripts/conpty.mjs desktop/binaries/conpty`.

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

1. Set the same version in `package.json`, `native/Cargo.toml`, and `desktop/Cargo.toml` (the app reads its version from `package.json`):

   ```bash
   npm version 0.2.0 --no-git-tag-version     # package.json and package-lock.json
   # edit `version` in native/Cargo.toml and desktop/Cargo.toml, then update both Cargo.lock files:
   cargo check --manifest-path native/Cargo.toml
   cargo check --manifest-path desktop/Cargo.toml
   ```

2. Commit, push, and wait for CI to pass on all three systems.
3. Tag and push the tag:

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

4. The [Release workflow](.github/workflows/release.yml) checks that the tag matches the three versions, tests and builds macOS (Apple silicon, Intel), Linux (x64, arm64, static), and Windows, bundles the desktop app (universal macOS, Windows x64, Linux x64 and arm64), and publishes a GitHub release with the archives and SHA-256 sums. The workflow verifies `install.sh` and `install.ps1` against the published release, then packs, tests, and publishes the same version to npm.

A new version with changed PTY host code restarts users' shells once, as "restored". Mention it in the release notes.

### npm package

The root `package.json` stays private because it builds the web UI. `npm/` contains the public `@pistacrab/westerm` package template and launcher. The packaging script reads the root version and downloads that version's published GitHub release. It verifies each archive's SHA-256, then copies only the executables and required Windows files to `dist-npm/`. The generated package has no runtime dependencies or install scripts.

The Release workflow publishes the tested tarball with [npm trusted publishing](https://docs.npmjs.com/trusted-publishers/). Stable releases use npm's default `latest` tag. Prereleases use `next`. A rerun skips versions already published to npm.

Before the first automated release, configure a trusted publisher for `@pistacrab/westerm` in npm with repository `SteelCrab/westerm`, workflow `release.yml`, and permission to run `npm publish`. Leave the environment field blank. You can also set this up with npm CLI 11.15 or later:

```bash
npm trust github @pistacrab/westerm --repo SteelCrab/westerm --file release.yml --allow-publish
```

Complete the first automated publication within two days of creating the trusted publisher. npm expires new configurations that have not been used within that window.

To build and test the package locally after a matching GitHub release exists:

```bash
npm run test:npm
npm run pack:npm
npm run test:npm-install
```

The package includes all five release builds (about 20 MB compressed). It supports Node.js 22 or later. `release.json` records the release URLs and archive checksums. Packaging on Linux requires `unzip` for the Windows archive.

The separate npm workflow tests packaging and installation on macOS, Linux, and Windows. It pins `WESTERM_NPM_VERSION` to an existing release so version bumps can be tested before publishing the new GitHub release.

### Cargo package

`native/` is also the public `westerm` crate. `build.rs` selects the source checkout's `dist/` or the crate's own `web-dist/`. Package preparation builds the UI and stages it with the license inside `native/`; installing the crate needs no Node.js.

```bash
npm ci
npm run test:cargo
npm run pack:cargo
npm run test:cargo-install
```

The install check extracts the `.crate` outside the repository, installs it into a temporary prefix, deletes the extracted web files, and verifies the installed dashboard's HTTP responses against the packaged files. It uses a separate state folder and an automatically assigned port. The Cargo package workflow runs these checks on macOS, Linux, and Windows.

`native/web-dist/` and `native/LICENSE` are generated and ignored by Git, but explicitly included by Cargo. Cargo treats these files as uncommitted changes, so packaging uses `--allow-dirty`. Before publishing, commit the source changes and verify that `git status --porcelain` is empty. The final guard below permits the generated files while rejecting changes reported by Git:

```bash
cargo login --registry crates-io
npm run pack:cargo
npm run test:cargo-install
test -z "$(git status --porcelain)" && \
  cargo publish --dry-run --allow-dirty --locked --registry crates-io --manifest-path native/Cargo.toml
test -z "$(git status --porcelain)" && \
  cargo publish --allow-dirty --locked --registry crates-io --manifest-path native/Cargo.toml
```

Publishing is manual. Match the package version to the reviewed release commit. Windows Cargo installs do not include the ConPTY helper files; document that limitation until a separate installation path exists for them.

## Security issues

Do not open a public issue. Use [private vulnerability reporting](https://github.com/SteelCrab/westerm/security/advisories/new).
