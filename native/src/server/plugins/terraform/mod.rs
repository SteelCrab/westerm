//! The Terraform plugin's server side: the projects in the session folders,
//! their modules and resources, runs, and plans. Every route answers 404
//! while the plugin is not installed.

pub(super) mod tree;
pub(super) mod workspaces;
pub(super) mod plan;
pub(super) mod registry;
pub(super) mod runs;
pub(super) mod scan;
pub(super) mod settings;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path as UrlPath, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use self::runs::Runs;
use self::scan::{Project, ScanEnv, ScanLimits, Tools};
use self::settings::Settings;
use super::super::{json_body, no_store, origin_allowed, read_allowed, Shared};
use super::login_env::{self, find_program, LoginEnv};
use crate::plugins::terraform::{root_dir, Action, Tool};
use crate::time::now_millis;

/// The plugin's id.
pub(super) const ID: &str = crate::plugins::terraform::ID;
/// A write body is at most this many bytes.
const MAX_BODY_BYTES: usize = 4096;
/// The page names at most this many folders to scan.
const MAX_FOLDERS: usize = 32;
/// How long `version -json` may take.
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// The projects of the last scan.
#[derive(Default)]
struct Found {
    projects: Vec<Project>,
    truncated: bool,
}

/// What the Terraform plugin keeps while the server runs.
#[derive(Default)]
pub(in crate::server) struct TerraformState {
    /// Where the plugin's files go; empty only in tests that never write one.
    state_dir: PathBuf,
    found: RwLock<Found>,
    settings: RwLock<Settings>,
    /// The version each project's tool gave, asked once.
    versions: tokio::sync::Mutex<HashMap<(String, Tool), Option<String>>>,
    /// The runs and the plan of each project.
    runs: Runs,
    /// What the public registries said about module versions.
    registry: registry::Cache,
    /// The workspaces each project's tool listed last; only these can be selected.
    listed: std::sync::Mutex<HashMap<String, Vec<String>>>,
}

impl TerraformState {
    /// Reads the settings. It removes nothing: the dashboard of a remote
    /// window loads the same state folder while the server runs.
    pub(in crate::server) fn load(state_dir: &Path) -> Self {
        Self { state_dir: state_dir.to_path_buf(), settings: RwLock::new(settings::load(state_dir)), runs: Runs::new(state_dir), ..Self::default() }
    }

    /// The server starts: runs of an earlier server are not waited for, a plan
    /// that was half way is closed, and old plans go.
    pub(in crate::server) fn on_server_start(&self) {
        Runs::clean_at_start(&self.state_dir, now_millis());
    }

    /// The plugin was uninstalled: its folder and what it remembers go.
    pub(in crate::server) async fn on_uninstall(&self) {
        let _ = std::fs::remove_dir_all(root_dir(&self.state_dir));
        if let Ok(mut found) = self.found.write() {
            *found = Found::default();
        }
        if let Ok(mut settings) = self.settings.write() {
            *settings = Settings::default();
        }
        self.versions.lock().await.clear();
        if let Ok(mut listed) = self.listed.lock() {
            listed.clear();
        }
    }

    /// The project with this id, when the last scan found it.
    pub(super) fn project(&self, id: &str) -> Option<Project> {
        self.found.read().ok()?.projects.iter().find(|project| project.id == id).cloned()
    }

    pub(super) fn settings(&self) -> Settings {
        self.settings.read().map(|settings| settings.clone()).unwrap_or_default()
    }

    /// Changes the settings and saves them.
    fn change_settings(&self, change: impl FnOnce(&mut Settings)) -> std::io::Result<()> {
        let mut settings = self.settings.write().map_err(|_| std::io::Error::other("the settings are locked by a failed thread"))?;
        change(&mut settings);
        settings::save(&self.state_dir, &settings)
    }
}

/// None when a read may go on; otherwise the answer.
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

/// Reads what the runners wrote since the last request. Off the async
/// threads: a plan's JSON can be large.
async fn settle(state: &Shared) {
    let state = state.clone();
    let _ = tokio::task::spawn_blocking(move || state.plugins.terraform.runs.settle(now_millis())).await;
}

/// The JSON body of a write (Origin and type checked, at most `MAX_BODY_BYTES`), or the status to answer.
fn write_body<T: serde::de::DeserializeOwned>(state: &Shared, headers: &HeaderMap, body: &Bytes) -> Result<T, StatusCode> {
    if !state.plugins.is_installed(ID) {
        return Err(StatusCode::NOT_FOUND);
    }
    if body.len() > MAX_BODY_BYTES || !origin_allowed(state, headers) {
        return Err(StatusCode::BAD_REQUEST);
    }
    json_body(state, headers, body).ok_or(StatusCode::BAD_REQUEST)
}

/// The answer for a project id the last scan did not find.
fn removed() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "removed": true }))).into_response()
}

/// A refusal with its code for the page.
fn refused(status: StatusCode, error: &str) -> Response {
    (status, Json(json!({ "error": error }))).into_response()
}

async fn login(state: &Shared) -> LoginEnv {
    state.plugins.login_env.get_or_load(login_env::load).await
}

/// The last value of `key` in the login environment, unless it is empty.
fn var(env: &LoginEnv, key: &str) -> Option<String> {
    env.vars.iter().rev().find(|(name, _)| name == key).map(|(_, value)| value.clone()).filter(|value| !value.is_empty())
}

fn tools(env: &LoginEnv) -> Tools {
    Tools { terraform: find_program(env, Tool::Terraform.program()).is_some(), tofu: find_program(env, Tool::Tofu.program()).is_some() }
}

fn scan_env(state: &Shared, env: &LoginEnv) -> ScanEnv {
    ScanEnv {
        home: var(env, "HOME").map_or_else(crate::state_dir::home_dir, PathBuf::from),
        tools: tools(env),
        tf_workspace: var(env, "TF_WORKSPACE"),
        tf_data_dir: var(env, "TF_DATA_DIR"),
        overrides: state.plugins.terraform.settings().tools,
    }
}

/// What `program` prints for `args` in `cwd`, with the login environment.
/// Err: the first line of what it wrote to stderr, or why it did not run.
pub(super) async fn capture(program: &Path, args: &[&str], cwd: &Path, env: &LoginEnv, timeout: Duration) -> Result<String, String> {
    let mut command = tokio::process::Command::new(program);
    command.args(args).current_dir(cwd).env_clear().envs(env.vars.iter().map(|(key, value)| (key, value))).kill_on_drop(true).stdin(std::process::Stdio::null());
    let output = match tokio::time::timeout(timeout, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return Err(error.to_string()),
        Err(_) => return Err("Timed out".to_owned()),
    };
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Err(error_line(&String::from_utf8_lossy(&output.stderr)))
}

/// The line of a tool's error output that says what went wrong: the first
/// with "Error", else the first with any text. Colors and the box the tool
/// draws around its errors are taken off.
fn error_line(stderr: &str) -> String {
    let mut plain = String::new();
    let mut chars = stderr.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // An escape sequence ends at its first letter.
            chars.by_ref().find(char::is_ascii_alphabetic);
        } else {
            plain.push(c);
        }
    }
    let lines: Vec<&str> = plain.lines().map(|line| line.trim_matches(|c: char| c.is_whitespace() || matches!(c, '│' | '╷' | '╵'))).filter(|line| !line.is_empty()).collect();
    lines.iter().find(|line| line.contains("Error")).or(lines.first()).map_or_else(|| "The command failed".to_owned(), |line| (*line).to_owned())
}

/// A project as the page reads it.
fn project_json(project: &Project, run: serde_json::Value, plan: serde_json::Value) -> serde_json::Value {
    json!({
        "id": project.id,
        "path": project.path,
        "name": project.name,
        "sub": project.sub,
        "tool": project.tool,
        "toolFound": project.tool_found,
        "workspace": project.workspace,
        "workspaceLocked": project.workspace_locked,
        "remote": project.remote,
        "needsInit": project.needs_init,
        "unreadable": project.unreadable,
        "run": run,
        "plan": plan,
    })
}

/// The absolute folders the query names with `folder=`, and whether it asks for a scan.
fn state_query(query: &str) -> (Vec<PathBuf>, bool) {
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
    let folders = pairs
        .iter()
        .filter(|(key, _)| key == "folder")
        .take(MAX_FOLDERS)
        .filter(|(_, value)| !value.is_empty() && !value.chars().any(char::is_control))
        .map(|(_, value)| PathBuf::from(value))
        .filter(|folder| folder.is_absolute())
        .collect();
    (folders, pairs.iter().any(|(key, value)| key == "scan" && value == "1"))
}

/// The tools, the settings, and the projects. `scan=1` walks the folders the
/// page names; without it the projects of the last scan are read again where
/// a run changes them (the workspace, the data folder, the tool).
async fn get_state(State(state): State<Shared>, headers: HeaderMap, RawQuery(query): RawQuery) -> Response {
    if let Some(refused) = refuse_read(&state, &headers) {
        return refused;
    }
    let (folders, scan) = state_query(&query.unwrap_or_default());
    settle(&state).await;
    let env = login(&state).await;
    let scan_env = scan_env(&state, &env);
    let terraform = &state.plugins.terraform;
    if scan {
        let walk_env = scan_env.clone();
        let scanned = tokio::task::spawn_blocking(move || scan::scan(&folders, &walk_env, &ScanLimits::DEFAULT)).await;
        if let (Ok((projects, truncated)), Ok(mut found)) = (scanned, terraform.found.write()) {
            *found = Found { projects, truncated };
        }
    } else if let Ok(mut found) = terraform.found.write() {
        for project in &mut found.projects {
            (project.tool, project.tool_found) = scan::pick_tool(&project.path, &scan_env.tools, scan_env.overrides.get(&project.path).copied());
            (project.workspace, project.workspace_locked) = scan::workspace_of(&project.data_dir, scan_env.tf_workspace.as_deref());
            project.needs_init = !project.data_dir.is_dir();
        }
    }
    let (projects, truncated) = terraform.found.read().map(|found| (found.projects.clone(), found.truncated)).unwrap_or_default();
    let runs = &terraform.runs;
    let projects: Vec<serde_json::Value> = projects.iter().map(|project| project_json(project, json!(runs.run_info(&project.id)), json!(runs.plan_info(project)))).collect();
    no_store(Json(json!({
        "tools": scan_env.tools,
        "shellPanes": super::shell_panes(crate::spawn::shell_kind()),
        "checkVersions": terraform.settings().check_versions,
        "projects": projects,
        "truncated": truncated,
    })))
}

/// The `terraform_version` of `version -json`.
fn version_of(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    Some(value.get("terraform_version")?.as_str()?.to_owned())
}

/// The version of the project's tool, run in the project's folder (a version
/// manager picks by folder). Asked once for each project and tool.
async fn version(state: &Shared, project: &Project, env: &LoginEnv) -> Option<String> {
    let mut versions = state.plugins.terraform.versions.lock().await;
    let key = (project.id.clone(), project.tool);
    if let Some(known) = versions.get(&key) {
        return known.clone();
    }
    let program = find_program(env, project.tool.program())?;
    let found = capture(&program, &["version", "-json"], &project.path, env, VERSION_TIMEOUT).await.ok().and_then(|text| version_of(&text));
    versions.insert(key, found.clone());
    found
}

/// The project's tree as its code says now.
async fn tree_of(project: &Project) -> Option<tree::Tree> {
    let (path, data_dir, tool) = (project.path.clone(), project.data_dir.clone(), project.tool);
    tokio::task::spawn_blocking(move || tree::build(&path, &data_dir, tool, &tree::Limits::DEFAULT)).await.ok()
}

/// A project's modules and resources, as its code says.
async fn get_tree(State(state): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<String>) -> Response {
    if let Some(refused) = refuse_read(&state, &headers) {
        return refused;
    }
    let Some(project) = state.plugins.terraform.project(&id) else { return removed() };
    settle(&state).await;
    let env = login(&state).await;
    let Some(mut tree) = tree_of(&project).await else { return StatusCode::INTERNAL_SERVER_ERROR.into_response() };
    if state.plugins.terraform.settings().check_versions {
        // What is known shows now; what is due is asked behind this answer and shows in a later one.
        let (newer, ask) = state.plugins.terraform.registry.known(project.tool, &tree.registry, now_millis()).await;
        registry::fill_newer(&mut tree.nodes, &newer);
        if !ask.is_empty() {
            let state = state.clone();
            tokio::spawn(async move { state.plugins.terraform.registry.refresh(ask, now_millis(), registry::fetch).await });
        }
    }
    let version = version(&state, &project, &env).await;
    let marks = state.plugins.terraform.runs.marks(&project);
    no_store(Json(json!({ "nodes": tree.nodes, "truncated": tree.truncated, "marks": marks, "version": version })))
}

#[derive(Deserialize)]
struct ToolBody {
    tool: Tool,
}

/// The user chooses the project's tool, among those installed.
async fn put_tool(State(state): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<String>, body: Bytes) -> Response {
    let ToolBody { tool } = match write_body(&state, &headers, &body) {
        Ok(body) => body,
        Err(status) => return status.into_response(),
    };
    let terraform = &state.plugins.terraform;
    let Some(project) = terraform.project(&id) else { return removed() };
    if !tools(&login(&state).await).has(tool) {
        return refused(StatusCode::BAD_REQUEST, "noTool");
    }
    if let Err(error) = terraform.change_settings(|settings| {
        settings.tools.insert(project.path.clone(), tool);
    }) {
        eprintln!("> terraform settings: {error}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsBody {
    check_versions: bool,
}

async fn put_settings(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    let SettingsBody { check_versions } = match write_body(&state, &headers, &body) {
        Ok(body) => body,
        Err(status) => return status.into_response(),
    };
    match state.plugins.terraform.change_settings(|settings| settings.check_versions = check_versions) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            eprintln!("> terraform settings: {error}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunBody {
    action: Action,
    target: Option<String>,
    /// For an apply: the time of the plan the user confirmed.
    plan_at: Option<u64>,
}

/// Starts a run: the answer's token opens the pane that runs it.
async fn post_run(State(state): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<String>, body: Bytes) -> Response {
    let RunBody { action, target, plan_at } = match write_body(&state, &headers, &body) {
        Ok(body) => body,
        Err(status) => return status.into_response(),
    };
    let terraform = &state.plugins.terraform;
    let Some(project) = terraform.project(&id) else { return removed() };
    settle(&state).await;
    // A plan is made from the code as it is now, and a target is one of its modules.
    let tree = if action == Action::Plan { tree_of(&project).await.unwrap_or_default() } else { tree::Tree::default() };
    let shell_panes = super::shell_panes(crate::spawn::shell_kind());
    match terraform.runs.start(&project, action, target.as_deref(), &tree, shell_panes, now_millis(), plan_at) {
        Ok((token, name)) => no_store(Json(json!({ "token": token, "name": name }))),
        Err(refusal) if refusal.conflict() => refused(StatusCode::CONFLICT, refusal.code()),
        Err(runs::Refusal::Internal) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Err(refusal) => refused(StatusCode::BAD_REQUEST, refusal.code()),
    }
}

/// The project's plan in its workspace; 204 when it has none.
async fn get_plan(State(state): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<String>) -> Response {
    if let Some(refused) = refuse_read(&state, &headers) {
        return refused;
    }
    let Some(project) = state.plugins.terraform.project(&id) else { return removed() };
    settle(&state).await;
    match state.plugins.terraform.runs.plan_answer(&project) {
        Some(plan) => no_store(Json(plan)),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// Removes every saved plan (not one a run is using).
async fn delete_plans(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if let Some(refused) = refuse_write(&state, &headers) {
        return refused;
    }
    state.plugins.terraform.runs.delete_all();
    StatusCode::NO_CONTENT.into_response()
}

/// The project's workspaces. This runs the tool, which can reach the backend:
/// the page asks only when the user opens the picker.
async fn get_workspaces(State(state): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<String>) -> Response {
    if let Some(refused) = refuse_read(&state, &headers) {
        return refused;
    }
    let terraform = &state.plugins.terraform;
    let Some(project) = terraform.project(&id) else { return removed() };
    let env = login(&state).await;
    let Some(program) = find_program(&env, project.tool.program()) else { return refused(StatusCode::BAD_REQUEST, "noTool") };
    match workspaces::list(&program, &project.path, &env, workspaces::WORKSPACE_TIMEOUT).await {
        Ok(names) => {
            if let Ok(mut listed) = terraform.listed.lock() {
                listed.insert(project.id.clone(), names.clone());
            }
            let (current, _) = scan::workspace_of(&project.data_dir, var(&env, "TF_WORKSPACE").as_deref());
            no_store(Json(json!({ "workspaces": names, "current": current })))
        }
        Err(error) => refused(StatusCode::BAD_GATEWAY, &error),
    }
}

#[derive(Deserialize)]
struct WorkspaceBody {
    name: String,
}

/// Switches the project's workspace to one of the names its tool listed last.
async fn post_workspace(State(state): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<String>, body: Bytes) -> Response {
    let WorkspaceBody { name } = match write_body(&state, &headers, &body) {
        Ok(body) => body,
        Err(status) => return status.into_response(),
    };
    let terraform = &state.plugins.terraform;
    let Some(project) = terraform.project(&id) else { return removed() };
    let env = login(&state).await;
    if var(&env, "TF_WORKSPACE").is_some() {
        return refused(StatusCode::CONFLICT, "locked");
    }
    settle(&state).await;
    if terraform.runs.run_info(&project.id).is_some_and(|run| run.state == runs::RunState::Running) {
        return refused(StatusCode::CONFLICT, "running");
    }
    let known = terraform.listed.lock().is_ok_and(|listed| listed.get(&project.id).is_some_and(|names| names.contains(&name)));
    if !known {
        return refused(StatusCode::BAD_REQUEST, "notListed");
    }
    let Some(program) = find_program(&env, project.tool.program()) else { return refused(StatusCode::BAD_REQUEST, "noTool") };
    match workspaces::select(&program, &project.path, &env, &name, workspaces::WORKSPACE_TIMEOUT).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => refused(StatusCode::BAD_GATEWAY, &error),
    }
}

pub(in crate::server) fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/plugins/terraform/state", get(get_state))
        .route("/api/plugins/terraform/projects/{id}/tree", get(get_tree))
        .route("/api/plugins/terraform/projects/{id}/tool", put(put_tool))
        .route("/api/plugins/terraform/settings", put(put_settings))
        .route("/api/plugins/terraform/projects/{id}/runs", post(post_run))
        .route("/api/plugins/terraform/projects/{id}/plan", get(get_plan))
        .route("/api/plugins/terraform/plans", delete(delete_plans))
        .route("/api/plugins/terraform/projects/{id}/workspaces", get(get_workspaces))
        .route("/api/plugins/terraform/projects/{id}/workspace", post(post_workspace))
}

// The fake tools of these tests are sh scripts, and runs are Unix only.
#[cfg(all(test, unix))]
pub(crate) mod tests {
    use super::*;
    use crate::server::plugins::login_env::LoginEnv;
    use crate::server::plugins::PluginState;
    use crate::server::tests::test_state;
    use axum::http::header;
    use std::fs;
    use std::sync::Arc;

    pub(crate) const ORIGIN: &str = "http://127.0.0.1:4317";

    /// A fake tool: answers `version -json`, and counts its calls in `calls.txt` beside it.
    const FAKE_TOOL: &str = "#!/bin/sh\necho \"$@\" >> \"$(dirname \"$0\")/calls.txt\"\nif [ \"$1 $2\" = \"version -json\" ]; then echo '{\"terraform_version\":\"1.9.8\"}'; fi\n";

    /// A state folder, a workspace folder `w` with the project `envs/dev`, and a `bin` folder for fake tools.
    pub(crate) struct Env {
        _keep: tempfile::TempDir,
        pub(crate) root: PathBuf,
        pub(crate) state: Shared,
    }

    impl Env {
        /// `tools`: the fake tools on the login PATH.
        pub(crate) async fn new(installed: bool, tools: &[&str]) -> Self {
            let keep = tempfile::tempdir().unwrap();
            let root = tree::canonical(keep.path()).unwrap();
            fs::create_dir_all(root.join("bin")).unwrap();
            fs::create_dir_all(root.join("state")).unwrap();
            for tool in tools {
                write_tool(&root, tool, FAKE_TOOL);
            }
            write(&root, "w/envs/dev/main.tf", "provider \"aws\" {}\nmodule \"m\" {\n  source = \"./m\"\n}\nresource \"aws_vpc\" \"this\" {}\n");
            write(&root, "w/envs/dev/m/main.tf", "resource \"x\" \"y\" {}\n");
            let mut state = test_state(&[ORIGIN]);
            state.plugins = Arc::new(PluginState::load(&root.join("state")));
            if installed {
                state.plugins.set_builtin(&[ID.into()]).unwrap();
            }
            let vars = vec![("PATH".to_owned(), format!("{}:/usr/bin:/bin", root.join("bin").display())), ("HOME".to_owned(), root.to_string_lossy().into_owned())];
            state.plugins.login_env.get_or_load(|| async { LoginEnv { vars } }).await;
            Self { _keep: keep, root, state: Arc::new(state) }
        }

        pub(crate) fn state_dir(&self) -> PathBuf {
            self.root.join("state")
        }

        /// `GET /state` with these query words.
        pub(crate) async fn get_state(&self, query: &str) -> serde_json::Value {
            let response = get_state(State(self.state.clone()), read_headers(), RawQuery(Some(query.to_owned()))).await;
            assert_eq!(response.status(), StatusCode::OK);
            body_json(response).await
        }

        /// Scans `w` and gives the answer.
        pub(crate) async fn scan(&self) -> serde_json::Value {
            self.get_state(&format!("scan=1&folder={}", self.root.join("w").display())).await
        }

        /// Scans and gives the id of `envs/dev`.
        pub(crate) async fn dev(&self) -> String {
            let state = self.scan().await;
            state["projects"].as_array().unwrap().iter().find(|project| project["name"] == "w/envs/dev" || project["name"] == "envs/dev").expect("envs/dev")["id"].as_str().unwrap().to_owned()
        }

        pub(crate) fn calls(&self) -> Vec<String> {
            fs::read_to_string(self.root.join("bin/calls.txt")).unwrap_or_default().lines().map(str::to_owned).collect()
        }
    }

    pub(crate) fn write(dir: &Path, name: &str, text: &str) {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    pub(crate) fn write_tool(root: &Path, name: &str, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = root.join("bin").join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
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

    fn body(value: serde_json::Value) -> Bytes {
        Bytes::from(value.to_string())
    }

    #[tokio::test]
    async fn routes_answer_404_while_the_plugin_is_off() {
        let env = Env::new(false, &["terraform"]).await;
        let id = "0123456789abcdef".to_owned();
        assert_eq!(get_state(State(env.state.clone()), read_headers(), RawQuery(None)).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(get_tree(State(env.state.clone()), read_headers(), UrlPath(id.clone())).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(put_tool(State(env.state.clone()), write_headers(), UrlPath(id), body(json!({ "tool": "tofu" }))).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(put_settings(State(env.state.clone()), write_headers(), body(json!({ "checkVersions": false }))).await.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn requests_from_other_sites_are_refused() {
        let env = Env::new(true, &["terraform"]).await;
        let mut cross = read_headers();
        cross.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert_eq!(get_state(State(env.state.clone()), cross, RawQuery(None)).await.status(), StatusCode::FORBIDDEN);
        let mut foreign = write_headers();
        foreign.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        assert_eq!(put_settings(State(env.state.clone()), foreign, body(json!({ "checkVersions": false }))).await.status(), StatusCode::BAD_REQUEST);
        assert!(env.state.plugins.terraform.settings().check_versions);
    }

    #[tokio::test]
    async fn state_lists_the_projects_of_the_folders() {
        let env = Env::new(true, &["tofu"]).await;
        let response = get_state(State(env.state.clone()), read_headers(), RawQuery(Some(format!("scan=1&folder={}", env.root.join("w").display())))).await;
        assert_eq!(response.headers()["cache-control"], "no-store");
        let state = body_json(response).await;
        assert_eq!(state["tools"], json!({ "terraform": false, "tofu": true }));
        assert_eq!(state["checkVersions"], true);
        assert_eq!(state["truncated"], false);
        assert_eq!(state["shellPanes"], crate::server::plugins::shell_panes(crate::spawn::shell_kind()));
        let projects = state["projects"].as_array().unwrap();
        assert_eq!(projects.len(), 1);
        let project = &projects[0];
        assert_eq!(project["id"].as_str().unwrap().len(), 16);
        assert_eq!(project["path"], json!(env.root.join("w/envs/dev")));
        assert_eq!((project["name"].as_str(), project["sub"].as_str()), (Some("envs/dev"), Some("~/w")));
        assert_eq!((project["tool"].as_str(), project["toolFound"].as_bool()), (Some("tofu"), Some(true)));
        assert_eq!((project["workspace"].as_str(), project["workspaceLocked"].as_bool()), (Some("default"), Some(false)));
        assert_eq!((project["remote"].as_bool(), project["needsInit"].as_bool()), (Some(false), Some(true)));
        assert_eq!(project["unreadable"], json!([]));
        assert_eq!((&project["run"], &project["plan"]), (&serde_json::Value::Null, &serde_json::Value::Null));
    }

    #[tokio::test]
    async fn relative_and_control_character_folders_are_ignored() {
        let env = Env::new(true, &["terraform"]).await;
        let state = env.get_state("scan=1&folder=w&folder=%2Ftmp%2Fa%0Ab&folder=").await;
        assert_eq!(state["projects"], json!([]));
    }

    #[tokio::test]
    async fn at_most_32_folders_are_scanned() {
        let env = Env::new(true, &["terraform"]).await;
        let mut query = "scan=1".to_owned();
        for i in 0..32 {
            query.push_str(&format!("&folder=%2Fnone%2F{i}"));
        }
        query.push_str(&format!("&folder={}", env.root.join("w").display()));
        assert_eq!(env.get_state(&query).await["projects"], json!([]), "the 33rd folder is not read");
    }

    #[tokio::test]
    async fn state_without_scan_does_not_walk_the_folders() {
        let env = Env::new(true, &["terraform"]).await;
        assert_eq!(env.scan().await["projects"].as_array().unwrap().len(), 1);
        write(&env.root, "w/envs/stage/main.tf", "provider \"aws\" {}\n");
        let folder = format!("folder={}", env.root.join("w").display());
        assert_eq!(env.get_state(&folder).await["projects"].as_array().unwrap().len(), 1, "no scan: the new folder is not seen");
        // What a run changes on disk shows without a scan.
        write(&env.root, "w/envs/dev/.terraform/environment", "dev");
        let project = env.get_state(&folder).await["projects"][0].clone();
        assert_eq!((project["workspace"].as_str(), project["needsInit"].as_bool()), (Some("dev"), Some(false)));
        assert_eq!(env.scan().await["projects"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn the_login_environment_sets_the_workspace_and_the_data_folder() {
        let env = Env::new(true, &["terraform"]).await;
        env.state.plugins.login_env.reset().await;
        let vars = vec![("PATH".to_owned(), "/usr/bin".to_owned()), ("TF_WORKSPACE".to_owned(), "prod".to_owned()), ("TF_DATA_DIR".to_owned(), ".tfdata".to_owned())];
        env.state.plugins.login_env.get_or_load(|| async { LoginEnv { vars } }).await;
        fs::create_dir_all(env.root.join("w/envs/dev/.tfdata")).unwrap();
        let project = env.scan().await["projects"][0].clone();
        assert_eq!((project["workspace"].as_str(), project["workspaceLocked"].as_bool()), (Some("prod"), Some(true)));
        assert_eq!(project["needsInit"], false);
        assert_eq!(project["toolFound"], false);
    }

    #[tokio::test]
    async fn a_tree_needs_an_id_of_the_last_scan() {
        let env = Env::new(true, &["terraform"]).await;
        for id in ["0123456789abcdef", "../x", ""] {
            let response = get_tree(State(env.state.clone()), read_headers(), UrlPath(id.to_owned())).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{id}");
            assert_eq!(body_json(response).await, json!({ "removed": true }));
        }
        let id = env.dev().await;
        let response = get_tree(State(env.state.clone()), read_headers(), UrlPath(id)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let tree = body_json(response).await;
        assert_eq!(tree["truncated"], false);
        assert_eq!(tree["marks"], json!([]));
        let addresses: Vec<&str> = tree["nodes"].as_array().unwrap().iter().map(|node| node["address"].as_str().unwrap()).collect();
        assert_eq!(addresses, ["module.m", "aws_vpc.this"]);
        assert_eq!(tree["nodes"][0]["children"][0]["address"], "module.m.x.y");
        assert_eq!(tree["nodes"][0]["newer"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn the_version_is_read_once_per_project() {
        let env = Env::new(true, &["terraform"]).await;
        let id = env.dev().await;
        for _ in 0..2 {
            let tree = body_json(get_tree(State(env.state.clone()), read_headers(), UrlPath(id.clone())).await).await;
            assert_eq!(tree["version"], "1.9.8");
        }
        assert_eq!(env.calls(), ["version -json"]);
    }

    #[tokio::test]
    async fn a_project_without_its_tool_has_no_version() {
        let env = Env::new(true, &[]).await;
        let id = env.dev().await;
        let tree = body_json(get_tree(State(env.state.clone()), read_headers(), UrlPath(id)).await).await;
        assert_eq!(tree["version"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn the_chosen_tool_is_saved_and_used() {
        let env = Env::new(true, &["terraform", "tofu"]).await;
        let id = env.dev().await;
        assert_eq!(env.scan().await["projects"][0]["tool"], "terraform");
        let response = put_tool(State(env.state.clone()), write_headers(), UrlPath(id.clone()), body(json!({ "tool": "tofu" }))).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let saved = fs::read_to_string(env.state_dir().join("terraform/settings.json")).unwrap();
        assert!(saved.contains(&env.root.join("w/envs/dev").to_string_lossy().into_owned()), "{saved}");
        // It shows at once, and after a scan.
        let folder = format!("folder={}", env.root.join("w").display());
        assert_eq!(env.get_state(&folder).await["projects"][0]["tool"], "tofu");
        assert_eq!(env.scan().await["projects"][0]["tool"], "tofu");
        let unknown = put_tool(State(env.state.clone()), write_headers(), UrlPath("0123456789abcdef".into()), body(json!({ "tool": "tofu" }))).await;
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
        let odd = put_tool(State(env.state.clone()), write_headers(), UrlPath(id), body(json!({ "tool": "terragrunt" }))).await;
        assert_eq!(odd.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn a_tool_that_is_not_installed_cannot_be_chosen() {
        let env = Env::new(true, &["terraform"]).await;
        let id = env.dev().await;
        let response = put_tool(State(env.state.clone()), write_headers(), UrlPath(id), body(json!({ "tool": "tofu" }))).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(response).await, json!({ "error": "noTool" }));
    }

    #[tokio::test]
    async fn the_version_check_setting_is_saved() {
        let env = Env::new(true, &["terraform"]).await;
        let response = put_settings(State(env.state.clone()), write_headers(), body(json!({ "checkVersions": false }))).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(env.get_state("").await["checkVersions"], false);
        assert!(!settings::load(&env.state_dir()).check_versions);
        let big = put_settings(State(env.state.clone()), write_headers(), Bytes::from(format!("{{\"checkVersions\":false,\"x\":\"{}\"}}", "a".repeat(5000)))).await;
        assert_eq!(big.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn uninstall_removes_the_terraform_folder() {
        let env = Env::new(true, &["terraform", "tofu"]).await;
        let id = env.dev().await;
        put_tool(State(env.state.clone()), write_headers(), UrlPath(id.clone()), body(json!({ "tool": "tofu" }))).await;
        assert!(env.state_dir().join("terraform").is_dir());
        env.state.plugins.terraform.on_uninstall().await;
        assert!(!env.state_dir().join("terraform").exists());
        assert!(env.state.plugins.terraform.project(&id).is_none());
        assert!(env.state.plugins.terraform.settings().tools.is_empty());
    }

    fn run_body(action: &str, target: Option<&str>) -> Bytes {
        body(match target {
            Some(target) => json!({ "action": action, "target": target }),
            None => json!({ "action": action }),
        })
    }

    /// What the runner does for a plan: starts, writes the plan and its JSON, ends.
    fn finish_plan(env: &Env, token: &str, json: serde_json::Value) {
        use crate::plugins::terraform::{take_spec, write_done, write_started, RunDone, RunStarted};
        let spec = take_spec(&env.state_dir(), token).expect("a spec");
        write_started(&env.state_dir(), token, &RunStarted { pid: std::process::id(), started_at: 1 }).unwrap();
        fs::write(spec.plan.as_ref().unwrap(), "plan bytes").unwrap();
        fs::write(spec.json.as_ref().unwrap(), json.to_string()).unwrap();
        write_done(&env.state_dir(), token, &RunDone { exit: 0, ended_at: crate::time::now_millis(), workspace: Some("default".into()), version: None, json_ok: true }).unwrap();
    }

    fn one_create() -> serde_json::Value {
        json!({ "format_version": "1.2", "resource_changes": [
            { "address": "aws_vpc.this", "mode": "managed", "type": "aws_vpc", "name": "this", "change": { "actions": ["create"], "before": null, "after": { "cidr": "10.0.0.0/16", "holder": "hidden-after-1" }, "after_sensitive": { "holder": true } } },
        ] })
    }

    #[tokio::test]
    async fn the_runs_route_answers_with_a_token_or_a_code() {
        if !crate::server::plugins::shell_panes(crate::spawn::shell_kind()) {
            return;
        }
        let env = Env::new(true, &["terraform"]).await;
        let id = env.dev().await;
        let response = post_run(State(env.state.clone()), write_headers(), UrlPath(id.clone()), run_body("plan", None)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let started = body_json(response).await;
        let token = started["token"].as_str().unwrap().to_owned();
        assert!(crate::plugins::terraform::is_token(&token));
        assert_eq!(started["name"], "envs/dev plan");
        // The session query of the new shell is allowed while the spec waits.
        assert!(crate::plugins::terraform::spec_path(&env.state_dir(), &token).is_file());

        let second = post_run(State(env.state.clone()), write_headers(), UrlPath(id.clone()), run_body("plan", None)).await;
        assert_eq!(second.status(), StatusCode::CONFLICT);
        assert_eq!(body_json(second).await, json!({ "error": "running" }));
        assert_eq!(env.scan().await["projects"][0]["run"]["state"], "running");

        for odd in [run_body("destroy", None), Bytes::from("{}"), run_body("plan", Some("module.nope"))] {
            let response = post_run(State(env.state.clone()), write_headers(), UrlPath(id.clone()), odd).await;
            assert!(matches!(response.status(), StatusCode::BAD_REQUEST | StatusCode::CONFLICT));
        }
        let unknown = post_run(State(env.state.clone()), write_headers(), UrlPath("0123456789abcdef".into()), run_body("plan", None)).await;
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_json(unknown).await, json!({ "removed": true }));
    }

    #[tokio::test]
    async fn a_bad_target_and_a_missing_plan_have_their_codes() {
        if !crate::server::plugins::shell_panes(crate::spawn::shell_kind()) {
            return;
        }
        let env = Env::new(true, &["terraform"]).await;
        let id = env.dev().await;
        let bad = post_run(State(env.state.clone()), write_headers(), UrlPath(id.clone()), run_body("plan", Some("module.nope"))).await;
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(bad).await, json!({ "error": "badTarget" }));
        let none = post_run(State(env.state.clone()), write_headers(), UrlPath(id.clone()), body(json!({ "action": "apply", "planAt": 5 }))).await;
        assert_eq!(none.status(), StatusCode::CONFLICT);
        assert_eq!(body_json(none).await, json!({ "error": "noPlan" }));
        let good = post_run(State(env.state.clone()), write_headers(), UrlPath(id), run_body("plan", Some("module.m"))).await;
        assert_eq!(good.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_project_without_its_tool_cannot_run() {
        let env = Env::new(true, &[]).await;
        let id = env.dev().await;
        let response = post_run(State(env.state.clone()), write_headers(), UrlPath(id), run_body("plan", None)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error = body_json(response).await["error"].as_str().unwrap().to_owned();
        assert!(error == "noTool" || error == "noShell", "{error}");
    }

    #[tokio::test]
    async fn a_finished_plan_shows_in_the_state_the_tree_and_the_plan_route() {
        if !crate::server::plugins::shell_panes(crate::spawn::shell_kind()) {
            return;
        }
        let env = Env::new(true, &["terraform"]).await;
        let id = env.dev().await;
        let none = get_plan(State(env.state.clone()), read_headers(), UrlPath(id.clone())).await;
        assert_eq!(none.status(), StatusCode::NO_CONTENT);
        let started = body_json(post_run(State(env.state.clone()), write_headers(), UrlPath(id.clone()), run_body("plan", None)).await).await;
        finish_plan(&env, started["token"].as_str().unwrap(), one_create());

        let project = env.get_state("").await["projects"][0].clone();
        assert_eq!((project["run"]["action"].as_str(), project["run"]["state"].as_str()), (Some("plan"), Some("ok")));
        assert_eq!((project["plan"]["state"].as_str(), project["plan"]["counts"]["add"].as_u64(), project["plan"]["applyable"].as_bool()), (Some("current"), Some(1), Some(true)));

        let tree = body_json(get_tree(State(env.state.clone()), read_headers(), UrlPath(id.clone())).await).await;
        assert_eq!(tree["marks"], json!([{ "module": "", "type": "aws_vpc", "name": "this", "mark": "create", "count": 1 }]));

        let response = get_plan(State(env.state.clone()), read_headers(), UrlPath(id.clone())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let plan = body_json(response).await;
        assert_eq!((plan["state"].as_str(), plan["changes"][0]["address"].as_str()), (Some("current"), Some("aws_vpc.this")));
        assert!(!plan.to_string().contains("hidden-"));

        // The code changes: the plan is stale and its marks go.
        write(&env.root, "w/envs/dev/main.tf", "provider \"aws\" {}\nresource \"aws_vpc\" \"this\" {}\n# edited\n");
        assert_eq!(env.get_state("").await["projects"][0]["plan"]["state"], "stale");
        let tree = body_json(get_tree(State(env.state.clone()), read_headers(), UrlPath(id.clone())).await).await;
        assert_eq!(tree["marks"], json!([]));
        // An apply names the plan it confirms; another one, or none, is refused before anything else.
        let shown = env.get_state("").await["projects"][0]["plan"]["at"].as_u64().unwrap();
        for at in [json!(shown + 1), json!(null)] {
            let other = post_run(State(env.state.clone()), write_headers(), UrlPath(id.clone()), body(json!({ "action": "apply", "planAt": at }))).await;
            assert_eq!(other.status(), StatusCode::CONFLICT);
            assert_eq!(body_json(other).await, json!({ "error": "stale" }));
        }
        let stale = post_run(State(env.state.clone()), write_headers(), UrlPath(id.clone()), body(json!({ "action": "apply", "planAt": shown }))).await;
        assert_eq!(stale.status(), StatusCode::CONFLICT);
        assert_eq!(body_json(stale).await, json!({ "error": "stale" }));

        // Deleting the saved plans.
        let mut foreign = write_headers();
        foreign.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        assert_eq!(delete_plans(State(env.state.clone()), foreign).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(delete_plans(State(env.state.clone()), write_headers()).await.status(), StatusCode::NO_CONTENT);
        assert_eq!(get_plan(State(env.state.clone()), read_headers(), UrlPath(id)).await.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn the_plan_routes_answer_404_while_the_plugin_is_off() {
        let env = Env::new(false, &["terraform"]).await;
        let id = "0123456789abcdef".to_owned();
        assert_eq!(post_run(State(env.state.clone()), write_headers(), UrlPath(id.clone()), run_body("plan", None)).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(get_plan(State(env.state.clone()), read_headers(), UrlPath(id)).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(delete_plans(State(env.state.clone()), write_headers()).await.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn only_a_starting_server_drops_the_runs_of_the_last_one() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("terraform/runs")).unwrap();
        fs::write(dir.path().join("terraform/runs/x.spec.json"), "{}").unwrap();
        // The dashboard of a remote window loads the same state folder while the server runs.
        let state = TerraformState::load(dir.path());
        assert!(dir.path().join("terraform/runs/x.spec.json").is_file(), "a run in progress is left alone");
        state.on_server_start();
        assert!(!dir.path().join("terraform/runs").exists());
    }

    /// A fake tool with three workspaces; `select` writes the environment file as the real tool does.
    const WORKSPACE_TOOL: &str = "#!/bin/sh\necho \"$@\" >> \"$(dirname \"$0\")/calls.txt\"\ncase \"$1 $2\" in\n  \"workspace list\") printf '  default\\n* dev\\n  prod-eu\\n' ;;\n  \"workspace select\") mkdir -p .terraform && printf '%s' \"$3\" > .terraform/environment ;;\nesac\n";

    async fn workspace_env() -> (Env, String) {
        let env = Env::new(true, &[]).await;
        write_tool(&env.root, "terraform", WORKSPACE_TOOL);
        let id = env.dev().await;
        (env, id)
    }

    fn select_body(name: &str) -> Bytes {
        body(json!({ "name": name }))
    }

    #[tokio::test]
    async fn a_listed_name_is_selected() {
        let (env, id) = workspace_env().await;
        let listed = get_workspaces(State(env.state.clone()), read_headers(), UrlPath(id.clone())).await;
        assert_eq!(listed.status(), StatusCode::OK);
        assert_eq!(body_json(listed).await, json!({ "workspaces": ["default", "dev", "prod-eu"], "current": "default" }));
        let selected = post_workspace(State(env.state.clone()), write_headers(), UrlPath(id.clone()), select_body("dev")).await;
        assert_eq!(selected.status(), StatusCode::NO_CONTENT);
        assert_eq!(env.calls(), ["workspace list", "workspace select dev"]);
        assert_eq!(env.get_state("").await["projects"][0]["workspace"], "dev");
    }

    #[tokio::test]
    async fn a_name_that_was_not_listed_is_refused() {
        let (env, id) = workspace_env().await;
        let early = post_workspace(State(env.state.clone()), write_headers(), UrlPath(id.clone()), select_body("dev")).await;
        assert_eq!(early.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(early).await, json!({ "error": "notListed" }));
        get_workspaces(State(env.state.clone()), read_headers(), UrlPath(id.clone())).await;
        let odd = post_workspace(State(env.state.clone()), write_headers(), UrlPath(id.clone()), select_body("x; rm -rf ~")).await;
        assert_eq!(odd.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(odd).await, json!({ "error": "notListed" }));
        assert_eq!(env.calls(), ["workspace list"], "nothing was selected");
        let unknown = post_workspace(State(env.state.clone()), write_headers(), UrlPath("0123456789abcdef".into()), select_body("dev")).await;
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn a_locked_or_running_project_is_refused() {
        let (env, id) = workspace_env().await;
        get_workspaces(State(env.state.clone()), read_headers(), UrlPath(id.clone())).await;
        if crate::server::plugins::shell_panes(crate::spawn::shell_kind()) {
            let started = post_run(State(env.state.clone()), write_headers(), UrlPath(id.clone()), run_body("validate", None)).await;
            assert_eq!(started.status(), StatusCode::OK);
            let busy = post_workspace(State(env.state.clone()), write_headers(), UrlPath(id.clone()), select_body("dev")).await;
            assert_eq!(busy.status(), StatusCode::CONFLICT);
            assert_eq!(body_json(busy).await, json!({ "error": "running" }));
        }
        // TF_WORKSPACE sets the workspace: the tool would refuse the switch.
        env.state.plugins.login_env.reset().await;
        let vars = vec![("PATH".to_owned(), format!("{}:/usr/bin:/bin", env.root.join("bin").display())), ("TF_WORKSPACE".to_owned(), "prod".to_owned())];
        env.state.plugins.login_env.get_or_load(|| async { LoginEnv { vars } }).await;
        env.get_state("").await;
        let locked = post_workspace(State(env.state.clone()), write_headers(), UrlPath(id), select_body("dev")).await;
        assert_eq!(locked.status(), StatusCode::CONFLICT);
        assert_eq!(body_json(locked).await, json!({ "error": "locked" }));
        assert!(!env.calls().iter().any(|call| call.starts_with("workspace select")));
    }

    #[tokio::test]
    async fn a_failing_list_gives_its_first_line() {
        let env = Env::new(true, &[]).await;
        write_tool(&env.root, "terraform", "#!/bin/sh\necho 'Error: No valid credential sources found' >&2\necho more >&2\nexit 1\n");
        let id = env.dev().await;
        let failed = get_workspaces(State(env.state.clone()), read_headers(), UrlPath(id)).await;
        assert_eq!(failed.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(body_json(failed).await, json!({ "error": "Error: No valid credential sources found" }));
    }

    #[tokio::test]
    async fn workspaces_need_the_tool_and_the_plugin() {
        let env = Env::new(true, &[]).await;
        let id = env.dev().await;
        let missing = get_workspaces(State(env.state.clone()), read_headers(), UrlPath(id.clone())).await;
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(missing).await, json!({ "error": "noTool" }));
        let off = Env::new(false, &["terraform"]).await;
        assert_eq!(get_workspaces(State(off.state.clone()), read_headers(), UrlPath(id.clone())).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(post_workspace(State(off.state.clone()), write_headers(), UrlPath(id), select_body("dev")).await.status(), StatusCode::NOT_FOUND);
    }

    /// A project that uses a registry module, installed at 1.0.0.
    async fn registry_env(namespace: &str) -> (Env, String) {
        let env = Env::new(true, &["terraform"]).await;
        write(&env.root, "w/envs/dev/main.tf", &format!("provider \"aws\" {{}}\nmodule \"vpc\" {{\n  source  = \"{namespace}/vpc/aws\"\n  version = \"~> 1.0\"\n}}\n"));
        write(&env.root, "w/envs/dev/.terraform/modules/vpc/main.tf", "");
        write(&env.root, "w/envs/dev/.terraform/modules/modules.json", r#"{"Modules":[{"Key":"vpc","Source":"x","Version":"1.0.0","Dir":".terraform/modules/vpc"}]}"#);
        let id = env.dev().await;
        (env, id)
    }

    async fn newer_of(env: &Env, id: &str) -> serde_json::Value {
        body_json(get_tree(State(env.state.clone()), read_headers(), UrlPath(id.to_owned())).await).await["nodes"][0]["newer"].clone()
    }

    #[tokio::test]
    async fn the_tree_route_names_a_newer_version_without_waiting_for_it() {
        let (env, id) = registry_env("acme-check-on").await;
        assert_eq!(newer_of(&env, &id).await, serde_json::Value::Null, "the first answer does not wait for the registry");
        let url = "https://registry.terraform.io/v1/modules/acme-check-on/vpc/aws/versions";
        for _ in 0..100 {
            if registry::test_fetch::asked(url) > 0 && newer_of(&env, &id).await != serde_json::Value::Null {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(newer_of(&env, &id).await, "9.9.9");
        assert_eq!(registry::test_fetch::asked(url), 1);
    }

    #[tokio::test]
    async fn the_tree_route_asks_nothing_when_the_check_is_off() {
        let (env, id) = registry_env("acme-check-off").await;
        let off = put_settings(State(env.state.clone()), write_headers(), body(json!({ "checkVersions": false }))).await;
        assert_eq!(off.status(), StatusCode::NO_CONTENT);
        for _ in 0..3 {
            assert_eq!(newer_of(&env, &id).await, serde_json::Value::Null);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(registry::test_fetch::asked("https://registry.terraform.io/v1/modules/acme-check-off/vpc/aws/versions"), 0);
    }
}
