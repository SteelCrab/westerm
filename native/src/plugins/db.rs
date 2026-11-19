//! The Databases plugin's console launches. The server writes what a console
//! runs into a private spec file under a random token; the session query
//! carries only that token, and the host types the spec once and deletes it.
//! So the browser never sends a command, and a password never reaches a
//! typed line (it stays in a secret file the spec points to).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::LaunchError;
use crate::quote::{quoted_line, ShellKind};
use crate::state_dir::{private_file, state_dir};

/// The plugin's id.
pub const ID: &str = "db";
/// The only session action: open a console.
pub const CONSOLE_ACTION: &str = "console";
/// The folder of waiting console specs, in the state folder.
const SPEC_DIR: &str = "db-consoles";

/// A console token as the server makes it: 32 lowercase hex digits.
pub fn is_token(text: &str) -> bool {
    super::is_lower_hex(text, 32)
}

/// A console to open. The token is always checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub token: String,
}

/// Where the spec of `token` waits.
pub fn spec_path(state_dir: &Path, token: &str) -> PathBuf {
    state_dir.join(SPEC_DIR).join(format!("{token}.json"))
}

/// Writes the program and arguments a console runs, readable by the user only.
pub fn write_spec(state_dir: &Path, token: &str, argv: &[String]) -> io::Result<()> {
    fs::create_dir_all(state_dir.join(SPEC_DIR))?;
    let body = serde_json::to_vec(&serde_json::json!({ "argv": argv })).map_err(io::Error::other)?;
    let mut file = private_file().write(true).create_new(true).open(spec_path(state_dir, token))?;
    file.write_all(&body)
}

/// Removes every waiting spec (the server starts: no page waits for them).
pub fn clear_specs(state_dir: &Path) {
    let _ = fs::remove_dir_all(state_dir.join(SPEC_DIR));
}

/// The words of the spec, which is deleted once read; None when it is gone or broken.
fn take_spec(state_dir: &Path, token: &str) -> Option<Vec<String>> {
    let path = spec_path(state_dir, token);
    let bytes = fs::read(&path).ok()?;
    let _ = fs::remove_file(&path);
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let words: Vec<String> = value.get("argv")?.as_array()?.iter().map(|word| word.as_str().map(str::to_owned)).collect::<Option<_>>()?;
    (!words.is_empty()).then_some(words)
}

/// The line a new shell types for `launch`, from the specs in `state_dir`.
/// The program runs through `env`, so no alias, function, or builtin of the
/// user's shell replaces it.
pub fn command_in(launch: &Launch, state_dir: &Path, shell: ShellKind) -> Result<String, LaunchError> {
    let argv = take_spec(state_dir, &launch.token).ok_or(LaunchError::Expired)?;
    let words: Vec<String> = ["env", "--"].into_iter().map(str::to_owned).chain(argv).collect();
    quoted_line(&words, shell).map_err(|_| LaunchError::ControlCharacter)
}

/// `command_in` with westerm's state folder.
pub fn command(launch: &Launch, shell: ShellKind) -> Result<String, LaunchError> {
    let dir = state_dir().map_err(|_| LaunchError::Expired)?;
    command_in(launch, &dir, shell)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_32_lowercase_hex() {
        assert!(is_token(&"a".repeat(32)));
        assert!(is_token("0123456789abcdef0123456789abcdef"));
        for bad in ["a".repeat(31), "a".repeat(33), "A".repeat(32), format!("{};", "a".repeat(31)), String::new()] {
            assert!(!is_token(&bad), "{bad}");
        }
    }

    #[test]
    fn a_console_types_its_spec_once_through_env() {
        let dir = tempfile::tempdir().unwrap();
        let token = "0".repeat(32);
        write_spec(dir.path(), &token, &["psql".into(), "host=h passfile=/x/y".into()]).unwrap();
        let launch = Launch { token: token.clone() };
        assert_eq!(command_in(&launch, dir.path(), ShellKind::Sh).as_deref(), Ok("'env' '--' 'psql' 'host=h passfile=/x/y'"));
        assert!(!spec_path(dir.path(), &token).exists(), "the spec is read once");
        assert_eq!(command_in(&launch, dir.path(), ShellKind::Sh), Err(LaunchError::Expired));
    }

    #[test]
    fn a_broken_spec_reads_as_expired() {
        let dir = tempfile::tempdir().unwrap();
        let token = "1".repeat(32);
        fs::create_dir_all(dir.path().join(SPEC_DIR)).unwrap();
        for body in ["{", r#"{"argv":[]}"#, r#"{"argv":[1]}"#] {
            fs::write(spec_path(dir.path(), &token), body).unwrap();
            assert_eq!(command_in(&Launch { token: token.clone() }, dir.path(), ShellKind::Sh), Err(LaunchError::Expired), "{body}");
        }
    }

    #[test]
    fn a_control_character_in_a_spec_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let token = "2".repeat(32);
        write_spec(dir.path(), &token, &["sqlite3".into(), "a\u{3}b".into()]).unwrap();
        assert_eq!(command_in(&Launch { token }, dir.path(), ShellKind::Sh), Err(LaunchError::ControlCharacter));
    }

    #[cfg(unix)]
    #[test]
    fn the_spec_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let token = "3".repeat(32);
        write_spec(dir.path(), &token, &["sqlite3".into()]).unwrap();
        let mode = fs::metadata(spec_path(dir.path(), &token)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
