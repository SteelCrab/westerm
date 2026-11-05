//! What the dashboard remembers per server, in `<state>/remote`:
//! - `hosts/<name>.json`: how to reach its gate without SSH next time.
//! - `pending-close.json`: sessions whose panes closed while the server was
//!   out of reach; they are ended there on the next connection.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::state_dir::private_file;

/// How to reach a server's gate, saved after an SSH setup.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Record {
    pub hostname: String,
    pub port: u16,
    pub fingerprint: String,
    /// This dashboard's `version_line()` when it set the server up. Another
    /// value means this westerm changed and the server needs a new setup.
    pub version: String,
    /// The `version_line()` for which the user chose to keep the server's
    /// older host instead of replacing it.
    #[serde(default)]
    pub declined: Option<String>,
}

/// `h-` + the destination, with every byte outside `[A-Za-z0-9.-]` as `_xx`.
pub fn file_name(target: &str) -> String {
    let mut name = String::from("h-");
    for byte in target.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-' {
            name.push(byte as char);
        } else {
            name.push_str(&format!("_{byte:02x}"));
        }
    }
    name.push_str(".json");
    name
}

fn host_path(dir: &Path, target: &str) -> PathBuf {
    dir.join("hosts").join(file_name(target))
}

/// Writes through a temporary file in the same folder (0600).
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let temp = path.with_extension(format!("tmp{}", std::process::id()));
    let mut file = private_file().create(true).truncate(true).write(true).open(&temp)?;
    file.write_all(bytes)?;
    fs::rename(&temp, path)
}

pub fn load(dir: &Path, target: &str) -> Option<Record> {
    serde_json::from_slice(&fs::read(host_path(dir, target)).ok()?).ok()
}

pub fn save(dir: &Path, target: &str, record: &Record) -> io::Result<()> {
    write_private(&host_path(dir, target), &serde_json::to_vec_pretty(record).map_err(io::Error::other)?)
}

/// Serializes the read-modify-write of `pending-close.json`.
static PENDING: Mutex<()> = Mutex::new(());

fn pending_path(dir: &Path) -> PathBuf {
    dir.join("pending-close.json")
}

fn read_pending(dir: &Path) -> BTreeMap<String, Vec<String>> {
    fs::read(pending_path(dir)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn write_pending(dir: &Path, pending: &BTreeMap<String, Vec<String>>) -> io::Result<()> {
    write_private(&pending_path(dir), &serde_json::to_vec(pending).map_err(io::Error::other)?)
}

/// Remembers that `session` on `target` should end at the next connection.
pub fn add_pending_close(dir: &Path, target: &str, session: &str) -> io::Result<()> {
    let _guard = PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut pending = read_pending(dir);
    let ids = pending.entry(target.to_owned()).or_default();
    if !ids.iter().any(|id| id == session) {
        ids.push(session.to_owned());
    }
    write_pending(dir, &pending)
}

/// The sessions to end on `target`, forgotten as they are returned.
pub fn take_pending_closes(dir: &Path, target: &str) -> Vec<String> {
    let _guard = PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut pending = read_pending(dir);
    let Some(ids) = pending.remove(target) else { return Vec::new() };
    let _ = write_pending(dir, &pending);
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_safe_for_any_target() {
        assert_eq!(file_name("prod"), "h-prod.json");
        assert_eq!(file_name("pista@msi-linux-private-pista"), "h-pista_40msi-linux-private-pista.json");
        assert_eq!(file_name("u@[::1]"), "h-u_40_5b_3a_3a1_5d.json");
        assert_eq!(file_name(".."), "h-...json");
        assert_eq!(file_name("dev_1"), "h-dev_5f1.json", "an underscore is encoded too, so names stay distinct");
        for target in ["a/b", "a\\b", "..", "~"] {
            assert!(!file_name(target).contains(['/', '\\']), "{target}");
        }
    }

    fn sample() -> Record {
        Record {
            hostname: "192.168.219.105".into(),
            port: 4317,
            fingerprint: "ab".repeat(32),
            version: "westerm 0.2.0 (host rs-1)".into(),
            declined: None,
        }
    }

    #[test]
    fn records_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path(), "prod"), None);
        save(dir.path(), "prod", &sample()).unwrap();
        assert_eq!(load(dir.path(), "prod"), Some(sample()));
        let declined = Record { declined: Some("westerm 0.3.0 (host rs-2)".into()), ..sample() };
        save(dir.path(), "prod", &declined).unwrap();
        assert_eq!(load(dir.path(), "prod"), Some(declined));
        assert_eq!(load(dir.path(), "other"), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("hosts/h-prod.json")).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn pending_closes_are_taken_once() {
        let dir = tempfile::tempdir().unwrap();
        add_pending_close(dir.path(), "t1", "sa1").unwrap();
        add_pending_close(dir.path(), "t1", "sb1").unwrap();
        add_pending_close(dir.path(), "t1", "sa1").unwrap();
        add_pending_close(dir.path(), "t2", "sc1").unwrap();
        assert_eq!(take_pending_closes(dir.path(), "t1"), ["sa1", "sb1"]);
        assert!(take_pending_closes(dir.path(), "t1").is_empty());
        assert_eq!(take_pending_closes(dir.path(), "t2"), ["sc1"]);
        assert!(take_pending_closes(dir.path(), "t3").is_empty());
    }
}
