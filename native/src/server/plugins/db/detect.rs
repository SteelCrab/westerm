//! Connections westerm can suggest: `DATABASE_URL` in the `.env` files of the
//! session folders, SQLite files at the top of those folders, and running
//! PostgreSQL, MySQL, and MariaDB containers in Docker. Nothing is saved:
//! the panel opens a filled-in dialog.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use super::super::login_env::LoginEnv;
use super::store::{Color, ConnectionInput, Engine, SslMode, Store};

/// At most this many SQLite files per folder.
const MAX_FILES: usize = 20;
/// At most this many folders are looked at.
pub const MAX_FOLDERS: usize = 32;
/// The first bytes of every SQLite database file.
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Env,
    File,
    Docker,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub source: Source,
    /// Where it was found, for the panel.
    pub label: String,
    pub input: ConnectionInput,
    pub password: Option<String>,
}

/// `path` with the home folder written as `~`.
pub(crate) fn short_home(path: &Path) -> String {
    let home = crate::state_dir::home_dir();
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// The `DATABASE_URL` of a `.env` file: `export` and quotes allowed.
pub fn parse_env(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let line = line.strip_prefix("export ").map(str::trim_start).unwrap_or(line);
        let value = line.strip_prefix("DATABASE_URL")?.trim_start().strip_prefix('=')?.trim();
        let value = match value.chars().next() {
            Some(quote @ ('"' | '\'')) => value[1..].split(quote).next().unwrap_or_default(),
            _ => value.split(" #").next().unwrap_or_default().trim(),
        };
        (!value.is_empty()).then(|| value.to_owned())
    })
}

/// A suggestion's fields: read-only, gray, its default name.
fn suggestion(engine: Engine) -> ConnectionInput {
    ConnectionInput {
        name: String::new(),
        engine,
        host: String::new(),
        port: None,
        user: String::new(),
        database: String::new(),
        ssl: SslMode::Prefer,
        path: None,
        ssh: None,
        color: Color::Gray,
        read_only: true,
    }
}

/// The name the dialog gives a connection: `<database>@<host>`, or the file's name.
fn default_name(input: &ConnectionInput) -> String {
    match input.engine {
        Engine::Sqlite => input.path.as_deref().and_then(|path| Path::new(path).file_name()).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
        _ if input.database.is_empty() => input.host.clone(),
        _ => format!("{}@{}", input.database, input.host),
    }
}

fn decode(text: &str) -> String {
    url::form_urlencoded::parse(format!("x={}", text.replace('+', "%2B")).as_bytes()).next().map(|(_, value)| value.into_owned()).unwrap_or_default()
}

/// A connection from a database URL. A relative SQLite path is taken from `base`.
pub fn url_to_input(url: &str, base: &Path) -> Option<(ConnectionInput, Option<String>)> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if let Some(rest) = ["sqlite://", "sqlite:", "file:"].iter().find_map(|prefix| lower.starts_with(prefix).then(|| &url[prefix.len()..])) {
        let path = decode(rest.split('?').next().unwrap_or_default());
        if path.is_empty() {
            return None;
        }
        let path = if path.starts_with('/') || path.starts_with("~/") { PathBuf::from(path) } else { base.join(path.trim_start_matches("./")) };
        let mut input = suggestion(Engine::Sqlite);
        input.path = Some(path.display().to_string());
        input.name = default_name(&input);
        return Some((input, None));
    }
    let parsed = url::Url::parse(url).ok()?;
    let engine = match parsed.scheme() {
        "postgres" | "postgresql" => Engine::Postgres,
        "mysql" | "mariadb" => Engine::Mysql,
        _ => return None,
    };
    let mut input = suggestion(engine);
    input.host = parsed.host_str().filter(|host| !host.is_empty())?.trim_matches(['[', ']']).to_owned();
    input.port = parsed.port().or(engine.default_port());
    input.user = decode(parsed.username());
    input.database = decode(parsed.path().trim_start_matches('/'));
    if let Some((_, mode)) = parsed.query_pairs().find(|(key, _)| key == "sslmode") {
        input.ssl = match mode.as_ref() {
            "disable" => SslMode::Disable,
            "require" => SslMode::Require,
            "verify-full" => SslMode::VerifyFull,
            _ => SslMode::Prefer,
        };
    }
    input.name = default_name(&input);
    let password = parsed.password().map(decode).filter(|password| !password.is_empty());
    Some((input, password))
}

/// SQLite files at the top of `dir`: `*.db`, `*.sqlite`, `*.sqlite3` that start like one.
pub fn sqlite_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| matches!(ext, "db" | "sqlite" | "sqlite3")))
        .filter(|path| {
            use std::io::Read;
            let mut head = [0u8; 16];
            std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut head)).is_ok() && &head == SQLITE_HEADER
        })
        .collect();
    files.sort();
    files.truncate(MAX_FILES);
    files
}

/// The image's own name: no registry, no tag.
fn image_name(image: &str) -> &str {
    let last = image.rsplit('/').next().unwrap_or(image);
    last.split([':', '@']).next().unwrap_or(last)
}

/// Running database containers with a published port, from `docker inspect` output.
pub fn containers_from_inspect(json: &str) -> Vec<Found> {
    let Ok(serde_json::Value::Array(containers)) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    containers
        .iter()
        .filter_map(|container| {
            let image = container.pointer("/Config/Image")?.as_str()?;
            let (engine, port_key) = match image_name(image) {
                "postgres" => (Engine::Postgres, "5432/tcp"),
                "mysql" | "mariadb" => (Engine::Mysql, "3306/tcp"),
                _ => return None,
            };
            let host_port: u16 = container
                .pointer("/NetworkSettings/Ports")?
                .get(port_key)?
                .as_array()?
                .iter()
                .find_map(|binding| binding.get("HostPort")?.as_str()?.parse().ok())?;
            let env: Vec<(&str, &str)> = container
                .pointer("/Config/Env")
                .and_then(|env| env.as_array())
                .map(|list| list.iter().filter_map(|item| item.as_str()?.split_once('=')).collect())
                .unwrap_or_default();
            let var = |key: &str| env.iter().find(|(name, _)| *name == key).map(|(_, value)| (*value).to_owned());
            let mut input = suggestion(engine);
            input.host = "localhost".into();
            input.port = Some(host_port);
            let password = match engine {
                Engine::Postgres => {
                    input.user = var("POSTGRES_USER").unwrap_or_else(|| "postgres".into());
                    input.database = var("POSTGRES_DB").unwrap_or_else(|| input.user.clone());
                    var("POSTGRES_PASSWORD")
                }
                _ => {
                    input.database = var("MYSQL_DATABASE").unwrap_or_default();
                    match var("MYSQL_USER") {
                        Some(user) => {
                            input.user = user;
                            var("MYSQL_PASSWORD")
                        }
                        None => {
                            input.user = "root".into();
                            var("MYSQL_ROOT_PASSWORD")
                        }
                    }
                }
            };
            input.name = default_name(&input);
            let name = container.get("Name").and_then(|name| name.as_str()).unwrap_or_default().trim_start_matches('/');
            let tag = image.rsplit('/').next().unwrap_or(image);
            Some(Found { source: Source::Docker, label: format!("Docker · {name} ({tag})"), input, password: password.filter(|password| !password.is_empty()) })
        })
        .collect()
}

/// Runs docker with `args` within `timeout`; its stdout when it worked.
async fn docker(program: &Path, env: &LoginEnv, args: &[&str], timeout: Duration) -> Option<String> {
    let mut command = tokio::process::Command::new(program);
    command.args(args).env_clear().envs(env.vars.iter().map(|(key, value)| (key, value))).kill_on_drop(true).stdin(std::process::Stdio::null());
    let output = tokio::time::timeout(timeout, command.output()).await.ok()?.ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Database containers of the Docker on the login PATH; none when there is no docker.
pub async fn docker_found(env: &LoginEnv, timeout: Duration) -> Vec<Found> {
    let Some(program) = super::console::find_program(env, "docker") else { return Vec::new() };
    let deadline = tokio::time::Instant::now() + timeout;
    let Some(ids) = docker(&program, env, &["ps", "-q"], timeout).await else { return Vec::new() };
    let ids: Vec<&str> = ids.split_whitespace().take(200).collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let left = deadline.saturating_duration_since(tokio::time::Instant::now());
    let args: Vec<&str> = ["inspect"].into_iter().chain(ids).collect();
    docker(&program, env, &args, left).await.map(|json| containers_from_inspect(&json)).unwrap_or_default()
}

/// Whether two connections point at the same database.
fn same_place(a: &ConnectionInput, b: &ConnectionInput) -> bool {
    let expand = |path: &Option<String>| path.as_deref().map(|path| match path.strip_prefix("~/") {
        Some(rest) => crate::state_dir::home_dir().join(rest),
        None => PathBuf::from(path),
    });
    a.engine == b.engine
        && match a.engine {
            Engine::Sqlite => expand(&a.path) == expand(&b.path),
            _ => a.host == b.host && a.port.or(a.engine.default_port()) == b.port.or(b.engine.default_port()) && a.database == b.database,
        }
}

/// Everything found in `folders` and Docker, without what `store` already has.
pub async fn detect(folders: &[PathBuf], store: &Store, env: &LoginEnv, timeout: Duration) -> Vec<Found> {
    let docker_task = docker_found(env, timeout);
    let mut found = Vec::new();
    for folder in folders.iter().filter(|folder| folder.is_absolute() && folder.is_dir()).take(MAX_FOLDERS) {
        for file in [".env", ".env.local"] {
            let Some(url) = std::fs::read_to_string(folder.join(file)).ok().and_then(|text| parse_env(&text)) else { continue };
            if let Some((input, password)) = url_to_input(&url, folder) {
                found.push(Found { source: Source::Env, label: format!("{file} DATABASE_URL · {}", short_home(folder)), input, password });
            }
        }
        for path in sqlite_files(folder) {
            let (input, _) = url_to_input(&format!("sqlite:{}", path.display()), folder).expect("a path makes a SQLite input");
            found.push(Found { source: Source::File, label: short_home(&path), input, password: None });
        }
    }
    found.extend(docker_task.await);
    let saved = store.list();
    let mut kept: Vec<Found> = Vec::new();
    for item in found {
        let known = saved.iter().any(|connection| same_place(&connection.input, &item.input)) || kept.iter().any(|other| same_place(&other.input, &item.input));
        if !known {
            kept.push(item);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn env_files_give_their_database_url() {
        assert_eq!(parse_env("# c\nexport DATABASE_URL=\"postgres://u:p@h/db\"\n"), Some("postgres://u:p@h/db".into()));
        assert_eq!(parse_env("OTHER=1\nDATABASE_URL='mysql://h/shop' # the shop\n"), Some("mysql://h/shop".into()));
        assert_eq!(parse_env("DATABASE_URL=postgres://h/db # comment\n"), Some("postgres://h/db".into()));
        assert_eq!(parse_env("DATABASE_URL_X=postgres://h/db"), None);
        assert_eq!(parse_env("# DATABASE_URL=postgres://h/db"), None);
        assert_eq!(parse_env("DATABASE_URL="), None);
    }

    #[test]
    fn urls_become_connections() {
        let base = Path::new("/work/site");
        let (pg, password) = url_to_input("postgresql://dev:p%40ss@db.local:6543/app?sslmode=require", base).unwrap();
        assert_eq!((pg.engine, pg.host.as_str(), pg.port, pg.user.as_str(), pg.database.as_str(), pg.ssl), (Engine::Postgres, "db.local", Some(6543), "dev", "app", SslMode::Require));
        assert_eq!(password.as_deref(), Some("p@ss"));
        assert_eq!(pg.name, "app@db.local");
        assert!(pg.read_only, "suggestions start read-only");
        let (my, none) = url_to_input("mariadb://root@127.0.0.1/shop", base).unwrap();
        assert_eq!((my.engine, my.port, none), (Engine::Mysql, Some(3306), None));
        let (lite, _) = url_to_input("sqlite:./data/dev.db", base).unwrap();
        // Compared as paths: Windows joins with `\`, so the text differs there.
        assert_eq!((lite.engine, lite.path.as_deref().map(Path::new), lite.name.as_str()), (Engine::Sqlite, Some(base.join("data").join("dev.db").as_path()), "dev.db"));
        let (prisma, _) = url_to_input("file:./dev.db", base).unwrap();
        assert_eq!(prisma.path.as_deref().map(Path::new), Some(base.join("dev.db").as_path()));
        let (absolute, _) = url_to_input("sqlite:///var/app.db", base).unwrap();
        assert_eq!(absolute.path.as_deref(), Some("/var/app.db"));
        assert!(url_to_input("redis://h", base).is_none());
        assert!(url_to_input("not a url", base).is_none());
    }

    #[test]
    fn only_real_sqlite_files_are_found() {
        let dir = tempfile::tempdir().unwrap();
        let header = |name: &str| fs::write(dir.path().join(name), [SQLITE_HEADER.as_slice(), b"rest"].concat()).unwrap();
        header("a.db");
        header("c.sqlite3");
        header("notes.txt");
        fs::write(dir.path().join("b.db"), "not sqlite").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/d.db"), [SQLITE_HEADER.as_slice(), b"x"].concat()).unwrap();
        let names: Vec<String> = sqlite_files(dir.path()).iter().map(|path| path.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["a.db", "c.sqlite3"]);
    }

    const INSPECT: &str = r#"[
      {"Name": "/site-db-1", "State": {"Running": true},
       "Config": {"Image": "docker.io/library/postgres:16", "Env": ["POSTGRES_PASSWORD=pw", "POSTGRES_DB=app", "PATH=/usr/bin"]},
       "NetworkSettings": {"Ports": {"5432/tcp": [{"HostIp": "0.0.0.0", "HostPort": "55432"}, {"HostIp": "::", "HostPort": "55432"}]}}},
      {"Name": "/shop-db", "State": {"Running": true},
       "Config": {"Image": "mysql:8", "Env": ["MYSQL_USER=shop", "MYSQL_PASSWORD=s3", "MYSQL_DATABASE=shop"]},
       "NetworkSettings": {"Ports": {"3306/tcp": [{"HostIp": "127.0.0.1", "HostPort": "3307"}]}}},
      {"Name": "/root-db", "State": {"Running": true},
       "Config": {"Image": "mariadb:11", "Env": ["MYSQL_ROOT_PASSWORD=r"]},
       "NetworkSettings": {"Ports": {"3306/tcp": [{"HostIp": "0.0.0.0", "HostPort": "3306"}]}}},
      {"Name": "/hidden", "State": {"Running": true},
       "Config": {"Image": "postgres:15", "Env": []},
       "NetworkSettings": {"Ports": {"5432/tcp": null}}},
      {"Name": "/web", "State": {"Running": true},
       "Config": {"Image": "nginx", "Env": []},
       "NetworkSettings": {"Ports": {"80/tcp": [{"HostIp": "0.0.0.0", "HostPort": "8080"}]}}}
    ]"#;

    #[test]
    fn docker_postgres_containers_fill_user_password_and_db() {
        let found = containers_from_inspect(INSPECT);
        assert_eq!(found.len(), 3, "{found:?}");
        let pg = &found[0];
        assert_eq!(pg.source, Source::Docker);
        assert_eq!(pg.label, "Docker · site-db-1 (postgres:16)");
        assert_eq!((pg.input.engine, pg.input.host.as_str(), pg.input.port, pg.input.user.as_str(), pg.input.database.as_str()), (Engine::Postgres, "localhost", Some(55432), "postgres", "app"));
        assert_eq!(pg.password.as_deref(), Some("pw"));
        let my = &found[1];
        assert_eq!((my.input.engine, my.input.port, my.input.user.as_str(), my.input.database.as_str(), my.password.as_deref()), (Engine::Mysql, Some(3307), "shop", "shop", Some("s3")));
        let root = &found[2];
        assert_eq!((root.input.user.as_str(), root.password.as_deref(), root.input.database.as_str()), ("root", Some("r"), ""));
    }

    #[cfg(unix)]
    fn fake_docker(dir: &Path, body: &str) -> LoginEnv {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("docker");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        LoginEnv { vars: vec![("PATH".into(), format!("{}:/bin:/usr/bin", dir.display()))] }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn registered_connections_are_not_found_again() {
        let dir = tempfile::tempdir().unwrap();
        let site = dir.path().join("site");
        fs::create_dir(&site).unwrap();
        fs::write(site.join(".env"), "DATABASE_URL=postgres://dev@localhost:5432/app\n").unwrap();
        fs::write(site.join(".env.local"), "DATABASE_URL=postgres://dev@localhost:5432/app\n").unwrap();
        fs::write(site.join("app.db"), [SQLITE_HEADER.as_slice(), b"x"].concat()).unwrap();
        let store = Store::load(dir.path());
        let env = LoginEnv::default();
        let found = detect(std::slice::from_ref(&site), &store, &env, Duration::from_secs(3)).await;
        assert_eq!(found.iter().map(|each| each.source).collect::<Vec<_>>(), [Source::Env, Source::File], "the same URL twice is one");
        assert_eq!(found[0].label, ".env DATABASE_URL · ~/site".replace("~/site", &short_home(&site)));
        store.create(found[0].input.clone()).unwrap();
        let again = detect(std::slice::from_ref(&site), &store, &env, Duration::from_secs(3)).await;
        assert_eq!(again.iter().map(|each| each.source).collect::<Vec<_>>(), [Source::File]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn docker_containers_are_read_through_the_cli() {
        let dir = tempfile::tempdir().unwrap();
        let inspect = dir.path().join("inspect.json");
        fs::write(&inspect, INSPECT).unwrap();
        let env = fake_docker(dir.path(), &format!("if [ \"$1\" = ps ]; then echo abc123; else cat '{}'; fi", inspect.display()));
        let found = docker_found(&env, Duration::from_secs(3)).await;
        assert_eq!(found.len(), 3);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_hung_docker_does_not_stall_detection() {
        let dir = tempfile::tempdir().unwrap();
        let site = dir.path().join("site");
        fs::create_dir(&site).unwrap();
        fs::write(site.join(".env"), "DATABASE_URL=mysql://h/shop\n").unwrap();
        let env = fake_docker(dir.path(), "sleep 10");
        let started = std::time::Instant::now();
        let found = detect(&[site], &Store::load(dir.path()), &env, Duration::from_millis(500)).await;
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
        assert_eq!(found.len(), 1, "the .env result still comes");
    }

    #[test]
    fn relative_or_missing_folders_are_skipped() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let found = runtime.block_on(detect(&[PathBuf::from("relative"), dir.path().join("gone")], &Store::load(dir.path()), &LoginEnv::default(), Duration::from_secs(1)));
        assert!(found.is_empty());
    }
}
