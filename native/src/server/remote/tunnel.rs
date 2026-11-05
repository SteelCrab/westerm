//! The fallback when UDP does not get through: `ssh -L` forwards a local
//! Unix socket to the server dashboard's socket. Each connection to it is
//! a new SSH channel, so panes still get a stream each. Shells survive on
//! the server as with QUIC, but a network change ends the tunnel, and the
//! panes reconnect through a new one. The tunnel never outlives the
//! dashboard, even one killed by a signal (see KEEP_OPEN).

use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;

use super::{ssh, Failure, HostStream};
use crate::protocol::RemoteFailure;

pub struct Tunnel {
    child: tokio::process::Child,
    socket: PathBuf,
    /// Never written. It closes when the dashboard exits, however it exits,
    /// which ends KEEP_OPEN on the server and then this ssh.
    _keep_open: tokio::process::ChildStdin,
}

impl Tunnel {
    /// Starts `ssh -N -L <local>:<remote>` and waits up to 10 s for the
    /// local socket.
    pub async fn start(target: &str, local: PathBuf, remote: &str) -> Result<Self, Failure> {
        let _ = std::fs::remove_file(&local);
        let mut child = tokio::process::Command::new("ssh")
            .args(tunnel_args(target, &local, remote))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| Failure::new(RemoteFailure::Transport, format!("cannot run ssh: {e}")))?;
        let keep_open = child.stdin.take().expect("ssh's stdin is piped");
        for _ in 0..100 {
            if local.exists() {
                return Ok(Self { child, socket: local, _keep_open: keep_open });
            }
            if let Ok(Some(_)) = child.try_wait() {
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_string(&mut stderr).await;
                }
                return Err(Failure::new(ssh::classify(&stderr), format!("SSH tunnel: {}", stderr.trim())));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(Failure::new(RemoteFailure::Transport, "the SSH tunnel did not open within 10 s"))
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

/// The tunnel session's remote command: it ends when the dashboard's end of
/// ssh's stdin closes, and ssh ends with it.
pub const KEEP_OPEN: &str = "cat >/dev/null";

pub fn tunnel_args(target: &str, local: &Path, remote: &str) -> Vec<String> {
    let mut args: Vec<String> = ["-T", "-o", "BatchMode=yes", "-o", "ExitOnForwardFailure=yes", "-o", "StreamLocalBindUnlink=yes"]
        .into_iter()
        .chain(["-o", "ServerAliveInterval=10", "-L"])
        .map(str::to_owned)
        .collect();
    args.push(format!("{}:{remote}", local.display()));
    args.extend(["--".to_owned(), target.to_owned(), KEEP_OPEN.to_owned()]);
    args
}

/// A new connection through the tunnel at `socket`.
pub async fn open(socket: &Path) -> io::Result<HostStream> {
    Ok(Box::new(tokio::net::UnixStream::connect(socket).await?))
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dashboard killed by a signal runs no destructors, so ssh must end
    /// on its own: it runs a remote command that ends when the dashboard's
    /// end of ssh's stdin closes, instead of `-N` (which runs forever).
    #[test]
    fn the_tunnel_ends_with_its_dashboard() {
        let args = tunnel_args("prod", Path::new("/s/t-1.sock"), "/home/me/.local/state/westerm/host.sock");
        assert!(!args.iter().any(|arg| arg == "-N"), "{args:?}");
        assert!(args.contains(&"/s/t-1.sock:/home/me/.local/state/westerm/host.sock".to_owned()));
        assert_eq!(args[args.len() - 3..], ["--", "prod", KEEP_OPEN]);
        assert!(!KEEP_OPEN.contains(['$', '\'', '"', '`', '{']), "a fixed command for any login shell");
    }
}
