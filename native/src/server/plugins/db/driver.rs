//! Connections to the databases: pools per (connection, database), queries
//! with limits, and values shown as text. Read-only is enforced by the
//! database itself. Every user Run uses a connection of its own that is closed
//! afterwards, so nothing a Run changes in its session (a transaction,
//! `SET`, `USE`) reaches the next Run, the tree, or another pane.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use sqlx::mysql::MySqlPool;
use sqlx::postgres::PgPool;
use sqlx::sqlite::SqlitePool;
use tokio_util::sync::CancellationToken;

use super::store::{Connection, Engine};

/// Where a database listens: its own host and port, or a tunnel's local end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}

impl Endpoint {
    /// The connection's own host and port.
    pub fn direct(connection: &Connection) -> Self {
        let input = &connection.input;
        Self { host: input.host.clone(), port: input.port.or(input.engine.default_port()).unwrap_or(0) }
    }
}

/// How much a query may read and how long things may take.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub rows: usize,
    pub cell: usize,
    pub total: usize,
    pub query: Duration,
    pub connect: Duration,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        rows: 1000,
        cell: 65536,
        total: 8 * 1024 * 1024,
        query: Duration::from_secs(30),
        connect: Duration::from_secs(10),
    };
}

/// A pool of one engine. Cloning shares the pool.
#[derive(Clone, Debug)]
pub enum Pool {
    Pg(PgPool),
    My(MySqlPool),
    Lite(SqlitePool),
}

#[derive(Debug, PartialEq, Eq)]
pub enum DbError {
    /// What the database (or the network) said: its first line.
    Db(String),
    /// The database wants a password and none is known.
    NeedPassword,
    Timeout,
    Cancelled,
    /// The connection is no longer saved.
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Column {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct QueryResult {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Option<String>>>,
    /// Cells cut at the cell limit: (row, column).
    pub cut: Vec<(usize, usize)>,
    /// More rows were there than the limits let through.
    pub more: bool,
    /// Rows a statement without columns changed.
    pub changed: Option<u64>,
    pub ms: u64,
}

/// The first line of a database or network error.
fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_owned()
}

/// An sqlx error as the UI shows it, with the database's code when it gave one.
fn db_error(error: sqlx::Error) -> (Option<String>, String) {
    match &error {
        sqlx::Error::Database(db) => (db.code().map(|code| code.into_owned()), first_line(db.message())),
        _ => (None, first_line(&error.to_string())),
    }
}

/// Whether an error after connecting without a password means "ask for one":
/// PostgreSQL's 28P01/28000 and MySQL's 1045 (access denied).
pub fn needs_password(code: Option<&str>, had_password: bool) -> bool {
    !had_password && matches!(code, Some("28P01" | "28000" | "1045"))
}

/// `~/…` with the home folder in front.
fn expand_home(path: &str) -> std::path::PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => crate::state_dir::home_dir().join(rest),
        None => std::path::PathBuf::from(path),
    }
}

const POOL_SIZE: u32 = 4;

/// Opens a pool for `db` of the connection (an empty `db` is its default).
pub async fn connect(connection: &Connection, db: &str, password: Option<&str>, at: &Endpoint, limits: &Limits) -> Result<Pool, DbError> {
    use sqlx::pool::PoolOptions;
    let input = &connection.input;
    let db = if db.is_empty() { input.database.as_str() } else { db };
    let read_only = input.read_only;
    let attempt = async {
        match input.engine {
            Engine::Postgres => {
                use sqlx::postgres::{PgConnectOptions, PgSslMode};
                let mut options = PgConnectOptions::new_without_pgpass()
                    .host(&at.host)
                    .port(at.port)
                    .username(&input.user)
                    .password(password.unwrap_or(""))
                    .application_name("westerm")
                    .ssl_mode(match input.ssl {
                        super::store::SslMode::Prefer => PgSslMode::Prefer,
                        super::store::SslMode::Disable => PgSslMode::Disable,
                        super::store::SslMode::Require => PgSslMode::Require,
                        super::store::SslMode::VerifyFull => PgSslMode::VerifyFull,
                    });
                if !db.is_empty() {
                    options = options.database(db);
                }
                if read_only {
                    options = options.options([("default_transaction_read_only", "on")]);
                }
                PoolOptions::new().max_connections(POOL_SIZE).acquire_timeout(limits.connect).connect_with(options).await.map(Pool::Pg)
            }
            Engine::Mysql => {
                use sqlx::mysql::{MySqlConnectOptions, MySqlSslMode};
                use sqlx::Executor;
                let mut options = MySqlConnectOptions::new()
                    .host(&at.host)
                    .port(at.port)
                    .username(&input.user)
                    .password(password.unwrap_or(""))
                    .ssl_mode(match input.ssl {
                        super::store::SslMode::Prefer => MySqlSslMode::Preferred,
                        super::store::SslMode::Disable => MySqlSslMode::Disabled,
                        super::store::SslMode::Require => MySqlSslMode::Required,
                        super::store::SslMode::VerifyFull => MySqlSslMode::VerifyIdentity,
                    });
                if !db.is_empty() {
                    options = options.database(db);
                }
                PoolOptions::<sqlx::MySql>::new()
                    .max_connections(POOL_SIZE)
                    .acquire_timeout(limits.connect)
                    .after_connect(move |conn: &mut sqlx::mysql::MySqlConnection, _| {
                        Box::pin(async move {
                            if read_only {
                                conn.execute("SET SESSION TRANSACTION READ ONLY").await?;
                            }
                            Ok(())
                        })
                    })
                    .connect_with(options)
                    .await
                    .map(Pool::My)
            }
            Engine::Sqlite => {
                use sqlx::sqlite::SqliteConnectOptions;
                let path = expand_home(input.path.as_deref().unwrap_or_default());
                let options = SqliteConnectOptions::new().filename(path).read_only(read_only).create_if_missing(false);
                PoolOptions::new().max_connections(POOL_SIZE).acquire_timeout(limits.connect).connect_with(options).await.map(Pool::Lite)
            }
        }
    };
    match tokio::time::timeout(limits.connect, attempt).await {
        Ok(Ok(pool)) => Ok(pool),
        Ok(Err(error)) => {
            let (code, message) = db_error(error);
            if needs_password(code.as_deref(), password.is_some_and(|password| !password.is_empty())) {
                Err(DbError::NeedPassword)
            } else {
                Err(DbError::Db(message))
            }
        }
        Err(_) => Err(DbError::Db(format!("No answer from {}:{} within {} s", at.host, at.port, limits.connect.as_secs_f32()))),
    }
}

/// `name` as an identifier of the engine's SQL.
pub fn quote_ident(engine: Engine, name: &str) -> String {
    match engine {
        Engine::Mysql => format!("`{}`", name.replace('`', "``")),
        Engine::Postgres | Engine::Sqlite => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

/// Builds a result row by row within the limits.
struct Collector<'a> {
    limits: &'a Limits,
    result: QueryResult,
    bytes: usize,
}

impl<'a> Collector<'a> {
    fn new(limits: &'a Limits, columns: Vec<Column>) -> Self {
        Self { limits, result: QueryResult { columns, ..QueryResult::default() }, bytes: 0 }
    }

    /// Adds a row; false when the limits are reached (the row is left out and `more` is set).
    fn push(&mut self, cells: Vec<Option<String>>) -> bool {
        if self.result.rows.len() >= self.limits.rows {
            self.result.more = true;
            return false;
        }
        let row = self.result.rows.len();
        let mut cut = Vec::new();
        let cells: Vec<Option<String>> = cells
            .into_iter()
            .enumerate()
            .map(|(column, cell)| {
                cell.map(|text| {
                    if text.len() <= self.limits.cell {
                        return text;
                    }
                    cut.push((row, column));
                    let mut end = self.limits.cell;
                    while !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    text[..end].to_owned()
                })
            })
            .collect();
        let size: usize = cells.iter().map(|cell| cell.as_ref().map_or(0, String::len)).sum();
        if self.bytes + size > self.limits.total && !self.result.rows.is_empty() {
            self.result.more = true;
            return false;
        }
        self.bytes += size;
        self.result.cut.extend(cut);
        self.result.rows.push(cells);
        true
    }
}

/// Bytes as `\x` and hex, the first 32 only.
fn hex_preview(bytes: &[u8]) -> String {
    let shown: String = bytes.iter().take(32).map(|byte| format!("{byte:02x}")).collect();
    if bytes.len() > 32 { format!("\\x{shown}…") } else { format!("\\x{shown}") }
}

mod text {
    //! Values as text, per engine. A type westerm cannot decode shows its name.
    use super::hex_preview;
    use sqlx::{Column as _, Row, TypeInfo, ValueRef};

    pub fn sqlite(row: &sqlx::sqlite::SqliteRow, index: usize) -> Option<String> {
        let raw = row.try_get_raw(index).ok()?;
        if raw.is_null() {
            return None;
        }
        let kind = raw.type_info().name().to_owned();
        Some(match kind.as_str() {
            "INTEGER" => row.try_get_unchecked::<i64, _>(index).map(|v| v.to_string()).unwrap_or(kind),
            "REAL" => row.try_get_unchecked::<f64, _>(index).map(|v| v.to_string()).unwrap_or(kind),
            "BLOB" => row.try_get_unchecked::<Vec<u8>, _>(index).map(|v| hex_preview(&v)).unwrap_or(kind),
            _ => row.try_get_unchecked::<String, _>(index).unwrap_or(kind),
        })
    }

    /// Text that decoded from a value of an unlisted type, when it reads as
    /// text; else the type's name. Binary values (an interval, an address)
    /// often decode as "text" full of control bytes, which would show as if
    /// it were the value.
    pub fn shown_or_type(decoded: Option<String>, kind: &str) -> String {
        match decoded {
            Some(text) if !text.chars().any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r')) => text,
            _ => format!("<{kind}>"),
        }
    }

    /// Anything else: text when it decodes as readable text, else the type's name.
    fn fallback<R: Row>(row: &R, index: usize, kind: &str) -> String
    where
        for<'r> String: sqlx::Decode<'r, R::Database>,
        for<'r> Vec<u8>: sqlx::Decode<'r, R::Database>,
        usize: sqlx::ColumnIndex<R>,
    {
        shown_or_type(row.try_get_unchecked::<String, _>(index).ok(), kind)
    }

    pub fn postgres(row: &sqlx::postgres::PgRow, index: usize) -> Option<String> {
        use sqlx::types::chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
        let raw = row.try_get_raw(index).ok()?;
        if raw.is_null() {
            return None;
        }
        let kind = row.columns()[index].type_info().name().to_owned();
        let got = match kind.as_str() {
            "BOOL" => row.try_get::<bool, _>(index).map(|v| v.to_string()),
            "INT2" => row.try_get::<i16, _>(index).map(|v| v.to_string()),
            "INT4" => row.try_get::<i32, _>(index).map(|v| v.to_string()),
            "INT8" => row.try_get::<i64, _>(index).map(|v| v.to_string()),
            "OID" => row.try_get::<sqlx::postgres::types::Oid, _>(index).map(|v| v.0.to_string()),
            "FLOAT4" => row.try_get::<f32, _>(index).map(|v| v.to_string()),
            "FLOAT8" => row.try_get::<f64, _>(index).map(|v| v.to_string()),
            "NUMERIC" => row.try_get::<sqlx::types::Decimal, _>(index).map(|v| v.to_string()),
            "UUID" => row.try_get::<sqlx::types::Uuid, _>(index).map(|v| v.to_string()),
            "JSON" | "JSONB" => row.try_get::<sqlx::types::JsonValue, _>(index).map(|v| v.to_string()),
            "TIMESTAMP" => row.try_get::<NaiveDateTime, _>(index).map(|v| v.to_string()),
            "TIMESTAMPTZ" => row.try_get::<DateTime<Utc>, _>(index).map(|v| v.to_string()),
            "DATE" => row.try_get::<NaiveDate, _>(index).map(|v| v.to_string()),
            "TIME" => row.try_get::<NaiveTime, _>(index).map(|v| v.to_string()),
            "BYTEA" => row.try_get::<Vec<u8>, _>(index).map(|v| hex_preview(&v)),
            "TEXT[]" | "VARCHAR[]" | "NAME[]" => row.try_get::<Vec<String>, _>(index).map(|v| format!("{{{}}}", v.join(","))),
            "INT4[]" => row.try_get::<Vec<i32>, _>(index).map(|v| format!("{v:?}")),
            "INT8[]" => row.try_get::<Vec<i64>, _>(index).map(|v| format!("{v:?}")),
            _ => return Some(fallback(row, index, &kind)),
        };
        Some(got.unwrap_or_else(|_| fallback(row, index, &kind)))
    }

    pub fn mysql(row: &sqlx::mysql::MySqlRow, index: usize) -> Option<String> {
        use sqlx::types::chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
        let raw = row.try_get_raw(index).ok()?;
        if raw.is_null() {
            return None;
        }
        let kind = row.columns()[index].type_info().name().to_owned();
        let got = if kind.contains("UNSIGNED") {
            row.try_get_unchecked::<u64, _>(index).map(|v| v.to_string())
        } else {
            match kind.as_str() {
                "BOOLEAN" | "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" | "YEAR" => {
                    row.try_get_unchecked::<i64, _>(index).map(|v| v.to_string())
                }
                "FLOAT" => row.try_get::<f32, _>(index).map(|v| v.to_string()),
                "DOUBLE" => row.try_get::<f64, _>(index).map(|v| v.to_string()),
                "DECIMAL" => row.try_get::<sqlx::types::Decimal, _>(index).map(|v| v.to_string()),
                "JSON" => row.try_get::<sqlx::types::JsonValue, _>(index).map(|v| v.to_string()),
                "DATE" => row.try_get::<NaiveDate, _>(index).map(|v| v.to_string()),
                "DATETIME" => row.try_get::<NaiveDateTime, _>(index).map(|v| v.to_string()),
                "TIMESTAMP" => row.try_get::<DateTime<Utc>, _>(index).map(|v| v.to_string()),
                "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BINARY" | "VARBINARY" | "BIT" | "GEOMETRY" => {
                    row.try_get_unchecked::<Vec<u8>, _>(index).map(|v| hex_preview(&v))
                }
                _ => return Some(fallback(row, index, &kind)),
            }
        };
        Some(got.unwrap_or_else(|_| fallback(row, index, &kind)))
    }
}

/// Whether `sql` holds more than one statement: a `;` outside quotes and
/// comments with more SQL after it. SQLite runs a second statement ahead of
/// the reader, so it is refused before it starts (PostgreSQL and MySQL refuse
/// it when preparing).
pub fn several_statements(sql: &str) -> bool {
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut ended = false;
    while i < bytes.len() {
        let rest = &bytes[i..];
        match bytes[i] {
            b'-' if rest.starts_with(b"--") => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if rest.starts_with(b"/*") => {
                i += 2;
                while i < bytes.len() && !bytes[i..].starts_with(b"*/") {
                    i += 1;
                }
                i += 2;
                continue;
            }
            byte if byte.is_ascii_whitespace() => {}
            _ if ended => return true,
            b';' => ended = true,
            quote @ (b'\'' | b'"' | b'`' | b'[') => {
                let close = if quote == b'[' { b']' } else { quote };
                i += 1;
                while i < bytes.len() && bytes[i] != close {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Reads the statement's rows (or the count it changed) within the limits.
async fn collect<'c, DB, C>(
    conn: &'c mut C,
    sql: &'c str,
    limits: &'c Limits,
    cell: fn(&DB::Row, usize) -> Option<String>,
) -> Result<QueryResult, DbError>
where
    DB: sqlx::Database,
    for<'e> &'e mut C: sqlx::Executor<'e, Database = DB>,
    for<'q> <DB as sqlx::Database>::Arguments<'q>: sqlx::IntoArguments<'q, DB>,
    usize: sqlx::ColumnIndex<DB::Row>,
{
    use futures_util::TryStreamExt;
    use sqlx::{Column as _, Executor, Row, Statement, TypeInfo, ValueRef};
    let started = Instant::now();
    let statement = (&mut *conn).prepare(sql).await.map_err(|error| DbError::Db(db_error(error).1))?;
    let columns: Vec<Column> =
        statement.columns().iter().map(|column| Column { name: column.name().to_owned(), ty: column.type_info().name().to_owned() }).collect();
    let has_columns = !columns.is_empty();
    let mut collector = Collector::new(limits, columns);
    let mut changed = 0u64;
    let mut stream = (&mut *conn).fetch_many(statement.query());
    while let Some(item) = stream.try_next().await.map_err(|error| DbError::Db(db_error(error).1))? {
        match item {
            sqlx::Either::Left(done) => {
                changed += rows_affected::<DB>(&done);
                // Only the first statement runs: later ones are never polled.
                break;
            }
            sqlx::Either::Right(row) => {
                if collector.result.columns.is_empty() {
                    collector.result.columns = row
                        .columns()
                        .iter()
                        .map(|column| Column { name: column.name().to_owned(), ty: column.type_info().name().to_owned() })
                        .collect();
                }
                if collector.result.rows.is_empty() {
                    // A column without a declared type (an expression in SQLite)
                    // takes the type of its first value.
                    for (index, column) in collector.result.columns.iter_mut().enumerate() {
                        let untyped = column.ty.is_empty() || column.ty == "NULL";
                        if let Some(value) = row.try_get_raw(index).ok().filter(|value| untyped && !value.is_null()) {
                            column.ty = value.type_info().name().to_owned();
                        }
                    }
                }
                let cells = (0..row.len()).map(|index| cell(&row, index)).collect();
                if !collector.push(cells) {
                    break;
                }
            }
        }
    }
    drop(stream);
    let mut result = collector.result;
    if !has_columns && result.rows.is_empty() {
        result.changed = Some(changed);
    }
    result.ms = started.elapsed().as_millis() as u64;
    Ok(result)
}

/// Rows a finished statement changed, for every engine.
fn rows_affected<DB: sqlx::Database>(done: &DB::QueryResult) -> u64 {
    let any: &dyn std::any::Any = done;
    if let Some(done) = any.downcast_ref::<sqlx::postgres::PgQueryResult>() {
        return done.rows_affected();
    }
    if let Some(done) = any.downcast_ref::<sqlx::mysql::MySqlQueryResult>() {
        return done.rows_affected();
    }
    if let Some(done) = any.downcast_ref::<sqlx::sqlite::SqliteQueryResult>() {
        return done.rows_affected();
    }
    0
}

/// Stops a statement another connection runs.
enum Interrupt {
    Pg(PgPool, i32),
    My(MySqlPool, u64),
    Lite(Arc<std::sync::atomic::AtomicBool>),
}

impl Interrupt {
    fn fire(self) {
        match self {
            Self::Pg(pool, pid) => {
                tokio::spawn(async move {
                    let _ = sqlx::query("SELECT pg_cancel_backend($1)").bind(pid).execute(&pool).await;
                });
            }
            Self::My(pool, id) => {
                tokio::spawn(async move {
                    let _ = sqlx::query(&format!("KILL QUERY {id}")).execute(&pool).await;
                });
            }
            Self::Lite(stop) => stop.store(true, std::sync::atomic::Ordering::Relaxed),
        }
    }
}

/// Waits for `work`, a cancel, or the time limit; the last two stop the statement.
async fn race<F>(work: F, interrupt: Interrupt, limits: &Limits, cancel: CancellationToken) -> Result<QueryResult, DbError>
where
    F: std::future::Future<Output = Result<QueryResult, DbError>>,
{
    tokio::select! {
        result = work => result,
        () = cancel.cancelled() => {
            interrupt.fire();
            Err(DbError::Cancelled)
        }
        () = tokio::time::sleep(limits.query) => {
            interrupt.fire();
            Err(DbError::Timeout)
        }
    }
}

/// Runs one statement on a connection of its own, closed afterwards. A
/// PostgreSQL `schema` becomes the search path for this Run only.
pub async fn run(pool: &Pool, sql: &str, schema: Option<&str>, limits: &Limits, cancel: CancellationToken) -> Result<QueryResult, DbError> {
    use sqlx::Connection as _;
    let failed = |error: sqlx::Error| DbError::Db(db_error(error).1);
    match pool {
        Pool::Pg(pg) => {
            let mut conn = pg.acquire().await.map_err(failed)?.detach();
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut conn).await.map_err(failed)?;
            if let Some(schema) = schema.filter(|schema| !schema.is_empty()) {
                sqlx::query("SELECT set_config('search_path', $1, false)")
                    .bind(quote_ident(Engine::Postgres, schema))
                    .execute(&mut conn)
                    .await
                    .map_err(failed)?;
            }
            let result = race(collect(&mut conn, sql, limits, text::postgres), Interrupt::Pg(pg.clone(), pid), limits, cancel).await;
            if result.is_ok() {
                let _ = conn.close().await;
            }
            result
        }
        Pool::My(my) => {
            let mut conn = my.acquire().await.map_err(failed)?.detach();
            let id: u64 = sqlx::query_scalar("SELECT CONNECTION_ID()").fetch_one(&mut conn).await.map_err(failed)?;
            let result = race(collect(&mut conn, sql, limits, text::mysql), Interrupt::My(my.clone(), id), limits, cancel).await;
            if result.is_ok() {
                let _ = conn.close().await;
            }
            result
        }
        Pool::Lite(lite) => {
            if several_statements(sql) {
                return Err(DbError::Db("Run one statement at a time".to_owned()));
            }
            let mut conn = lite.acquire().await.map_err(failed)?.detach();
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let flag = Arc::clone(&stop);
            conn.lock_handle().await.map_err(failed)?.set_progress_handler(1000, move || !flag.load(std::sync::atomic::Ordering::Relaxed));
            let result = race(collect(&mut conn, sql, limits, text::sqlite), Interrupt::Lite(stop), limits, cancel).await;
            if result.is_ok() {
                let _ = conn.close().await;
            }
            result
        }
    }
}

/// The server's version text.
pub async fn version(pool: &Pool) -> Result<String, DbError> {
    let failed = |error: sqlx::Error| DbError::Db(db_error(error).1);
    match pool {
        Pool::Pg(pg) => sqlx::query_scalar::<_, String>("SHOW server_version").fetch_one(pg).await.map(|v| format!("PostgreSQL {v}")).map_err(failed),
        Pool::My(my) => sqlx::query_scalar::<_, String>("SELECT VERSION()").fetch_one(my).await.map(|v| format!("MySQL {v}")).map_err(failed),
        Pool::Lite(lite) => sqlx::query_scalar::<_, String>("SELECT sqlite_version()").fetch_one(lite).await.map(|v| format!("SQLite {v}")).map_err(failed),
    }
}

impl Pool {
    async fn close(&self) {
        match self {
            Self::Pg(pool) => pool.close().await,
            Self::My(pool) => pool.close().await,
            Self::Lite(pool) => pool.close().await,
        }
    }
}

/// The pools of every connection, and the queries running now.
#[derive(Default)]
pub struct Pools {
    inner: Arc<Mutex<PoolsInner>>,
}

#[derive(Default)]
struct PoolsInner {
    entries: HashMap<(String, String), Entry>,
    connecting: HashMap<String, usize>,
    failures: HashMap<String, String>,
    queries: HashMap<String, CancellationToken>,
    /// One SSH tunnel per connection that has `ssh` set.
    tunnels: HashMap<String, TunnelEntry>,
    /// Bumped when a connection's pools close (edit, delete, disconnect), so
    /// a connect that was already running with the old settings is not kept.
    generations: HashMap<String, u64>,
    /// Bumped when every pool closes.
    epoch: u64,
    sweeping: bool,
}

/// Which settings a connect started with; see `Pools::begin_connect`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Generation {
    epoch: u64,
    own: u64,
}

impl PoolsInner {
    fn generation(&self, id: &str) -> Generation {
        Generation { epoch: self.epoch, own: self.generations.get(id).copied().unwrap_or(0) }
    }

    fn bump(&mut self, id: &str) {
        *self.generations.entry(id.to_owned()).or_default() += 1;
    }

    fn done_connecting(&mut self, id: &str) {
        if let Some(count) = self.connecting.get_mut(id) {
            *count -= 1;
            if *count == 0 {
                self.connecting.remove(id);
            }
        }
    }
}

/// What a connect that lost the race to an edit or a delete answers.
const CHANGED: &str = "The connection changed while it was connecting. Try again.";

struct Entry {
    pool: Pool,
    used: Instant,
}

struct TunnelEntry {
    tunnel: Arc<super::tunnel::Tunnel>,
    /// A console used it: the idle sweep leaves it open (until Disconnect,
    /// Delete, uninstall, or the server stops).
    console: bool,
}

/// How often pools nobody uses are looked for, and how long they may sit.
const SWEEP_EVERY: Duration = Duration::from_secs(60);
const IDLE_CLOSE: Duration = Duration::from_secs(600);

impl Pools {
    fn lock(&self) -> std::sync::MutexGuard<'_, PoolsInner> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The pool for (connection, db), opened when there is none.
    pub async fn get_or_connect(&self, connection: &Connection, db: &str, password: Option<&str>, at: &Endpoint, limits: &Limits) -> Result<Pool, DbError> {
        {
            let mut inner = self.lock();
            if let Some(entry) = inner.entries.get_mut(&(connection.id.clone(), db.to_owned())) {
                entry.used = Instant::now();
                return Ok(entry.pool.clone());
            }
        }
        let generation = self.begin_connect(&connection.id);
        match connect(connection, db, password, at, limits).await {
            Ok(pool) => {
                if self.finish_connect(&connection.id, db, generation, pool.clone()).await {
                    Ok(pool)
                } else {
                    Err(DbError::Db(CHANGED.to_owned()))
                }
            }
            Err(error) => {
                let mut inner = self.lock();
                inner.done_connecting(&connection.id);
                match &error {
                    DbError::Db(message) if inner.generation(&connection.id) == generation => {
                        inner.failures.insert(connection.id.clone(), message.clone());
                    }
                    DbError::NeedPassword => {
                        inner.failures.remove(&connection.id);
                    }
                    _ => {}
                }
                Err(error)
            }
        }
    }

    /// A connect starts: the connection shows "connecting", and the settings
    /// it started with are remembered.
    pub fn begin_connect(&self, id: &str) -> Generation {
        let mut inner = self.lock();
        *inner.connecting.entry(id.to_owned()).or_default() += 1;
        inner.generation(id)
    }

    /// A connect ended with `pool`: kept when nothing closed the connection
    /// meanwhile; otherwise closed, and false.
    pub async fn finish_connect(&self, id: &str, db: &str, generation: Generation, pool: Pool) -> bool {
        let kept = {
            let mut inner = self.lock();
            inner.done_connecting(id);
            let current = inner.generation(id) == generation;
            if current {
                inner.failures.remove(id);
                inner.entries.insert((id.to_owned(), db.to_owned()), Entry { pool: pool.clone(), used: Instant::now() });
                if !inner.sweeping && tokio::runtime::Handle::try_current().is_ok() {
                    inner.sweeping = true;
                    let weak = Arc::downgrade(&self.inner);
                    tokio::spawn(async move {
                        loop {
                            tokio::time::sleep(SWEEP_EVERY).await;
                            let Some(inner) = weak.upgrade() else { break };
                            Pools { inner }.sweep(IDLE_CLOSE).await;
                        }
                    });
                }
            }
            current
        };
        if !kept {
            pool.close().await;
        }
        kept
    }

    /// Where the connection's database answers: its own host, or the local
    /// end of its SSH tunnel (opened when there is none, or when ssh ended).
    pub async fn endpoint(&self, connection: &Connection, ssh: Option<&std::path::Path>, ready: Duration) -> Result<Endpoint, DbError> {
        let Some(alias) = connection.input.ssh.as_deref() else {
            return Ok(Endpoint::direct(connection));
        };
        let dead = {
            let mut inner = self.lock();
            match inner.tunnels.get(&connection.id) {
                Some(entry) if entry.tunnel.alive() => return Ok(entry.tunnel.local()),
                Some(_) => inner.tunnels.remove(&connection.id).map(|entry| entry.tunnel),
                None => None,
            }
        };
        if let Some(dead) = dead {
            // Pools that went through the old tunnel point at a closed port.
            self.close_pools(&connection.id).await;
            dead.close().await;
        }
        let ssh = ssh.ok_or_else(|| DbError::Db("ssh not found on the login shell's PATH".to_owned()))?;
        let direct = Endpoint::direct(connection);
        let generation = self.lock().generation(&connection.id);
        match super::tunnel::Tunnel::open(ssh, alias, &direct.host, direct.port, ready).await {
            Ok(tunnel) => {
                let local = tunnel.local();
                if self.lock().generation(&connection.id) != generation {
                    // Edited or deleted while ssh started: this tunnel has old settings.
                    tunnel.close().await;
                    return Err(DbError::Db(CHANGED.to_owned()));
                }
                let replaced = self.lock().tunnels.insert(connection.id.clone(), TunnelEntry { tunnel: Arc::new(tunnel), console: false });
                if let Some(old) = replaced {
                    old.tunnel.close().await;
                }
                Ok(local)
            }
            Err(error) => {
                self.lock().failures.insert(connection.id.clone(), error.clone());
                Err(DbError::Db(error))
            }
        }
    }

    /// A console uses the connection's tunnel: keep it through the idle sweep.
    pub fn mark_console(&self, id: &str) {
        if let Some(entry) = self.lock().tunnels.get_mut(id) {
            entry.console = true;
        }
    }

    /// How many tunnels are open.
    #[cfg(test)]
    pub fn tunnel_count(&self) -> usize {
        self.lock().tunnels.len()
    }

    async fn close_pools(&self, id: &str) {
        let closing: Vec<Pool> = {
            let mut inner = self.lock();
            let keys: Vec<_> = inner.entries.keys().filter(|(owner, _)| owner == id).cloned().collect();
            keys.iter().filter_map(|key| inner.entries.remove(key)).map(|entry| entry.pool).collect()
        };
        for pool in closing {
            pool.close().await;
        }
    }

    /// Closes every pool and the tunnel of the connection.
    pub async fn disconnect(&self, id: &str) {
        {
            let mut inner = self.lock();
            inner.failures.remove(id);
            inner.bump(id);
        }
        self.close_pools(id).await;
        let tunnel = self.lock().tunnels.remove(id);
        if let Some(entry) = tunnel {
            entry.tunnel.close().await;
        }
    }

    /// Closes every pool and tunnel.
    pub async fn disconnect_all(&self) {
        let (pools, tunnels): (Vec<Pool>, Vec<TunnelEntry>) = {
            let mut inner = self.lock();
            inner.failures.clear();
            inner.epoch += 1;
            (inner.entries.drain().map(|(_, entry)| entry.pool).collect(), inner.tunnels.drain().map(|(_, entry)| entry).collect())
        };
        for pool in pools {
            pool.close().await;
        }
        for entry in tunnels {
            entry.tunnel.close().await;
        }
    }

    /// How many connections have a pool open.
    pub fn open_count(&self) -> usize {
        let inner = self.lock();
        inner.entries.keys().map(|(id, _)| id).collect::<std::collections::HashSet<_>>().len()
    }

    /// "open", "connecting", "failed" (with the error), or "idle".
    pub fn status(&self, id: &str) -> (&'static str, Option<String>) {
        let inner = self.lock();
        if inner.connecting.contains_key(id) {
            ("connecting", None)
        } else if inner.entries.keys().any(|(owner, _)| owner == id) {
            ("open", None)
        } else if let Some(error) = inner.failures.get(id) {
            ("failed", Some(error.clone()))
        } else {
            ("idle", None)
        }
    }

    /// Closes pools nobody used for `idle`, then tunnels left without a pool
    /// that no console uses.
    pub async fn sweep(&self, idle: Duration) {
        let (pools, tunnels): (Vec<Pool>, Vec<TunnelEntry>) = {
            let mut inner = self.lock();
            let stale: Vec<_> = inner.entries.iter().filter(|(_, entry)| entry.used.elapsed() >= idle).map(|(key, _)| key.clone()).collect();
            let pools = stale.iter().filter_map(|key| inner.entries.remove(key)).map(|entry| entry.pool).collect();
            let unused: Vec<String> = inner
                .tunnels
                .iter()
                .filter(|(id, entry)| !entry.console && !inner.entries.keys().any(|(owner, _)| owner == *id))
                .map(|(id, _)| id.clone())
                .collect();
            let tunnels = unused.iter().filter_map(|id| inner.tunnels.remove(id)).collect();
            (pools, tunnels)
        };
        for pool in pools {
            pool.close().await;
        }
        for entry in tunnels {
            entry.tunnel.close().await;
        }
    }

    /// A cancel switch for a query id.
    pub fn start_query(&self, query_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.lock().queries.insert(query_id.to_owned(), token.clone());
        token
    }

    pub fn finish_query(&self, query_id: &str) {
        self.lock().queries.remove(query_id);
    }

    /// Whether a running query had that id.
    pub fn cancel_query(&self, query_id: &str) -> bool {
        match self.lock().queries.get(query_id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::server::plugins::db::store::tests::sqlite;

    /// Small result limits and a short connect limit. The query limit is
    /// long: a first write on a slow CI disk (Windows) can take over 300 ms.
    /// A test of the query limit sets its own.
    pub(crate) fn small() -> Limits {
        Limits { rows: 1000, cell: 1000, total: 1 << 20, query: Duration::from_secs(10), connect: Duration::from_millis(300) }
    }

    pub(crate) fn file_connection(dir: &std::path::Path, read_only: bool) -> Connection {
        let path = dir.join("t.db").display().to_string();
        Connection { id: "0123456789abcdef".into(), input: super::super::store::ConnectionInput { read_only, ..sqlite("t", &path) } }
    }

    pub(crate) async fn pool(dir: &std::path::Path, read_only: bool) -> Pool {
        let connection = file_connection(dir, read_only);
        if !read_only {
            std::fs::OpenOptions::new().create(true).append(true).open(connection.input.path.as_deref().unwrap()).unwrap();
        }
        connect(&connection, "", None, &Endpoint::direct(&connection), &small()).await.unwrap()
    }

    pub(crate) async fn sql(pool: &Pool, text: &str) -> Result<QueryResult, DbError> {
        run(pool, text, None, &small(), CancellationToken::new()).await
    }

    const COUNT_TO: &str = "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?) SELECT i FROM n";

    #[tokio::test]
    async fn results_stop_at_the_row_limit() {
        let dir = tempfile::tempdir().unwrap();
        let pool = pool(dir.path(), false).await;
        let many = sql(&pool, &COUNT_TO.replace('?', "1500")).await.unwrap();
        assert_eq!(many.rows.len(), 1000);
        assert!(many.more);
        assert_eq!(many.columns, vec![Column { name: "i".into(), ty: "INTEGER".into() }]);
        let exact = sql(&pool, &COUNT_TO.replace('?', "1000")).await.unwrap();
        assert_eq!(exact.rows.len(), 1000);
        assert!(!exact.more);
        assert_eq!(exact.rows[0], vec![Some("1".to_owned())]);
    }

    #[tokio::test]
    async fn a_result_stops_at_the_byte_budget() {
        let dir = tempfile::tempdir().unwrap();
        let pool = pool(dir.path(), false).await;
        let limits = Limits { total: 4096, ..small() };
        let text = COUNT_TO.replace('?', "10").replace("SELECT i FROM n", "SELECT printf('%.*c', 1000, 'x') FROM n");
        let result = run(&pool, &text, None, &limits, CancellationToken::new()).await.unwrap();
        assert!(result.rows.len() <= 4, "{}", result.rows.len());
        assert!(result.more);
    }

    #[tokio::test]
    async fn read_only_connections_refuse_writes() {
        let dir = tempfile::tempdir().unwrap();
        let rw = pool(dir.path(), false).await;
        sql(&rw, "CREATE TABLE t (a)").await.unwrap();
        let ro = pool(dir.path(), true).await;
        let refused = sql(&ro, "INSERT INTO t VALUES (1)").await;
        assert!(matches!(&refused, Err(DbError::Db(message)) if message.contains("readonly")), "{refused:?}");
        assert_eq!(sql(&rw, "SELECT count(*) AS n FROM t").await.unwrap().rows, vec![vec![Some("0".to_owned())]]);
    }

    #[tokio::test]
    async fn nulls_blobs_and_long_cells_are_shown_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let pool = pool(dir.path(), false).await;
        let result = sql(&pool, "SELECT NULL, x'00ff', zeroblob(40), 'é' || printf('%.*c', 40000, 'a'), 1.5").await.unwrap();
        let row = &result.rows[0];
        assert_eq!(row[0], None);
        assert_eq!(row[1].as_deref(), Some("\\x00ff"));
        assert_eq!(row[2].as_deref(), Some(format!("\\x{}…", "00".repeat(32)).as_str()));
        let long = row[3].as_deref().unwrap();
        assert!(long.len() <= 1000 && long.starts_with("éa"), "{}", long.len());
        assert_eq!(row[4].as_deref(), Some("1.5"));
        assert_eq!(result.cut, vec![(0, 3)]);
    }

    #[tokio::test]
    async fn a_transaction_does_not_outlive_its_run() {
        let dir = tempfile::tempdir().unwrap();
        let pool = pool(dir.path(), false).await;
        sql(&pool, "CREATE TABLE t (a)").await.unwrap();
        sql(&pool, "BEGIN").await.unwrap();
        sql(&pool, "INSERT INTO t VALUES (1)").await.unwrap();
        assert!(sql(&pool, "ROLLBACK").await.is_err(), "no transaction is open any more");
        assert_eq!(sql(&pool, "SELECT count(*) FROM t").await.unwrap().rows, vec![vec![Some("1".to_owned())]]);
    }

    #[tokio::test]
    async fn a_slow_query_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let pool = pool(dir.path(), false).await;
        let started = Instant::now();
        let forever = "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n) SELECT count(*) FROM n";
        let limits = Limits { query: Duration::from_millis(300), ..small() };
        assert_eq!(run(&pool, forever, None, &limits, CancellationToken::new()).await, Err(DbError::Timeout));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn a_cancelled_query_stops() {
        let dir = tempfile::tempdir().unwrap();
        let pool = pool(dir.path(), false).await;
        let cancel = CancellationToken::new();
        let later = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            later.cancel();
        });
        let forever = "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n) SELECT count(*) FROM n";
        assert_eq!(run(&pool, forever, None, &small(), cancel).await, Err(DbError::Cancelled));
    }

    #[tokio::test]
    async fn writes_report_changed_rows() {
        let dir = tempfile::tempdir().unwrap();
        let pool = pool(dir.path(), false).await;
        sql(&pool, "CREATE TABLE t (a)").await.unwrap();
        sql(&pool, "INSERT INTO t VALUES (1), (2), (3)").await.unwrap();
        let result = sql(&pool, "UPDATE t SET a = 0").await.unwrap();
        assert_eq!(result.changed, Some(3));
        assert!(result.columns.is_empty());
    }

    #[tokio::test]
    async fn several_statements_are_refused_before_any_runs() {
        let dir = tempfile::tempdir().unwrap();
        let pool = pool(dir.path(), false).await;
        sql(&pool, "CREATE TABLE t (a)").await.unwrap();
        let result = sql(&pool, "SELECT 1 AS x; INSERT INTO t VALUES (9)").await;
        assert_eq!(result, Err(DbError::Db("Run one statement at a time".to_owned())));
        assert_eq!(sql(&pool, "SELECT count(*) FROM t").await.unwrap().rows, vec![vec![Some("0".to_owned())]]);
        assert!(sql(&pool, "SELECT 'a;b' AS x; -- done").await.is_ok(), "a quoted ; and a trailing comment are one statement");
    }

    #[test]
    fn statements_are_counted_outside_quotes_and_comments() {
        for one in ["SELECT 1", "SELECT 1;", "SELECT 1; ", "SELECT ';'", "SELECT \"a;b\"", "SELECT 1 -- ; x\n", "SELECT /* ; */ 1", "SELECT [a;b]", "SELECT 1; -- end"] {
            assert!(!several_statements(one), "{one}");
        }
        for many in ["SELECT 1; SELECT 2", "BEGIN; INSERT INTO t VALUES (1)", "SELECT 1;/* c */SELECT 2"] {
            assert!(several_statements(many), "{many}");
        }
    }

    #[tokio::test]
    async fn an_unresponsive_host_fails_within_the_connect_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        let connection = Connection { id: "0123456789abcdef".into(), input: super::super::store::tests::pg("silent") };
        let at = Endpoint { host: "127.0.0.1".into(), port };
        let started = Instant::now();
        let result = connect(&connection, "", Some("pw"), &at, &small()).await;
        assert!(matches!(result, Err(DbError::Db(_))), "{result:?}");
        assert!(started.elapsed() < Duration::from_millis(1500), "{:?}", started.elapsed());
    }

    #[test]
    fn values_westerm_cannot_decode_show_their_type_not_bytes() {
        assert_eq!(text::shown_or_type(Some("hello\tworld\n".into()), "CITEXT"), "hello\tworld\n");
        assert_eq!(text::shown_or_type(Some("\u{0}\u{0}\u{1}\u{2c}".into()), "INTERVAL"), "<INTERVAL>");
        assert_eq!(text::shown_or_type(Some("\u{7}x".into()), "INET"), "<INET>");
        assert_eq!(text::shown_or_type(None, "MONEY"), "<MONEY>");
    }

    #[test]
    fn identifiers_are_quoted_per_engine() {
        assert_eq!(quote_ident(Engine::Postgres, r#"we"ird"#), r#""we""ird""#);
        assert_eq!(quote_ident(Engine::Sqlite, "a b"), r#""a b""#);
        assert_eq!(quote_ident(Engine::Mysql, "a`b"), "`a``b`");
    }

    #[test]
    fn auth_failures_without_a_password_ask_for_one() {
        assert!(needs_password(Some("28P01"), false));
        assert!(needs_password(Some("28000"), false));
        assert!(needs_password(Some("1045"), false));
        assert!(!needs_password(Some("28P01"), true), "a wrong password is an error to show");
        assert!(!needs_password(Some("42P01"), false));
        assert!(!needs_password(None, false));
    }

    #[tokio::test]
    async fn pools_report_status_and_count_open_connections() {
        let dir = tempfile::tempdir().unwrap();
        let connection = file_connection(dir.path(), false);
        std::fs::write(connection.input.path.as_deref().unwrap(), b"").unwrap();
        let pools = Pools::default();
        assert_eq!(pools.status(&connection.id), ("idle", None));
        pools.get_or_connect(&connection, "", None, &Endpoint::direct(&connection), &small()).await.unwrap();
        assert_eq!(pools.status(&connection.id), ("open", None));
        assert_eq!(pools.open_count(), 1);
        pools.sweep(Duration::from_secs(600)).await;
        assert_eq!(pools.open_count(), 1, "recently used");
        pools.sweep(Duration::ZERO).await;
        assert_eq!(pools.open_count(), 0);
        let missing = Connection { input: super::super::store::ConnectionInput { path: Some("/nonexistent/x.db".into()), ..connection.input.clone() }, ..connection.clone() };
        assert!(pools.get_or_connect(&missing, "", None, &Endpoint::direct(&missing), &small()).await.is_err());
        assert_eq!(pools.status(&missing.id).0, "failed");
    }

    /// A connection from `WESTERM_TEST_PG_URL` or `WESTERM_TEST_MYSQL_URL`
    /// (`scheme://user:password@host:port/db`); None skips the test.
    pub(crate) fn from_env(name: &str, engine: Engine) -> Option<(Connection, String)> {
        let url = std::env::var(name).ok()?;
        let rest = url.split_once("://")?.1;
        let (auth, place) = rest.rsplit_once('@')?;
        let (user, password) = auth.split_once(':').unwrap_or((auth, ""));
        let (hostport, db) = place.split_once('/').unwrap_or((place, ""));
        let (host, port) = hostport.split_once(':').map(|(h, p)| (h, p.parse().ok())).unwrap_or((hostport, None));
        let input = super::super::store::ConnectionInput {
            engine,
            host: host.to_owned(),
            port,
            user: user.to_owned(),
            database: db.to_owned(),
            ssl: super::super::store::SslMode::Prefer,
            ..super::super::store::tests::pg("env")
        };
        Some((Connection { id: "fedcba9876543210".into(), input: super::super::store::validate(input).ok()? }, password.to_owned()))
    }

    #[tokio::test]
    async fn pg_read_only_survives_a_set_in_an_earlier_run() {
        let Some((connection, password)) = from_env("WESTERM_TEST_PG_URL", Engine::Postgres) else { return };
        let pool = connect(&connection, "", Some(&password), &Endpoint::direct(&connection), &Limits::DEFAULT).await.unwrap();
        run(&pool, "SET default_transaction_read_only = off", None, &Limits::DEFAULT, CancellationToken::new()).await.unwrap();
        let shown = run(&pool, "SHOW default_transaction_read_only", None, &Limits::DEFAULT, CancellationToken::new()).await.unwrap();
        assert_eq!(shown.rows, vec![vec![Some("on".to_owned())]]);
        let refused = run(&pool, "CREATE TABLE westerm_probe (a int)", None, &Limits::DEFAULT, CancellationToken::new()).await;
        assert!(matches!(refused, Err(DbError::Db(_))), "{refused:?}");
    }

    #[tokio::test]
    async fn mysql_read_only_survives_a_set_in_an_earlier_run() {
        let Some((connection, password)) = from_env("WESTERM_TEST_MYSQL_URL", Engine::Mysql) else { return };
        let pool = connect(&connection, "", Some(&password), &Endpoint::direct(&connection), &Limits::DEFAULT).await.unwrap();
        run(&pool, "SET SESSION TRANSACTION READ WRITE", None, &Limits::DEFAULT, CancellationToken::new()).await.unwrap();
        let shown = run(&pool, "SELECT @@transaction_read_only", None, &Limits::DEFAULT, CancellationToken::new()).await.unwrap();
        assert_eq!(shown.rows, vec![vec![Some("1".to_owned())]]);
    }

    fn tunneled(ssh: &str) -> Connection {
        let input = super::super::store::ConnectionInput { ssh: Some(ssh.into()), host: "db.internal".into(), ..super::super::store::tests::pg("t") };
        Connection { id: "00000000000000aa".into(), input: super::super::store::validate(input).unwrap() }
    }

    #[test]
    fn a_direct_connection_needs_no_tunnel() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let pools = Pools::default();
            let connection = Connection { id: "00000000000000ab".into(), input: super::super::store::tests::pg("d") };
            let at = pools.endpoint(&connection, None, Duration::from_secs(1)).await.unwrap();
            assert_eq!(at, Endpoint { host: "localhost".into(), port: 5432 });
            assert_eq!(pools.tunnel_count(), 0);
            let missing = pools.endpoint(&tunneled("bastion"), None, Duration::from_secs(1)).await;
            assert_eq!(missing, Err(DbError::Db("ssh not found on the login shell's PATH".into())));
        });
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_console_keeps_its_tunnel_through_the_idle_sweep() {
        use super::super::tunnel::tests::{fake_ssh, has_python};
        if !has_python() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let ssh = fake_ssh(dir.path());
        let connection = tunneled("bastion");
        let pools = Pools::default();
        let at = pools.endpoint(&connection, Some(&ssh), Duration::from_secs(5)).await.unwrap();
        assert_eq!(at.host, "127.0.0.1");
        assert_eq!(pools.endpoint(&connection, Some(&ssh), Duration::from_secs(5)).await.unwrap(), at, "the tunnel is reused");
        assert_eq!(pools.tunnel_count(), 1);
        pools.sweep(Duration::ZERO).await;
        assert_eq!(pools.tunnel_count(), 0, "no pool and no console: the sweep closes it");

        pools.endpoint(&connection, Some(&ssh), Duration::from_secs(5)).await.unwrap();
        pools.mark_console(&connection.id);
        pools.sweep(Duration::ZERO).await;
        assert_eq!(pools.tunnel_count(), 1, "a console's tunnel stays");
        pools.disconnect(&connection.id).await;
        assert_eq!(pools.tunnel_count(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failed_tunnel_marks_the_connection_failed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join("ssh");
        std::fs::write(&ssh, "#!/bin/sh\necho 'me@bastion: Permission denied (publickey).' >&2\nexit 255\n").unwrap();
        std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let connection = tunneled("bastion");
        let pools = Pools::default();
        let failed = pools.endpoint(&connection, Some(&ssh), Duration::from_secs(5)).await;
        assert!(matches!(&failed, Err(DbError::Db(message)) if message.starts_with("SSH needs a key or an agent for bastion")), "{failed:?}");
        assert_eq!(pools.status(&connection.id).0, "failed");
    }

    #[tokio::test]
    async fn a_connect_that_ends_after_a_disconnect_is_not_kept() {
        let dir = tempfile::tempdir().unwrap();
        let connection = file_connection(dir.path(), false);
        std::fs::write(connection.input.path.as_deref().unwrap(), b"").unwrap();
        let pools = Pools::default();
        let generation = pools.begin_connect(&connection.id);
        // The connection is edited (say, made read-only) while the old connect runs.
        pools.disconnect(&connection.id).await;
        let stale = connect(&connection, "", None, &Endpoint::direct(&connection), &small()).await.unwrap();
        assert!(!pools.finish_connect(&connection.id, "", generation, stale).await);
        assert_eq!(pools.open_count(), 0, "the stale pool is closed, not kept");
        assert_eq!(pools.status(&connection.id), ("idle", None));

        let generation = pools.begin_connect(&connection.id);
        let fresh = connect(&connection, "", None, &Endpoint::direct(&connection), &small()).await.unwrap();
        assert!(pools.finish_connect(&connection.id, "", generation, fresh).await);
        assert_eq!(pools.open_count(), 1);
    }

    #[test]
    fn queries_can_be_cancelled_by_id() {
        let pools = Pools::default();
        let token = pools.start_query("aaaaaaaaaaaaaaaa");
        assert!(pools.cancel_query("aaaaaaaaaaaaaaaa"));
        assert!(token.is_cancelled());
        pools.finish_query("aaaaaaaaaaaaaaaa");
        assert!(!pools.cancel_query("aaaaaaaaaaaaaaaa"));
    }
}
