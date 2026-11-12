//! Remote westerm: a server's own PTY host, reached over QUIC. These parts
//! are shared by the gate (on the server) and the dashboard (here). None of
//! them are host code (see HOST_SOURCES in build.rs).

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub mod authorized;
#[cfg(unix)]
pub mod daemon;
#[cfg(unix)]
pub mod dashboard;
#[cfg(unix)]
pub mod gate;
pub mod identity;
pub mod punch;
#[cfg(unix)]
pub mod punch_gate;
pub mod report;
#[cfg(unix)]
pub mod setup;
pub mod stun;
pub mod tls;

/// TLS application protocol: the dashboard ↔ host rules carried over QUIC.
pub const ALPN: &[u8] = b"westerm/1";
/// The UDP port a gate tries first, then the next ones up to PORT_SPAN in all.
pub const DEFAULT_PORT: u16 = 4317;
pub const PORT_SPAN: u16 = 10;
/// Keeps NAT mappings open, and makes a dead path show within IDLE_TIMEOUT.
pub const KEEP_ALIVE: Duration = Duration::from_secs(4);
/// A connection that hears nothing for this long is dead; panes reconnect.
/// Shells keep running on the server meanwhile.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a new QUIC connection may take before the SSH tunnel is tried.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);

/// `<state>/remote`: keys, certificates, and remote records. Mode 0700.
pub fn remote_dir(state: &Path) -> io::Result<PathBuf> {
    let dir = state.join("remote");
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_dir_is_private() {
        let state = tempfile::tempdir().unwrap();
        let dir = remote_dir(state.path()).unwrap();
        assert_eq!(dir, state.path().join("remote"));
        assert!(dir.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        }
    }
}
