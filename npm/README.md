# westerm

**We**b **S**ession **Term**inal. A web terminal whose sessions survive closing the window, restarting the program, and rebooting.

## Install

Requires Node.js 22 or later.

```bash
npm install -g @pistacrab/westerm
westerm
```

Or run without a global installation:

```bash
npx @pistacrab/westerm
```

With Bun:

```bash
bun add -g @pistacrab/westerm   # the westerm command starts through Node.js
bunx @pistacrab/westerm         # runs without Node.js
```

Then open http://127.0.0.1:4317.

```bash
westerm --port 4318  # use another port
westerm stop         # save every session and stop
westerm --version
```

## Platforms

The package includes the official release executables for macOS (Apple silicon and Intel), Linux (arm64 and x64, static musl), and Windows (x64, also runs on Arm through emulation). Windows includes Microsoft's ConPTY and its MIT license.

Installation needs no Rust compiler, install scripts, or separate binary downloads. The package includes all supported platforms, so the npm download is about 20 MB.

## Update

```bash
npm install -g @pistacrab/westerm@latest
```

Restart westerm after updating. Open shells may return once as restored sessions. The built-in `westerm update` command and dashboard updater replace the bundled executable directly; use npm to keep the npm package version in sync.

## More

- [Usage](https://github.com/SteelCrab/westerm/blob/main/docs/usage.md)
- [Desktop apps and other installation methods](https://github.com/SteelCrab/westerm/blob/main/docs/install.md)
- [Source and issues](https://github.com/SteelCrab/westerm)
- [Release notes](https://github.com/SteelCrab/westerm/releases)

MIT licensed. Built on [wterm](https://www.npmjs.com/package/@wterm/core).
