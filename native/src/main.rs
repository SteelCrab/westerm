//! westerm — Web Session Terminal. A web terminal whose sessions survive
//! closed tabs, restarts, and reboots. One binary; the UI is built on wterm.
//!
//!   westerm [serve] [--port N] [--allow-origin URL]...
//!   westerm stop        (asks the PTY host to save every session and stop)
//!   westerm update [--check]  (installs the newest release)
//!   westerm host        (started by `serve`; owns the PTYs)
//!   westerm gate        (on a server: QUIC entrance to the host for remote dashboards)
//!   westerm remote-dashboard  (on a server: the dashboard a remote window talks to, on a Unix socket)
//!   westerm remote-setup [--replace-host]  (run over SSH by a remote dashboard)

mod cwd;
// The PTY host's files (HOST_SOURCES in build.rs) keep these lints quiet
// here: any change to them changes the host build id, which restarts every
// shell. Fix them with the next change that touches the host anyway.
#[allow(clippy::large_enum_variant, clippy::collapsible_if)]
mod host;
mod i18n;
mod ipc;
// Windows leaves out the server side of remote westerm (the gate and
// remote-setup run on Unix only), so the code only it uses is unused there.
// Unix builds still report unused code.
#[cfg_attr(not(unix), allow(dead_code))]
mod protocol;
#[cfg_attr(not(unix), allow(dead_code))]
mod remote;
mod screen;
mod server;
mod spawn;
#[allow(clippy::redundant_closure)] // A host file, as above.
mod state_dir;
mod time;
#[cfg_attr(not(unix), allow(dead_code))]
mod update;

const USAGE: &str = "usage: westerm [serve] [--port N] [--allow-origin URL]...\n       westerm stop\n       westerm update [--check]\n       westerm host\n       westerm gate\n       westerm remote-dashboard\n       westerm remote-setup [--replace-host]\n       westerm --version";

fn main() {
    let mut args = std::env::args().skip(1).peekable();
    let command = match args.peek().map(String::as_str) {
        Some("serve" | "host" | "stop" | "update" | "gate" | "remote-dashboard" | "remote-setup") => args.next().unwrap(),
        _ => "serve".to_owned(),
    };
    let mut port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(4317);
    let mut allow_origins = Vec::new();
    let mut check_only = false;
    let mut replace_host = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" if command == "update" => check_only = true,
            "--replace-host" if command == "remote-setup" => replace_host = true,
            "--port" => port = args.next().and_then(|p| p.parse().ok()).unwrap_or_else(|| exit_usage()),
            "--allow-origin" => allow_origins.push(args.next().unwrap_or_else(|| exit_usage())),
            "--version" | "-V" => {
                println!("{} — Web Session Terminal, built on wterm", version_line());
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
        "gate" => gate(&runtime),
        "remote-dashboard" => remote_dashboard(&runtime),
        "remote-setup" => remote_setup(&runtime, replace_host),
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

#[cfg(unix)]
fn gate(runtime: &tokio::runtime::Runtime) -> std::io::Result<()> {
    let state = state_dir::state_dir()?;
    runtime.block_on(remote::gate::run(&state))
}

#[cfg(unix)]
fn remote_dashboard(runtime: &tokio::runtime::Runtime) -> std::io::Result<()> {
    runtime.block_on(remote::dashboard::run(&state_dir::state_dir()?))
}

#[cfg(unix)]
fn remote_setup(runtime: &tokio::runtime::Runtime, replace_host: bool) -> std::io::Result<()> {
    runtime.block_on(remote::setup::run(replace_host))
}

/// Remote westerm serves Linux; the server side is not built for Windows.
#[cfg(windows)]
fn gate(_runtime: &tokio::runtime::Runtime) -> std::io::Result<()> {
    unix_only("gate")
}

#[cfg(windows)]
fn remote_dashboard(_runtime: &tokio::runtime::Runtime) -> std::io::Result<()> {
    unix_only("remote-dashboard")
}

#[cfg(windows)]
fn remote_setup(_runtime: &tokio::runtime::Runtime, _replace_host: bool) -> std::io::Result<()> {
    unix_only("remote-setup")
}

#[cfg(windows)]
fn unix_only(command: &str) -> ! {
    eprintln!("westerm: {command} runs on Linux and macOS only");
    std::process::exit(2);
}

/// "westerm 0.1.0 (host rs-…)": the version and the host build. A remote
/// westerm must report the same line as the dashboard that installs it.
pub fn version_line() -> String {
    format!("westerm {} (host {})", env!("CARGO_PKG_VERSION"), host::HOST_BUILD)
}

#[cfg(test)]
mod tests {
    use super::version_line;

    #[test]
    fn version_line_names_the_version_and_host_build() {
        assert!(version_line().starts_with(&format!("westerm {} (host rs-", env!("CARGO_PKG_VERSION"))));
        assert!(version_line().ends_with(')'));
    }
}
