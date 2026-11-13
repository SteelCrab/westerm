//! Consoles: the `psql`, `mysql`, or `sqlite3` line a new shell types, and
//! the secret files that keep the password off that line. A secret file is
//! private (0600), one per connection, and removed when the server starts,
//! when its connection is deleted or its password changes, and on uninstall.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::super::login_env::LoginEnv;
use super::driver::Endpoint;
use super::store::{Connection, Engine};

/// The folder of secret files, in the state folder.
const SECRET_DIR: &str = "db-secrets";

/// Which console programs were found on the login shell's PATH.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Clients {
    pub psql: bool,
    /// "mysql", else "mariadb", else none.
    pub mysql: Option<&'static str>,
    pub sqlite3: bool,
    /// Whether a console pane can open at all (a shell westerm can type into).
    pub consoles: bool,
}

/// The console programs on the PATH of `env`.
pub fn find_clients(env: &LoginEnv, consoles: bool) -> Clients {
    let found = |name: &str| find_program(env, name).is_some();
    Clients {
        psql: found("psql"),
        mysql: ["mysql", "mariadb"].into_iter().find(|name| found(name)),
        sqlite3: found("sqlite3"),
        consoles,
    }
}

pub use super::super::login_env::find_program;

/// One line of a PostgreSQL password file: `host:port:db:user:password`,
/// with `\` and `:` escaped.
pub fn pgpass_line(at: &Endpoint, db: &str, user: &str, password: &str) -> String {
    let escape = |text: &str| text.replace('\\', "\\\\").replace(':', "\\:");
    format!("{}:{}:{}:{}:{}", escape(&at.host), at.port, escape(db), escape(user), escape(password))
}

/// A value of a MySQL option file, in double quotes.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A MySQL option file for the console: where to connect, as whom, and
/// (for a read-only connection) the statement each session starts with.
pub fn mycnf(at: &Endpoint, user: &str, password: Option<&str>, read_only: bool) -> String {
    let mut text = format!("[client]\nhost={}\nport={}\nuser={}\n", quoted(&at.host), at.port, quoted(user));
    if let Some(password) = password {
        text.push_str(&format!("password={}\n", quoted(password)));
    }
    if read_only {
        text.push_str("init-command=\"SET SESSION TRANSACTION READ ONLY\"\n");
    }
    text
}

/// A libpq connection value in single quotes.
fn conninfo_value(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The program and arguments of a console for `db` of the connection.
/// `secret` is the password or option file; `mysql` the client program.
pub fn argv(connection: &Connection, db: &str, at: &Endpoint, secret: Option<&Path>, mysql: &str) -> Vec<String> {
    let input = &connection.input;
    let db = if db.is_empty() { input.database.as_str() } else { db };
    match input.engine {
        Engine::Postgres => {
            let ssl = serde_json::to_value(input.ssl).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default();
            let mut parts = vec![
                format!("host={}", conninfo_value(&at.host)),
                format!("port={}", conninfo_value(&at.port.to_string())),
                format!("user={}", conninfo_value(&input.user)),
            ];
            if !db.is_empty() {
                parts.push(format!("dbname={}", conninfo_value(db)));
            }
            parts.push(format!("sslmode={}", conninfo_value(&ssl)));
            if let Some(secret) = secret {
                parts.push(format!("passfile={}", conninfo_value(&secret.display().to_string())));
            }
            if input.read_only {
                parts.push("options='-c default_transaction_read_only=on'".to_owned());
            }
            vec!["psql".to_owned(), parts.join(" ")]
        }
        Engine::Mysql => {
            let mut words = vec![mysql.to_owned()];
            if let Some(secret) = secret {
                words.push(format!("--defaults-extra-file={}", secret.display()));
            }
            // `--database=` keeps a name that starts with `-` from reading as an option.
            if !db.is_empty() {
                words.push(format!("--database={db}"));
            }
            words
        }
        Engine::Sqlite => {
            let path = input.path.as_deref().unwrap_or_default();
            let path = match path.strip_prefix("~/") {
                Some(rest) => crate::state_dir::home_dir().join(rest).display().to_string(),
                None => path.to_owned(),
            };
            let mut words = vec!["sqlite3".to_owned()];
            if input.read_only {
                words.push("-readonly".to_owned());
            }
            words.push(path);
            words
        }
    }
}

/// The private files that hold console passwords.
pub struct SecretFiles {
    dir: PathBuf,
}

impl SecretFiles {
    pub fn new(state_dir: &Path) -> Self {
        Self { dir: state_dir.join(SECRET_DIR) }
    }

    /// Writes `<id>.<ext>`, readable by the user only. An older file is replaced.
    pub fn write(&self, id: &str, ext: &str, text: &str) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!("{id}.{ext}"));
        let _ = fs::remove_file(&path);
        let mut file = crate::state_dir::private_file().write(true).create_new(true).open(&path)?;
        file.write_all(text.as_bytes())?;
        Ok(path)
    }

    /// Removes the files of one connection.
    pub fn remove(&self, id: &str) {
        for ext in ["pgpass", "cnf"] {
            let _ = fs::remove_file(self.dir.join(format!("{id}.{ext}")));
        }
    }

    /// Removes every secret file.
    pub fn clear(&self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::super::store::tests::{pg, sqlite};
    use super::super::store::ConnectionInput;
    use super::*;

    fn at(host: &str, port: u16) -> Endpoint {
        Endpoint { host: host.into(), port }
    }

    fn connection(input: ConnectionInput) -> Connection {
        Connection { id: "0123456789abcdef".into(), input: super::super::store::validate(input).unwrap() }
    }

    #[test]
    fn console_argv_never_holds_the_password() {
        let secret = Path::new("/state/db-secrets/0123456789abcdef.pgpass");
        let words = argv(&connection(pg("p")), "app", &at("localhost", 5432), Some(secret), "mysql");
        assert_eq!(words[0], "psql");
        assert!(words.iter().all(|word| !word.contains("s3cr")));
        assert!(words[1].contains("passfile='/state/db-secrets/0123456789abcdef.pgpass'"), "{}", words[1]);
        let my = argv(&connection(ConnectionInput { engine: Engine::Mysql, ..pg("m") }), "shop", &at("h", 3306), Some(Path::new("/s/x.cnf")), "mariadb");
        assert_eq!(my, vec!["mariadb".to_owned(), "--defaults-extra-file=/s/x.cnf".into(), "--database=shop".into()]);
        let no_db = argv(&connection(ConnectionInput { engine: Engine::Mysql, database: String::new(), ..pg("m") }), "", &at("h", 3306), Some(Path::new("/s/x.cnf")), "mysql");
        assert_eq!(no_db.len(), 2, "no database word when there is none");
    }

    #[test]
    fn a_mysql_database_name_is_never_read_as_an_option() {
        let words = argv(&connection(ConnectionInput { engine: Engine::Mysql, ..pg("m") }), "--init-command=SET SESSION TRANSACTION READ WRITE", &at("h", 3306), Some(Path::new("/s/x.cnf")), "mysql");
        assert_eq!(words.last().map(String::as_str), Some("--database=--init-command=SET SESSION TRANSACTION READ WRITE"));
        assert!(!words.iter().any(|word| word.starts_with("--init-command")), "{words:?}");
    }

    #[test]
    fn read_only_consoles_pass_the_read_only_switches() {
        let pg_words = argv(&connection(pg("p")), "app", &at("localhost", 5432), None, "mysql");
        assert!(pg_words[1].ends_with(" options='-c default_transaction_read_only=on'"), "{}", pg_words[1]);
        assert!(!pg_words[1].contains("passfile"), "no password, no passfile");
        let writable = argv(&connection(ConnectionInput { read_only: false, ..pg("p") }), "app", &at("localhost", 5432), None, "mysql");
        assert!(!writable[1].contains("default_transaction_read_only"));
        let lite = argv(&connection(sqlite("s", "/data/app.db")), "", &at("", 0), None, "mysql");
        assert_eq!(lite, vec!["sqlite3".to_owned(), "-readonly".into(), "/data/app.db".into()]);
        let lite_rw = argv(&connection(ConnectionInput { read_only: false, ..sqlite("s", "/data/app.db") }), "", &at("", 0), None, "mysql");
        assert_eq!(lite_rw, vec!["sqlite3".to_owned(), "/data/app.db".into()]);
        assert!(mycnf(&at("h", 3306), "u", None, true).contains("init-command=\"SET SESSION TRANSACTION READ ONLY\""));
        assert!(!mycnf(&at("h", 3306), "u", None, false).contains("init-command"));
    }

    #[test]
    fn sqlite_paths_under_home_are_spelled_out() {
        let lite = argv(&connection(sqlite("s", "~/data/app.db")), "", &at("", 0), None, "mysql");
        assert_eq!(lite[2], crate::state_dir::home_dir().join("data/app.db").display().to_string());
    }

    #[test]
    fn pgpass_and_mycnf_escape_their_values() {
        assert_eq!(pgpass_line(&at("h", 5432), "d", "u", r"a:b\c"), r"h:5432:d:u:a\:b\\c");
        let cnf = mycnf(&at("h", 3306), "u", Some(r#"p"w\"#), false);
        assert!(cnf.starts_with("[client]\n"), "{cnf}");
        assert!(cnf.contains(r#"password="p\"w\\""#), "{cnf}");
        assert!(cnf.contains("host=\"h\"\nport=3306\nuser=\"u\"\n"), "{cnf}");
        assert!(!mycnf(&at("h", 3306), "u", None, false).contains("password"));
    }

    #[test]
    fn conninfo_values_with_spaces_and_quotes_are_quoted() {
        let odd = ConnectionInput { user: "it's me".into(), database: r"we\ird db".into(), ..pg("p") };
        let words = argv(&connection(odd), "", &at("db.local", 6543), None, "mysql");
        assert!(words[1].starts_with(r"host='db.local' port='6543' user='it\'s me' dbname='we\\ird db' sslmode='prefer'"), "{}", words[1]);
    }

    #[cfg(unix)]
    #[test]
    fn secret_files_are_private_and_cleared() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let files = SecretFiles::new(dir.path());
        let path = files.write("0123456789abcdef", "pgpass", "h:1:d:u:p\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "h:1:d:u:p\n");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let other = files.write("fedcba9876543210", "cnf", "[client]\n").unwrap();
        files.write("0123456789abcdef", "pgpass", "h:1:d:u:q\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "h:1:d:u:q\n", "a new file replaces the old");
        files.remove("0123456789abcdef");
        assert!(!path.exists());
        assert!(other.exists());
        files.clear();
        assert!(!other.exists());
    }

    #[cfg(unix)]
    #[test]
    fn mariadb_is_used_when_mysql_is_missing() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        for program in ["mariadb", "psql"] {
            let path = dir.path().join(program);
            fs::write(&path, "#!/bin/sh\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::write(dir.path().join("sqlite3"), "not executable").unwrap();
        let env = LoginEnv { vars: vec![("PATH".into(), dir.path().display().to_string())] };
        assert_eq!(find_clients(&env, true), Clients { psql: true, mysql: Some("mariadb"), sqlite3: false, consoles: true });
        assert_eq!(find_clients(&LoginEnv::default(), false), Clients::default());
    }
}
