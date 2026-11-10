//! The routes that list, fetch, install, update, and remove external
//! plugins, and the ones a plugin's panel reaches through the page: `run`,
//! storage, and the browser-pane check. Each write route takes a JSON body
//! from the dashboard's own page; every answer is `no-store`.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::catalog::{self, IndexEntry, UpdateMode};
use super::manifest::{Manifest, Permissions};
use super::stage::{self, Commit, Failure, Staged};
use super::browser;
use super::run::{run_capped, RunError};
use super::store::{self, ExternalInfo, Source};
use crate::server::plugins::login_env;
use crate::server::plugins::PluginState;
use crate::server::{Shared, json_body, no_store, origin_allowed, read_allowed};
use crate::state_dir::home_dir;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use crate::time::now_millis;

/// A request body is at most this many bytes.
const MAX_BODY_BYTES: usize = 4096;
/// A `run` or `browser` body (arguments, an address) is at most this many bytes.
const MAX_RUN_BODY_BYTES: usize = 65_536;
/// A program started by `run` gets this long.
const RUN_TIMEOUT: Duration = Duration::from_secs(10);
/// `run` keeps this many bytes of stdout, and as many of stderr.
const RUN_OUTPUT_CAP: usize = 1_048_576;
/// At most this many `run` calls of one plugin are in flight.
const MAX_RUNS: usize = 4;
/// A plugin's stored JSON value is at most this many bytes.
const MAX_STORAGE_BYTES: usize = 65_536;

pub(super) fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/plugins/catalog", get(get_catalog))
        .route("/api/plugins/fetch", post(post_fetch))
        .route("/api/plugins/install", post(post_install))
        .route("/api/plugins/{id}/update", post(post_update))
        .route("/api/plugins/{id}/run", post(post_run))
        .route("/api/plugins/{id}/storage", get(get_storage).put(put_storage))
        .route("/api/plugins/{id}/browser", post(post_browser))
        .route("/api/plugins/{id}", delete(delete_plugin))
}

/// A JSON body from the dashboard's own page, within the size limit.
fn parse<T: DeserializeOwned>(state: &Shared, headers: &HeaderMap, body: &Bytes) -> Result<T, Failure> {
    parse_sized(state, headers, body, MAX_BODY_BYTES)
}

/// A JSON body from the dashboard's own page, within `limit` bytes.
fn parse_sized<T: DeserializeOwned>(state: &Shared, headers: &HeaderMap, body: &Bytes, limit: usize) -> Result<T, Failure> {
    if body.len() > limit {
        return Err(Failure::bad("the request is too big"));
    }
    json_body(state, headers, body).ok_or_else(|| Failure::bad("the request must be a JSON object of the right shape from the dashboard"))
}

/// Runs blocking work (curl, tar, files) off the async threads.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, Failure> + Send + 'static) -> Result<T, Failure> {
    tokio::task::spawn_blocking(work).await.unwrap_or_else(|_| Err(Failure::internal("the request failed inside the server")))
}

fn answer(result: Result<impl Serialize, Failure>) -> Response {
    match result {
        Ok(body) => no_store(Json(body)),
        Err(failure) => failure.into_response(),
    }
}

/// An installed plugin and what the next update looks like.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogPlugin {
    #[serde(flatten)]
    info: ExternalInfo,
    /// The newer version found, if any.
    update: Option<String>,
    update_mode: UpdateMode,
}

#[derive(Serialize)]
struct Catalog {
    /// The verified plugin list, or `null` when it could not be loaded.
    index: Option<Vec<IndexEntry>>,
    installed: Vec<CatalogPlugin>,
}

/// The plugin list, the installed plugins, and which have a newer version.
/// Opening the management screen is what asks for this, so it is also the
/// moment the newer versions are looked for.
async fn get_catalog(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    answer(
        blocking(move || {
            let now = now_millis();
            let externals = &state.plugins.externals;
            let index = externals.catalog.index.get(&externals.sources, now);
            catalog::check_updates(&state.plugins, now);
            let installed = state
                .plugins
                .external_infos()
                .into_iter()
                .map(|info| CatalogPlugin {
                    update: catalog::available_update(&state.plugins, &info.id, &info.version),
                    update_mode: catalog::update_mode(&info.source),
                    info,
                })
                .collect();
            Ok(Catalog { index, installed })
        })
        .await,
    )
}

#[derive(Deserialize)]
struct FetchBody {
    url: Option<String>,
    path: Option<String>,
}

#[derive(Serialize)]
struct Fetched {
    token: String,
    manifest: Manifest,
    size: Option<u64>,
    source: Source,
}

/// Downloads or reads a package, checks it, and keeps it for the install call.
async fn post_fetch(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    answer(fetch(state, &headers, &body).await)
}

async fn fetch(state: Shared, headers: &HeaderMap, body: &Bytes) -> Result<Fetched, Failure> {
    let FetchBody { url, path } = parse(&state, headers, body)?;
    let staged = blocking(move || {
        let externals = &state.plugins.externals;
        let staged = match (url, path) {
            (Some(typed), None) => {
                // Only https:// addresses come in from the page.
                let download = stage::package_url(&externals.sources, &typed)?;
                let mut staged = stage::stage_from_url(&externals.sources, &state.plugins.state_dir, &download)?;
                staged.source.url = Some(typed);
                staged
            }
            (None, Some(path)) => stage::stage_folder(&path)?,
            _ => return Err(Failure::bad("give either url or path")),
        };
        let fetched = Fetched { token: staged.token.clone(), manifest: staged.manifest.clone(), size: staged.size, source: staged.source.clone() };
        externals.pending.insert(staged, now_millis());
        Ok(fetched)
    })
    .await?;
    Ok(staged)
}

#[derive(Deserialize)]
struct InstallBody {
    token: Option<String>,
    index: Option<String>,
    granted: Permissions,
    trusted: Option<bool>,
    /// A reinstall: the fetched package replaces the installed plugin of
    /// the same id like an update (its storage stays).
    #[serde(default)]
    replace: bool,
}

/// Installs a fetched package (the user trusts its source) or an index entry.
async fn post_install(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    let result = match parse::<InstallBody>(&state, &headers, &body) {
        Ok(request) => install(state, request).await,
        Err(failure) => Err(failure),
    };
    answer(result)
}

async fn install(state: Shared, request: InstallBody) -> Result<ExternalInfo, Failure> {
    match request {
        InstallBody { token: Some(token), index: None, granted, trusted, replace } => {
            if trusted != Some(true) {
                return Err(Failure::bad("confirm that you trust the source of this plugin"));
            }
            install_fetched(state, &token, &granted, replace).await
        }
        InstallBody { token: None, index: Some(_), replace: true, .. } => Err(Failure::bad("replace works only with a token")),
        InstallBody { token: None, index: Some(id), granted, .. } => install_from_index(state, &id, &granted).await,
        _ => Err(Failure::bad("give either token or index")),
    }
}

/// Installs a fetched package. With `replace` it takes the place of the
/// installed plugin of the same id as an update does: the storage stays, the
/// old version's folder goes, and `granted` must be everything the new
/// package asks for, so the user has confirmed what it adds.
async fn install_fetched(state: Shared, token: &str, granted: &Permissions, replace: bool) -> Result<ExternalInfo, Failure> {
    let pending = &state.plugins.externals.pending;
    let (id, asked) = pending
        .with(token, now_millis(), |staged| (!staged.update).then(|| (staged.manifest.id.clone(), staged.manifest.permissions.clone())))
        .flatten()
        .ok_or_else(|| Failure::not_found("unknown or expired token"))?;
    if !stage::same_permissions(granted, &asked) {
        return Err(Failure::permissions_changed(&asked));
    }
    if !replace && stage::readable(&state.plugins, &id) {
        return Err(Failure::conflict("already installed"));
    }
    let mode = if replace && state.plugins.external(&id).is_some() { Commit::Update } else { Commit::Install };
    let staged = pending.take(token, now_millis()).ok_or_else(|| Failure::not_found("unknown or expired token"))?;
    blocking(move || stage::commit(&state.plugins, staged, mode)).await
}

async fn install_from_index(state: Shared, id: &str, granted: &Permissions) -> Result<ExternalInfo, Failure> {
    let id = id.to_owned();
    let granted = granted.clone();
    blocking(move || {
        let externals = &state.plugins.externals;
        let entries = externals.catalog.index.get(&externals.sources, now_millis()).ok_or_else(|| Failure::gateway("Could not load the plugin list"))?;
        let entry = entries.into_iter().find(|entry| entry.id == id).ok_or_else(|| Failure::not_found("the plugin is not in the plugin list"))?;
        if stage::readable(&state.plugins, &id) {
            return Err(Failure::conflict("already installed"));
        }
        let mut staged = stage::stage_package(&externals.sources, &state.plugins.state_dir, &entry.url, Some(&entry.sha256))?;
        staged.source = Source { kind: super::store::SourceKind::Index, url: Some(entry.url), path: None };
        let refusal = if staged.manifest.id != entry.id {
            Some(Failure::bad(format!("the package is {}, but the plugin list says {}", staged.manifest.id, entry.id)))
        } else if !stage::same_permissions(&granted, &staged.manifest.permissions) {
            Some(Failure::permissions_changed(&staged.manifest.permissions))
        } else {
            None
        };
        if let Some(failure) = refusal {
            staged.discard();
            return Err(failure);
        }
        stage::commit(&state.plugins, staged, Commit::Install)
    })
    .await
}

#[derive(Deserialize)]
struct UpdateBody {
    token: Option<String>,
    granted: Option<Permissions>,
}

/// What the page needs to ask the user again: the new version, everything it
/// asks for, and what it asks for beyond what is granted now.
#[derive(Serialize)]
struct UpdateConfirm {
    token: String,
    version: String,
    permissions: Permissions,
    added: Permissions,
}

/// Updates to the newest version, or reloads a folder. A version that asks for
/// more than the user granted waits for a second call with its token.
async fn post_update(State(state): State<Shared>, headers: HeaderMap, Path(id): Path<String>, body: Bytes) -> Response {
    match parse::<UpdateBody>(&state, &headers, &body) {
        Ok(request) => update(state, &id, request).await,
        Err(failure) => failure.into_response(),
    }
}

async fn update(state: Shared, id: &str, request: UpdateBody) -> Response {
    let Some(record) = state.plugins.external(id) else { return Failure::not_found("not installed").into_response() };
    if stage::needs_reinstall(&record) {
        return Failure::conflict("reinstall to update").into_response();
    }
    match request {
        UpdateBody { token: Some(token), granted: Some(granted) } => answer(confirm_update(state, id, &token, &granted).await),
        UpdateBody { token: None, granted: None } => {
            let found = blocking({
                let state = state.clone();
                move || stage::stage_update(&state.plugins, &record).map(|staged| staged.map(|staged| (staged, record)))
            })
            .await;
            match found {
                Ok(Some((staged, record))) => offer_update(state, staged, &record).await,
                Ok(None) => answer(unchanged(&state, id)),
                Err(failure) => failure.into_response(),
            }
        }
        _ => Failure::bad("give both token and granted, or neither").into_response(),
    }
}

/// The installed plugin as it is, for an update that found nothing new.
fn unchanged(state: &Shared, id: &str) -> Result<ExternalInfo, Failure> {
    state
        .plugins
        .external_infos()
        .into_iter()
        .find(|info| info.id == id)
        .ok_or_else(|| Failure::conflict("the plugin's files cannot be read; install it again"))
}

/// A new version is ready: install it at once, or keep it and ask for the
/// permissions it adds.
async fn offer_update(state: Shared, staged: Staged, record: &super::store::ExternalRecord) -> Response {
    let added = record.granted.added(&staged.manifest.permissions);
    if added.is_empty() {
        return answer(blocking(move || stage::commit(&state.plugins, staged, Commit::Update)).await);
    }
    let confirm = UpdateConfirm {
        token: staged.token.clone(),
        version: staged.manifest.version.clone(),
        permissions: staged.manifest.permissions.clone(),
        added,
    };
    state.plugins.externals.pending.insert(staged, now_millis());
    (StatusCode::CONFLICT, no_store(Json(confirm))).into_response()
}

async fn confirm_update(state: Shared, id: &str, token: &str, granted: &Permissions) -> Result<ExternalInfo, Failure> {
    let pending = &state.plugins.externals.pending;
    let asked = pending
        .with(token, now_millis(), |staged| (staged.update && staged.manifest.id == id).then(|| staged.manifest.permissions.clone()))
        .flatten()
        .ok_or_else(|| Failure::not_found("unknown or expired token"))?;
    if !stage::same_permissions(granted, &asked) {
        return Err(Failure::permissions_changed(&asked));
    }
    let staged = pending.take(token, now_millis()).ok_or_else(|| Failure::not_found("unknown or expired token"))?;
    blocking(move || stage::commit(&state.plugins, staged, Commit::Update)).await
}

/// Removes the package, the storage, and the record. A folder source's own
/// folder and the panes it opened stay.
async fn delete_plugin(State(state): State<Shared>, headers: HeaderMap, Path(id): Path<String>) -> Response {
    if !origin_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match blocking(move || stage::remove(&state.plugins, &id)).await {
        Ok(()) => no_store(StatusCode::NO_CONTENT),
        Err(failure) => failure.into_response(),
    }
}

#[derive(Deserialize)]
struct RunBody {
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
}

/// Runs a program the user granted this plugin, with an argument array and
/// no shell. A program that exits with an error is still a 200 with its code.
async fn post_run(State(state): State<Shared>, headers: HeaderMap, Path(id): Path<String>, body: Bytes) -> Response {
    let request = match parse_sized::<RunBody>(&state, &headers, &body, MAX_RUN_BODY_BYTES) {
        Ok(request) => request,
        Err(failure) => return failure.into_response(),
    };
    if state.plugins.external(&id).is_none() {
        return Failure::not_found("not installed").into_response();
    }
    if !state.plugins.granted_run(&id, &request.program) {
        return Failure::new(StatusCode::FORBIDDEN, "not permitted").into_response();
    }
    let cwd = match request.cwd.map(PathBuf::from) {
        None => home_dir(),
        Some(cwd) if !cwd.is_absolute() => return Failure::bad("cwd must be an absolute path").into_response(),
        Some(cwd) if !cwd.is_dir() => return Failure::bad("cwd is not a folder").into_response(),
        Some(cwd) => cwd,
    };
    let Some(_permit) = run_permit(&state, &id) else {
        return Failure::new(StatusCode::TOO_MANY_REQUESTS, "too many runs at once").into_response();
    };
    let env = state.plugins.login_env.get_or_load(login_env::load).await;
    match run_capped(&request.program, &request.args, &cwd, &env, RUN_TIMEOUT, RUN_OUTPUT_CAP).await {
        Ok(output) => no_store(Json(output)),
        Err(RunError::TimedOut) => Failure::new(StatusCode::GATEWAY_TIMEOUT, "timed out").into_response(),
        Err(RunError::NotFound) => Failure::gateway(format!("{} not found", request.program)).into_response(),
        Err(RunError::Other(error)) => Failure::gateway(error).into_response(),
    }
}

/// One of the plugin's `MAX_RUNS` slots, or `None` when all are taken.
fn run_permit(state: &Shared, id: &str) -> Option<OwnedSemaphorePermit> {
    let slots = state
        .plugins
        .externals
        .runs
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(id.to_owned())
        .or_insert_with(|| Arc::new(Semaphore::new(MAX_RUNS)))
        .clone();
    slots.try_acquire_owned().ok()
}

/// The plugin's stored JSON value, or `null` before the first write.
async fn get_storage(State(state): State<Shared>, headers: HeaderMap, Path(id): Path<String>) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if state.plugins.external(&id).is_none() {
        return Failure::not_found("not installed").into_response();
    }
    match std::fs::read(store::data_path(&state.plugins.state_dir, &id)) {
        Ok(saved) => no_store(([(header::CONTENT_TYPE, "application/json")], saved)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => no_store(([(header::CONTENT_TYPE, "application/json")], "null")),
        Err(error) => Failure::internal(format!("cannot read the storage: {error}")).into_response(),
    }
}

/// Replaces the plugin's stored value with any JSON of at most 64 KiB.
async fn put_storage(State(state): State<Shared>, headers: HeaderMap, Path(id): Path<String>, body: Bytes) -> Response {
    if body.len() > MAX_STORAGE_BYTES {
        return Failure::new(StatusCode::PAYLOAD_TOO_LARGE, "plugin storage holds at most 64 KiB").into_response();
    }
    if json_body::<serde_json::Value>(&state, &headers, &body).is_none() {
        return Failure::bad("the request must be JSON from the dashboard").into_response();
    }
    match blocking(move || write_storage(&state.plugins, &id, &body)).await {
        Ok(()) => no_store(StatusCode::NO_CONTENT),
        Err(failure) => failure.into_response(),
    }
}

/// Writes `data.json` through a temp file. The write lock keeps a removal
/// from running between the record check and the write.
fn write_storage(plugins: &PluginState, id: &str, body: &[u8]) -> Result<(), Failure> {
    let _writing = plugins.externals.write_lock.lock().unwrap_or_else(PoisonError::into_inner);
    if plugins.external(id).is_none() {
        return Err(Failure::not_found("not installed"));
    }
    let path = store::data_path(&plugins.state_dir, id);
    let temp = path.with_extension("json.tmp");
    let written = std::fs::create_dir_all(store::plugin_dir(&plugins.state_dir, id))
        .and_then(|()| crate::state_dir::private_file().write(true).create(true).truncate(true).open(&temp))
        .and_then(|mut out| out.write_all(body))
        .and_then(|()| std::fs::rename(&temp, &path));
    written.map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        Failure::internal(format!("cannot save the storage: {error}"))
    })
}

#[derive(Deserialize)]
struct BrowserBody {
    url: String,
}

/// Says whether the page may open `url` in a browser pane. The page opens
/// the pane only after a 204.
async fn post_browser(State(state): State<Shared>, headers: HeaderMap, Path(id): Path<String>, body: Bytes) -> Response {
    let BrowserBody { url } = match parse_sized(&state, &headers, &body, MAX_RUN_BODY_BYTES) {
        Ok(request) => request,
        Err(failure) => return failure.into_response(),
    };
    let Some(record) = state.plugins.external(&id) else { return Failure::not_found("not installed").into_response() };
    if !record.granted.panes {
        return Failure::new(StatusCode::FORBIDDEN, "needs the panes permission").into_response();
    }
    match browser::refused(&url, &state.origins) {
        None => no_store(StatusCode::NO_CONTENT),
        Some(reason @ browser::OWN_ADDRESS) => Failure::new(StatusCode::FORBIDDEN, reason).into_response(),
        Some(reason) => Failure::bad(reason).into_response(),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::server::plugins::PluginState;
    use crate::server::plugins::login_env::LoginEnv;
    use crate::server::plugins::external::Sources;
    use crate::server::plugins::external::stage::{self, Staged};
    use crate::server::plugins::external::store::staging_dir;
    use crate::server::tests::test_state;
    use crate::update::sha256_hex;
    use serde_json::{Value, json};
    use std::path::{Path as FsPath, PathBuf};
    use std::process::Command;
    use std::sync::Arc;

    const ID: &str = "k8s-tools.kubernetes";
    const ORIGIN: &str = "http://127.0.0.1:4317";

    fn headers(json: bool) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(axum::http::header::HOST, "127.0.0.1:4317".parse().unwrap());
        headers.insert(axum::http::header::ORIGIN, ORIGIN.parse().unwrap());
        if json {
            headers.insert(axum::http::header::CONTENT_TYPE, "application/json".parse().unwrap());
        }
        headers
    }

    async fn read(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() })
    }

    fn grant(run: &[&str]) -> Value {
        json!({ "run": run, "panes": true, "background": true })
    }

    /// A test server: a state folder, a work folder for packages and the
    /// index, and `Sources` that point at the work folder.
    struct Env {
        state_dir: tempfile::TempDir,
        work: tempfile::TempDir,
        state: Shared,
    }

    impl Env {
        fn new() -> Self {
            let state_dir = tempfile::tempdir().unwrap();
            let work = tempfile::tempdir().unwrap();
            let mut state = test_state(&[ORIGIN]);
            let mut plugins = PluginState::load(state_dir.path());
            plugins.externals.sources = Sources {
                index_url: format!("file://{}/index.json", work.path().display()),
                github_api: format!("file://{}/api", work.path().display()),
                allow_file: true,
            };
            state.plugins = Arc::new(plugins);
            Self { state_dir, work, state: Arc::new(state) }
        }

        fn sources(&self) -> &Sources {
            &self.state.plugins.externals.sources
        }

        fn plugins(&self) -> PathBuf {
            self.state_dir.path().join("plugins")
        }

        fn staging_is_empty(&self) -> bool {
            std::fs::read_dir(staging_dir(self.state_dir.path())).is_ok_and(|mut items| items.next().is_none())
                || !staging_dir(self.state_dir.path()).exists()
        }

        /// A folder with a plugin: `plugin.json` (id `id`) and its panel and icon.
        fn folder(&self, name: &str, id: &str, version: &str, run: &[&str]) -> PathBuf {
            let folder = self.work.path().join(name);
            std::fs::create_dir_all(&folder).unwrap();
            let manifest = json!({
                "id": id, "name": "Kubernetes", "version": version, "description": "See pods.", "panel": "panel.html", "icon": "icon.svg",
                "permissions": { "run": run, "panes": true, "background": true }
            });
            std::fs::write(folder.join("plugin.json"), manifest.to_string()).unwrap();
            std::fs::write(folder.join("panel.html"), format!("<p>{version}</p>")).unwrap();
            std::fs::write(folder.join("icon.svg"), "<svg/>").unwrap();
            folder.canonicalize().unwrap()
        }

        /// A `.tar.gz` of the plugin; with `top`, everything sits in a folder of that name.
        fn tarball(&self, name: &str, id: &str, version: &str, run: &[&str], top: Option<&str>) -> PathBuf {
            let source = match top {
                Some(top) => {
                    let parent = self.work.path().join(format!("{name}-parent"));
                    let folder = self.folder(&format!("{name}-parent/{top}"), id, version, run);
                    assert!(folder.starts_with(parent.canonicalize().unwrap()));
                    parent
                }
                None => self.folder(&format!("{name}-files"), id, version, run),
            };
            let archive = self.work.path().join(format!("{name}.tar.gz"));
            let names: Vec<&str> = match top {
                Some(top) => vec![top],
                None => vec!["plugin.json", "panel.html", "icon.svg"],
            };
            let packed = Command::new("tar").arg("-czf").arg(&archive).arg("-C").arg(&source).args(names).status().unwrap();
            assert!(packed.success());
            archive
        }

        fn url(path: &FsPath) -> String {
            format!("file://{}", path.display())
        }

        /// Writes the index with one entry for `ID`.
        fn index(&self, version: &str, tarball: &FsPath, sha256: Option<&str>, run: &[&str]) {
            let sum = sha256.map_or_else(|| sha256_hex(tarball).unwrap(), str::to_owned);
            let entry = json!({
                "id": ID, "name": "Kubernetes", "description": "See pods.", "repo": "https://github.com/k8s-tools/westerm-k8s",
                "version": version, "url": Self::url(tarball), "sha256": sum, "permissions": { "run": run, "panes": true, "background": true }
            });
            std::fs::write(self.work.path().join("index.json"), json!([entry]).to_string()).unwrap();
            self.state.plugins.externals.catalog.index.clear();
        }

        async fn catalog(&self) -> (StatusCode, Value) {
            read(get_catalog(State(self.state.clone()), headers(false)).await).await
        }

        async fn fetch(&self, body: Value) -> (StatusCode, Value) {
            read(post_fetch(State(self.state.clone()), headers(true), Bytes::from(body.to_string())).await).await
        }

        async fn install(&self, body: Value) -> (StatusCode, Value) {
            read(post_install(State(self.state.clone()), headers(true), Bytes::from(body.to_string())).await).await
        }

        async fn update(&self, id: &str, body: Value) -> (StatusCode, Value) {
            read(post_update(State(self.state.clone()), headers(true), Path(id.to_owned()), Bytes::from(body.to_string())).await).await
        }

        async fn delete(&self, id: &str) -> (StatusCode, Value) {
            read(delete_plugin(State(self.state.clone()), headers(false), Path(id.to_owned())).await).await
        }

        /// Installs version `version` from the index (the tarball and the index entry are made here).
        async fn install_from_index(&self, version: &str, run: &[&str]) -> Value {
            let tarball = self.tarball(&format!("k8s-{version}"), ID, version, run, None);
            self.index(version, &tarball, None, run);
            let (status, info) = self.install(json!({ "index": ID, "granted": grant(run) })).await;
            assert_eq!(status, StatusCode::OK, "{info}");
            info
        }

        /// A new server over the same state folder, as after a restart.
        fn restart(&mut self) {
            let mut state = test_state(&[ORIGIN]);
            let mut plugins = PluginState::load(self.state_dir.path());
            plugins.externals.sources = self.sources().clone();
            state.plugins = Arc::new(plugins);
            self.state = Arc::new(state);
        }

        /// Puts an executable `name` that runs `body` in a folder and makes
        /// that folder the PATH of the remembered login environment.
        async fn fake(&self, name: &str, body: &str) {
            use std::os::unix::fs::PermissionsExt;
            let bin = self.work.path().join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let path = bin.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            let login = LoginEnv { vars: vec![("PATH".into(), format!("{}:/bin:/usr/bin", bin.display()))] };
            self.state.plugins.login_env.reset().await;
            self.state.plugins.login_env.get_or_load(|| async { login }).await;
        }

        async fn run(&self, id: &str, body: Value) -> (StatusCode, Value) {
            read(post_run(State(self.state.clone()), headers(true), Path(id.to_owned()), Bytes::from(body.to_string())).await).await
        }

        async fn storage(&self, id: &str) -> (StatusCode, Value) {
            read(get_storage(State(self.state.clone()), headers(false), Path(id.to_owned())).await).await
        }

        async fn set_storage(&self, id: &str, body: String) -> (StatusCode, Value) {
            read(put_storage(State(self.state.clone()), headers(true), Path(id.to_owned()), Bytes::from(body)).await).await
        }

        async fn browser(&self, id: &str, url: &str) -> (StatusCode, Value) {
            let body = Bytes::from(json!({ "url": url }).to_string());
            read(post_browser(State(self.state.clone()), headers(true), Path(id.to_owned()), body).await).await
        }

        /// Keeps a staged package the way the fetch route does and returns its token.
        fn keep(&self, staged: Staged) -> String {
            let token = staged.token.clone();
            self.state.plugins.externals.pending.insert(staged, crate::time::now_millis());
            token
        }
    }

    #[tokio::test]
    async fn a_folder_install_previews_then_installs() {
        let env = Env::new();
        let folder = env.folder("k8s", ID, "0.3.1", &["kubectl"]);

        let (status, fetched) = env.fetch(json!({ "path": folder })).await;
        assert_eq!(status, StatusCode::OK, "{fetched}");
        assert_eq!(fetched["manifest"]["id"], ID);
        assert_eq!(fetched["manifest"]["permissions"]["run"], json!(["kubectl"]));
        assert_eq!(fetched["source"], json!({ "kind": "folder", "path": folder }));
        assert_eq!(fetched["token"].as_str().unwrap().len(), 32);
        let token = fetched["token"].clone();
        assert!(!env.state.plugins.is_installed(ID), "a preview installs nothing");

        let (status, body) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]) })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "needs trusted: true");
        assert!(body["error"].as_str().unwrap().contains("trust"));
        let (status, _) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": false })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!env.state.plugins.is_installed(ID));

        let (status, info) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true })).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        assert!(env.state.plugins.is_installed(ID));
        assert_eq!(info["id"], ID);
        assert_eq!(info["source"], json!({ "kind": "folder", "path": folder }));
        assert_eq!(info["granted"], grant(&["kubectl"]));
        assert!(!env.plugins().join(ID).exists(), "a folder is used in place and not copied");
        let record = env.state.plugins.external(ID).unwrap();
        assert_eq!(record.source.path.as_deref(), folder.to_str());

        let (status, _) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true })).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "a token works once");
    }

    #[tokio::test]
    async fn a_folder_must_be_an_absolute_existing_plugin_folder() {
        let env = Env::new();
        env.folder("k8s", ID, "0.3.1", &[]);
        for path in ["relative/k8s", "k8s", "../k8s"] {
            let (status, body) = env.fetch(json!({ "path": path })).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
            assert!(body["error"].as_str().unwrap().contains("absolute"), "{body}");
        }
        let missing = env.work.path().join("nope");
        assert_eq!(env.fetch(json!({ "path": missing })).await.0, StatusCode::BAD_REQUEST);
        let empty = env.work.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        assert_eq!(env.fetch(json!({ "path": empty })).await.0, StatusCode::BAD_REQUEST, "no plugin.json");
        // A link to a folder is stored as the folder it points to.
        let link = env.work.path().join("link");
        std::os::unix::fs::symlink(env.work.path().join("k8s"), &link).unwrap();
        let (status, fetched) = env.fetch(json!({ "path": link })).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(fetched["source"]["path"], json!(env.work.path().join("k8s").canonicalize().unwrap()));
        // One of url or path, nothing else.
        assert_eq!(env.fetch(json!({})).await.0, StatusCode::BAD_REQUEST);
        assert_eq!(env.fetch(json!({ "path": link, "url": "https://example.test/a.tar.gz" })).await.0, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn the_fetch_route_takes_only_https_addresses() {
        let env = Env::new();
        let tarball = env.tarball("k8s", ID, "0.3.1", &["kubectl"], None);
        for url in [Env::url(&tarball), "http://example.test/k8s.tar.gz".into(), "https://example.test/notes.txt".into(), "ftp://x/k.tar.gz".into()] {
            let (status, body) = env.fetch(json!({ "url": url })).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{url}");
            assert!(body["error"].is_string());
        }
        assert!(env.staging_is_empty());
    }

    #[tokio::test]
    async fn a_url_install_keeps_the_package() {
        let env = Env::new();
        let tarball = env.tarball("k8s", ID, "0.3.1", &["kubectl"], None);
        let staged = stage::stage_from_url(env.sources(), env.state_dir.path(), &Env::url(&tarball)).unwrap();
        assert_eq!(staged.manifest.id, ID);
        assert!(staged.size.unwrap() > 0);
        assert!(!env.staging_is_empty(), "the package waits in the staging folder");
        let token = env.keep(staged);

        let (status, info) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true })).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        let package = env.plugins().join(ID).join("0.3.1");
        assert!(package.join("plugin.json").is_file() && package.join("panel.html").is_file(), "{package:?}");
        assert_eq!(info["source"]["kind"], "url");
        assert_eq!(info["source"]["url"], Env::url(&tarball));
        assert!(env.staging_is_empty());
        let record = env.state.plugins.external(ID).unwrap();
        assert_eq!(record.sha256, Some(sha256_hex(&tarball).unwrap()));
        assert_eq!(record.granted.run, vec!["kubectl"]);
        let file: Value = serde_json::from_slice(&std::fs::read(env.state_dir.path().join("plugins.json")).unwrap()).unwrap();
        assert_eq!(file["external"][0]["source"]["kind"], "url");
        assert_eq!(file["installed"], json!([ID]));
    }

    #[tokio::test]
    async fn a_package_with_one_top_folder_installs_the_folders_contents() {
        let env = Env::new();
        let tarball = env.tarball("k8s", ID, "0.3.1", &["kubectl"], Some("k8s-package"));
        let staged = stage::stage_from_url(env.sources(), env.state_dir.path(), &Env::url(&tarball)).unwrap();
        let token = env.keep(staged);
        let (status, info) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true })).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        let package = env.plugins().join(ID).join("0.3.1");
        assert!(package.join("plugin.json").is_file() && package.join("icon.svg").is_file());
        assert!(!package.join("k8s-package").exists(), "the top folder is the package, not part of it");
        assert!(env.staging_is_empty(), "the staging folder is cleaned up");
    }

    #[tokio::test]
    async fn install_needs_the_confirmed_permissions() {
        let env = Env::new();
        let tarball = env.tarball("k8s", ID, "0.3.1", &["kubectl"], None);
        let token = env.keep(stage::stage_from_url(env.sources(), env.state_dir.path(), &Env::url(&tarball)).unwrap());

        let (status, body) = env.install(json!({ "token": token, "granted": { "run": [] }, "trusted": true })).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "permissions changed");
        assert_eq!(body["permissions"], grant(&["kubectl"]));
        assert!(!env.state.plugins.is_installed(ID));
        assert!(!env.plugins().join(ID).exists());
        // Fewer or more programs, or a different switch, are all a change.
        let (status, _) = env.install(json!({ "token": token, "granted": { "run": ["kubectl", "helm"], "panes": true, "background": true }, "trusted": true })).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = env.install(json!({ "token": token, "granted": { "run": ["kubectl"], "panes": false, "background": true }, "trusted": true })).await;
        assert_eq!(status, StatusCode::CONFLICT);
        // The package waits: the right grant installs it, in any order.
        let (status, _) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true })).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn a_second_install_of_the_same_plugin_is_refused() {
        let env = Env::new();
        env.install_from_index("0.3.1", &["kubectl"]).await;
        let (status, body) = env.install(json!({ "index": ID, "granted": grant(&["kubectl"]) })).await;
        assert_eq!((status, &body["error"]), (StatusCode::CONFLICT, &json!("already installed")));
        let tarball = env.tarball("again", ID, "0.3.1", &["kubectl"], None);
        let token = env.keep(stage::stage_from_url(env.sources(), env.state_dir.path(), &Env::url(&tarball)).unwrap());
        let (status, body) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true })).await;
        assert_eq!((status, &body["error"]), (StatusCode::CONFLICT, &json!("already installed")));
        assert!(env.plugins().join(ID).join("0.3.1").join("plugin.json").is_file(), "the installed one is untouched");
    }

    #[tokio::test]
    async fn unknown_tokens_and_index_ids_are_404() {
        let env = Env::new();
        let (status, _) = env.install(json!({ "token": "0".repeat(32), "granted": grant(&[]), "trusted": true })).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let tarball = env.tarball("k8s", ID, "0.3.1", &[], None);
        env.index("0.3.1", &tarball, None, &[]);
        let (status, _) = env.install(json!({ "index": "nobody.nothing", "granted": grant(&[]) })).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        // Neither, or both, of token and index.
        assert_eq!(env.install(json!({ "granted": grant(&[]) })).await.0, StatusCode::BAD_REQUEST);
        assert_eq!(env.install(json!({ "token": "a", "index": ID, "granted": grant(&[]), "trusted": true })).await.0, StatusCode::BAD_REQUEST);
        assert_eq!(env.install(json!({ "index": ID })).await.0, StatusCode::BAD_REQUEST, "granted is required");
    }

    #[tokio::test]
    async fn an_index_install_checks_the_sha256() {
        let env = Env::new();
        let tarball = env.tarball("k8s", ID, "0.3.1", &["kubectl"], None);
        env.index("0.3.1", &tarball, Some(&"0".repeat(64)), &["kubectl"]);

        let (status, body) = env.install(json!({ "index": ID, "granted": grant(&["kubectl"]) })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(body["error"].as_str().unwrap().contains("sha256"), "{body}");
        assert!(!env.plugins().join(ID).exists());
        assert!(env.staging_is_empty());
        assert!(!env.state.plugins.is_installed(ID));

        env.index("0.3.1", &tarball, None, &["kubectl"]);
        let (status, info) = env.install(json!({ "index": ID, "granted": grant(&["kubectl"]) })).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        assert_eq!(info["source"]["kind"], "index");
        assert!(env.plugins().join(ID).join("0.3.1").join("plugin.json").is_file());
        assert_eq!(env.state.plugins.external(ID).unwrap().sha256, Some(sha256_hex(&tarball).unwrap()));
        assert!(env.staging_is_empty());
    }

    #[tokio::test]
    async fn an_index_install_without_an_index_is_502() {
        let env = Env::new();
        let (status, body) = env.install(json!({ "index": ID, "granted": grant(&[]) })).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(body["error"].as_str().unwrap().contains("plugin list"));
        // A package that cannot be downloaded is 502 too, with nothing left.
        let tarball = env.tarball("k8s", ID, "0.3.1", &[], None);
        env.index("0.3.1", &tarball, None, &[]);
        std::fs::remove_file(&tarball).unwrap();
        let (status, _) = env.install(json!({ "index": ID, "granted": grant(&[]) })).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(env.staging_is_empty() && !env.plugins().join(ID).exists());
    }

    #[tokio::test]
    async fn a_failed_install_leaves_no_folder() {
        let env = Env::new();
        // The package's plugin.json has an id that is not allowed.
        let bad = env.tarball("bad", "A.b", "0.3.1", &[], None);
        env.index("0.3.1", &bad, None, &[]);
        let (status, body) = env.install(json!({ "index": ID, "granted": grant(&[]) })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(body["error"].as_str().unwrap().contains("id"), "{body}");
        assert!(!env.plugins().join(ID).exists() && !env.plugins().join("A.b").exists());
        assert!(env.staging_is_empty());

        // A good package with another id than the index says.
        let other = env.tarball("other", "someone.else", "0.3.1", &[], None);
        env.index("0.3.1", &other, None, &[]);
        let (status, body) = env.install(json!({ "index": ID, "granted": grant(&[]) })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(!env.plugins().join(ID).exists() && !env.plugins().join("someone.else").exists());
        assert!(env.staging_is_empty());
        assert!(!env.state.plugins.is_installed(ID) && !env.state.plugins.is_installed("someone.else"));

        // A direct address with a broken package.
        let err = stage::stage_from_url(env.sources(), env.state_dir.path(), &Env::url(&bad)).unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        let err = stage::stage_from_url(env.sources(), env.state_dir.path(), &Env::url(&env.work.path().join("missing.tar.gz"))).unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_GATEWAY);
        assert!(env.staging_is_empty());
    }

    #[tokio::test]
    async fn an_update_that_adds_permissions_asks_again() {
        let env = Env::new();
        let first = env.install_from_index("0.3.1", &["kubectl"]).await;
        let installed_at = first["installedAt"].as_u64().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let tarball = env.tarball("k8s-0.4.0", ID, "0.4.0", &["kubectl", "helm"], None);
        env.index("0.4.0", &tarball, None, &["kubectl", "helm"]);

        let (status, confirm) = env.update(ID, json!({})).await;
        assert_eq!(status, StatusCode::CONFLICT, "{confirm}");
        assert_eq!(confirm["added"], json!({ "run": ["helm"], "panes": false, "background": false }));
        assert_eq!(confirm["version"], "0.4.0");
        assert_eq!(confirm["permissions"], grant(&["kubectl", "helm"]));
        let token = confirm["token"].clone();
        assert_eq!(token.as_str().unwrap().len(), 32);
        assert_eq!(env.state.plugins.external(ID).unwrap().version, "0.3.1", "nothing changed yet");
        assert_eq!(env.state.plugins.external(ID).unwrap().granted.run, vec!["kubectl"]);
        assert!(env.plugins().join(ID).join("0.3.1").is_dir());

        // The token needs the new grant, and a new install cannot use it.
        let (status, body) = env.update(ID, json!({ "token": token, "granted": grant(&["kubectl"]) })).await;
        assert_eq!((status, &body["error"]), (StatusCode::CONFLICT, &json!("permissions changed")));
        let (status, _) = env.install(json!({ "token": token, "granted": grant(&["kubectl", "helm"]), "trusted": true })).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "an update token is not an install token");

        let (status, info) = env.update(ID, json!({ "token": token, "granted": grant(&["kubectl", "helm"]) })).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        assert_eq!(info["version"], "0.4.0");
        assert_eq!(info["granted"], grant(&["kubectl", "helm"]));
        assert!(info["installedAt"].as_u64().unwrap() > installed_at);
        assert!(!env.plugins().join(ID).join("0.3.1").exists(), "the old version is deleted");
        assert!(env.plugins().join(ID).join("0.4.0").join("plugin.json").is_file());
        assert_eq!(env.state.plugins.external(ID).unwrap().granted.run, vec!["kubectl", "helm"]);
        assert!(env.staging_is_empty());
        let (status, _) = env.update(ID, json!({ "token": token, "granted": grant(&["kubectl", "helm"]) })).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "a token works once");
    }

    #[tokio::test]
    async fn an_update_with_the_same_or_fewer_permissions_goes_through() {
        let env = Env::new();
        env.install_from_index("0.3.1", &["kubectl", "helm"]).await;
        let tarball = env.tarball("k8s-0.4.0", ID, "0.4.0", &["kubectl"], None);
        env.index("0.4.0", &tarball, None, &["kubectl"]);
        let (status, info) = env.update(ID, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        assert_eq!(info["version"], "0.4.0");
        // A narrower set takes effect at once.
        assert_eq!(info["granted"], grant(&["kubectl"]));
        assert!(!env.state.plugins.granted_run(ID, "helm") && env.state.plugins.granted_run(ID, "kubectl"));
        assert!(!env.plugins().join(ID).join("0.3.1").exists() && env.plugins().join(ID).join("0.4.0").is_dir());

        // Nothing newer: the plugin as it is.
        let (status, again) = env.update(ID, json!({})).await;
        assert_eq!((status, again["version"].clone()), (StatusCode::OK, json!("0.4.0")));
        assert!(env.plugins().join(ID).join("0.4.0").is_dir());
    }

    #[tokio::test]
    async fn an_update_that_goes_wrong_keeps_the_installed_version() {
        let env = Env::new();
        env.install_from_index("0.3.1", &["kubectl"]).await;
        let good = env.tarball("k8s-0.4.0", ID, "0.4.0", &["kubectl"], None);
        // The sum is wrong.
        env.index("0.4.0", &good, Some(&"f".repeat(64)), &["kubectl"]);
        assert_eq!(env.update(ID, json!({})).await.0, StatusCode::BAD_REQUEST);
        // The package has another id.
        let other = env.tarball("other", "someone.else", "0.4.0", &["kubectl"], None);
        env.index("0.4.0", &other, None, &["kubectl"]);
        assert_eq!(env.update(ID, json!({})).await.0, StatusCode::BAD_REQUEST);
        // The index says 0.5.0 but the package is 0.3.1.
        let same = env.tarball("same", ID, "0.3.1", &["kubectl"], None);
        env.index("0.5.0", &same, None, &["kubectl"]);
        let (status, body) = env.update(ID, json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(body["error"].as_str().unwrap().contains("not newer"));

        assert_eq!(env.state.plugins.external(ID).unwrap().version, "0.3.1");
        assert!(env.plugins().join(ID).join("0.3.1").join("plugin.json").is_file());
        assert!(env.staging_is_empty());
        assert_eq!(env.plugins().join(ID).read_dir().unwrap().count(), 1);
    }

    #[tokio::test]
    async fn a_tarball_install_says_reinstall_to_update() {
        let env = Env::new();
        let tarball = env.tarball("k8s", ID, "0.3.1", &["kubectl"], None);
        let token = env.keep(stage::stage_from_url(env.sources(), env.state_dir.path(), &Env::url(&tarball)).unwrap());
        env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true })).await;

        let (status, catalog) = env.catalog().await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(catalog["installed"][0]["updateMode"], "reinstall");
        assert_eq!(catalog["installed"][0]["update"], Value::Null);
        let (status, body) = env.update(ID, json!({})).await;
        assert_eq!((status, &body["error"]), (StatusCode::CONFLICT, &json!("reinstall to update")));
        assert!(env.plugins().join(ID).join("0.3.1").is_dir());
    }

    /// Installs `ID` version `version` from a `.tar.gz` link and returns its token-less info.
    async fn install_tarball(env: &Env, name: &str, version: &str, run: &[&str]) -> Value {
        let tarball = env.tarball(name, ID, version, run, None);
        let token = env.keep(stage::stage_from_url(env.sources(), env.state_dir.path(), &Env::url(&tarball)).unwrap());
        let (status, info) = env.install(json!({ "token": token, "granted": grant(run), "trusted": true })).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        info
    }

    /// Fetches `ID` version `version` from a new `.tar.gz` link; returns the token.
    fn fetch_tarball(env: &Env, name: &str, version: &str, run: &[&str]) -> String {
        let tarball = env.tarball(name, ID, version, run, None);
        env.keep(stage::stage_from_url(env.sources(), env.state_dir.path(), &Env::url(&tarball)).unwrap())
    }

    #[tokio::test]
    async fn a_reinstall_replaces_the_plugin_and_keeps_its_data() {
        let env = Env::new();
        install_tarball(&env, "k8s-0.3.1", "0.3.1", &["kubectl"]).await;
        assert_eq!(env.set_storage(ID, r#"{"context":"dev"}"#.into()).await.0, StatusCode::NO_CONTENT);

        // Without `replace` the installed plugin is in the way.
        let token = fetch_tarball(&env, "k8s-0.4.0", "0.4.0", &["kubectl"]);
        let (status, body) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true })).await;
        assert_eq!((status, &body["error"]), (StatusCode::CONFLICT, &json!("already installed")));

        let (status, info) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true, "replace": true })).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        assert_eq!(info["version"], "0.4.0");
        assert_eq!(env.state.plugins.external(ID).unwrap().version, "0.4.0");
        assert!(env.plugins().join(ID).join("0.4.0").join("plugin.json").is_file());
        assert!(!env.plugins().join(ID).join("0.3.1").exists(), "the old version's folder is deleted");
        assert_eq!(env.storage(ID).await, (StatusCode::OK, json!({"context": "dev"})), "the plugin's data stays");
        assert!(env.staging_is_empty());

        // The same version again replaces the files in place.
        let token = fetch_tarball(&env, "k8s-0.4.0-again", "0.4.0", &["kubectl"]);
        let (status, info) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true, "replace": true })).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        assert!(env.plugins().join(ID).join("0.4.0").join("plugin.json").is_file());
        assert_eq!(env.storage(ID).await, (StatusCode::OK, json!({"context": "dev"})));
        assert!(env.staging_is_empty());
    }

    #[tokio::test]
    async fn a_reinstall_that_asks_for_more_needs_the_whole_grant() {
        let env = Env::new();
        install_tarball(&env, "k8s-0.3.1", "0.3.1", &["kubectl"]).await;
        let token = fetch_tarball(&env, "k8s-0.4.0", "0.4.0", &["kubectl", "helm"]);
        // The old grant is not enough: the user confirms what the new package asks.
        let (status, body) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true, "replace": true })).await;
        assert_eq!((status, &body["error"]), (StatusCode::CONFLICT, &json!("permissions changed")));
        assert_eq!(body["permissions"], grant(&["kubectl", "helm"]));
        assert_eq!(env.state.plugins.external(ID).unwrap().version, "0.3.1", "the installed plugin is untouched");
        assert!(env.plugins().join(ID).join("0.3.1").join("plugin.json").is_file());

        let (status, info) = env.install(json!({ "token": token, "granted": grant(&["kubectl", "helm"]), "trusted": true, "replace": true })).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        assert_eq!(env.state.plugins.external(ID).unwrap().granted.run, vec!["kubectl", "helm"]);
    }

    #[tokio::test]
    async fn a_reinstall_that_fails_keeps_the_installed_plugin() {
        let env = Env::new();
        install_tarball(&env, "k8s-0.3.1", "0.3.1", &["kubectl"]).await;
        assert_eq!(env.set_storage(ID, r#"{"context":"dev"}"#.into()).await.0, StatusCode::NO_CONTENT);
        let token = fetch_tarball(&env, "k8s-0.3.1-again", "0.3.1", &["kubectl"]);
        // A folder in the way of the temp file makes writing plugins.json fail.
        std::fs::create_dir(env.state_dir.path().join("plugins.json.tmp")).unwrap();
        let (status, body) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true, "replace": true })).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
        let package = env.plugins().join(ID).join("0.3.1");
        assert!(package.join("plugin.json").is_file() && package.join("panel.html").is_file(), "the same version's files are back");
        assert!(stage::readable(&env.state.plugins, ID));
        assert_eq!(env.storage(ID).await, (StatusCode::OK, json!({"context": "dev"})));
        assert!(env.staging_is_empty());
    }

    #[tokio::test]
    async fn replace_needs_a_fetched_package() {
        let env = Env::new();
        env.install_from_index("0.3.1", &["kubectl"]).await;
        let (status, body) = env.install(json!({ "index": ID, "granted": grant(&["kubectl"]), "replace": true })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }

    #[tokio::test]
    async fn a_folder_reload_reads_the_folder_again() {
        let env = Env::new();
        let folder = env.folder("k8s", ID, "0.3.1", &["kubectl"]);
        let (_, fetched) = env.fetch(json!({ "path": folder })).await;
        env.install(json!({ "token": fetched["token"], "granted": grant(&["kubectl"]), "trusted": true })).await;
        let (_, catalog) = env.catalog().await;
        assert_eq!(catalog["installed"][0]["updateMode"], "reload");

        // The same files again: fine, and the time moves.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let before = env.state.plugins.external(ID).unwrap().installed_at;
        let (status, info) = env.update(ID, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        assert!(info["installedAt"].as_u64().unwrap() > before);

        // A new version with the same permissions.
        env.folder("k8s", ID, "0.3.2", &["kubectl"]);
        let (status, info) = env.update(ID, json!({})).await;
        assert_eq!((status, info["version"].clone()), (StatusCode::OK, json!("0.3.2")));

        // helm is added: the user confirms it first.
        env.folder("k8s", ID, "0.3.3", &["kubectl", "helm"]);
        let (status, confirm) = env.update(ID, json!({})).await;
        assert_eq!(status, StatusCode::CONFLICT, "{confirm}");
        assert_eq!(confirm["added"], json!({ "run": ["helm"], "panes": false, "background": false }));
        assert_eq!(confirm["version"], "0.3.3");
        assert_eq!(env.state.plugins.external(ID).unwrap().version, "0.3.2");
        let (status, info) = env.update(ID, json!({ "token": confirm["token"], "granted": grant(&["kubectl", "helm"]) })).await;
        assert_eq!((status, info["version"].clone()), (StatusCode::OK, json!("0.3.3")));
        assert!(env.state.plugins.granted_run(ID, "helm"));

        // Fewer programs narrow the grant at once.
        env.folder("k8s", ID, "0.3.4", &[]);
        let (status, info) = env.update(ID, json!({})).await;
        assert_eq!((status, info["granted"].clone()), (StatusCode::OK, grant(&[])));
        assert!(!env.state.plugins.granted_run(ID, "kubectl"));

        // The folder is broken, or gone.
        std::fs::write(folder.join("plugin.json"), "{}").unwrap();
        assert_eq!(env.update(ID, json!({})).await.0, StatusCode::BAD_REQUEST);
        std::fs::remove_dir_all(&folder).unwrap();
        assert_eq!(env.update(ID, json!({})).await.0, StatusCode::BAD_REQUEST);
        assert_eq!(env.state.plugins.external(ID).unwrap().version, "0.3.4");
    }

    #[tokio::test]
    async fn a_github_plugin_updates_from_its_latest_release() {
        let env = Env::new();
        let first = env.tarball("k8s-0.3.1", ID, "0.3.1", &["kubectl"], None);
        let token = env.keep(stage::stage_from_url(env.sources(), env.state_dir.path(), &Env::url(&first)).unwrap());
        // Install it as a repository address would: the record keeps the repository.
        let mut staged = env.state.plugins.externals.pending.take(&token, crate::time::now_millis()).unwrap();
        staged.source.url = Some("https://github.com/k8s-tools/westerm-k8s".into());
        let token = env.keep(staged);
        env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true })).await;

        let second = env.tarball("k8s-0.4.0", ID, "0.4.0", &["kubectl"], None);
        let releases = env.work.path().join("api/repos/k8s-tools/westerm-k8s/releases");
        std::fs::create_dir_all(&releases).unwrap();
        let release = |tag: &str, assets: Value| std::fs::write(releases.join("latest"), json!({ "tag_name": tag, "assets": assets }).to_string()).unwrap();
        release("v0.3.1", json!([]));
        let (_, catalog) = env.catalog().await;
        assert_eq!(catalog["installed"][0]["updateMode"], "check");
        assert_eq!(catalog["installed"][0]["update"], Value::Null);
        let (status, same) = env.update(ID, json!({})).await;
        assert_eq!((status, same["version"].clone()), (StatusCode::OK, json!("0.3.1")), "nothing newer");

        release("v0.4.0", json!([]));
        let (status, body) = env.update(ID, json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "a release without the package file: {body}");
        release("v0.4.0", json!([{ "name": "westerm-plugin.tar.gz", "browser_download_url": Env::url(&second) }]));
        let (status, info) = env.update(ID, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        assert_eq!(info["version"], "0.4.0");
        assert_eq!(info["source"]["url"], "https://github.com/k8s-tools/westerm-k8s", "the record keeps the repository");
        assert!(env.plugins().join(ID).join("0.4.0").is_dir() && !env.plugins().join(ID).join("0.3.1").exists());
    }

    #[tokio::test]
    async fn uninstall_removes_files_and_storage_but_not_a_folder() {
        let env = Env::new();
        env.install_from_index("0.3.1", &["kubectl"]).await;
        std::fs::write(env.plugins().join(ID).join("data.json"), r#"{"a":1}"#).unwrap();
        env.state.plugins.set_builtin(&["docker".into()]).unwrap();
        let (status, body) = env.delete(ID).await;
        assert_eq!((status, body), (StatusCode::NO_CONTENT, Value::Null));
        assert!(!env.plugins().join(ID).exists(), "the package and the storage are gone");
        assert!(!env.state.plugins.is_installed(ID));
        assert_eq!(env.state.plugins.installed(), vec!["docker"], "built-in plugins stay");
        assert!(env.state.plugins.external_infos().is_empty());
        let file: Value = serde_json::from_slice(&std::fs::read(env.state_dir.path().join("plugins.json")).unwrap()).unwrap();
        assert_eq!(file, json!({ "installed": ["docker"] }));

        let folder = env.folder("k8s", ID, "0.3.1", &["kubectl"]);
        let (_, fetched) = env.fetch(json!({ "path": folder })).await;
        env.install(json!({ "token": fetched["token"], "granted": grant(&["kubectl"]), "trusted": true })).await;
        std::fs::create_dir_all(env.plugins().join(ID)).unwrap();
        std::fs::write(env.plugins().join(ID).join("data.json"), "{}").unwrap();
        assert_eq!(env.delete(ID).await.0, StatusCode::NO_CONTENT);
        assert!(folder.join("plugin.json").is_file(), "the developer's folder stays");
        assert!(!env.plugins().join(ID).exists());

        assert_eq!(env.delete("docker").await.0, StatusCode::NOT_FOUND);
        assert_eq!(env.delete(ID).await.0, StatusCode::NOT_FOUND, "already gone");
        assert!(env.state.plugins.is_installed("docker"));
    }

    #[tokio::test]
    async fn a_record_without_files_can_be_removed_and_installed_again() {
        let mut env = Env::new();
        env.install_from_index("0.3.1", &["kubectl"]).await;
        std::fs::remove_dir_all(env.plugins().join(ID).join("0.3.1")).unwrap();
        env.restart();
        assert!(env.state.plugins.external_infos().is_empty(), "it cannot be read");
        assert!(env.state.plugins.external(ID).is_some(), "but the record is there");

        // Installing replaces the record.
        let tarball = env.tarball("k8s", ID, "0.3.1", &["kubectl"], None);
        let token = env.keep(stage::stage_from_url(env.sources(), env.state_dir.path(), &Env::url(&tarball)).unwrap());
        let (status, info) = env.install(json!({ "token": token, "granted": grant(&["kubectl"]), "trusted": true })).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        assert_eq!(env.state.plugins.external_infos().len(), 1);
        assert!(env.plugins().join(ID).join("0.3.1").join("plugin.json").is_file());

        // Removing works from the record alone.
        std::fs::remove_dir_all(env.plugins().join(ID).join("0.3.1")).unwrap();
        env.restart();
        assert_eq!(env.delete(ID).await.0, StatusCode::NO_CONTENT);
        assert!(env.state.plugins.external(ID).is_none());
        assert!(!env.plugins().join(ID).exists());
    }

    #[tokio::test]
    async fn the_catalog_marks_updates_and_survives_a_missing_index() {
        let env = Env::new();
        env.install_from_index("0.3.1", &["kubectl"]).await;
        std::fs::remove_file(env.work.path().join("index.json")).unwrap();
        env.state.plugins.externals.catalog.index.clear();

        let (status, catalog) = env.catalog().await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(catalog["index"], Value::Null, "{catalog}");
        let installed = &catalog["installed"];
        assert_eq!(installed.as_array().unwrap().len(), 1);
        assert_eq!(installed[0]["id"], ID);
        assert_eq!(installed[0]["version"], "0.3.1");
        assert_eq!(installed[0]["name"], "Kubernetes");
        assert_eq!(installed[0]["update"], Value::Null);
        assert_eq!(installed[0]["updateMode"], "check");

        let tarball = env.tarball("k8s-0.4.0", ID, "0.4.0", &["kubectl"], None);
        env.index("0.4.0", &tarball, None, &["kubectl"]);
        let (status, catalog) = env.catalog().await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(catalog["installed"][0]["update"], "0.4.0");
        assert_eq!(catalog["installed"][0]["updateMode"], "check");
        let entry = &catalog["index"][0];
        assert_eq!((entry["id"].clone(), entry["version"].clone()), (json!(ID), json!("0.4.0")));
        assert_eq!(entry["sha256"], sha256_hex(&tarball).unwrap());
        assert_eq!(entry["permissions"]["run"], json!(["kubectl"]));
        assert!(entry["url"].is_string() && entry["repo"].is_string() && entry["description"].is_string());
    }

    #[tokio::test]
    async fn the_catalog_drops_bad_entries_and_asks_nothing_before_it_is_opened() {
        let env = Env::new();
        let (status, catalog) = env.catalog().await;
        assert_eq!((status, catalog), (StatusCode::OK, json!({ "index": null, "installed": [] })));
        let good = json!({ "id": ID, "name": "K", "version": "1.0.0", "url": "https://example.test/k.tar.gz", "sha256": "a".repeat(64) });
        let bad = json!({ "id": "docker", "name": "D", "version": "1.0.0", "url": "https://example.test/d.tar.gz", "sha256": "a".repeat(64) });
        std::fs::write(env.work.path().join("index.json"), json!([bad, good, 7]).to_string()).unwrap();
        let (_, catalog) = env.catalog().await;
        let ids: Vec<Value> = catalog["index"].as_array().unwrap().iter().map(|entry| entry["id"].clone()).collect();
        assert_eq!(ids, vec![json!(ID)]);
    }

    #[tokio::test]
    async fn the_routes_refuse_other_sites_and_big_bodies() {
        let env = Env::new();
        let folder = env.folder("k8s", ID, "0.3.1", &[]);
        let body = || Bytes::from(json!({ "path": folder }).to_string());
        let state = || State(env.state.clone());

        // A page on another site, or a body that is not JSON.
        let mut foreign = headers(true);
        foreign.insert(axum::http::header::ORIGIN, "https://evil.example".parse().unwrap());
        assert_eq!(post_fetch(state(), foreign.clone(), body()).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(post_install(state(), foreign.clone(), body()).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(post_update(state(), foreign.clone(), Path(ID.into()), Bytes::from("{}")).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(delete_plugin(state(), foreign, Path(ID.into())).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(post_fetch(state(), headers(false), body()).await.status(), StatusCode::BAD_REQUEST);
        let big = json!({ "path": folder, "pad": "x".repeat(4096) }).to_string();
        assert_eq!(post_fetch(state(), headers(true), Bytes::from(big)).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(post_fetch(state(), headers(true), body()).await.status(), StatusCode::OK, "the same request from the page works");

        let mut cross = headers(false);
        cross.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert_eq!(get_catalog(state(), cross).await.status(), StatusCode::FORBIDDEN);
        let response = get_catalog(state(), headers(false)).await;
        assert_eq!(response.headers()["cache-control"], "no-store");
        for response in [
            post_fetch(state(), headers(true), Bytes::from("{}")).await,
            post_update(state(), headers(true), Path("a.b".into()), Bytes::from("{}")).await,
            delete_plugin(state(), headers(false), Path("a.b".into())).await,
        ] {
            assert_eq!(response.headers()["cache-control"], "no-store", "{}", response.status());
        }
    }

    #[test]
    fn the_routes_do_not_clash_with_the_plugin_routes() {
        // axum panics when two routes overlap.
        let _ = crate::server::plugins::routes();
    }

    #[tokio::test]
    async fn waiting_packages_are_limited_and_expire() {
        let env = Env::new();
        let tarball = env.tarball("k8s", ID, "0.3.1", &[], None);
        let pending = &env.state.plugins.externals.pending;
        let stage = |created: u64| {
            let mut staged = stage::stage_from_url(env.sources(), env.state_dir.path(), &Env::url(&tarball)).unwrap();
            staged.created = created;
            staged
        };
        let mut tokens = Vec::new();
        let mut first_dir = None;
        for created in 1_000..1_004 {
            let staged = stage(created);
            tokens.push(staged.token.clone());
            first_dir.get_or_insert_with(|| staged.dir.clone().unwrap());
            pending.insert(staged, created);
        }
        let first_dir = first_dir.unwrap();
        assert!(first_dir.exists());
        let fifth = stage(2_000);
        tokens.push(fifth.token.clone());
        pending.insert(fifth, 2_000);
        assert!(pending.with(&tokens[0], 2_000, |_| ()).is_none(), "the oldest was pushed out");
        assert!(!first_dir.exists(), "and its files are deleted");
        assert!(tokens[1..].iter().all(|token| pending.with(token, 2_000, |_| ()).is_some()));
        assert_eq!(std::fs::read_dir(staging_dir(env.state_dir.path())).unwrap().count(), 4);

        // Fifteen minutes after they were made they are gone, files too.
        let minutes_15 = 15 * 60 * 1000;
        assert!(pending.with(&tokens[4], 2_000 + minutes_15 - 1, |_| ()).is_some());
        assert!(pending.take(&tokens[4], 2_000 + minutes_15).is_none());
        assert!(env.staging_is_empty());
    }

    #[tokio::test]
    async fn an_ungranted_program_is_refused_without_running() {
        let env = Env::new();
        env.install_from_index("0.3.1", &["kubectl"]).await;
        let marker = env.work.path().join("ran");
        env.fake("helm", &format!("touch '{}'", marker.display())).await;
        let (status, body) = env.run(ID, json!({ "program": "helm", "args": [], "cwd": null })).await;
        assert_eq!((status, body), (StatusCode::FORBIDDEN, json!({ "error": "not permitted" })));
        // A path to a granted name is not the granted name.
        let path = env.work.path().join("bin").join("helm");
        let (status, _) = env.run(ID, json!({ "program": path, "args": [], "cwd": null })).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!marker.exists(), "helm never ran");

        env.fake("kubectl", "printf '%s|' \"$@\"").await;
        let (status, body) = env.run(ID, json!({ "program": "kubectl", "args": ["get", "pods", "-n", "a b"], "cwd": null })).await;
        assert_eq!((status, body), (StatusCode::OK, json!({ "code": 0, "stdout": "get|pods|-n|a b|", "stderr": "" })));
    }

    #[tokio::test]
    async fn run_needs_the_plugin_installed() {
        let env = Env::new();
        env.fake("kubectl", "exit 0").await;
        let body = json!({ "program": "kubectl", "args": [], "cwd": null });
        assert_eq!(env.run(ID, body.clone()).await.0, StatusCode::NOT_FOUND);
        env.state.plugins.set_builtin(&["docker".into()]).unwrap();
        assert_eq!(env.run("docker", json!({ "program": "docker", "args": [], "cwd": null })).await.0, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn run_answers_bad_bodies_and_failures() {
        let env = Env::new();
        env.install_from_index("0.3.1", &["kubectl", "helm"]).await;
        env.fake("kubectl", "echo boom >&2\nexit 2").await;
        // Not JSON from the page, a bad shape, too big: 400.
        let ok = json!({ "program": "kubectl", "args": [], "cwd": null }).to_string();
        let send = |headers: HeaderMap, body: String| post_run(State(env.state.clone()), headers, Path(ID.into()), Bytes::from(body));
        assert_eq!(send(headers(false), ok.clone()).await.status(), StatusCode::BAD_REQUEST);
        let mut foreign = headers(true);
        foreign.insert(axum::http::header::ORIGIN, "https://evil.example".parse().unwrap());
        assert_eq!(send(foreign, ok.clone()).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(send(headers(true), json!({ "program": "kubectl", "args": "get" }).to_string()).await.status(), StatusCode::BAD_REQUEST);
        let big = json!({ "program": "kubectl", "args": ["x".repeat(65_536)], "cwd": null }).to_string();
        assert_eq!(send(headers(true), big).await.status(), StatusCode::BAD_REQUEST);
        // A program that fails still answers 200 with its code.
        let (status, body) = read(send(headers(true), ok).await).await;
        assert_eq!((status, body), (StatusCode::OK, json!({ "code": 2, "stdout": "", "stderr": "boom\n" })));
        // A granted program that is not on the PATH: 502.
        let (status, body) = env.run(ID, json!({ "program": "helm", "args": [] })).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(body["error"].as_str().unwrap().contains("helm"), "{body}");
        let response = send(headers(true), "{}".into()).await;
        assert_eq!(response.headers()["cache-control"], "no-store");
    }

    #[tokio::test]
    async fn a_fifth_run_is_refused() {
        let env = Env::new();
        env.install_from_index("0.3.1", &["kubectl"]).await;
        let gate = env.work.path().join("gate");
        env.fake("kubectl", &format!("while [ ! -e '{}' ]; do sleep 0.05; done", gate.display())).await;
        let body = || Bytes::from(json!({ "program": "kubectl", "args": [], "cwd": null }).to_string());
        let running: Vec<_> = (0..4)
            .map(|_| tokio::spawn(post_run(State(env.state.clone()), headers(true), Path(ID.into()), body())))
            .collect();
        let busy = || env.state.plugins.externals.runs.lock().unwrap().get(ID).is_some_and(|runs| runs.available_permits() == 0);
        for _ in 0..100 {
            if busy() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(busy(), "four runs are in flight");
        let (status, body5) = read(post_run(State(env.state.clone()), headers(true), Path(ID.into()), body()).await).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body5}");
        std::fs::write(&gate, "").unwrap();
        for run in running {
            assert_eq!(run.await.unwrap().status(), StatusCode::OK);
        }
        assert_eq!(post_run(State(env.state.clone()), headers(true), Path(ID.into()), body()).await.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_relative_cwd_is_refused() {
        let env = Env::new();
        env.install_from_index("0.3.1", &["kubectl"]).await;
        let marker = env.work.path().join("ran");
        env.fake("kubectl", &format!("touch '{}'\npwd -P", marker.display())).await;
        for cwd in ["work", "./work", ""] {
            let (status, _) = env.run(ID, json!({ "program": "kubectl", "args": [], "cwd": cwd })).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{cwd:?}");
        }
        let missing = env.work.path().join("missing");
        assert_eq!(env.run(ID, json!({ "program": "kubectl", "args": [], "cwd": missing })).await.0, StatusCode::BAD_REQUEST);
        assert!(!marker.exists());

        let (status, body) = env.run(ID, json!({ "program": "kubectl", "args": [], "cwd": null })).await;
        assert_eq!(status, StatusCode::OK);
        let home = crate::state_dir::home_dir().canonicalize().unwrap();
        assert_eq!(body["stdout"].as_str().unwrap().trim_end(), home.display().to_string(), "no cwd runs in the home folder");
        let (_, body) = env.run(ID, json!({ "program": "kubectl", "args": [], "cwd": env.work.path() })).await;
        assert_eq!(body["stdout"].as_str().unwrap().trim_end(), env.work.path().canonicalize().unwrap().display().to_string());
    }

    #[tokio::test]
    async fn storage_round_trips_and_stops_at_64_kib() {
        use std::os::unix::fs::PermissionsExt;
        let env = Env::new();
        assert_eq!(env.storage(ID).await.0, StatusCode::NOT_FOUND, "not installed");
        assert_eq!(env.set_storage(ID, r#"{"a":1}"#.into()).await.0, StatusCode::NOT_FOUND);
        env.install_from_index("0.3.1", &["kubectl"]).await;

        let response = get_storage(State(env.state.clone()), headers(false), Path(ID.into())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["content-type"], "application/json");
        assert_eq!(axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap(), "null");

        assert_eq!(env.set_storage(ID, r#"{"a":1}"#.into()).await, (StatusCode::NO_CONTENT, Value::Null));
        assert_eq!(env.storage(ID).await, (StatusCode::OK, json!({ "a": 1 })));
        let data = env.plugins().join(ID).join("data.json");
        assert_eq!(std::fs::metadata(&data).unwrap().permissions().mode() & 0o777, 0o600);

        let sized = |bytes: usize| format!(r#"{{"a":"{}"}}"#, "x".repeat(bytes - 8));
        assert_eq!(sized(65_536).len(), 65_536);
        assert_eq!(env.set_storage(ID, sized(65_536)).await.0, StatusCode::NO_CONTENT);
        assert_eq!(env.set_storage(ID, sized(65_537)).await.0, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(env.storage(ID).await.1["a"].as_str().unwrap().len(), 65_528, "the big value stays");
        assert_eq!(env.set_storage(ID, "[1,2]".into()).await.0, StatusCode::NO_CONTENT, "any JSON");
        assert_eq!(env.storage(ID).await.1, json!([1, 2]));
        assert_eq!(env.set_storage(ID, "{not json".into()).await.0, StatusCode::BAD_REQUEST);

        let mut cross = headers(false);
        cross.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert_eq!(get_storage(State(env.state.clone()), cross, Path(ID.into())).await.status(), StatusCode::FORBIDDEN);
        let mut foreign = headers(true);
        foreign.insert(axum::http::header::ORIGIN, "https://evil.example".parse().unwrap());
        let response = put_storage(State(env.state.clone()), foreign, Path(ID.into()), Bytes::from("{}")).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(env.storage(ID).await.1, json!([1, 2]));

        // Removing the plugin removes its storage.
        assert_eq!(env.delete(ID).await.0, StatusCode::NO_CONTENT);
        assert!(!data.exists());
    }

    #[tokio::test]
    async fn opening_a_browser_needs_panes() {
        let mut env = Env::new();
        assert_eq!(env.browser(ID, "http://localhost:3000").await.0, StatusCode::NOT_FOUND, "not installed");
        env.install_from_index("0.3.1", &["kubectl"]).await;
        assert_eq!(env.browser(ID, "http://localhost:3000/").await, (StatusCode::NO_CONTENT, Value::Null));
        assert_eq!(env.browser(ID, "http://127.1:4317/").await, (StatusCode::FORBIDDEN, json!({ "error": "this is westerm's own address" })));
        assert_eq!(env.browser(ID, "ftp://x").await, (StatusCode::BAD_REQUEST, json!({ "error": "only http and https" })));
        assert_eq!(env.browser(ID, "http://").await.0, StatusCode::BAD_REQUEST);

        // The user did not grant panes.
        let file_path = env.state_dir.path().join("plugins.json");
        let mut file: Value = serde_json::from_slice(&std::fs::read(&file_path).unwrap()).unwrap();
        file["external"][0]["granted"]["panes"] = json!(false);
        std::fs::write(&file_path, file.to_string()).unwrap();
        env.restart();
        assert_eq!(env.browser(ID, "http://localhost:3000").await, (StatusCode::FORBIDDEN, json!({ "error": "needs the panes permission" })));

        let mut foreign = headers(true);
        foreign.insert(axum::http::header::ORIGIN, "https://evil.example".parse().unwrap());
        let body = Bytes::from(json!({ "url": "http://localhost:3000" }).to_string());
        assert_eq!(post_browser(State(env.state.clone()), foreign, Path(ID.into()), body).await.status(), StatusCode::BAD_REQUEST);
    }
}
