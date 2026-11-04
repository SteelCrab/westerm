//! Private state directory (mode 0700): the host socket, saved screens, and
//! the dashboard layout. WESTERM_STATE_DIR overrides it (tests use a temp dir).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub fn state_dir() -> std::io::Result<PathBuf> {
    let dir = match std::env::var_os("WESTERM_STATE_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => state_base().join("westerm"),
    };
    fs::create_dir_all(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

fn state_base() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local").join("state"))
}

/// Where versions before the rename kept their state. None when a custom
/// state directory is in use, since then there is nothing to migrate.
pub fn legacy_state_dir() -> Option<PathBuf> {
    if std::env::var_os("WESTERM_STATE_DIR").is_some() {
        return None;
    }
    Some(state_base().join("wterm-dashboard")).filter(|dir| dir.is_dir())
}

pub fn host_socket(dir: &Path) -> PathBuf {
    dir.join("host.sock")
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}
