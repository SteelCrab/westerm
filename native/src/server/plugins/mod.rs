//! `/api/plugins`: which plugins the user installed. The list lives in
//! `plugins.json` in the state folder and is also kept in memory, so a
//! session check does not read the disk. Built-in plugins are switched on
//! and off; an external plugin is installed with a record of what the user
//! allowed it, and removed again.

mod docker;
mod external;
pub(super) mod git;
mod login_env;

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use self::external::manifest::Manifest;
use self::external::store::{self, ExternalInfo, ExternalRecord, PluginsFile};
use super::{json_body, no_store, read_allowed, Shared};
use crate::plugins::{PluginLaunch, PLUGIN_IDS};
use crate::protocol::AttachRequest;

/// A `PUT /api/plugins` body is at most this many bytes.
const MAX_BODY_BYTES: usize = 4096;

/// The installed plugins, in memory and in `plugins.json`.
#[derive(Default)]
pub(super) struct PluginState {
    path: PathBuf,
    /// Where packages and plugin storage live.
    state_dir: PathBuf,
    /// What `plugins.json` holds. The record is the truth for "is this
    /// external plugin installed", even when its folder cannot be read.
    installed: RwLock<PluginsFile>,
    /// The `plugin.json` of every record whose folder could be read at start
    /// or when it was installed. A plugin without one is left out of lists.
    manifests: RwLock<HashMap<String, Manifest>>,
    /// The login shell's environment, which `docker` and `run` start from.
    login_env: login_env::LoginEnvCache,
    /// Fetched packages, the plugin list, and where plugins come from.
    externals: external::ExternalState,
}

/// The shape of the `PUT` body.
#[derive(Deserialize)]
struct InstalledBody {
    installed: Vec<String>,
}

/// The known built-in ids among `ids`, once each, in the order of `PLUGIN_IDS`.
fn known(ids: &[String]) -> Vec<&'static str> {
    PLUGIN_IDS.iter().copied().filter(|id| ids.iter().any(|wanted| wanted == id)).collect()
}

fn poisoned() -> io::Error {
    io::Error::other("the plugin list is locked by a failed thread")
}

impl PluginState {
    /// Reads `plugins.json`; a missing or broken file means only the
    /// default-on plugins. Then reads the `plugin.json` of each external plugin.
    pub(super) fn load(state_dir: &Path) -> Self {
        let path = state_dir.join("plugins.json");
        let file = std::fs::read(&path).map(|bytes| store::parse_plugins_file(&bytes)).unwrap_or_default();
        let manifests = file
            .external
            .iter()
            .filter_map(|record| {
                let root = store::package_dir(state_dir, record);
                let manifest = external::manifest::read(&root, crate::update::CURRENT).ok()?;
                Some((record.id.clone(), manifest))
            })
            .collect();
        Self {
            path,
            state_dir: state_dir.to_path_buf(),
            installed: RwLock::new(file),
            manifests: RwLock::new(manifests),
            login_env: login_env::LoginEnvCache::default(),
            externals: external::ExternalState::default(),
        }
    }

    /// Whether the plugin is on (built-in) or has a record (external).
    pub(super) fn is_installed(&self, id: &str) -> bool {
        self.installed.read().is_ok_and(|file| file.installed.iter().any(|on| on == id))
    }

    /// The ids the page may use: the built-in ones that are on, then the
    /// external ones whose package could be read.
    pub(super) fn installed(&self) -> Vec<String> {
        let (Ok(file), Ok(manifests)) = (self.installed.read(), self.manifests.read()) else { return Vec::new() };
        let external = file.external.iter().filter(|record| manifests.contains_key(&record.id)).map(|record| record.id.clone());
        file.builtin().into_iter().map(str::to_owned).chain(external).collect()
    }

    /// The record of an external plugin. It is found even when the package
    /// folder cannot be read.
    pub(super) fn external(&self, id: &str) -> Option<ExternalRecord> {
        self.installed.read().ok()?.external.iter().find(|record| record.id == id).cloned()
    }

    /// The external plugins whose package could be read, in record order.
    pub(super) fn external_infos(&self) -> Vec<ExternalInfo> {
        let (Ok(file), Ok(manifests)) = (self.installed.read(), self.manifests.read()) else { return Vec::new() };
        file.external.iter().filter_map(|record| Some(ExternalInfo::new(record, manifests.get(&record.id)?))).collect()
    }

    /// Whether the plugin may run `program`: only what the user granted, not
    /// what the package asks for now.
    pub(super) fn granted_run(&self, id: &str, program: &str) -> bool {
        self.external(id).is_some_and(|record| record.granted.run.iter().any(|name| name == program))
    }

    /// Whether the user let the plugin open panes.
    pub(super) fn granted_panes(&self, id: &str) -> bool {
        self.external(id).is_some_and(|record| record.granted.panes)
    }

    /// Replaces the built-in list, dropping unknown and external ids; records
    /// and installed external plugins stay. The file is written first, so a
    /// failed write leaves the memory as it was. Returns the built-in ids
    /// that are on.
    pub(super) fn set_builtin(&self, ids: &[String]) -> io::Result<Vec<String>> {
        let builtin = known(ids);
        let mut current = self.installed.write().map_err(|_| poisoned())?;
        let next = PluginsFile::new(&builtin, current.external.clone());
        self.write_file(&next)?;
        *current = next;
        Ok(builtin.into_iter().map(str::to_owned).collect())
    }

    /// Adds a record, or replaces the one with the same id (also one whose
    /// folder cannot be read). `manifest` is the package's `plugin.json`.
    pub(super) fn put_external(&self, record: ExternalRecord, manifest: Manifest) -> io::Result<()> {
        if record.id != manifest.id || !crate::plugins::external::is_external_id(&record.id) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "the record and plugin.json must have the same external id"));
        }
        let mut current = self.installed.write().map_err(|_| poisoned())?;
        let mut external = current.external.clone();
        match external.iter_mut().find(|known| known.id == record.id) {
            Some(slot) => slot.clone_from(&record),
            None => external.push(record.clone()),
        }
        let next = PluginsFile::new(&current.builtin(), external);
        self.write_file(&next)?;
        *current = next;
        if let Ok(mut manifests) = self.manifests.write() {
            manifests.insert(record.id, manifest);
        }
        Ok(())
    }

    /// Forgets a record. `None` when there was none. The package folder and
    /// storage are the caller's to delete.
    pub(super) fn remove_external(&self, id: &str) -> io::Result<Option<ExternalRecord>> {
        let mut current = self.installed.write().map_err(|_| poisoned())?;
        let Some(at) = current.external.iter().position(|record| record.id == id) else { return Ok(None) };
        let mut external = current.external.clone();
        let removed = external.remove(at);
        let next = PluginsFile::new(&current.builtin(), external);
        self.write_file(&next)?;
        *current = next;
        if let Ok(mut manifests) = self.manifests.write() {
            manifests.remove(id);
        }
        Ok(Some(removed))
    }

    /// Writes `plugins.json` through a temp file, so a crash leaves the old one.
    fn write_file(&self, file: &PluginsFile) -> io::Result<()> {
        let temp = self.path.with_extension("json.tmp");
        let body = serde_json::to_vec(file).map_err(io::Error::other)?;
        crate::state_dir::private_file()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp)
            .and_then(|mut out| out.write_all(&body))
            .and_then(|()| std::fs::rename(&temp, &self.path))
    }
}

/// Deletes packages that were half installed when the server last stopped.
pub(super) fn clear_staging(state_dir: &Path) {
    let _ = std::fs::remove_dir_all(store::staging_dir(state_dir));
}

/// A session that starts a plugin needs that plugin installed. An external
/// launch also needs the `panes` grant and its program in what the user
/// granted (the page checks both too).
pub(super) fn session_allowed(request: &AttachRequest, plugins: &PluginState) -> bool {
    request.plugin.as_ref().is_none_or(|launch| {
        plugins.is_installed(launch.plugin())
            && match launch {
                PluginLaunch::External(launch) => {
                    plugins.granted_panes(&launch.id)
                        && launch.argv.first().is_some_and(|program| plugins.granted_run(&launch.id, program))
                }
                PluginLaunch::Docker(_) => true,
            }
    })
}

pub(super) fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/plugins", get(get_plugins).put(put_plugins))
        .merge(git::routes())
        .merge(docker::routes())
        .merge(external::routes())
}

/// Starts the daily check for newer plugin versions.
pub(super) fn spawn_daily_check(state: Shared) {
    external::catalog::spawn_daily_check(state);
}

async fn get_plugins(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    no_store(Json(json!({
        "installed": state.plugins.installed(),
        "external": state.plugins.external_infos(),
        "shellPanes": shell_panes(crate::spawn::shell_kind()),
    })))
}

/// Whether a plugin's `openShell` can open a pane: only on macOS and Linux,
/// with a login shell westerm can quote for (sh-family or fish). The page
/// refuses the call without opening a pane otherwise.
fn shell_panes(shell: Option<crate::quote::ShellKind>) -> bool {
    cfg!(unix) && shell.is_some()
}

/// Replaces the built-in list; external ids in the body are ignored. `json_body`
/// checks the Origin and the JSON type.
async fn put_plugins(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    if body.len() > MAX_BODY_BYTES {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(InstalledBody { installed }) = json_body(&state, &headers, &body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let had_docker = state.plugins.is_installed("docker");
    match state.plugins.set_builtin(&installed) {
        Ok(_) => {
            // Installing reads the login shell's environment again.
            if !had_docker && state.plugins.is_installed("docker") {
                state.plugins.login_env.reset().await;
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => {
            eprintln!("> plugins.json: {error}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::tests::test_state;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn installed_lists_drop_unknown_ids_and_broken_files() {
        let read = |body: &[u8]| external::store::parse_plugins_file(body).installed;
        // Git is on by default, so every list starts with it.
        assert_eq!(read(br#"{"installed":["docker","nope"]}"#), vec!["git", "docker"]);
        assert_eq!(read(br#"{"installed":["docker","docker"]}"#), vec!["git", "docker"]);
        assert_eq!(read(b"{not json"), vec!["git"]);
        assert_eq!(read(b""), vec!["git"]);
        assert_eq!(read(br#"{"installed":"docker"}"#), vec!["git"]);
    }

    #[test]
    fn installing_writes_the_file_and_memory() {
        let dir = tempfile::tempdir().unwrap();
        let state = PluginState::load(dir.path());
        assert!(!state.is_installed("docker"));
        let kept = state.set_builtin(&["git".into(), "docker".into(), "nope".into()]).unwrap();
        assert_eq!(kept, vec!["git", "docker"]);
        assert!(state.is_installed("docker"));
        assert_eq!(state.installed(), vec!["git", "docker"]);
        assert!(PluginState::load(dir.path()).is_installed("docker"), "read back from plugins.json");
        assert_eq!(std::fs::read_to_string(dir.path().join("plugins.json")).unwrap(), r#"{"installed":["git","docker"]}"#);
        state.set_builtin(&["git".into()]).unwrap();
        assert!(!state.is_installed("docker"));
        assert!(!PluginState::load(dir.path()).is_installed("docker"));
    }

    #[test]
    fn git_is_installed_until_the_user_turns_it_off() {
        let dir = tempfile::tempdir().unwrap();
        let state = PluginState::load(dir.path());
        assert_eq!(state.installed(), vec!["git"], "no plugins.json yet");
        assert!(!dir.path().join("plugins.json").exists(), "reading the default writes nothing");

        // A list without Git turns it off, and the file remembers that.
        state.set_builtin(&["docker".into()]).unwrap();
        assert!(!state.is_installed("git"));
        assert_eq!(std::fs::read_to_string(dir.path().join("plugins.json")).unwrap(), r#"{"installed":["docker"],"off":["git"]}"#);
        assert_eq!(PluginState::load(dir.path()).installed(), vec!["docker"], "it stays off after a restart");

        state.set_builtin(&["git".into(), "docker".into()]).unwrap();
        assert_eq!(PluginState::load(dir.path()).installed(), vec!["git", "docker"]);

        // A file written before the Git plugin existed keeps Git on.
        std::fs::write(dir.path().join("plugins.json"), r#"{"installed":["docker"]}"#).unwrap();
        assert_eq!(PluginState::load(dir.path()).installed(), vec!["git", "docker"]);
    }

    #[cfg(unix)]
    #[test]
    fn the_list_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        PluginState::load(dir.path()).set_builtin(&["docker".into()]).unwrap();
        let mode = std::fs::metadata(dir.path().join("plugins.json")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    fn request(extra: &[(&str, &str)]) -> AttachRequest {
        let query: HashMap<String, String> = [("session", "sabc1"), ("kind", "shell"), ("cols", "80"), ("rows", "24"), ("width", "640"), ("height", "408")]
            .into_iter()
            .chain(extra.iter().copied())
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        AttachRequest::from_query(&query).unwrap()
    }

    #[test]
    fn a_plugin_session_needs_the_plugin_installed() {
        let dir = tempfile::tempdir().unwrap();
        let state = PluginState::load(dir.path());
        let launch = request(&[("plugin", "docker"), ("action", "logs"), ("target", "0123456789ab")]);
        assert!(launch.plugin.is_some());
        assert!(!session_allowed(&launch, &state));
        state.set_builtin(&["docker".into()]).unwrap();
        assert!(session_allowed(&launch, &state));
        let plain = request(&[]);
        state.set_builtin(&[]).unwrap();
        assert!(session_allowed(&plain, &state), "a request without a plugin is always allowed");
    }

    #[test]
    fn an_external_plugin_session_needs_it_installed() {
        let dir = tempfile::tempdir().unwrap();
        let state = PluginState::load(dir.path());
        state.set_builtin(&["docker".into()]).unwrap();
        let launch = request(&[("plugin", "k8s-tools.kubernetes"), ("action", "run"), ("target", r#"["kubectl","get","pods"]"#)]);
        assert!(launch.plugin.is_some());
        assert!(!session_allowed(&launch, &state), "a plugin without a record is refused");
    }

    fn shared(dir: &Path) -> Shared {
        let mut state = test_state(&["http://127.0.0.1:4317"]);
        state.plugins = std::sync::Arc::new(PluginState::load(dir));
        Arc::new(state)
    }

    fn page_headers(json: bool) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(axum::http::header::HOST, "127.0.0.1:4317".parse().unwrap());
        headers.insert(axum::http::header::ORIGIN, "http://127.0.0.1:4317".parse().unwrap());
        if json {
            headers.insert(axum::http::header::CONTENT_TYPE, "application/json".parse().unwrap());
        }
        headers
    }

    async fn body_text(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    const K8S_JSON: &str = r#"{"id":"k8s-tools.kubernetes","name":"Kubernetes","version":"0.3.1","description":"See pods.","panel":"panel.html","icon":"icon.svg","permissions":{"run":["kubectl","helm"],"panes":true,"background":true}}"#;
    const K8S: &str = "k8s-tools.kubernetes";

    /// Unpacks the Kubernetes test package under `state_dir` and returns its
    /// record (granted: `run`) and manifest.
    fn k8s_package(state_dir: &Path, run: &[&str]) -> (external::store::ExternalRecord, external::manifest::Manifest) {
        use external::store::{ExternalRecord, Source, SourceKind};
        let record = ExternalRecord {
            id: K8S.into(),
            version: "0.3.1".into(),
            source: Source { kind: SourceKind::Index, url: Some("https://example.test/k8s.tar.gz".into()), path: None },
            sha256: Some("ab12".into()),
            granted: external::manifest::Permissions { run: run.iter().map(|name| (*name).to_owned()).collect(), panes: true, background: true },
            installed_at: 1_794_139_521_000,
        };
        let root = external::store::package_dir(state_dir, &record);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("plugin.json"), K8S_JSON).unwrap();
        std::fs::write(root.join("panel.html"), "<p>panel</p>").unwrap();
        std::fs::write(root.join("icon.svg"), "<svg/>").unwrap();
        let manifest = external::manifest::read(&root, "0.4.0").unwrap();
        (record, manifest)
    }

    fn with_k8s(dir: &Path, run: &[&str]) -> PluginState {
        let state = PluginState::load(dir);
        let (record, manifest) = k8s_package(dir, run);
        state.put_external(record, manifest).unwrap();
        state
    }

    #[test]
    fn plugin_panes_need_a_shell_westerm_can_quote_for() {
        use crate::quote::ShellKind;
        assert_eq!(shell_panes(Some(ShellKind::Sh)), cfg!(unix));
        assert_eq!(shell_panes(Some(ShellKind::Fish)), cfg!(unix));
        assert!(!shell_panes(None), "nu, pwsh, Windows");
        assert!(!shell_panes(ShellKind::from_path("/usr/bin/nu")));
    }

    #[test]
    fn granted_decides_what_runs() {
        let dir = tempfile::tempdir().unwrap();
        let state = with_k8s(dir.path(), &["kubectl"]);
        // The package's plugin.json asks for ["kubectl","helm"]; the user granted only kubectl.
        assert!(state.granted_run(K8S, "kubectl"));
        assert!(!state.granted_run(K8S, "helm"), "plugin.json cannot widen granted");
        assert!(!state.granted_run("docker", "docker"));
        assert!(!state.granted_run("a.b", "kubectl"), "no record, no grant");
        let again = PluginState::load(dir.path());
        assert!(again.granted_run(K8S, "kubectl") && !again.granted_run(K8S, "helm"), "granted comes back from plugins.json");
    }

    #[test]
    fn an_external_session_needs_the_granted_program() {
        let dir = tempfile::tempdir().unwrap();
        let state = PluginState::load(dir.path());
        let get = request(&[("plugin", K8S), ("action", "run"), ("target", r#"["kubectl","get"]"#)]);
        let helm = request(&[("plugin", K8S), ("action", "run"), ("target", r#"["helm"]"#)]);
        assert!(!session_allowed(&get, &state), "not installed yet");
        let (record, manifest) = k8s_package(dir.path(), &["kubectl"]);
        state.put_external(record, manifest).unwrap();
        assert!(session_allowed(&get, &state));
        assert!(!session_allowed(&helm, &state), "helm is not granted");
        assert!(state.remove_external(K8S).unwrap().is_some());
        assert!(!session_allowed(&get, &state), "refused after the plugin is removed");
    }

    #[test]
    fn an_external_session_needs_the_panes_grant() {
        let dir = tempfile::tempdir().unwrap();
        let state = PluginState::load(dir.path());
        let (mut record, manifest) = k8s_package(dir.path(), &["kubectl"]);
        record.granted.panes = false;
        state.put_external(record, manifest).unwrap();
        let get = request(&[("plugin", K8S), ("action", "run"), ("target", r#"["kubectl","get"]"#)]);
        assert!(!session_allowed(&get, &state), "a plugin without panes opens no pane, even with run granted");
    }

    #[test]
    fn a_builtin_change_keeps_external_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let state = with_k8s(dir.path(), &["kubectl"]);
        assert_eq!(state.set_builtin(&["docker".into(), "x.y".into(), K8S.into()]).unwrap(), vec!["docker"]);
        assert_eq!(state.installed(), vec!["docker", K8S]);
        state.set_builtin(&[]).unwrap();
        assert_eq!(state.installed(), vec![K8S], "turning Docker off keeps the external plugin");
        let again = PluginState::load(dir.path());
        assert_eq!(again.installed(), vec![K8S]);
        assert_eq!(again.external(K8S).unwrap().granted.run, vec!["kubectl"]);
        let file: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.path().join("plugins.json")).unwrap()).unwrap();
        assert_eq!(file["installed"], json!([K8S]));
        assert_eq!(file["external"][0]["id"], K8S);
    }

    #[tokio::test]
    async fn a_builtin_put_keeps_external_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path());
        let (record, manifest) = k8s_package(dir.path(), &["kubectl"]);
        state.plugins.put_external(record, manifest).unwrap();
        let put = |body: &str| put_plugins(State(state.clone()), page_headers(true), Bytes::from(body.to_owned()));
        let read_file = || serde_json::from_slice::<serde_json::Value>(&std::fs::read(dir.path().join("plugins.json")).unwrap()).unwrap();

        assert_eq!(put(r#"{"installed":[]}"#).await.status(), StatusCode::NO_CONTENT);
        let file = read_file();
        assert_eq!(file["installed"], json!([K8S]));
        assert_eq!(file["external"][0]["id"], K8S);
        assert_eq!(file["external"][0]["granted"]["run"], json!(["kubectl"]));

        assert_eq!(put(r#"{"installed":["docker","x.y"]}"#).await.status(), StatusCode::NO_CONTENT);
        assert_eq!(read_file()["installed"], json!(["docker", K8S]));
        assert_eq!(state.plugins.installed(), vec!["docker", K8S]);

        // An external id alone in the body changes nothing and does not remove the record.
        assert_eq!(put(r#"{"installed":["k8s-tools.kubernetes"]}"#).await.status(), StatusCode::NO_CONTENT);
        assert_eq!(read_file()["installed"], json!([K8S]));
        assert!(state.plugins.external(K8S).is_some());
    }

    #[tokio::test]
    async fn the_list_route_carries_external_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path());
        let (record, manifest) = k8s_package(dir.path(), &["kubectl"]);
        state.plugins.put_external(record.clone(), manifest).unwrap();
        let list = || async { serde_json::from_str::<serde_json::Value>(&body_text(get_plugins(State(state.clone()), page_headers(false)).await).await).unwrap() };

        assert_eq!(
            list().await,
            json!({
                "installed": ["git", K8S],
                "external": [{
                    "id": K8S, "version": "0.3.1", "name": "Kubernetes", "description": "See pods.",
                    "i18n": {}, "icon": "icon.svg", "panel": "panel.html",
                    "source": {"kind": "index", "url": "https://example.test/k8s.tar.gz"},
                    "granted": {"run": ["kubectl"], "panes": true, "background": true},
                    "installedAt": 1_794_139_521_000u64
                }],
                "shellPanes": shell_panes(crate::spawn::shell_kind())
            })
        );

        // A plugin without an icon sends null.
        let root = external::store::package_dir(dir.path(), &record);
        std::fs::write(root.join("plugin.json"), K8S_JSON.replace(r#""icon":"icon.svg","#, "")).unwrap();
        let reloaded = shared(dir.path());
        let body: serde_json::Value = serde_json::from_str(&body_text(get_plugins(State(reloaded), page_headers(false)).await).await).unwrap();
        assert_eq!(body["external"][0]["icon"], serde_json::Value::Null);

        // A record whose folder is gone is left out of both lists, but the file keeps it.
        std::fs::remove_dir_all(&root).unwrap();
        let gone = shared(dir.path());
        let body: serde_json::Value = serde_json::from_str(&body_text(get_plugins(State(gone.clone()), page_headers(false)).await).await).unwrap();
        assert_eq!(body["installed"], json!(["git"]));
        assert_eq!(body["external"], json!([]));
        assert!(gone.plugins.external(K8S).is_some());
    }

    #[test]
    fn the_record_is_found_when_the_folder_is_unreadable_and_a_new_install_replaces_it() {
        let dir = tempfile::tempdir().unwrap();
        let state = with_k8s(dir.path(), &["kubectl"]);
        std::fs::remove_dir_all(dir.path().join("plugins").join(K8S)).unwrap();
        let broken = PluginState::load(dir.path());
        assert!(broken.external_infos().is_empty());
        assert_eq!(broken.installed(), vec!["git"]);
        assert_eq!(broken.external(K8S).unwrap().version, "0.3.1", "the record is the truth");
        assert!(broken.is_installed(K8S));

        let (mut record, manifest) = k8s_package(dir.path(), &["helm"]);
        record.installed_at += 1;
        broken.put_external(record, manifest).unwrap();
        assert_eq!(broken.installed(), vec!["git", K8S]);
        assert_eq!(broken.external_infos().len(), 1);
        assert_eq!(broken.external(K8S).unwrap().granted.run, vec!["helm"]);
        assert_eq!(PluginState::load(dir.path()).external(K8S).unwrap().installed_at, 1_794_139_521_001);
        drop(state);
    }

    #[test]
    fn a_record_and_a_manifest_must_agree_on_the_id() {
        let dir = tempfile::tempdir().unwrap();
        let state = PluginState::load(dir.path());
        let (mut record, manifest) = k8s_package(dir.path(), &["kubectl"]);
        record.id = "other.plugin".into();
        assert_eq!(state.put_external(record, manifest).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert_eq!(state.installed(), vec!["git"]);
        assert!(!dir.path().join("plugins.json").exists());
    }

    #[test]
    fn removing_forgets_the_record() {
        let dir = tempfile::tempdir().unwrap();
        let state = with_k8s(dir.path(), &["kubectl"]);
        state.set_builtin(&["docker".into()]).unwrap();
        assert!(state.remove_external("a.b").unwrap().is_none(), "an unknown id changes nothing");
        let removed = state.remove_external(K8S).unwrap().unwrap();
        assert_eq!(removed.id, K8S);
        assert_eq!(state.installed(), vec!["docker"]);
        assert!(state.external(K8S).is_none() && state.external_infos().is_empty());
        let again = PluginState::load(dir.path());
        assert_eq!(again.installed(), vec!["docker"]);
        assert!(again.external(K8S).is_none());
    }

    #[test]
    fn a_failed_write_leaves_the_memory_alone() {
        let dir = tempfile::tempdir().unwrap();
        let state = with_k8s(dir.path(), &["kubectl"]);
        // A folder in the way of the temp file makes the write fail.
        std::fs::create_dir(dir.path().join("plugins.json.tmp")).unwrap();
        assert!(state.set_builtin(&["docker".into()]).is_err());
        assert!(state.remove_external(K8S).is_err());
        assert_eq!(state.installed(), vec!["git", K8S]);
        assert!(state.external(K8S).is_some());
    }

    #[test]
    fn starting_clears_half_installed_packages() {
        let dir = tempfile::tempdir().unwrap();
        let staging = external::store::staging_dir(dir.path());
        std::fs::create_dir_all(staging.join("abc")).unwrap();
        std::fs::write(staging.join("abc").join("plugin.json"), "{}").unwrap();
        clear_staging(dir.path());
        assert!(!staging.exists());
        clear_staging(dir.path());
    }

    #[tokio::test]
    async fn the_routes_read_and_replace_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path());
        let put = |headers: HeaderMap, body: &str| put_plugins(State(state.clone()), headers, Bytes::from(body.to_owned()));

        let response = get_plugins(State(state.clone()), page_headers(false)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let panes = shell_panes(crate::spawn::shell_kind());
        assert_eq!(body_text(response).await, format!(r#"{{"external":[],"installed":["git"],"shellPanes":{panes}}}"#));

        let response = put(page_headers(true), r#"{"installed":["docker","nope"]}"#).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(body_text(response).await.is_empty());
        assert!(state.plugins.is_installed("docker"));
        assert_eq!(
            body_text(get_plugins(State(state.clone()), page_headers(false)).await).await,
            format!(r#"{{"external":[],"installed":["docker"],"shellPanes":{panes}}}"#)
        );

        // Refused: not JSON, bad shape, too big, wrong Origin. The list stays.
        assert_eq!(put(page_headers(false), r#"{"installed":[]}"#).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(put(page_headers(true), "[]").await.status(), StatusCode::BAD_REQUEST);
        let big = format!(r#"{{"installed":[],"pad":"{}"}}"#, "x".repeat(MAX_BODY_BYTES));
        assert_eq!(put(page_headers(true), &big).await.status(), StatusCode::BAD_REQUEST);
        let mut foreign = page_headers(true);
        foreign.insert(axum::http::header::ORIGIN, "https://evil.example".parse().unwrap());
        assert_eq!(put(foreign, r#"{"installed":[]}"#).await.status(), StatusCode::BAD_REQUEST);
        assert!(state.plugins.is_installed("docker"));

        let mut cross = page_headers(false);
        cross.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert_eq!(get_plugins(State(state), cross).await.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn installing_docker_reads_its_environment_again() {
        let dir = tempfile::tempdir().unwrap();
        let state = shared(dir.path());
        let env = |path: &str| login_env::LoginEnv { vars: vec![("PATH".into(), path.into())] };
        let put = |ids: &str| put_plugins(State(state.clone()), page_headers(true), Bytes::from(format!(r#"{{"installed":{ids}}}"#)));

        state.plugins.login_env.get_or_load(|| async { env("/one") }).await;
        assert_eq!(put(r#"["docker"]"#).await.status(), StatusCode::NO_CONTENT);
        assert_eq!(state.plugins.login_env.get_or_load(|| async { env("/two") }).await, env("/two"), "install forgot it");

        assert_eq!(put(r#"["docker"]"#).await.status(), StatusCode::NO_CONTENT);
        assert_eq!(state.plugins.login_env.get_or_load(|| async { env("/three") }).await, env("/two"), "saving it again keeps it");
    }
}
