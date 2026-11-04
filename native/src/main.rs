//! westerm — Web Session Terminal. A web terminal whose sessions survive
//! closed tabs, restarts, and reboots. One binary; the UI is built on wterm.
//!
//!   westerm [serve] [--port N] [--allow-origin URL]...
//!   westerm stop        (asks the PTY host to save every session and stop)
//!   westerm update [--check]  (installs the newest release)
//!   westerm host        (started by `serve`; owns the PTYs)

mod cwd;
mod host;
mod i18n;
mod ipc;
mod protocol;
mod screen;
mod server;
mod spawn;
mod state_dir;
mod time;
mod update;

const USAGE: &str = "usage: westerm [serve] [--port N] [--allow-origin URL]...\n       westerm stop\n       westerm update [--check]\n       westerm host\n       westerm --version";

fn main() {
    let mut args = std::env::args().skip(1).peekable();
    let command = match args.peek().map(String::as_str) {
        Some("serve" | "host" | "stop" | "update") => args.next().unwrap(),
        _ => "serve".to_owned(),
    };
    let mut port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(4317);
    let mut allow_origins = Vec::new();
    let mut check_only = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" if command == "update" => check_only = true,
            "--port" => port = args.next().and_then(|p| p.parse().ok()).unwrap_or_else(|| exit_usage()),
            "--allow-origin" => allow_origins.push(args.next().unwrap_or_else(|| exit_usage())),
            "--version" | "-V" => {
                println!("westerm {} (host {}) — Web Session Terminal, built on wterm", env!("CARGO_PKG_VERSION"), host::HOST_BUILD);
                return;
            }
            _ => exit_usage(),
        }
    }

    update::clean_up();
    if command == "update" {
        if let Err(error) = update::run_cli(check_only) {
            eprintln!("westerm: {error}");
            std::process::exit(1);
        }
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let result = match command.as_str() {
        "host" => runtime.block_on(host::run()),
        "stop" => runtime.block_on(server::stop_host()),
        _ => runtime.block_on(server::run(server::Options { port, allow_origins })),
    };
    if let Err(error) = result {
        eprintln!("westerm: {error}");
        std::process::exit(1);
    }
}

fn exit_usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2);
}
