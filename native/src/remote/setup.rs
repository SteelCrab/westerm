//! `westerm remote-setup [--replace-host]`: what a dashboard runs over SSH.
//! It authorizes the dashboard's certificate (read from stdin), makes sure a
//! host and a gate run, and reports how to reach them as one JSON line.
//!
//! It never replaces a running host of another build on its own: that ends
//! the programs in its shells, so the dashboard asks the user first and then
//! runs this again with `--replace-host`.

use std::io::{self, BufRead};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::gate::{self, GateRecord};
use super::report::{HostInfo, SetupReport, SETUP_VERSION};
use super::{authorized, identity, remote_dir};
use crate::host::HOST_BUILD;
use crate::ipc;
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

    match health(&state).await {
        None => start_host(&state).await.map_err(io::Error::other)?,
        Some(running) if running.build != HOST_BUILD && replace_host => {
            println!("> replacing pty host {} with {HOST_BUILD}", running.build);
            stop_and_wait(&state).await;
            start_host(&state).await.map_err(io::Error::other)?;
        }
        Some(_) => {}
    }
    let gate = ensure_gate(&state).await?;
    // The gate made or loaded its identity before it wrote its record.
    let gate_identity = identity::load_or_create(&dir, "gate")?;
    let host = health(&state).await.ok_or_else(|| io::Error::other("the PTY host does not answer"))?;
    let report = SetupReport {
        v: SETUP_VERSION,
        version: crate::version_line(),
        port: gate.port,
        fingerprint: identity::fingerprint(&gate_identity.cert),
        socket: ipc::describe(&state),
        host: HostInfo { build: host.build, protocol: host.protocol, sessions: host.sessions },
        kill_on_logout: kill_on_logout(),
    };
    println!("{}", serde_json::to_string(&report).map_err(io::Error::other)?);
    Ok(())
}

/// The running gate when it is this build; otherwise a new one. A gate holds
/// no shells, so replacing it costs connected dashboards one reconnect.
async fn ensure_gate(state: &Path) -> io::Result<GateRecord> {
    let version = crate::version_line();
    if gate::is_running(state) {
        if let Some(record) = gate::read_record(state).filter(|record| record.version == version) {
            return Ok(record);
        }
        gate::stop_running(state).await?;
    }
    let log_path = state.join("gate.log");
    let log = private_file().create(true).append(true).open(&log_path)?;
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("gate").stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log);
    let child = spawn_detached(&mut command)?;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Some(record) = gate::read_record(state).filter(|record| record.pid == child.id()) {
            return Ok(record);
        }
    }
    let last = std::fs::read_to_string(&log_path)
        .ok()
        .and_then(|text| text.lines().rev().find(|line| !line.trim().is_empty()).map(str::to_owned))
        .unwrap_or_default();
    Err(io::Error::other(format!("the gate did not start: {last} (see {})", log_path.display())))
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
