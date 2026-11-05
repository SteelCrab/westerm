//! The clients a gate accepts: one certificate fingerprint per line. Only
//! `westerm remote-setup` adds lines, and it runs over SSH, so only someone
//! who can log in to the server can authorize a client. Deleting a line
//! refuses that client from its next connection on.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use super::identity::is_fingerprint;
use crate::state_dir::private_file;

/// The valid fingerprints in file order; other lines are ignored. A missing
/// file is an empty list.
pub fn read(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .map(|text| text.lines().map(str::trim).filter(|line| is_fingerprint(line)).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Adds `fingerprint` unless it is listed. True when it was added.
pub fn add(path: &Path, fingerprint: &str) -> io::Result<bool> {
    if !is_fingerprint(fingerprint) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a certificate fingerprint"));
    }
    if read(path).iter().any(|known| known == fingerprint) {
        return Ok(false);
    }
    // A file edited by hand may lack its last newline.
    let needs_newline = fs::read(path).is_ok_and(|bytes| bytes.last().is_some_and(|&last| last != b'\n'));
    let mut file = private_file().create(true).append(true).open(path)?;
    let line = if needs_newline { format!("\n{fingerprint}\n") } else { format!("{fingerprint}\n") };
    file.write_all(line.as_bytes())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn adds_each_fingerprint_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("authorized_clients");
        let fp = "ab".repeat(32);
        assert!(read(&path).is_empty(), "a missing file is an empty list");
        assert!(add(&path, &fp).unwrap());
        assert!(!add(&path, &fp).unwrap());
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"junk\n").unwrap();
        assert_eq!(read(&path), vec![fp.clone()]);
        assert_eq!(add(&path, "nope").unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
        let other = "cd".repeat(32);
        assert!(add(&path, &other).unwrap());
        assert_eq!(read(&path), vec![fp, other]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
