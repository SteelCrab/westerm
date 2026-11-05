//! The dashboard's ssh commands for remote westerm. Each remote command is a
//! fixed string: ssh hands it to the server's login shell as a script, so
//! nothing from the user or the network goes into it. Data (a fingerprint,
//! an executable) travels on stdin. The destination is validated and comes
//! after `--`, so it cannot be read as an option.
//!
//! Commands run with BatchMode: a key or ssh-agent must log in, since there
//! is nobody to type a password.

use std::process::Stdio;

use tokio::io::AsyncWriteExt;

use super::Failure;
use crate::protocol::RemoteFailure;

/// The server's OS and CPU, and the version line of its westerm if any.
pub const PROBE: &str = "uname -sm; ~/.local/bin/westerm --version 2>/dev/null || true";
/// Puts the executable from stdin in place by rename, so a running westerm
/// keeps its old file. It must run first: an upload cut off by a closed pane
/// ends `cat` early but cleanly, and must not replace a working westerm.
pub const UPLOAD: &str = "mkdir -p ~/.local/bin && cat > ~/.local/bin/.westerm.upload && chmod 755 ~/.local/bin/.westerm.upload && ~/.local/bin/.westerm.upload --version >/dev/null && mv -f ~/.local/bin/.westerm.upload ~/.local/bin/westerm";
pub const SETUP: &str = "~/.local/bin/westerm remote-setup";
pub const SETUP_REPLACE: &str = "~/.local/bin/westerm remote-setup --replace-host";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    /// `uname -s`, e.g. "Linux".
    pub os: String,
    /// `uname -m`, e.g. "x86_64".
    pub arch: String,
    /// "westerm 0.2.0 (host rs-…)" when westerm is installed and runs.
    pub version: Option<String>,
}

pub fn parse_probe(stdout: &str) -> Option<Probe> {
    let mut lines = stdout.lines();
    let mut words = lines.next()?.split_whitespace();
    let (Some(os), Some(arch), None) = (words.next(), words.next(), words.next()) else {
        return None;
    };
    let version = lines
        .next()
        .map(str::trim)
        .filter(|line| line.starts_with("westerm "))
        .map(|line| line.split(" — ").next().unwrap_or(line).trim().to_owned());
    Some(Probe { os: os.to_owned(), arch: arch.to_owned(), version })
}

/// Where ssh really connects for a destination, from `ssh -G`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub hostname: String,
    /// Through a jump host or a proxy command: UDP cannot follow that path.
    pub proxied: bool,
}

pub fn parse_config(stdout: &str) -> Option<Resolved> {
    let mut hostname = None;
    let mut proxied = false;
    for line in stdout.lines() {
        let Some((key, value)) = line.trim().split_once(' ') else { continue };
        let value = value.trim();
        match key.to_ascii_lowercase().as_str() {
            "hostname" => hostname = Some(value.to_owned()),
            "proxyjump" | "proxycommand" if !value.eq_ignore_ascii_case("none") => proxied = true,
            _ => {}
        }
    }
    Some(Resolved { hostname: hostname?, proxied })
}

const UNREACHABLE: &[&str] = &[
    "Could not resolve hostname",
    "Connection timed out",
    "Operation timed out",
    "Connection refused",
    "No route to host",
    "Network is unreachable",
];

/// What went wrong, from ssh's error output.
pub fn classify(stderr: &str) -> RemoteFailure {
    if stderr.contains("Permission denied (") {
        RemoteFailure::Auth
    } else if stderr.contains("Host key verification failed") || stderr.to_ascii_lowercase().contains("host key") {
        RemoteFailure::HostKey
    } else if UNREACHABLE.iter().any(|needle| stderr.contains(needle)) {
        RemoteFailure::Unreachable
    } else {
        RemoteFailure::Setup
    }
}

pub fn command_args(target: &str, remote_command: &str) -> Vec<String> {
    ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "--", target, remote_command]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The last `max` characters of `text`.
fn tail(text: &str, max: usize) -> &str {
    text.char_indices().rev().nth(max.saturating_sub(1)).map_or(text, |(at, _)| &text[at..])
}

/// Runs a fixed `remote_command` on `target` and returns its stdout. The
/// ssh process ends with the future, e.g. when the pane closes.
pub async fn run(target: &str, remote_command: &str, stdin: Option<Vec<u8>>) -> Result<String, Failure> {
    let mut command = tokio::process::Command::new("ssh");
    command
        .args(command_args(target, remote_command))
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|e| Failure::new(RemoteFailure::Setup, format!("cannot run ssh: {e}")))?;
    // Feed stdin while ssh runs, so a large upload cannot block its output.
    let writer = stdin.zip(child.stdin.take()).map(|(bytes, mut pipe)| {
        tokio::spawn(async move {
            let _ = pipe.write_all(&bytes).await;
            let _ = pipe.shutdown().await;
        })
    });
    let output = child.wait_with_output().await.map_err(|e| Failure::new(RemoteFailure::Setup, e.to_string()))?;
    if let Some(writer) = writer {
        let _ = writer.await;
    }
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(Failure::new(classify(&stderr), tail(stderr.trim(), 300)))
}

/// `ssh -G`: the configuration ssh would use, without connecting.
pub async fn resolve(target: &str) -> Result<Resolved, Failure> {
    let output = tokio::process::Command::new("ssh")
        .args(["-G", "--", target])
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| Failure::new(RemoteFailure::Setup, format!("cannot run ssh: {e}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    match parse_config(&stdout) {
        Some(resolved) if output.status.success() => Ok(resolved),
        _ => Err(Failure::new(RemoteFailure::Setup, tail(String::from_utf8_lossy(&output.stderr).trim(), 300))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_output_gives_os_arch_and_version() {
        let p = parse_probe("Linux x86_64\nwesterm 0.2.0 (host rs-1) — Web Session Terminal, built on wterm\n").unwrap();
        assert_eq!((p.os.as_str(), p.arch.as_str(), p.version.as_deref()), ("Linux", "x86_64", Some("westerm 0.2.0 (host rs-1)")));
        assert_eq!(parse_probe("Linux aarch64\n").unwrap().version, None);
        assert_eq!(parse_probe("Linux x86_64\nzsh: no such file\n").unwrap().version, None);
        assert!(parse_probe("").is_none());
        assert!(parse_probe("Linux\n").is_none());
    }

    #[test]
    fn ssh_config_gives_the_host_and_whether_it_is_proxied() {
        let r = parse_config("user pista\nhostname 192.168.219.105\nport 22\nproxyjump none\n").unwrap();
        assert_eq!((r.hostname.as_str(), r.proxied), ("192.168.219.105", false));
        assert!(parse_config("hostname h\nproxyjump bastion\n").unwrap().proxied);
        assert!(parse_config("hostname h\nproxycommand ssh -W %h:%p jump\n").unwrap().proxied);
        assert!(!parse_config("hostname h\nproxycommand none\n").unwrap().proxied);
        assert!(parse_config("port 22\n").is_none());
    }

    #[test]
    fn ssh_errors_are_classified() {
        assert_eq!(classify("pista@h: Permission denied (publickey).\r\n"), RemoteFailure::Auth);
        assert_eq!(classify("Host key verification failed.\r\n"), RemoteFailure::HostKey);
        assert_eq!(classify("No ED25519 host key is known for h and you have requested strict checking."), RemoteFailure::HostKey);
        assert_eq!(classify("ssh: connect to host 1.2.3.4 port 22: Connection refused"), RemoteFailure::Unreachable);
        assert_eq!(classify("ssh: Could not resolve hostname nope: nodename nor servname provided"), RemoteFailure::Unreachable);
        assert_eq!(classify("boom"), RemoteFailure::Setup);
    }

    #[test]
    fn targets_only_appear_after_the_double_dash() {
        assert_eq!(command_args("prod", PROBE), ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "--", "prod", PROBE]);
        for command in [PROBE, UPLOAD, SETUP, SETUP_REPLACE] {
            assert!(!command.contains(['$', '\'', '"', '`', '{']), "{command}");
        }
    }

    /// Shells a server may log in with. ssh runs a remote command as
    /// `<login shell> -c <command>`; each fixed command must mean the same in all.
    #[cfg(unix)]
    #[test]
    fn fixed_commands_work_in_every_available_shell() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::{Command, Stdio};
        let on_path = |name: &str| {
            std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path).map(|dir| dir.join(name)).find(|file| file.is_file()))
        };
        let shells: Vec<_> = ["sh", "bash", "zsh", "fish"].into_iter().filter_map(|name| on_path(name).map(|path| (name, path))).collect();
        assert!(shells.iter().any(|(name, _)| *name == "sh"), "sh is always there");
        // A stand-in executable that answers --version as westerm does.
        let good = b"#!/bin/sh\necho 'westerm 9.9.9 (host rs-test) - test build'\n";
        for (name, shell) in shells {
            let home = tempfile::tempdir().unwrap();
            let upload = |payload: &[u8]| {
                let mut child = Command::new(&shell)
                    .args(["-c", UPLOAD])
                    .env("HOME", home.path())
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap();
                {
                    use std::io::Write;
                    child.stdin.take().unwrap().write_all(payload).unwrap();
                }
                child.wait().unwrap().success()
            };
            assert!(upload(good), "{name}: upload");
            let installed = home.path().join(".local/bin/westerm");
            assert_eq!(std::fs::read(&installed).unwrap(), good, "{name}");
            assert_eq!(std::fs::metadata(&installed).unwrap().permissions().mode() & 0o777, 0o755, "{name}");

            // A cut-off upload (the pane closed mid-way) must not replace a working westerm.
            assert!(!upload(b"\x7fELF cut off"), "{name}: a broken upload fails");
            assert_eq!(std::fs::read(&installed).unwrap(), good, "{name}: the installed westerm stays");

            let probe = Command::new(&shell).args(["-c", PROBE]).env("HOME", home.path()).output().unwrap();
            assert!(probe.status.success(), "{name}: probe");
            let parsed = parse_probe(&String::from_utf8_lossy(&probe.stdout));
            assert!(
                parsed.is_some_and(|p| p.version.as_deref() == Some("westerm 9.9.9 (host rs-test) - test build")),
                "{name}: {:?}",
                String::from_utf8_lossy(&probe.stdout)
            );
        }
    }
}
