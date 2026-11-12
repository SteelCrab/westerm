//! SSH tunnels to databases behind a host from `~/.ssh/config`: the system
//! `ssh` forwards a free local port to the database. Keys and agents only
//! (BatchMode): a host that wants a password or a second factor fails with a
//! message that says so.

use std::path::Path;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use super::driver::Endpoint;

/// How much of ssh's error output is read.
const STDERR_BYTES: usize = 4096;

/// A running `ssh -N -L`. Dropping it stops ssh; `close` also waits for it.
pub struct Tunnel {
    child: Mutex<Option<Child>>,
    local: Endpoint,
}

/// What a failed tunnel tells the user: a refused key is explained.
pub fn error_text(alias: &str, stderr: &str) -> String {
    let first = stderr.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    if first.is_empty() {
        format!("ssh to {alias} ended")
    } else if stderr.contains("Permission denied") {
        format!("SSH needs a key or an agent for {alias}: {first}")
    } else {
        first.to_owned()
    }
}

/// The arguments of the ssh that forwards `local_port` to `host:port` through `alias`.
pub fn ssh_args(alias: &str, local_port: u16, host: &str, port: u16) -> Vec<String> {
    [
        "-N".to_owned(),
        "-L".to_owned(),
        format!("127.0.0.1:{local_port}:{host}:{port}"),
        "-o".to_owned(),
        "ExitOnForwardFailure=yes".to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        alias.to_owned(),
    ]
    .to_vec()
}

/// A local port nobody listens on now.
fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind(("127.0.0.1", 0)).and_then(|listener| listener.local_addr()).map(|address| address.port()).map_err(|error| error.to_string())
}

/// The first part of what a finished ssh wrote to stderr.
async fn stderr_of(child: &mut Child) -> String {
    let mut buffer = Vec::new();
    if let Some(stderr) = child.stderr.as_mut() {
        let _ = tokio::time::timeout(Duration::from_millis(500), stderr.take(STDERR_BYTES as u64).read_to_end(&mut buffer)).await;
    }
    String::from_utf8_lossy(&buffer).into_owned()
}

impl Tunnel {
    /// Starts ssh and waits until the local port answers, ssh exits, or `ready` passes.
    pub async fn open(ssh: &Path, alias: &str, host: &str, port: u16, ready: Duration) -> Result<Tunnel, String> {
        let local_port = free_port()?;
        let mut child = Command::new(ssh)
            .args(ssh_args(alias, local_port, host, port))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| format!("Could not start ssh: {error}"))?;
        let deadline = Instant::now() + ready;
        loop {
            if let Ok(Some(_)) = child.try_wait() {
                return Err(error_text(alias, &stderr_of(&mut child).await));
            }
            let probe = tokio::net::TcpStream::connect(("127.0.0.1", local_port));
            if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_millis(200), probe).await {
                return Ok(Tunnel { child: Mutex::new(Some(child)), local: Endpoint { host: "127.0.0.1".into(), port: local_port } });
            }
            if Instant::now() >= deadline {
                let _ = child.kill().await;
                return Err(format!("ssh to {alias} did not open the tunnel within {} s", ready.as_secs_f32()));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Where the database answers through the tunnel.
    pub fn local(&self) -> Endpoint {
        self.local.clone()
    }

    /// Whether ssh still runs.
    pub fn alive(&self) -> bool {
        let mut slot = self.child.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.as_mut().is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    /// Stops ssh and waits for it.
    pub async fn close(&self) {
        let child = self.child.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(mut child) = child {
            let _ = child.kill().await;
        }
    }
}

#[cfg(all(test, unix))]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    pub(crate) fn has_python() -> bool {
        std::process::Command::new("python3").arg("--version").output().is_ok_and(|output| output.status.success())
    }

    /// A fake `ssh` that records its arguments and its pid, then listens on the `-L` port.
    pub(crate) fn fake_ssh(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("ssh");
        let body = format!(
            "#!/bin/sh\necho \"$@\" > '{args}'\necho $$ > '{pid}'\nPORT=$(echo \"$3\" | cut -d: -f2)\n\
             exec python3 -c 'import socket,sys,time; s=socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind((\"127.0.0.1\", int(sys.argv[1]))); s.listen(); time.sleep(60)' \"$PORT\"\n",
            args = dir.join("args.txt").display(),
            pid = dir.join("ssh.pid").display(),
        );
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn script(dir: &Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("ssh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    async fn gone(pid: libc::pid_t) -> bool {
        for _ in 0..100 {
            // SAFETY: signal 0 only checks that the process exists.
            if unsafe { libc::kill(pid, 0) } == -1 {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    #[test]
    fn the_ssh_arguments_are_fixed() {
        assert_eq!(
            ssh_args("bastion", 50123, "db.internal", 5432),
            ["-N", "-L", "127.0.0.1:50123:db.internal:5432", "-o", "ExitOnForwardFailure=yes", "-o", "BatchMode=yes", "bastion"]
        );
    }

    #[test]
    fn a_refused_key_is_explained() {
        assert_eq!(
            error_text("bastion", "user@bastion: Permission denied (publickey).\r\nmore"),
            "SSH needs a key or an agent for bastion: user@bastion: Permission denied (publickey)."
        );
        assert_eq!(error_text("bastion", "ssh: Could not resolve hostname bastion\n"), "ssh: Could not resolve hostname bastion");
        assert_eq!(error_text("bastion", ""), "ssh to bastion ended");
    }

    #[tokio::test]
    async fn a_tunnel_passes_the_fixed_ssh_options() {
        if !has_python() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let ssh = fake_ssh(dir.path());
        let tunnel = Tunnel::open(&ssh, "bastion", "db.internal", 5432, Duration::from_secs(5)).await.unwrap();
        let local = tunnel.local();
        assert_eq!(local.host, "127.0.0.1");
        let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
        assert_eq!(args.trim(), format!("-N -L 127.0.0.1:{}:db.internal:5432 -o ExitOnForwardFailure=yes -o BatchMode=yes bastion", local.port));
        assert!(tunnel.alive());
        tunnel.close().await;
    }

    #[tokio::test]
    async fn a_refused_key_explains_itself() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = script(dir.path(), "echo 'user@bastion: Permission denied (publickey).' >&2\nexit 255");
        let error = Tunnel::open(&ssh, "bastion", "db", 5432, Duration::from_secs(5)).await.err().unwrap();
        assert_eq!(error, "SSH needs a key or an agent for bastion: user@bastion: Permission denied (publickey).");
    }

    #[tokio::test]
    async fn a_silent_ssh_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = script(dir.path(), "sleep 30");
        let started = Instant::now();
        let error = Tunnel::open(&ssh, "bastion", "db", 5432, Duration::from_millis(300)).await.err().unwrap();
        assert!(error.contains("bastion"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn closing_a_tunnel_ends_ssh() {
        if !has_python() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let tunnel = Tunnel::open(&fake_ssh(dir.path()), "bastion", "db", 5432, Duration::from_secs(5)).await.unwrap();
        let pid: libc::pid_t = std::fs::read_to_string(dir.path().join("ssh.pid")).unwrap().trim().parse().unwrap();
        tunnel.close().await;
        assert!(!tunnel.alive());
        assert!(gone(pid).await);
    }
}
