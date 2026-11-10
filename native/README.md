# westerm

**We**b **S**ession **Term**inal. A web terminal whose sessions survive closed tabs, restarts, and reboots.

## Install

Use a current stable Rust toolchain and your platform's C compiler and linker:

```bash
cargo install westerm --locked
westerm --version
```

The crate includes the built web UI. Installing and running it does not need Node.js.
This installs the CLI and web dashboard. Desktop installers are available from [GitHub Releases](https://github.com/SteelCrab/westerm/releases).

## Run

```bash
westerm
```

Open http://127.0.0.1:4317. Use `westerm --port 4318` to choose another port.
Run `westerm stop` to save sessions and stop the background host.

Update a Cargo installation with `cargo install westerm --locked`.
The built-in `westerm update` instead replaces the executable with a GitHub release build.

## Windows

Cargo installs the executable without Microsoft's bundled `conpty.dll` and `OpenConsole.exe`.
Without them, westerm uses Windows' built-in ConPTY, which drops Kitty graphics.
Use the [npm package or GitHub release](https://github.com/SteelCrab/westerm/blob/main/docs/install.md) for the bundled Windows files.

## Source and license

Source, usage, and installation details: [SteelCrab/westerm](https://github.com/SteelCrab/westerm).

MIT. See LICENSE.
