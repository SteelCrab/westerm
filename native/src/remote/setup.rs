//! `westerm remote-setup [--replace-host]`: what a dashboard runs over SSH.
//! It authorizes the dashboard's certificate (read from stdin), makes sure a
//! host, a server dashboard and a gate run, and reports how to reach them as one JSON line.
//!
//! It never replaces a running host of another build on its own: that ends
//! the programs in its shells, so the dashboard asks the user first and then
//! runs this again with `--replace-host`.

use std::cell::OnceCell;
use std::io::{self, BufRead, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::daemon::{Daemon, Record};
use super::dashboard::{self, DashboardRecord};
use super::gate::{self, GateRecord};
use super::report::{HostInfo, SetupReport, SETUP_VERSION};
use super::{authorized, identity, remote_dir};
use crate::host::HOST_BUILD;
use crate::server::host_client::{health, spawn_detached, start_host, stop_and_wait};
use crate::state_dir::{private_file, state_dir};

pub async fn run(replace_host: bool) -> io::Result<()> {
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    let fingerprint = line.trim();
    if !identity::is_fingerprint(fingerprint) {
        eprintln!("westerm: expected a client fingerprint on stdin");
        std::process::exit(2);
    }
    let state = state_dir()?;
    let dir = remote_dir(&state)?;
    authorized::add(&dir.join("authorized_clients"), fingerprint)?;

    // Read once, and only when the host or the dashboard starts.
    let login_env = OnceCell::new();
    let env = || login_env.get_or_init(|| tokio::task::block_in_place(server_env)).as_slice();
    match health(&state).await {
        None => start_host(&state, env()).await.map_err(io::Error::other)?,
        Some(running) if running.build != HOST_BUILD && replace_host => {
            println!("> replacing pty host {} with {HOST_BUILD}", running.build);
            stop_and_wait(&state).await;
            start_host(&state, env()).await.map_err(io::Error::other)?;
        }
        Some(_) => {}
    }
    ensure_dashboard(&state, env).await?;
    let gate = ensure_gate(&state).await?;
    // The gate made or loaded its identity before it wrote its record.
    let gate_identity = identity::load_or_create(&dir, "gate")?;
    let host = health(&state).await.ok_or_else(|| io::Error::other("the PTY host does not answer"))?;
    let report = SetupReport {
        v: SETUP_VERSION,
        version: crate::version_line(),
        port: gate.port,
        fingerprint: identity::fingerprint(&gate_identity.cert),
        dashboard: dashboard::socket_path(&state).display().to_string(),
        host: HostInfo { build: host.build, protocol: host.protocol, sessions: host.sessions },
        kill_on_logout: kill_on_logout(),
    };
    println!("{}", serde_json::to_string(&report).map_err(io::Error::other)?);
    Ok(())
}

/// The running server dashboard when it is this build; otherwise a new one,
/// started with `env` (a host it starts later inherits it). It holds no
/// shells (the host does), so replacing it costs open remote windows one
/// reconnect. It is ready when its socket file exists.
async fn ensure_dashboard<'a>(state: &Path, env: impl FnOnce() -> &'a [(String, String)]) -> io::Result<DashboardRecord> {
    let socket = dashboard::socket_path(state);
    ensure_daemon(state, &dashboard::DASHBOARD, "remote-dashboard", "the remote dashboard", env, || socket.exists()).await
}

/// The running gate when it is this build; otherwise a new one. A gate holds
/// no shells, so replacing it costs connected dashboards one reconnect.
async fn ensure_gate(state: &Path) -> io::Result<GateRecord> {
    ensure_daemon(state, &gate::GATE, "gate", "the gate", || &[], || true).await
}

/// The record of `daemon` when it runs and is this build. Otherwise it stops
/// the old copy, starts `westerm <subcommand>` detached with `env` (its
/// output goes to `<name>.log`), and waits up to 5 s for the new copy's
/// record and for `ready`. `what` names the daemon in the error.
async fn ensure_daemon<'a, R: Record>(
    state: &Path,
    daemon: &Daemon,
    subcommand: &str,
    what: &str,
    env: impl FnOnce() -> &'a [(String, String)],
    ready: impl Fn() -> bool,
) -> io::Result<R> {
    let version = crate::version_line();
    if daemon.is_running(state) {
        if let Some(record) = daemon.read_record::<R>(state).filter(|record| record.version() == version) {
            return Ok(record);
        }
        daemon.stop_running(state).await?;
    }
    let log_path = daemon.log_path(state);
    let log = private_file().create(true).append(true).open(&log_path)?;
    let mut command = Command::new(std::env::current_exe()?);
    command.arg(subcommand).envs(env().iter().map(|(key, value)| (key, value)));
    command.stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log);
    let child = spawn_detached(&mut command)?;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Some(record) = daemon.read_record::<R>(state).filter(|record| record.pid() == child.id()) {
            if ready() {
                return Ok(record);
            }
        }
    }
    Err(io::Error::other(format!("{what} did not start: {} (see {})", last_line(&log_path), log_path.display())))
}

/// Printed before `env` in the login shell: rc files may print first.
const ENV_MARK: &str = "__WESTERM_LOGIN_ENV__";
/// An rc file that waits for input (or hangs) costs at most this.
const LOGIN_ENV_TIMEOUT: Duration = Duration::from_secs(5);

/// The environment for the host and the server dashboard: `PATH` as the
/// user's interactive login shell has it, and its `LANG` when this process
/// has none. SSH runs `remote-setup` without an interactive shell, so zsh
/// never reads `~/.zshrc`, where many servers add `~/.local/bin`; programs
/// installed there (tode, terminal-browser) would not start in a pane.
fn server_env() -> Vec<(String, String)> {
    let captured = std::env::var("SHELL").ok().filter(|shell| !shell.is_empty()).and_then(|shell| capture_login_env(&shell));
    let path_now = std::env::var("PATH").unwrap_or_default();
    let home = std::env::var("HOME").ok();
    let has_lang = std::env::var_os("LANG").is_some();
    env_for_server(captured.as_deref().and_then(parse_login_env), &path_now, home.as_deref(), has_lang)
}

/// What `$SHELL -l -i -c 'echo MARK; env'` prints, with stdin from
/// /dev/null and no stderr; None when it fails or takes over
/// `LOGIN_ENV_TIMEOUT` (then it is killed). It runs in a session of its own,
/// so an interactive shell cannot take a terminal.
fn capture_login_env(shell: &str) -> Option<String> {
    let mut command = Command::new(shell);
    command.args(["-l", "-i", "-c", &format!("echo {ENV_MARK}; env")]);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = spawn_detached(&mut command).ok()?;
    let mut stdout = child.stdout.take()?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let _ = stdout.read_to_end(&mut output);
        let _ = sender.send(output);
    });
    let output = receiver.recv_timeout(LOGIN_ENV_TIMEOUT).ok();
    // Also after a finished read: a shell still running its logout files.
    let _ = child.kill();
    let _ = child.wait();
    output.map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// What the login shell's environment holds.
#[derive(Debug, PartialEq, Eq)]
struct LoginEnv {
    path: String,
    lang: Option<String>,
}

/// `PATH` and `LANG` from the `env` output after `ENV_MARK`. None without
/// the mark or without a non-empty `PATH`.
fn parse_login_env(output: &str) -> Option<LoginEnv> {
    let (_, env) = output.split_once(ENV_MARK)?;
    let value = |name: &str| env.lines().find_map(|line| line.strip_prefix(name)?.strip_prefix('=')).map(str::to_owned);
    let path = value("PATH").filter(|path| !path.is_empty())?;
    Some(LoginEnv { path, lang: value("LANG").filter(|lang| !lang.is_empty()) })
}

/// `PATH` (and `LANG` when this process has none) from the login shell;
/// without it, this process's PATH with `$HOME/.local/bin` (where westerm
/// itself is installed) added when missing.
fn env_for_server(captured: Option<LoginEnv>, path_now: &str, home: Option<&str>, has_lang: bool) -> Vec<(String, String)> {
    let Some(captured) = captured else {
        return vec![("PATH".to_owned(), fallback_path(path_now, home))];
    };
    let mut env = vec![("PATH".to_owned(), captured.path)];
    if let Some(lang) = captured.lang.filter(|_| !has_lang) {
        env.push(("LANG".to_owned(), lang));
    }
    env
}

/// `path` with `<home>/.local/bin` at the end when it is not in it yet.
fn fallback_path(path: &str, home: Option<&str>) -> String {
    let Some(home) = home.filter(|home| !home.is_empty()) else { return path.to_owned() };
    let local_bin = format!("{}/.local/bin", home.trim_end_matches('/'));
    if path.split(':').any(|dir| dir == local_bin) {
        path.to_owned()
    } else if path.is_empty() {
        local_bin
    } else {
        format!("{path}:{local_bin}")
    }
}

/// The last non-empty line of a log, or nothing.
fn last_line(log_path: &Path) -> String {
    std::fs::read_to_string(log_path)
        .ok()
        .and_then(|text| text.lines().rev().find(|line| !line.trim().is_empty()).map(str::to_owned))
        .unwrap_or_default()
}

/// True when the server ends a user's processes at logout (systemd-logind
/// `KillUserProcesses`) and lingering is off: then the host and the gate end
/// with the SSH login that started them. False when it cannot tell.
fn kill_on_logout() -> bool {
    let mut conf = std::fs::read_to_string("/etc/systemd/logind.conf").unwrap_or_default();
    if let Ok(entries) = std::fs::read_dir("/etc/systemd/logind.conf.d") {
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "conf"))
            .collect();
        paths.sort();
        for path in paths {
            conf.push('\n');
            conf.push_str(&std::fs::read_to_string(path).unwrap_or_default());
        }
    }
    if !kills_user_processes(&conf) {
        return false;
    }
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    match Command::new("loginctl").args(["show-user", &uid.to_string(), "--property=Linger"]).stderr(Stdio::null()).output() {
        Ok(output) if output.status.success() => !linger_enabled(&String::from_utf8_lossy(&output.stdout)),
        _ => false,
    }
}

/// The last uncommented `KillUserProcesses=` setting is on.
fn kills_user_processes(logind_conf: &str) -> bool {
    logind_conf
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#') && !line.starts_with(';'))
        .filter_map(|line| line.strip_prefix("KillUserProcesses="))
        .last()
        .is_some_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "yes" | "true" | "on" | "1"))
}

/// `loginctl show-user <uid> --property=Linger` says yes.
fn linger_enabled(loginctl: &str) -> bool {
    loginctl.lines().any(|line| line.trim() == "Linger=yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_login_shells_path_after_its_noise() {
        let output = format!("Welcome!\nPATH=/noise\n{ENV_MARK}\nHOME=/home/u\nPATH=/home/u/.local/bin:/usr/bin\nLANG=en_US.UTF-8\n");
        let env = parse_login_env(&output).unwrap();
        assert_eq!(env, LoginEnv { path: "/home/u/.local/bin:/usr/bin".into(), lang: Some("en_US.UTF-8".into()) });
        assert_eq!(parse_login_env(&format!("{ENV_MARK}\nPATH=/usr/bin\n")).unwrap().lang, None);
        assert_eq!(parse_login_env(""), None, "the shell printed nothing");
        assert_eq!(parse_login_env("PATH=/usr/bin\n"), None, "no mark: not the env output");
        assert_eq!(parse_login_env(&format!("{ENV_MARK}\nPATH=\n")), None);
    }

    #[test]
    fn the_server_env_falls_back_to_this_path_and_local_bin() {
        assert_eq!(fallback_path("/usr/bin", Some("/home/u")), "/usr/bin:/home/u/.local/bin");
        assert_eq!(fallback_path("/home/u/.local/bin:/usr/bin", Some("/home/u/")), "/home/u/.local/bin:/usr/bin", "added once");
        assert_eq!(fallback_path("", Some("/home/u")), "/home/u/.local/bin");
        assert_eq!(fallback_path("/usr/bin", None), "/usr/bin");
        let path = |env: &[(String, String)]| env.iter().find(|(key, _)| key == "PATH").map(|(_, value)| value.clone());
        assert_eq!(path(&env_for_server(None, "/usr/bin", Some("/home/u"), true)), Some("/usr/bin:/home/u/.local/bin".into()));
        let captured = || Some(LoginEnv { path: "/opt/bin:/usr/bin".into(), lang: Some("ko_KR.UTF-8".into()) });
        let env = env_for_server(captured(), "/usr/bin", Some("/home/u"), false);
        assert_eq!(env, [("PATH".into(), "/opt/bin:/usr/bin".into()), ("LANG".into(), "ko_KR.UTF-8".into())]);
        assert_eq!(env_for_server(captured(), "/usr/bin", None, true).len(), 1, "this process's LANG stays");
    }

    #[test]
    fn reads_logind_settings() {
        assert!(!kills_user_processes("#KillUserProcesses=yes\n"));
        assert!(kills_user_processes("[Login]\nKillUserProcesses=yes\n"));
        assert!(!kills_user_processes("KillUserProcesses=yes\nKillUserProcesses=no\n"));
        assert!(!kills_user_processes(""));
        assert!(linger_enabled("Linger=yes\n"));
        assert!(!linger_enabled("Linger=no\n"));
        assert!(!linger_enabled(""));
    }
}
