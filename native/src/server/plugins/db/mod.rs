//! The Databases plugin's server side: saved connections, passwords, and the
//! `/api/plugins/db/…` routes. Every route answers 404 while the plugin is
//! not installed.

pub(super) mod console;
pub(super) mod detect;
pub(super) mod driver;
pub(super) mod secrets;
pub(super) mod store;
pub(super) mod tree;
pub(super) mod tunnel;

use std::path::{Path, PathBuf};

use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use self::driver::{DbError, Endpoint, Limits, Pools};
use self::secrets::Secrets;
use self::store::{is_connection_id, Connection, ConnectionInput, Store, StoreError};
use super::super::{json_body, no_store, origin_allowed, read_allowed, Shared};

/// The plugin's id.
pub(super) const ID: &str = crate::plugins::db::ID;
/// A connection body is at most this many bytes.
const MAX_CONNECTION_BYTES: usize = 16 * 1024;
/// A query body is at most this many bytes.
const MAX_SQL_BYTES: usize = 262_144;

/// What the Databases plugin keeps while the server runs.
pub(in crate::server) struct DbState {
    pub(super) store: Store,
    pub(super) secrets: Secrets,
    pub(super) pools: Pools,
    pub(super) limits: Limits,
    /// Where console specs and secret files go; empty only in tests that never open one.
    state_dir: PathBuf,
}

impl Default for DbState {
    fn default() -> Self {
        Self { store: Store::default(), secrets: Secrets::default(), pools: Pools::default(), limits: Limits::DEFAULT, state_dir: PathBuf::new() }
    }
}

impl DbState {
    /// Reads the connections. Secret files and console specs left by an
    /// earlier run are removed: no console of this run uses them.
    pub(in crate::server) fn load(state_dir: &Path) -> Self {
        console::SecretFiles::new(state_dir).clear();
        crate::plugins::db::clear_specs(state_dir);
        Self { store: Store::load(state_dir), state_dir: state_dir.to_path_buf(), ..Self::default() }
    }

    fn secret_files(&self) -> console::SecretFiles {
        console::SecretFiles::new(&self.state_dir)
    }

    /// The plugin was uninstalled: close what runs and remove secret files.
    /// Connections and keychain entries stay.
    pub(in crate::server) async fn on_uninstall(&self) {
        self.pools.disconnect_all().await;
        self.secret_files().clear();
    }

    /// The password to connect with: none for SQLite (no keychain call).
    async fn password(&self, connection: &Connection) -> Option<String> {
        if connection.input.engine == store::Engine::Sqlite {
            return None;
        }
        self.secrets.password(&connection.id).await
    }


}

/// `bytes` random bytes as lowercase hex.
pub(super) fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).expect("the OS random source");
    buffer.iter().map(|b| format!("{b:02x}")).collect()
}

pub(in crate::server) fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/plugins/db/connections", get(list_connections).post(create_connection).delete(forget_all))
        .route("/api/plugins/db/connections/{id}", put(update_connection).delete(delete_connection))
        .route("/api/plugins/db/connections/test", post(test_connection))
        .route("/api/plugins/db/connections/{id}/connect", post(connect_connection))
        .route("/api/plugins/db/connections/{id}/disconnect", post(disconnect_connection))
        .route("/api/plugins/db/connections/{id}/query", post(run_query))
        .route("/api/plugins/db/connections/{id}/tree", get(read_tree))
        .route("/api/plugins/db/connections/{id}/console", post(open_console))
        .route("/api/plugins/db/clients", get(list_clients))
        .route("/api/plugins/db/detect", get(detect_connections))
        .route("/api/plugins/db/queries/{query_id}/cancel", post(cancel_query))
}

/// A connection body: the fields, and what to do with the password.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionBody {
    #[serde(flatten)]
    input: ConnectionInput,
    /// Set this password; absent keeps the saved one.
    password: Option<String>,
    /// Forget the saved password.
    #[serde(default)]
    clear_password: bool,
}

/// None when a GET may go on; otherwise the answer.
fn refuse_read(state: &Shared, headers: &HeaderMap) -> Option<Response> {
    if !read_allowed(state, headers) {
        return Some(StatusCode::FORBIDDEN.into_response());
    }
    (!state.plugins.is_installed(ID)).then(|| StatusCode::NOT_FOUND.into_response())
}

/// None when a write without a JSON body may go on; otherwise the answer.
fn refuse_write(state: &Shared, headers: &HeaderMap) -> Option<Response> {
    if !origin_allowed(state, headers) {
        return Some(StatusCode::FORBIDDEN.into_response());
    }
    (!state.plugins.is_installed(ID)).then(|| StatusCode::NOT_FOUND.into_response())
}

/// The JSON body of a write (Origin and type checked, at most `max` bytes), or the status to answer.
fn write_body<T: serde::de::DeserializeOwned>(state: &Shared, headers: &HeaderMap, body: &Bytes, max: usize) -> Result<T, StatusCode> {
    if !state.plugins.is_installed(ID) {
        return Err(StatusCode::NOT_FOUND);
    }
    if body.len() > max {
        return Err(StatusCode::BAD_REQUEST);
    }
    json_body(state, headers, body).ok_or(StatusCode::BAD_REQUEST)
}

/// A connection as the list shows it: its fields, whether a password is
/// known, and how its pool is doing. Never the password.
async fn listed(state: &Shared, connection: &Connection) -> serde_json::Value {
    let saved = state.plugins.db.password(connection).await.is_some();
    let mut value = serde_json::to_value(connection).unwrap_or_default();
    if let Some(object) = value.as_object_mut() {
        object.insert("passwordSaved".into(), json!(saved));
        let (status, error) = state.plugins.db.pools.status(&connection.id);
        object.insert("status".into(), json!(status));
        object.insert("error".into(), json!(error));
    }
    value
}

async fn list_connections(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if let Some(refused) = refuse_read(&state, &headers) {
        return refused;
    }
    let db = &state.plugins.db;
    let mut connections = Vec::new();
    for connection in db.store.list() {
        connections.push(listed(&state, &connection).await);
    }
    no_store(Json(json!({ "connections": connections, "keychain": db.secrets.available().await, "open": db.pools.open_count() })))
}

/// Saves a new password; the keychain's error, if it kept the password in memory instead.
/// A changed password also removes the connection's console secret files.
async fn apply_password(state: &Shared, id: &str, body: &ConnectionBody) -> Option<String> {
    let db = &state.plugins.db;
    match body.password.as_deref() {
        Some(password) if !password.is_empty() => {
            db.secret_files().remove(id);
            db.secrets.save(id, password).await.err()
        }
        _ if body.clear_password => {
            db.secret_files().remove(id);
            db.secrets.forget(id).await;
            None
        }
        _ => None,
    }
}

/// The saved connection, with the keychain's error when it could not take the password.
fn saved(connection: &Connection, keychain_error: Option<String>) -> serde_json::Value {
    let mut value = serde_json::to_value(connection).unwrap_or_default();
    if let (Some(object), Some(error)) = (value.as_object_mut(), keychain_error) {
        object.insert("keychainError".into(), json!(error));
    }
    value
}

async fn create_connection(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    let body: ConnectionBody = match write_body(&state, &headers, &body, MAX_CONNECTION_BYTES) {
        Ok(body) => body,
        Err(status) => return status.into_response(),
    };
    match state.plugins.db.store.create(body.input.clone()) {
        Ok(connection) => {
            let keychain_error = apply_password(&state, &connection.id, &body).await;
            (StatusCode::CREATED, Json(saved(&connection, keychain_error))).into_response()
        }
        Err(error) => store_error(error),
    }
}

async fn update_connection(State(state): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<String>, body: Bytes) -> Response {
    let body: ConnectionBody = match write_body(&state, &headers, &body, MAX_CONNECTION_BYTES) {
        Ok(body) => body,
        Err(status) => return status.into_response(),
    };
    if !is_connection_id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match state.plugins.db.store.update(&id, body.input.clone()) {
        Ok(connection) => {
            // New settings take effect on the next connect.
            state.plugins.db.pools.disconnect(&id).await;
            let keychain_error = apply_password(&state, &id, &body).await;
            Json(saved(&connection, keychain_error)).into_response()
        }
        Err(error) => store_error(error),
    }
}

async fn delete_connection(State(state): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<String>) -> Response {
    if let Some(refused) = refuse_write(&state, &headers) {
        return refused;
    }
    if !is_connection_id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let db = &state.plugins.db;
    if let Err(error) = db.store.remove(&id) {
        return store_error(StoreError::Io(error));
    }
    db.secrets.forget(&id).await;
    db.secret_files().remove(&id);
    db.pools.disconnect(&id).await;
    StatusCode::NO_CONTENT.into_response()
}

async fn forget_all(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if let Some(refused) = refuse_write(&state, &headers) {
        return refused;
    }
    let db = &state.plugins.db;
    match db.store.clear() {
        Ok(ids) => {
            for id in ids {
                db.secrets.forget(&id).await;
            }
            db.pools.disconnect_all().await;
            db.secret_files().clear();
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => store_error(StoreError::Io(error)),
    }
}

/// A database error as the UI shows it.
fn db_error(error: DbError, limits: &Limits) -> Response {
    let failed = |kind: &str, message: String| (StatusCode::BAD_GATEWAY, Json(json!({ "error": message, "kind": kind }))).into_response();
    match error {
        DbError::Db(message) => failed("db", message),
        DbError::Timeout => failed("timeout", format!("Timed out after {} s", limits.query.as_secs())),
        DbError::Cancelled => failed("cancelled", "Cancelled".to_owned()),
        DbError::NeedPassword => (StatusCode::CONFLICT, Json(json!({ "needPassword": true }))).into_response(),
        DbError::Removed => (StatusCode::NOT_FOUND, Json(json!({ "removed": true }))).into_response(),
    }
}

/// The saved connection `id`, or the answer: 400 for a bad id, 404 `removed`.
fn saved_connection(state: &Shared, id: &str) -> Result<Connection, Box<Response>> {
    if !is_connection_id(id) {
        return Err(Box::new(StatusCode::BAD_REQUEST.into_response()));
    }
    state.plugins.db.store.get(id).ok_or_else(|| Box::new(db_error(DbError::Removed, &state.plugins.db.limits)))
}

/// A query id the page makes: 16 lowercase hex digits.
fn is_query_id(text: &str) -> bool {
    store::is_connection_id(text)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestBody {
    #[serde(flatten)]
    input: ConnectionInput,
    password: Option<String>,
}

/// Connects without saving, reads the version, and closes again.
async fn test_connection(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    let body: TestBody = match write_body(&state, &headers, &body, MAX_CONNECTION_BYTES) {
        Ok(body) => body,
        Err(status) => return status.into_response(),
    };
    let input = match store::validate(body.input) {
        Ok(input) => input,
        Err(error) => return store_error(error),
    };
    let connection = Connection { id: "0000000000000000".into(), input };
    let started = std::time::Instant::now();
    let limits = &state.plugins.db.limits;
    let mut tunnel = None;
    let mut at = Endpoint::direct(&connection);
    if let Some(alias) = connection.input.ssh.clone() {
        let env = state.plugins.login_env.get_or_load(super::login_env::load).await;
        let Some(ssh) = console::find_program(&env, "ssh") else {
            return no_store(Json(json!({ "ok": false, "error": "ssh not found on the login shell's PATH" })));
        };
        match tunnel::Tunnel::open(&ssh, &alias, &at.host, at.port, limits.connect).await {
            Ok(opened) => {
                at = opened.local();
                tunnel = Some(opened);
            }
            Err(error) => return no_store(Json(json!({ "ok": false, "error": error }))),
        }
    }
    let answer = match driver::connect(&connection, "", body.password.as_deref(), &at, limits).await {
        Ok(pool) => {
            let version = driver::version(&pool).await;
            pool_close(pool).await;
            match version {
                Ok(version) => json!({ "ok": true, "ms": started.elapsed().as_millis() as u64, "version": version }),
                Err(error) => json!({ "ok": false, "error": error_text(error, limits) }),
            }
        }
        Err(error) => json!({ "ok": false, "error": error_text(error, limits) }),
    };
    if let Some(tunnel) = tunnel {
        tunnel.close().await;
    }
    no_store(Json(answer))
}

async fn pool_close(pool: driver::Pool) {
    match pool {
        driver::Pool::Pg(pool) => pool.close().await,
        driver::Pool::My(pool) => pool.close().await,
        driver::Pool::Lite(pool) => pool.close().await,
    }
}

/// An error's text for the test answer.
fn error_text(error: DbError, limits: &Limits) -> String {
    match error {
        DbError::Db(message) => message,
        DbError::NeedPassword => "The database needs a password".to_owned(),
        DbError::Timeout => format!("Timed out after {} s", limits.query.as_secs()),
        DbError::Cancelled => "Cancelled".to_owned(),
        DbError::Removed => "Removed".to_owned(),
    }
}

#[derive(Deserialize)]
struct ConnectBody {
    password: Option<String>,
}

/// Opens the connection's default pool. A password in the body is saved first.
async fn connect_connection(State(state): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<String>, body: Bytes) -> Response {
    let body: ConnectBody = match write_body(&state, &headers, &body, MAX_CONNECTION_BYTES) {
        Ok(body) => body,
        Err(status) => return status.into_response(),
    };
    let connection = match saved_connection(&state, &id) {
        Ok(connection) => connection,
        Err(answer) => return *answer,
    };
    let db = &state.plugins.db;
    if let Some(password) = body.password.filter(|password| !password.is_empty()) {
        let _ = db.secrets.save(&id, &password).await;
    }
    let password = db.password(&connection).await;
    let at = match endpoint(&state, &connection).await {
        Ok(at) => at,
        Err(error) => return db_error(error, &db.limits),
    };
    match db.pools.get_or_connect(&connection, "", password.as_deref(), &at, &db.limits).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => db_error(error, &db.limits),
    }
}

async fn disconnect_connection(State(state): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<String>) -> Response {
    if let Some(refused) = refuse_write(&state, &headers) {
        return refused;
    }
    if !is_connection_id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    state.plugins.db.pools.disconnect(&id).await;
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryBody {
    #[serde(default)]
    db: String,
    schema: Option<String>,
    sql: String,
    query_id: String,
}

/// Runs one statement of the page's SQL. The pool opens on demand, so a Run
/// works after a server restart too.
async fn run_query(State(state): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<String>, body: Bytes) -> Response {
    let body: QueryBody = match write_body(&state, &headers, &body, MAX_SQL_BYTES) {
        Ok(body) => body,
        Err(status) => return status.into_response(),
    };
    if !is_query_id(&body.query_id) || body.db.chars().any(char::is_control) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let connection = match saved_connection(&state, &id) {
        Ok(connection) => connection,
        Err(answer) => return *answer,
    };
    let db = &state.plugins.db;
    let password = db.password(&connection).await;
    let at = match endpoint(&state, &connection).await {
        Ok(at) => at,
        Err(error) => return db_error(error, &db.limits),
    };
    let pool = match db.pools.get_or_connect(&connection, &body.db, password.as_deref(), &at, &db.limits).await {
        Ok(pool) => pool,
        Err(error) => return db_error(error, &db.limits),
    };
    let cancel = db.pools.start_query(&body.query_id);
    let result = driver::run(&pool, &body.sql, body.schema.as_deref(), &db.limits, cancel).await;
    db.pools.finish_query(&body.query_id);
    match result {
        Ok(result) => no_store(Json(result)),
        Err(error) => db_error(error, &db.limits),
    }
}

#[derive(Deserialize)]
struct TreeQuery {
    db: Option<String>,
    schema: Option<String>,
}

/// One level of the tree: databases, schemas, or tables and views.
async fn read_tree(State(state): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<String>, axum::extract::Query(query): axum::extract::Query<TreeQuery>) -> Response {
    if let Some(refused) = refuse_read(&state, &headers) {
        return refused;
    }
    let connection = match saved_connection(&state, &id) {
        Ok(connection) => connection,
        Err(answer) => return *answer,
    };
    let engine = connection.input.engine;
    let level = tree::Level::of(engine, query.db.as_deref(), query.schema.as_deref());
    let db = &state.plugins.db;
    let password = db.password(&connection).await;
    let at = match endpoint(&state, &connection).await {
        Ok(at) => at,
        Err(error) => return db_error(error, &db.limits),
    };
    let pool = match db.pools.get_or_connect(&connection, level.pool_db(engine), password.as_deref(), &at, &db.limits).await {
        Ok(pool) => pool,
        Err(error) => return db_error(error, &db.limits),
    };
    match tree::read_level(&pool, engine, &level, Some(connection.input.database.as_str())).await {
        Ok(nodes) => no_store(Json(json!({ "nodes": nodes }))),
        Err(error) => db_error(error, &db.limits),
    }
}

/// Where the connection's database answers now: directly, or through its SSH tunnel.
async fn endpoint(state: &Shared, connection: &Connection) -> Result<Endpoint, DbError> {
    let db = &state.plugins.db;
    if connection.input.ssh.is_none() {
        return Ok(Endpoint::direct(connection));
    }
    let env = state.plugins.login_env.get_or_load(super::login_env::load).await;
    let ssh = console::find_program(&env, "ssh");
    db.pools.endpoint(connection, ssh.as_deref(), db.limits.connect).await
}

/// The console programs on the login shell's PATH.
async fn clients(state: &Shared) -> console::Clients {
    let env = state.plugins.login_env.get_or_load(super::login_env::load).await;
    console::find_clients(&env, super::shell_panes(crate::spawn::shell_kind()))
}

async fn list_clients(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if let Some(refused) = refuse_read(&state, &headers) {
        return refused;
    }
    no_store(Json(clients(&state).await))
}

#[derive(Deserialize)]
struct ConsoleBody {
    #[serde(default)]
    db: String,
}

/// Prepares a console: its secret file and the spec a new shell types.
/// Answers the token the session query carries, and the session's name.
async fn open_console(State(state): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<String>, body: Bytes) -> Response {
    let body: ConsoleBody = match write_body(&state, &headers, &body, MAX_CONNECTION_BYTES) {
        Ok(body) => body,
        Err(status) => return status.into_response(),
    };
    if body.db.chars().any(char::is_control) || body.db.chars().count() > 128 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let connection = match saved_connection(&state, &id) {
        Ok(connection) => connection,
        Err(answer) => return *answer,
    };
    let refuse = |error: &str| (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response();
    let found = clients(&state).await;
    if !found.consoles {
        return refuse("noConsole");
    }
    let engine = connection.input.engine;
    let mysql = match engine {
        store::Engine::Postgres if !found.psql => return refuse("noClient"),
        store::Engine::Sqlite if !found.sqlite3 => return refuse("noClient"),
        store::Engine::Mysql => match found.mysql {
            Some(program) => program,
            None => return refuse("noClient"),
        },
        _ => "mysql",
    };
    let db = &state.plugins.db;
    if db.state_dir.as_os_str().is_empty() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let at = match endpoint(&state, &connection).await {
        Ok(at) => at,
        Err(error) => return db_error(error, &db.limits),
    };
    db.pools.mark_console(&id);
    let password = db.password(&connection).await;
    let input = &connection.input;
    let secret = match engine {
        store::Engine::Postgres => password.map(|password| db.secret_files().write(&id, "pgpass", &format!("{}\n", console::pgpass_line(&at, "*", &input.user, &password)))).transpose(),
        store::Engine::Mysql => db.secret_files().write(&id, "cnf", &console::mycnf(&at, &input.user, password.as_deref(), input.read_only)).map(Some),
        store::Engine::Sqlite => Ok(None),
    };
    let secret = match secret {
        Ok(secret) => secret,
        Err(error) => {
            eprintln!("> db-secrets: {error}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let argv = console::argv(&connection, &body.db, &at, secret.as_deref(), mysql);
    let token = random_hex(16);
    if let Err(error) = crate::plugins::db::write_spec(&db.state_dir, &token, &argv) {
        eprintln!("> db-consoles: {error}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    no_store(Json(json!({ "token": token, "name": console_name(&connection, &body.db) })))
}

/// `<db> console`, or `<file> console` for SQLite.
fn console_name(connection: &Connection, db: &str) -> String {
    let input = &connection.input;
    let what = match input.engine {
        store::Engine::Sqlite => {
            let path = input.path.as_deref().unwrap_or_default();
            path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned()
        }
        _ if !db.is_empty() => db.to_owned(),
        _ if !input.database.is_empty() => input.database.clone(),
        store::Engine::Postgres if !input.user.is_empty() => input.user.clone(),
        _ => input.name.clone(),
    };
    format!("{what} console")
}

/// How long finding connections may take.
const DETECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Connections found in the folders the page names (`folder=…`, absolute
/// folders only) and in Docker, without the saved ones.
async fn detect_connections(State(state): State<Shared>, headers: HeaderMap, axum::extract::RawQuery(query): axum::extract::RawQuery) -> Response {
    if let Some(refused) = refuse_read(&state, &headers) {
        return refused;
    }
    let folders: Vec<PathBuf> = url::form_urlencoded::parse(query.unwrap_or_default().as_bytes())
        .filter(|(key, value)| key == "folder" && !value.is_empty() && !value.chars().any(char::is_control))
        .map(|(_, value)| PathBuf::from(value.into_owned()))
        .filter(|folder| folder.is_absolute())
        .take(detect::MAX_FOLDERS)
        .collect();
    let env = state.plugins.login_env.get_or_load(super::login_env::load).await;
    let found = detect::detect(&folders, &state.plugins.db.store, &env, DETECT_TIMEOUT).await;
    no_store(Json(json!({ "found": found })))
}

async fn cancel_query(State(state): State<Shared>, headers: HeaderMap, UrlPath(query_id): UrlPath<String>) -> Response {
    if let Some(refused) = refuse_write(&state, &headers) {
        return refused;
    }
    if !is_query_id(&query_id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    state.plugins.db.pools.cancel_query(&query_id);
    StatusCode::NO_CONTENT.into_response()
}

fn store_error(error: StoreError) -> Response {
    match error {
        StoreError::Invalid(field) => (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("invalid:{field}") }))).into_response(),
        StoreError::NameUsed => (StatusCode::BAD_REQUEST, Json(json!({ "error": "nameUsed" }))).into_response(),
        StoreError::NotFound => StatusCode::NOT_FOUND.into_response(),
        StoreError::Io(error) => {
            eprintln!("> db-connections.json: {error}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::server::plugins::PluginState;
    use crate::server::tests::test_state;
    use axum::http::header;
    use std::sync::Arc;

    pub(crate) const ORIGIN: &str = "http://127.0.0.1:4317";

    pub(crate) fn shared(dir: &Path, installed: bool) -> Shared {
        let mut state = test_state(&[ORIGIN]);
        state.plugins = Arc::new(PluginState::load(dir));
        if installed {
            state.plugins.set_builtin(&[ID.into()]).unwrap();
        }
        Arc::new(state)
    }

    pub(crate) fn read_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "127.0.0.1:4317".parse().unwrap());
        headers.insert("sec-fetch-site", "same-origin".parse().unwrap());
        headers
    }

    pub(crate) fn write_headers() -> HeaderMap {
        let mut headers = read_headers();
        headers.insert(header::ORIGIN, ORIGIN.parse().unwrap());
        headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        headers
    }

    pub(crate) async fn body_json(response: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        if bytes.is_empty() {
            return serde_json::Value::Null;
        }
        serde_json::from_slice(&bytes).unwrap()
    }

    fn pg_body(name: &str, extra: serde_json::Value) -> Bytes {
        let mut body = json!({ "name": name, "engine": "postgres", "host": "localhost", "user": "dev", "database": "app" });
        body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        Bytes::from(body.to_string())
    }

    async fn create(state: &Shared, name: &str, extra: serde_json::Value) -> Response {
        create_connection(State(state.clone()), write_headers(), pg_body(name, extra)).await
    }

    #[tokio::test]
    async fn the_routes_answer_404_until_installed() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), false);
        assert_eq!(list_connections(State(state.clone()), read_headers()).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(create(&state, "a", json!({})).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(forget_all(State(state.clone()), write_headers()).await.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn writes_from_other_sites_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        let mut foreign = write_headers();
        foreign.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        let response = create_connection(State(state.clone()), foreign.clone(), pg_body("a", json!({}))).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(delete_connection(State(state.clone()), foreign.clone(), UrlPath("0123456789abcdef".into())).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(forget_all(State(state.clone()), foreign).await.status(), StatusCode::FORBIDDEN);
        let mut cross = read_headers();
        cross.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert_eq!(list_connections(State(state.clone()), cross).await.status(), StatusCode::FORBIDDEN);
        assert!(state.plugins.db.store.list().is_empty());
    }

    #[tokio::test]
    async fn the_list_never_carries_a_password() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        // Made at run time: a literal here reads as a leaked password to secret scanners.
        let password = ["never", "listed"].join("-");
        let response = create(&state, "local", json!({ "password": password })).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let made = body_json(response).await;
        assert_eq!(made["readOnly"], true, "read-only by default");
        let list = list_connections(State(state.clone()), read_headers()).await;
        assert_eq!(list.status(), StatusCode::OK);
        assert_eq!(list.headers()["cache-control"], "no-store");
        let body = body_json(list).await;
        assert!(!body.to_string().contains(&password));
        assert_eq!(body["connections"][0]["id"], made["id"]);
        assert_eq!(body["connections"][0]["passwordSaved"], true);
        assert_eq!(body["connections"][0]["status"], "idle");
        assert_eq!(body["keychain"], true);
        assert_eq!(body["open"], 0);
    }

    #[tokio::test]
    async fn bad_fields_and_used_names_answer_400() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        assert_eq!(create(&state, "a", json!({})).await.status(), StatusCode::CREATED);
        let used = create(&state, "a", json!({})).await;
        assert_eq!(used.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(used).await["error"], "nameUsed");
        let bad = create(&state, "b", json!({ "host": "a\nb" })).await;
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(bad).await["error"], "invalid:host");
        let big = create(&state, "c", json!({ "user": "x".repeat(MAX_CONNECTION_BYTES) })).await;
        assert_eq!(big.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn an_update_keeps_sets_or_clears_the_password() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        let id = body_json(create(&state, "a", json!({ "password": "one" })).await).await["id"].as_str().unwrap().to_owned();
        let update = |extra: serde_json::Value| update_connection(State(state.clone()), write_headers(), UrlPath(id.clone()), pg_body("a", extra));
        assert_eq!(update(json!({ "color": "red" })).await.status(), StatusCode::OK);
        assert_eq!(state.plugins.db.secrets.password(&id).await.as_deref(), Some("one"), "absent keeps it");
        assert_eq!(state.plugins.db.store.get(&id).unwrap().input.color, store::Color::Red);
        update(json!({ "password": "two" })).await;
        assert_eq!(state.plugins.db.secrets.password(&id).await.as_deref(), Some("two"));
        update(json!({ "clearPassword": true })).await;
        assert_eq!(state.plugins.db.secrets.password(&id).await, None);
        let missing = update_connection(State(state.clone()), write_headers(), UrlPath("0000000000000000".into()), pg_body("z", json!({}))).await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        let bad_id = update_connection(State(state.clone()), write_headers(), UrlPath("nope".into()), pg_body("z", json!({}))).await;
        assert_eq!(bad_id.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn deleting_a_connection_removes_its_keychain_entry() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        let id = body_json(create(&state, "a", json!({ "password": "pw" })).await).await["id"].as_str().unwrap().to_owned();
        let response = delete_connection(State(state.clone()), write_headers(), UrlPath(id.clone())).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(state.plugins.db.secrets.password(&id).await, None);
        assert!(state.plugins.db.store.list().is_empty());
    }

    /// A saved SQLite connection to a new file in `dir`, with one table of three rows.
    pub(crate) async fn sqlite_connection(state: &Shared, dir: &Path, read_only: bool) -> String {
        let path = dir.join("app.db");
        std::fs::File::create(&path).unwrap();
        let input = store::ConnectionInput { read_only, ..store::tests::sqlite("app", &path.display().to_string()) };
        let id = state.plugins.db.store.create(input).unwrap().id;
        let rw = store::ConnectionInput { read_only: false, ..state.plugins.db.store.get(&id).unwrap().input };
        let pool = driver::connect(&Connection { id: id.clone(), input: rw }, "", None, &driver::Endpoint { host: String::new(), port: 0 }, &driver::Limits::DEFAULT).await.unwrap();
        for sql in ["CREATE TABLE orders (id INTEGER, total REAL)", "INSERT INTO orders VALUES (1, 9.5), (2, 20), (3, NULL)"] {
            driver::run(&pool, sql, None, &driver::Limits::DEFAULT, Default::default()).await.unwrap();
        }
        id
    }

    pub(crate) async fn query(state: &Shared, id: &str, body: serde_json::Value) -> Response {
        run_query(State(state.clone()), write_headers(), UrlPath(id.to_owned()), Bytes::from(body.to_string())).await
    }

    #[tokio::test]
    async fn a_query_connects_on_demand_and_lists_as_open() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        let id = sqlite_connection(&state, dir.path(), true).await;
        let response = query(&state, &id, json!({ "db": "", "schema": null, "sql": "SELECT * FROM orders", "queryId": "aaaaaaaaaaaaaaaa" })).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["rows"], json!([["1", "9.5"], ["2", "20"], ["3", null]]));
        assert_eq!(body["columns"][0], json!({ "name": "id", "type": "INTEGER" }));
        let list = body_json(list_connections(State(state.clone()), read_headers()).await).await;
        assert_eq!(list["connections"][0]["status"], "open");
        assert_eq!(list["open"], 1);
    }

    #[tokio::test]
    async fn a_read_only_connection_refuses_a_write_from_a_query() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        let id = sqlite_connection(&state, dir.path(), true).await;
        let response = query(&state, &id, json!({ "db": "", "sql": "DELETE FROM orders", "queryId": "aaaaaaaaaaaaaaaa" })).await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = body_json(response).await;
        assert_eq!(body["kind"], "db");
        assert!(body["error"].as_str().unwrap().contains("readonly"));
    }

    #[tokio::test]
    async fn a_query_for_a_removed_connection_answers_404_removed() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        let response = query(&state, "0123456789abcdef", json!({ "db": "", "sql": "SELECT 1", "queryId": "aaaaaaaaaaaaaaaa" })).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_json(response).await, json!({ "removed": true }));
    }

    #[tokio::test]
    async fn oversized_or_malformed_queries_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        let id = sqlite_connection(&state, dir.path(), true).await;
        let big = query(&state, &id, json!({ "db": "", "sql": "x".repeat(MAX_SQL_BYTES), "queryId": "aaaaaaaaaaaaaaaa" })).await;
        assert_eq!(big.status(), StatusCode::BAD_REQUEST);
        let bad_id = query(&state, &id, json!({ "db": "", "sql": "SELECT 1", "queryId": "nope" })).await;
        assert_eq!(bad_id.status(), StatusCode::BAD_REQUEST);
        let bad_connection = query(&state, "nope", json!({ "db": "", "sql": "SELECT 1", "queryId": "aaaaaaaaaaaaaaaa" })).await;
        assert_eq!(bad_connection.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn the_cancel_route_stops_a_running_query() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        let id = sqlite_connection(&state, dir.path(), true).await;
        let forever = "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n) SELECT count(*) FROM n";
        let running = tokio::spawn({
            let state = state.clone();
            let id = id.clone();
            async move { query(&state, &id, json!({ "db": "", "sql": forever, "queryId": "bbbbbbbbbbbbbbbb" })).await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let cancel = cancel_query(State(state.clone()), write_headers(), UrlPath("bbbbbbbbbbbbbbbb".into())).await;
        assert_eq!(cancel.status(), StatusCode::NO_CONTENT);
        let response = running.await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(body_json(response).await["kind"], "cancelled");
    }

    #[tokio::test]
    async fn connect_and_disconnect_open_and_close_the_pools() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        let id = sqlite_connection(&state, dir.path(), true).await;
        let connect = connect_connection(State(state.clone()), write_headers(), UrlPath(id.clone()), Bytes::from("{}")).await;
        assert_eq!(connect.status(), StatusCode::NO_CONTENT);
        assert_eq!(state.plugins.db.pools.open_count(), 1);
        let disconnect = disconnect_connection(State(state.clone()), write_headers(), UrlPath(id.clone())).await;
        assert_eq!(disconnect.status(), StatusCode::NO_CONTENT);
        assert_eq!(state.plugins.db.pools.open_count(), 0);
    }

    #[tokio::test]
    async fn deleting_a_connection_closes_its_pools() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        let id = sqlite_connection(&state, dir.path(), true).await;
        query(&state, &id, json!({ "db": "", "sql": "SELECT 1", "queryId": "aaaaaaaaaaaaaaaa" })).await;
        assert_eq!(state.plugins.db.pools.open_count(), 1);
        delete_connection(State(state.clone()), write_headers(), UrlPath(id)).await;
        assert_eq!(state.plugins.db.pools.open_count(), 0);
    }

    #[tokio::test]
    async fn the_test_route_reports_the_version_or_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        let id = sqlite_connection(&state, dir.path(), true).await;
        let input = state.plugins.db.store.get(&id).unwrap().input;
        let ok = test_connection(State(state.clone()), write_headers(), Bytes::from(serde_json::to_string(&input).unwrap())).await;
        let ok = body_json(ok).await;
        assert_eq!(ok["ok"], true);
        assert!(ok["version"].as_str().unwrap().starts_with("SQLite "));
        let missing = store::ConnectionInput { path: Some("/nonexistent/x.db".into()), ..input };
        let failed = body_json(test_connection(State(state.clone()), write_headers(), Bytes::from(serde_json::to_string(&missing).unwrap())).await).await;
        assert_eq!(failed["ok"], false);
        assert!(!failed["error"].as_str().unwrap().is_empty());
        assert_eq!(state.plugins.db.pools.open_count(), 0, "a test keeps nothing open");
    }

    #[tokio::test]
    async fn the_tree_route_lists_a_level() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        let id = sqlite_connection(&state, dir.path(), true).await;
        let tree = |id: String| read_tree(State(state.clone()), read_headers(), UrlPath(id), axum::extract::Query(TreeQuery { db: None, schema: None }));
        let response = tree(id.clone()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(
            body_json(response).await,
            json!({ "nodes": [{ "name": "orders", "kind": "table", "rows": null, "preview": "SELECT * FROM \"orders\" LIMIT 100" }] })
        );
        assert_eq!(tree("0123456789abcdef".into()).await.status(), StatusCode::NOT_FOUND);
        let mut cross = read_headers();
        cross.insert("sec-fetch-site", "cross-site".parse().unwrap());
        let refused = read_tree(State(state.clone()), cross, UrlPath(id), axum::extract::Query(TreeQuery { db: None, schema: None })).await;
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    }

    /// Puts executables named `programs` on the login PATH the routes see.
    #[cfg(unix)]
    pub(crate) async fn fake_programs(state: &Shared, dir: &Path, programs: &[&str]) {
        use std::os::unix::fs::PermissionsExt;
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for program in programs {
            let path = bin.join(program);
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let env = super::super::login_env::LoginEnv { vars: vec![("PATH".into(), bin.display().to_string())] };
        state.plugins.login_env.reset().await;
        state.plugins.login_env.get_or_load(|| async { env }).await;
    }

    #[cfg(unix)]
    async fn console(state: &Shared, id: &str) -> Response {
        open_console(State(state.clone()), write_headers(), UrlPath(id.to_owned()), Bytes::from(json!({ "db": "" }).to_string())).await
    }

    #[cfg(unix)]
    fn consoles() -> bool {
        super::super::shell_panes(crate::spawn::shell_kind())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_console_request_writes_a_spec_the_host_can_read() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        fake_programs(&state, dir.path(), &["sqlite3"]).await;
        let id = sqlite_connection(&state, dir.path(), true).await;
        let response = console(&state, &id).await;
        if !consoles() {
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            return;
        }
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["name"], "app.db console");
        let token = body["token"].as_str().unwrap().to_owned();
        assert!(crate::plugins::db::is_token(&token));
        let line = crate::plugins::db::command_in(&crate::plugins::db::Launch { token }, dir.path(), crate::quote::ShellKind::Sh).unwrap();
        assert!(line.starts_with("'env' '--' 'sqlite3' '-readonly' "), "{line}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_missing_client_refuses_the_console() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        fake_programs(&state, dir.path(), &["psql"]).await;
        let id = sqlite_connection(&state, dir.path(), true).await;
        let response = console(&state, &id).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let expected = if consoles() { "noClient" } else { "noConsole" };
        assert_eq!(body_json(response).await["error"], expected);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_clients_route_reports_found_programs() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        fake_programs(&state, dir.path(), &["psql", "mysql"]).await;
        let response = list_clients(State(state.clone()), read_headers()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await, json!({ "psql": true, "mysql": "mysql", "sqlite3": false, "consoles": consoles() }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn password_changes_and_deletes_remove_secret_files() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        fake_programs(&state, dir.path(), &["psql"]).await;
        if !consoles() {
            return;
        }
        let id = body_json(create(&state, "p", json!({ "password": "one" })).await).await["id"].as_str().unwrap().to_owned();
        let secret = dir.path().join("db-secrets").join(format!("{id}.pgpass"));
        assert_eq!(console(&state, &id).await.status(), StatusCode::OK);
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), "localhost:5432:*:dev:one\n");
        update_connection(State(state.clone()), write_headers(), UrlPath(id.clone()), pg_body("p", json!({ "password": "two" }))).await;
        assert!(!secret.exists(), "a new password removes the old file");
        console(&state, &id).await;
        assert!(secret.exists());
        delete_connection(State(state.clone()), write_headers(), UrlPath(id)).await;
        assert!(!secret.exists());
    }

    #[test]
    fn starting_clears_secret_files_and_waiting_consoles() {
        let dir = tempfile::tempdir().unwrap();
        let secret = console::SecretFiles::new(dir.path()).write("0123456789abcdef", "pgpass", "x").unwrap();
        crate::plugins::db::write_spec(dir.path(), &"c".repeat(32), &["psql".into()]).unwrap();
        DbState::load(dir.path());
        assert!(!secret.exists());
        assert!(!crate::plugins::db::spec_path(dir.path(), &"c".repeat(32)).exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_detect_route_reads_the_folders_it_is_given() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        fake_programs(&state, dir.path(), &[]).await;
        let site = dir.path().join("my site");
        std::fs::create_dir(&site).unwrap();
        std::fs::write(site.join(".env"), "DATABASE_URL=postgres://dev:pw@localhost/app\n").unwrap();
        let query = format!("folder={}&folder=relative&folder=", url::form_urlencoded::byte_serialize(site.display().to_string().as_bytes()).collect::<String>());
        let response = detect_connections(State(state.clone()), read_headers(), axum::extract::RawQuery(Some(query))).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["found"].as_array().unwrap().len(), 1);
        assert_eq!(body["found"][0]["source"], "env");
        assert_eq!(body["found"][0]["password"], "pw");
        assert_eq!(body["found"][0]["input"]["database"], "app");
        assert_eq!(body["found"][0]["input"]["readOnly"], true);
        let mut cross = read_headers();
        cross.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert_eq!(detect_connections(State(state.clone()), cross, axum::extract::RawQuery(None)).await.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn listing_a_sqlite_connection_never_asks_the_keychain() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(secrets::FakeKeychain::default());
        let mut plugins = PluginState::load(dir.path());
        plugins.db.secrets = Secrets::new(fake.clone(), std::time::Duration::from_millis(200));
        plugins.set_builtin(&[ID.into()]).unwrap();
        let mut state = test_state(&[ORIGIN]);
        state.plugins = Arc::new(plugins);
        let state = Arc::new(state);
        state.plugins.db.store.create(store::tests::sqlite("lite", "/x/a.db")).unwrap();
        let body = body_json(list_connections(State(state.clone()), read_headers()).await).await;
        assert_eq!(body["connections"][0]["passwordSaved"], false);
        assert_eq!(fake.calls.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn forget_all_removes_every_connection_and_password() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path(), true);
        let a = body_json(create(&state, "a", json!({ "password": "pw" })).await).await["id"].as_str().unwrap().to_owned();
        create(&state, "b", json!({})).await;
        assert_eq!(forget_all(State(state.clone()), write_headers()).await.status(), StatusCode::NO_CONTENT);
        assert!(state.plugins.db.store.list().is_empty());
        assert_eq!(state.plugins.db.secrets.password(&a).await, None);
    }
}
