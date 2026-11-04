//! Saved sessions on disk: what the host needs to bring a session back after
//! its process ended (program restart or reboot). Same files the Node host
//! wrote, so existing saves keep working.

use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::protocol::{is_id, SessionKind};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub version: u8,
    pub id: String,
    pub kind: SessionKind,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    /// Last known working directory; a revived process starts here.
    pub cwd: PathBuf,
    /// ISO time of the last save.
    pub saved_at: String,
}

/// Saved screens are bounded so a busy log cannot fill the disk.
pub const SCREEN_MAX_BYTES: usize = 2 * 1024 * 1024;
const PRUNE_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);

pub struct SessionStore {
    dir: PathBuf,
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&temp)?;
    file.write_all(data)?;
    fs::rename(temp, path)
}

impl SessionStore {
    pub fn new(state_dir: &Path) -> std::io::Result<Self> {
        let dir = state_dir.join("sessions");
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn path(&self, id: &str, ext: &str) -> Option<PathBuf> {
        is_id(id).then(|| self.dir.join(format!("{id}.{ext}")))
    }

    pub fn read_meta(&self, id: &str) -> Option<SessionMeta> {
        let meta: SessionMeta = serde_json::from_slice(&fs::read(self.path(id, "json")?).ok()?).ok()?;
        (meta.version == 1 && meta.id == id).then_some(meta)
    }

    pub fn write_meta(&self, meta: &SessionMeta) -> std::io::Result<()> {
        let path = self.path(&meta.id, "json").ok_or(std::io::ErrorKind::InvalidInput)?;
        let mut json = serde_json::to_vec(meta)?;
        json.push(b'\n');
        write_atomic(&path, &json)
    }

    pub fn read_screen(&self, id: &str) -> Option<String> {
        fs::read_to_string(self.path(id, "screen")?).ok()
    }

    pub fn write_screen(&self, id: &str, screen: &str) -> std::io::Result<()> {
        let path = self.path(id, "screen").ok_or(std::io::ErrorKind::InvalidInput)?;
        // Keep the newest part: drop whole lines from the front past the limit.
        let mut text = screen;
        if text.len() > SCREEN_MAX_BYTES {
            let mut cut = text.len() - SCREEN_MAX_BYTES / 2;
            while !text.is_char_boundary(cut) {
                cut += 1;
            }
            text = &text[cut..];
            if let Some(newline) = text.find('\n') {
                text = &text[newline + 1..];
            }
        }
        write_atomic(&path, text.as_bytes())
    }

    pub fn remove(&self, id: &str) {
        for ext in ["json", "screen"] {
            if let Some(path) = self.path(id, ext) {
                let _ = fs::remove_file(path);
            }
        }
    }

    /// Deletes sessions nobody attached to for 30 days.
    pub fn prune(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else { return };
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let old = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .is_ok_and(|modified| now.duration_since(modified).unwrap_or_default() > PRUNE_AFTER);
            if old {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_and_screens_round_trip_and_reject_bad_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path()).unwrap();
        let meta = SessionMeta {
            version: 1,
            id: "sabc".into(),
            kind: SessionKind::Shell,
            url: None,
            target: Some("prod".into()),
            cwd: "/tmp".into(),
            saved_at: "2026-11-04T00:00:00.000Z".into(),
        };
        store.write_meta(&meta).unwrap();
        assert_eq!(store.read_meta("sabc"), Some(meta));
        store.write_screen("sabc", "hello\r\n").unwrap();
        assert_eq!(store.read_screen("sabc").as_deref(), Some("hello\r\n"));
        assert!(store.read_meta("../etc").is_none());
        store.remove("sabc");
        assert!(store.read_meta("sabc").is_none() && store.read_screen("sabc").is_none());
    }

    #[test]
    fn reads_files_the_node_host_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path()).unwrap();
        fs::write(
            dir.path().join("sessions/sold.json"),
            r#"{"version":1,"id":"sold","kind":"shell","url":null,"cwd":"/Users/x","savedAt":"2026-11-04T14:01:07.000Z"}"#,
        )
        .unwrap();
        let meta = store.read_meta("sold").unwrap();
        assert_eq!(meta.target, None);
        assert_eq!(meta.cwd, PathBuf::from("/Users/x"));
    }

    #[test]
    fn huge_screens_keep_the_tail() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path()).unwrap();
        let text: String = (0..300_000).map(|i| format!("line {i}\n")).collect();
        store.write_screen("sbig", &text).unwrap();
        let saved = store.read_screen("sbig").unwrap();
        assert!(saved.len() <= SCREEN_MAX_BYTES);
        assert!(saved.ends_with("line 299999\n"));
        assert!(saved.starts_with("line "), "starts at a whole line");
    }
}
