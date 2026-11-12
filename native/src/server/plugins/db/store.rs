//! The saved connections: `db-connections.json` in the state folder, without
//! passwords (those live in the keychain, `secrets.rs`). Kept in memory too.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

const FILE: &str = "db-connections.json";
/// At most this many connections.
const MAX_CONNECTIONS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Postgres,
    Mysql,
    Sqlite,
}

impl Engine {
    /// The port a new connection takes when none is given.
    pub fn default_port(self) -> Option<u16> {
        match self {
            Self::Postgres => Some(5432),
            Self::Mysql => Some(3306),
            Self::Sqlite => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SslMode {
    #[default]
    Prefer,
    Disable,
    Require,
    VerifyFull,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Color {
    #[default]
    Gray,
    Green,
    Yellow,
    Red,
    Blue,
}

fn yes() -> bool {
    true
}

/// A connection as the dialog sends it. New connections are read-only unless they say otherwise.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInput {
    pub name: String,
    pub engine: Engine,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub ssl: SslMode,
    /// A SQLite file: absolute, or `~/…`.
    #[serde(default)]
    pub path: Option<String>,
    /// A `~/.ssh/config` host (or `[user@]host`) to tunnel through.
    #[serde(default)]
    pub ssh: Option<String>,
    #[serde(default)]
    pub color: Color,
    #[serde(default = "yes")]
    pub read_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    #[serde(flatten)]
    pub input: ConnectionInput,
}

#[derive(Debug)]
pub enum StoreError {
    /// A field that does not pass the checks: its name.
    Invalid(&'static str),
    NameUsed,
    NotFound,
    Io(io::Error),
}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    version: u32,
    connections: Vec<Connection>,
}

/// The connections, in memory and in the file.
#[derive(Default)]
pub struct Store {
    path: PathBuf,
    connections: RwLock<Vec<Connection>>,
}

/// A connection id: 16 lowercase hex digits.
pub fn is_connection_id(text: &str) -> bool {
    text.len() == 16 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// No control characters (newlines, NUL, escapes).
fn clean(text: &str) -> bool {
    !text.chars().any(char::is_control)
}

/// `text` when it is at most `max` characters and clean.
fn bounded(text: &str, max: usize) -> bool {
    text.chars().count() <= max && clean(text)
}

/// An absolute path on Unix or Windows, or one under the home folder (`~/`).
fn is_file_path(path: &str) -> bool {
    let absolute = path.starts_with('/') || path.starts_with("~/") || path.starts_with("\\\\") || {
        let bytes = path.as_bytes();
        bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/')
    };
    absolute && bounded(path, 4096)
}

/// `input` checked and tidied: the port filled in, fields the engine does not use cleared.
pub fn validate(input: ConnectionInput) -> Result<ConnectionInput, StoreError> {
    let name = input.name.trim().to_owned();
    if name.is_empty() || !bounded(&name, 64) {
        return Err(StoreError::Invalid("name"));
    }
    if input.engine == Engine::Sqlite {
        let path = input.path.filter(|path| is_file_path(path)).ok_or(StoreError::Invalid("path"))?;
        return Ok(ConnectionInput {
            name,
            host: String::new(),
            port: None,
            user: String::new(),
            database: String::new(),
            ssl: SslMode::default(),
            path: Some(path),
            ssh: None,
            ..input
        });
    }
    let host = input.host.trim().to_owned();
    if host.is_empty() || !bounded(&host, 255) || host.chars().any(char::is_whitespace) {
        return Err(StoreError::Invalid("host"));
    }
    if input.port == Some(0) {
        return Err(StoreError::Invalid("port"));
    }
    if !bounded(&input.user, 128) {
        return Err(StoreError::Invalid("user"));
    }
    if !bounded(&input.database, 128) {
        return Err(StoreError::Invalid("database"));
    }
    let ssh = match input.ssh.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(target) if crate::protocol::normalize_ssh_target(target).as_deref() == Some(target) => Some(target.to_owned()),
        Some(_) => return Err(StoreError::Invalid("ssh")),
    };
    Ok(ConnectionInput { name, host, port: input.port.or(input.engine.default_port()), path: None, ssh, ..input })
}

fn lock_error() -> io::Error {
    io::Error::other("the connection list is locked by a failed thread")
}

impl Store {
    /// Reads the file. A missing file is no connections; a broken one is moved
    /// aside to `.bak` (never overwritten) and reads as none.
    pub fn load(state_dir: &Path) -> Self {
        let path = state_dir.join(FILE);
        let connections = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<File>(&bytes) {
                Ok(file) => file.connections,
                Err(_) => {
                    let _ = fs::rename(&path, state_dir.join(format!("{FILE}.bak")));
                    Vec::new()
                }
            },
            Err(_) => Vec::new(),
        };
        Self { path, connections: RwLock::new(connections) }
    }

    pub fn list(&self) -> Vec<Connection> {
        self.connections.read().map(|list| list.clone()).unwrap_or_default()
    }

    pub fn get(&self, id: &str) -> Option<Connection> {
        self.connections.read().ok()?.iter().find(|connection| connection.id == id).cloned()
    }

    pub fn create(&self, input: ConnectionInput) -> Result<Connection, StoreError> {
        let input = validate(input)?;
        let mut list = self.connections.write().map_err(|_| lock_error())?;
        if list.iter().any(|other| other.input.name == input.name) {
            return Err(StoreError::NameUsed);
        }
        if list.len() >= MAX_CONNECTIONS {
            return Err(StoreError::Invalid("count"));
        }
        let id = loop {
            let id = super::random_hex(8);
            if !list.iter().any(|other| other.id == id) {
                break id;
            }
        };
        let connection = Connection { id, input };
        let mut next = list.clone();
        next.push(connection.clone());
        self.write(&next)?;
        *list = next;
        Ok(connection)
    }

    pub fn update(&self, id: &str, input: ConnectionInput) -> Result<Connection, StoreError> {
        let input = validate(input)?;
        let mut list = self.connections.write().map_err(|_| lock_error())?;
        let index = list.iter().position(|other| other.id == id).ok_or(StoreError::NotFound)?;
        if list.iter().any(|other| other.id != id && other.input.name == input.name) {
            return Err(StoreError::NameUsed);
        }
        let connection = Connection { id: id.to_owned(), input };
        let mut next = list.clone();
        next[index] = connection.clone();
        self.write(&next)?;
        *list = next;
        Ok(connection)
    }

    /// Whether there was such a connection.
    pub fn remove(&self, id: &str) -> io::Result<bool> {
        let mut list = self.connections.write().map_err(|_| lock_error())?;
        let next: Vec<Connection> = list.iter().filter(|other| other.id != id).cloned().collect();
        if next.len() == list.len() {
            return Ok(false);
        }
        self.write(&next)?;
        *list = next;
        Ok(true)
    }

    /// Removes every connection; returns their ids.
    pub fn clear(&self) -> io::Result<Vec<String>> {
        let mut list = self.connections.write().map_err(|_| lock_error())?;
        self.write(&[])?;
        Ok(std::mem::take(&mut *list).into_iter().map(|connection| connection.id).collect())
    }

    /// Writes the whole list to a private file next to it, then renames it into place.
    fn write(&self, connections: &[Connection]) -> io::Result<()> {
        let body = serde_json::to_vec_pretty(&File { version: 1, connections: connections.to_vec() }).map_err(io::Error::other)?;
        let temp = self.path.with_extension("json.tmp");
        let mut file = crate::state_dir::private_file().write(true).create(true).truncate(true).open(&temp)?;
        file.write_all(&body)?;
        file.sync_all()?;
        fs::rename(&temp, &self.path)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn pg(name: &str) -> ConnectionInput {
        ConnectionInput {
            name: name.to_owned(),
            engine: Engine::Postgres,
            host: "localhost".into(),
            port: None,
            user: "dev".into(),
            database: "app".into(),
            ssl: SslMode::Prefer,
            path: None,
            ssh: None,
            color: Color::Gray,
            read_only: true,
        }
    }

    pub(crate) fn sqlite(name: &str, path: &str) -> ConnectionInput {
        ConnectionInput {
            name: name.to_owned(),
            engine: Engine::Sqlite,
            host: String::new(),
            port: None,
            user: String::new(),
            database: String::new(),
            ssl: SslMode::Prefer,
            path: Some(path.to_owned()),
            ssh: None,
            color: Color::Gray,
            read_only: true,
        }
    }

    #[test]
    fn connections_round_trip_without_passwords() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::load(dir.path());
        let made = store.create(pg("local")).unwrap();
        assert_eq!(made.input.port, Some(5432), "the default port is filled in");
        let again = Store::load(dir.path());
        assert_eq!(again.list(), vec![made.clone()]);
        assert_eq!(again.get(&made.id), Some(made));
        let text = fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert!(!text.contains("password"));
    }

    #[test]
    fn a_broken_file_is_moved_aside_and_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(FILE), "{").unwrap();
        let store = Store::load(dir.path());
        assert!(store.list().is_empty());
        assert_eq!(fs::read_to_string(dir.path().join(format!("{FILE}.bak"))).unwrap(), "{");
        store.create(pg("x")).unwrap();
        assert_eq!(fs::read_to_string(dir.path().join(format!("{FILE}.bak"))).unwrap(), "{", "the backup stays");
    }

    #[test]
    fn ids_are_16_lowercase_hex_and_names_are_unique() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::load(dir.path());
        let a = store.create(pg("a")).unwrap();
        assert!(is_connection_id(&a.id), "{}", a.id);
        assert!(matches!(store.create(pg("a")), Err(StoreError::NameUsed)));
        let b = store.create(pg("b")).unwrap();
        assert_ne!(a.id, b.id);
        assert!(matches!(store.update(&b.id, pg("a")), Err(StoreError::NameUsed)));
        assert_eq!(store.update(&a.id, pg("a")).unwrap().id, a.id, "keeping its own name is fine");
        assert!(matches!(store.update("0000000000000000", pg("z")), Err(StoreError::NotFound)));
    }

    #[test]
    fn control_characters_and_bad_paths_are_refused() {
        let bad = [
            ConnectionInput { host: "a\nb".into(), ..pg("x") },
            ConnectionInput { host: "a b".into(), ..pg("x") },
            ConnectionInput { name: String::new(), ..pg("x") },
            ConnectionInput { name: "n".repeat(65), ..pg("x") },
            ConnectionInput { user: "u\u{0}".into(), ..pg("x") },
            ConnectionInput { database: "d".repeat(129), ..pg("x") },
            ConnectionInput { ssh: Some("-oProxyCommand=x".into()), ..pg("x") },
            ConnectionInput { host: String::new(), ..pg("x") },
            sqlite("x", "rel/x.db"),
            sqlite("x", "/a\u{7}b.db"),
            ConnectionInput { path: None, ..sqlite("x", "/x.db") },
        ];
        for input in bad {
            assert!(matches!(validate(input.clone()), Err(StoreError::Invalid(_))), "{input:?}");
        }
    }

    #[test]
    fn validation_clears_what_the_engine_does_not_use() {
        let tidy = validate(ConnectionInput { host: "h".into(), ssh: Some("bastion".into()), port: Some(1), ..sqlite("x", "~/a.db") }).unwrap();
        assert_eq!((tidy.host.as_str(), tidy.port, tidy.ssh), ("", None, None));
        let tidy = validate(ConnectionInput { path: Some("/x.db".into()), ..pg("x") }).unwrap();
        assert_eq!(tidy.path, None);
        assert_eq!(validate(ConnectionInput { engine: Engine::Mysql, ..pg("x") }).unwrap().port, Some(3306));
        assert_eq!(validate(ConnectionInput { ssh: Some("me@bastion".into()), ..pg("x") }).unwrap().ssh.as_deref(), Some("me@bastion"));
    }

    #[test]
    fn removing_and_clearing_forget_connections() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::load(dir.path());
        let a = store.create(pg("a")).unwrap();
        let b = store.create(pg("b")).unwrap();
        assert!(store.remove(&a.id).unwrap());
        assert!(!store.remove(&a.id).unwrap());
        assert_eq!(store.clear().unwrap(), vec![b.id]);
        assert!(Store::load(dir.path()).list().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        Store::load(dir.path()).create(pg("a")).unwrap();
        let mode = fs::metadata(dir.path().join(FILE)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
