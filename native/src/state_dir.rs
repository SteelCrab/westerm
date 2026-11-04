//! Private state directory: the host endpoint, saved screens, and the
//! dashboard layout. WESTERM_STATE_DIR overrides it (tests use a temp dir).
//!
//! - macOS and Linux: $XDG_STATE_HOME/westerm or ~/.local/state/westerm, mode 0700.
//! - Windows: %LOCALAPPDATA%\westerm, which only the user can open.

use std::fs;
use std::path::PathBuf;

pub fn state_dir() -> std::io::Result<PathBuf> {
    let dir = match std::env::var_os("WESTERM_STATE_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => state_base().join("westerm"),
    };
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

fn state_base() -> PathBuf {
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local);
    }
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

/// The user's home: %USERPROFILE% on Windows (where OpenSSH looks), $HOME elsewhere.
pub fn home_dir() -> PathBuf {
    let order: &[&str] = if cfg!(windows) { &["USERPROFILE", "HOME"] } else { &["HOME"] };
    order
        .iter()
        .find_map(|key| std::env::var_os(key))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Options for a file only the user may read: mode 0600 on Unix. On Windows
/// the state directory's own permissions already keep others out.
pub fn private_file() -> fs::OpenOptions {
    #[allow(unused_mut)]
    let mut options = fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}
