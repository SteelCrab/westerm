# Contributing to westerm

**English** · [한국어](CONTRIBUTING.ko.md)

Thank you for helping. This guide covers how to set up, what to check before a pull request, and the rules that keep the project working.

## Ways to help

- **Report a bug**: open an [issue](https://github.com/SteelCrab/westerm/issues). See [Reporting a bug](#reporting-a-bug).
- **Suggest an idea**: open an issue first, especially for a large change. Agree on the direction before you write code.
- **Fix or build something**: send a pull request.
- **Translate**: improve a UI language, or add one. See [UI text and languages](#ui-text-and-languages).

## Set up

Requirements:

- Node.js 24 or later
- Rust (stable, with cargo)
- macOS. Linux may work but is not tested.
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

- Keep the path short. The host socket path must fit in 104 bytes (a Unix socket limit).
- `npm run dev` always forwards to :4317. Stop your everyday dashboard first (Ctrl+C). Your shells stay alive in the host.
- Stop a test host with `curl --unix-socket /tmp/westerm-dev/host.sock -X POST http://host/shutdown`, then delete the folder.

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
| `tests/` | UI tests (`node:test`) |
| `native/tests/` | Rust integration tests against the real executable |
| `docs/` | Usage, session persistence, architecture (Korean) |

Read [docs/architecture.md](docs/architecture.md) for how the parts fit together.

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

## Reporting a bug

Include:

- `westerm --version` output (it includes the host build id)
- OS and browser, with versions
- Steps to reproduce, what you expected, and what happened
- Relevant lines from `~/.local/state/westerm/host.log` and the browser console

Remove private data first: host names, paths, and screen contents.

## Security issues

Do not open a public issue. Use [private vulnerability reporting](https://github.com/SteelCrab/westerm/security/advisories/new).
