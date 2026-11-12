//! The Docker plugin's server side: container reads and container actions
//! through the `docker` CLI. Arguments are always arrays; no shell sees them.
//! `docker` runs with the Docker keys of the user's login environment.

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::process::Stdio;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::process::Command;

use super::login_env::{self, LoginEnv, LoginEnvCache};
use crate::plugins::docker::is_container_id;
use crate::server::{no_store, origin_allowed, read_allowed, Shared};

/// The plugin's id, as `plugins.json` and the routes name it.
const ID: &str = "docker";
/// Each read (`version`, `ps`, `inspect`) gets this long.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// Docker waits up to 10 s for a container to stop, so actions get longer.
const ACTION_TIMEOUT: Duration = Duration::from_secs(30);
/// A list longer than this shows its newest containers only.
const MAX_CONTAINERS: usize = 200;
/// The only variables taken from the login shell and handed to `docker`.
const ENV_KEYS: [&str; 6] = ["PATH", "DOCKER_HOST", "DOCKER_CONTEXT", "DOCKER_CONFIG", "DOCKER_CERT_PATH", "DOCKER_TLS_VERIFY"];

/// The environment `docker` runs in: the allowed keys only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DockerEnv {
    vars: Vec<(String, String)>,
}

impl DockerEnv {
    /// The Docker keys of the login environment, in its order.
    pub(super) fn from_login(env: &LoginEnv) -> Self {
        Self { vars: env.vars.iter().filter(|(key, _)| ENV_KEYS.contains(&key.as_str())).cloned().collect() }
    }
}

/// What a finished process left behind.
struct Output {
    ok: bool,
    stdout: String,
    stderr: String,
}

enum ExecError {
    NotFound,
    TimedOut,
    Other(String),
}

/// Runs `command` to the end within `timeout`. A timeout kills the process.
async fn exec(mut command: Command, timeout: Duration) -> Result<Output, ExecError> {
    command.stdin(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flash from a GUI app
    match tokio::time::timeout(timeout, command.output()).await {
        Err(_) => Err(ExecError::TimedOut),
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => Err(ExecError::NotFound),
        Ok(Err(error)) => Err(ExecError::Other(error.to_string())),
        Ok(Ok(output)) => Ok(Output {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }),
    }
}

/// `docker <args>` in `env`. The allowed keys come from `env` alone, so a
/// key the login shell did not set is not taken from the server either.
fn docker(env: &DockerEnv, args: &[&str]) -> Command {
    let mut command = Command::new("docker");
    command.args(args);
    for key in ENV_KEYS {
        command.env_remove(key);
    }
    command.envs(env.vars.iter().map(|(key, value)| (key, value)));
    command
}

/// The first non-empty line of Docker's complaint.
fn first_line(text: &str) -> String {
    text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default().to_owned()
}

/// Why a failed `docker` call failed, in one line.
fn failure_line(output: &Output) -> String {
    match first_line(&output.stderr) {
        line if line.is_empty() => "docker failed".to_owned(),
        line => line,
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Status {
    Ok,
    NoCli,
    Down,
}

#[derive(Serialize)]
struct Engine {
    status: Status,
    version: Option<String>,
    error: Option<String>,
}

impl Engine {
    fn down(version: Option<String>, error: String) -> Self {
        Self { status: Status::Down, version, error: Some(error) }
    }
}

#[derive(Serialize)]
struct DockerState {
    engine: Engine,
    containers: Vec<Container>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Container {
    id: String,
    name: String,
    service: Option<String>,
    project: Option<String>,
    working_dir: Option<String>,
    image: String,
    state: String,
    health: Option<String>,
    exit_code: Option<i32>,
    finished_at: Option<u64>,
    ports: Vec<u16>,
}

/// One element of `docker inspect` output; only what the panel shows.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Inspected {
    id: String,
    name: String,
    config: InspectConfig,
    state: InspectState,
    network_settings: InspectNetwork,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct InspectConfig {
    image: String,
    labels: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct InspectState {
    status: String,
    exit_code: Option<i32>,
    finished_at: String,
    health: Option<InspectHealth>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct InspectHealth {
    status: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct InspectNetwork {
    ports: serde_json::Value,
}

/// The host ports published over TCP, once each, ascending. IPv4 and IPv6
/// bindings of a port count once; unpublished ports have no host port.
fn host_ports(ports: &serde_json::Value) -> Vec<u16> {
    let Some(map) = ports.as_object() else { return Vec::new() };
    let found: BTreeSet<u16> = map
        .iter()
        .filter(|(key, _)| key.ends_with("/tcp"))
        .filter_map(|(_, bindings)| bindings.as_array())
        .flatten()
        .filter_map(|binding| binding["HostPort"].as_str()?.parse().ok())
        .collect();
    found.into_iter().collect()
}

/// The containers in `docker inspect` output; unreadable output gives none.
fn parse_inspect(json: &str) -> Vec<Container> {
    let Ok(list) = serde_json::from_str::<Vec<Inspected>>(json) else { return Vec::new() };
    list.into_iter()
        .map(|item| {
            let label = |key: &str| item.config.labels.as_ref().and_then(|labels| labels.get(key)).cloned();
            let running = item.state.status == "running";
            Container {
                name: item.name.trim_start_matches('/').to_owned(),
                service: label("com.docker.compose.service"),
                project: label("com.docker.compose.project"),
                working_dir: label("com.docker.compose.project.working_dir"),
                image: item.config.image,
                health: item.state.health.map(|health| health.status).filter(|status| !status.is_empty()),
                exit_code: item.state.exit_code.filter(|_| !running),
                finished_at: crate::time::parse_iso_millis(&item.state.finished_at),
                ports: host_ports(&item.network_settings.ports),
                state: item.state.status,
                id: item.id,
            }
        })
        .collect()
}

/// The engine's state and the containers. A failing call is a `down` engine
/// with Docker's own first line, or "timed out".
async fn read_state(env: &DockerEnv, timeout: Duration) -> DockerState {
    let down = |version: Option<String>, error: String| DockerState { engine: Engine::down(version, error), containers: Vec::new() };
    let version = match exec(docker(env, &["version", "--format", "{{.Server.Version}}"]), timeout).await {
        Ok(output) if output.ok => output.stdout.trim().to_owned(),
        Ok(output) => return down(None, failure_line(&output)),
        Err(ExecError::NotFound) => {
            return DockerState { engine: Engine { status: Status::NoCli, version: None, error: None }, containers: Vec::new() };
        }
        Err(ExecError::TimedOut) => return down(None, "timed out".to_owned()),
        Err(ExecError::Other(error)) => return down(None, error),
    };
    let version = Some(version);
    let ids = match exec(docker(env, &["ps", "-a", "-q", "--no-trunc"]), timeout).await {
        Ok(output) if output.ok => output.stdout,
        Ok(output) => return down(version, failure_line(&output)),
        Err(ExecError::TimedOut) => return down(version, "timed out".to_owned()),
        Err(ExecError::NotFound) => return down(version, "docker not found".to_owned()),
        Err(ExecError::Other(error)) => return down(version, error),
    };
    let ids: Vec<&str> = ids.lines().map(str::trim).filter(|id| !id.is_empty()).take(MAX_CONTAINERS).collect();
    let engine = |version| Engine { status: Status::Ok, version, error: None };
    if ids.is_empty() {
        return DockerState { engine: engine(version), containers: Vec::new() };
    }
    let mut args = vec!["inspect", "--type", "container"];
    args.extend(&ids);
    match exec(docker(env, &args), timeout).await {
        Ok(output) => {
            let containers = parse_inspect(&output.stdout);
            // A container removed since `ps` fails the call but the rest still print.
            if !output.ok && containers.is_empty() {
                return down(version, failure_line(&output));
            }
            DockerState { engine: engine(version), containers }
        }
        Err(ExecError::TimedOut) => down(version, "timed out".to_owned()),
        Err(ExecError::NotFound) => down(version, "docker not found".to_owned()),
        Err(ExecError::Other(error)) => down(version, error),
    }
}

/// The `docker` arguments for a container action, or `None` for an action or
/// id this plugin does not accept.
fn action_args<'a>(action: &str, id: &'a str) -> Option<[&'a str; 2]> {
    let word = match action {
        "start" => "start",
        "stop" => "stop",
        "restart" => "restart",
        "remove" => "rm",
        _ => return None,
    };
    is_container_id(id).then_some([word, id])
}

/// Runs a container action. The error is Docker's first line, or "timed out".
async fn run_action(env: &DockerEnv, id: &str, action: &str, timeout: Duration) -> Result<(), String> {
    let args = action_args(action, id).ok_or_else(|| "invalid action".to_owned())?;
    match exec(docker(env, &args), timeout).await {
        Ok(output) if output.ok => Ok(()),
        Ok(output) => Err(failure_line(&output)),
        Err(ExecError::NotFound) => Err("docker not found".to_owned()),
        Err(ExecError::TimedOut) => Err("timed out".to_owned()),
        Err(ExecError::Other(error)) => Err(error),
    }
}

pub(super) fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/plugins/docker/state", get(get_state))
        .route("/api/plugins/docker/containers/{id}/{action}", post(post_action))
}

#[derive(Deserialize)]
struct StateQuery {
    retry: Option<String>,
}

async fn get_state(State(state): State<Shared>, headers: HeaderMap, Query(query): Query<StateQuery>) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !state.plugins.is_installed(ID) {
        return StatusCode::NOT_FOUND.into_response();
    }
    no_store(Json(current_state(&state.plugins.login_env, query.retry.as_deref() == Some("1"), login_env::load).await))
}

/// The state in the remembered environment. `retry` is the UI's Retry
/// button: read the environment again first.
async fn current_state<F, Fut>(cache: &LoginEnvCache, retry: bool, load: F) -> DockerState
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = LoginEnv>,
{
    if retry {
        cache.reset().await;
    }
    read_state(&DockerEnv::from_login(&cache.get_or_load(load).await), READ_TIMEOUT).await
}

async fn post_action(State(state): State<Shared>, headers: HeaderMap, Path((id, action)): Path<(String, String)>) -> Response {
    if !origin_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !state.plugins.is_installed(ID) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if action_args(&action, &id).is_none() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let env = DockerEnv::from_login(&state.plugins.login_env.get_or_load(login_env::load).await);
    match run_action(&env, &id, &action, ACTION_TIMEOUT).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => (StatusCode::BAD_GATEWAY, Json(json!({ "error": error }))).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"[
  {
    "Id": "9f1c2d3e4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d",
    "Name": "/westerm-site-api-1",
    "Config": {
      "Image": "node:22-slim",
      "Labels": {
        "com.docker.compose.project": "westerm-site",
        "com.docker.compose.service": "api",
        "com.docker.compose.project.working_dir": "/Users/p/westerm-site",
        "maintainer": "someone"
      }
    },
    "State": {
      "Status": "running",
      "Running": true,
      "ExitCode": 0,
      "FinishedAt": "0001-01-01T00:00:00Z",
      "Health": { "Status": "healthy", "FailingStreak": 0 }
    },
    "NetworkSettings": {
      "Ports": {
        "3000/tcp": [
          { "HostIp": "0.0.0.0", "HostPort": "3000" },
          { "HostIp": "::", "HostPort": "3000" }
        ],
        "5432/tcp": null
      }
    }
  },
  {
    "Id": "0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b",
    "Name": "/worker",
    "Config": { "Image": "alpine:3.20", "Labels": null },
    "State": {
      "Status": "exited",
      "Running": false,
      "ExitCode": 1,
      "FinishedAt": "2026-11-08T12:05:21.5Z"
    },
    "NetworkSettings": { "Ports": {} }
  }
]"#;

    #[test]
    fn docker_env_keeps_only_docker_keys() {
        let login = LoginEnv { vars: vec![("PATH".into(), "/a".into()), ("SECRET".into(), "x".into()), ("DOCKER_HOST".into(), "unix:///c.sock".into())] };
        assert_eq!(DockerEnv::from_login(&login), DockerEnv { vars: vec![("PATH".into(), "/a".into()), ("DOCKER_HOST".into(), "unix:///c.sock".into())] });
    }

    #[test]
    fn inspect_output_becomes_containers() {
        let list = parse_inspect(FIXTURE);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id.len(), 64);
        assert_eq!(list[0].name, "westerm-site-api-1");
        assert_eq!(list[0].service.as_deref(), Some("api"));
        assert_eq!(list[0].project.as_deref(), Some("westerm-site"));
        assert_eq!(list[0].working_dir.as_deref(), Some("/Users/p/westerm-site"));
        assert_eq!(list[0].image, "node:22-slim");
        assert_eq!(list[0].state, "running");
        assert_eq!(list[0].health.as_deref(), Some("healthy"));
        assert_eq!(list[0].ports, vec![3000]);
        assert_eq!(list[0].exit_code, None);
        assert_eq!(list[0].finished_at, None);
        assert_eq!(list[1].name, "worker");
        assert_eq!(list[1].state, "exited");
        assert_eq!(list[1].health, None);
        assert_eq!(list[1].exit_code, Some(1));
        assert_eq!(list[1].finished_at, Some(1_794_139_521_000));
        assert_eq!(list[1].project, None);
        assert!(list[1].ports.is_empty());
        assert!(parse_inspect("not json").is_empty());
    }

    #[test]
    fn host_ports_are_unique_sorted_and_tcp_only() {
        let ports = serde_json::json!({
            "8080/tcp": [{ "HostIp": "0.0.0.0", "HostPort": "8080" }],
            "53/udp": [{ "HostIp": "0.0.0.0", "HostPort": "53" }],
            "80/tcp": [{ "HostIp": "0.0.0.0", "HostPort": "80" }, { "HostIp": "::", "HostPort": "80" }],
            "9/tcp": [{ "HostIp": "0.0.0.0", "HostPort": "" }],
            "443/tcp": null
        });
        assert_eq!(host_ports(&ports), vec![80, 8080]);
        assert!(host_ports(&serde_json::Value::Null).is_empty());
    }

    #[test]
    fn actions_and_ids_are_checked() {
        let id = "0123456789ab";
        assert_eq!(action_args("start", id), Some(["start", id]));
        assert_eq!(action_args("stop", id), Some(["stop", id]));
        assert_eq!(action_args("restart", id), Some(["restart", id]));
        assert_eq!(action_args("remove", id), Some(["rm", id]));
        assert_eq!(action_args("kill", id), None);
        assert_eq!(action_args("rm", id), None);
        assert_eq!(action_args("start", "ABC"), None);
        assert_eq!(action_args("start", "0123456789ab; ls"), None);
        assert_eq!(action_args("start", "--all"), None);
    }

    #[test]
    fn the_engine_serializes_in_the_documented_shape() {
        let state = DockerState { engine: Engine::down(None, "boom".into()), containers: vec![] };
        assert_eq!(
            serde_json::to_string(&state).unwrap(),
            r#"{"engine":{"status":"down","version":null,"error":"boom"},"containers":[]}"#
        );
        let list = parse_inspect(FIXTURE);
        let json = serde_json::to_value(&list[1]).unwrap();
        assert_eq!(json["exitCode"], 1);
        assert_eq!(json["finishedAt"], 1_794_139_521_000_u64);
        assert!(json["workingDir"].is_null());
    }

    #[cfg(unix)]
    mod routes {
        use super::*;
        use axum::http::header;
        use std::os::unix::fs::PermissionsExt;
        use std::sync::Arc;

        fn shared(installed: bool) -> (Shared, tempfile::TempDir) {
            let dir = tempfile::tempdir().unwrap();
            let mut state = crate::server::tests::test_state(&["http://127.0.0.1:4317"]);
            state.plugins = std::sync::Arc::new(crate::server::plugins::PluginState::load(dir.path()));
            if installed {
                state.plugins.set_builtin(&["docker".into()]).unwrap();
            }
            (Arc::new(state), dir)
        }

        fn headers() -> HeaderMap {
            let mut headers = HeaderMap::new();
            headers.insert(header::HOST, "127.0.0.1:4317".parse().unwrap());
            headers.insert(header::ORIGIN, "http://127.0.0.1:4317".parse().unwrap());
            headers
        }

        /// Makes the cache hand out an env whose PATH is a folder with a fake `docker`.
        async fn fake_docker(state: &Shared, dir: &std::path::Path, body: &str) {
            let path = dir.join("docker");
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            let env = LoginEnv { vars: vec![("PATH".into(), format!("{}:/bin:/usr/bin", dir.display()))] };
            state.plugins.login_env.reset().await;
            state.plugins.login_env.get_or_load(|| async { env }).await;
        }

        async fn text(response: Response) -> String {
            String::from_utf8(axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap().to_vec()).unwrap()
        }

        fn query(retry: Option<&str>) -> Query<StateQuery> {
            Query(StateQuery { retry: retry.map(str::to_owned) })
        }

        #[tokio::test]
        async fn a_plugin_that_is_not_installed_answers_404() {
            let (state, _dir) = shared(false);
            assert_eq!(get_state(State(state.clone()), headers(), query(None)).await.status(), StatusCode::NOT_FOUND);
            let response = post_action(State(state), headers(), Path(("0123456789ab".into(), "start".into()))).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn requests_from_other_sites_are_refused() {
            let (state, _dir) = shared(true);
            let mut cross = headers();
            cross.insert("sec-fetch-site", "cross-site".parse().unwrap());
            assert_eq!(get_state(State(state.clone()), cross, query(None)).await.status(), StatusCode::FORBIDDEN);
            let mut foreign = headers();
            foreign.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
            let response = post_action(State(state), foreign, Path(("0123456789ab".into(), "start".into()))).await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }

        #[tokio::test]
        async fn a_bad_id_or_action_answers_400_without_running_docker() {
            let (state, dir) = shared(true);
            let marker = dir.path().join("ran");
            fake_docker(&state, dir.path(), &format!("touch '{}'", marker.display())).await;
            for (id, action) in [("ABC", "start"), ("0123456789ab", "kill"), ("0123456789ab", "rm"), ("--all", "stop")] {
                let response = post_action(State(state.clone()), headers(), Path((id.into(), action.into()))).await;
                assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{id} {action}");
            }
            assert!(!marker.exists());
        }

        #[tokio::test]
        async fn an_action_answers_204_or_502_with_the_error_line() {
            let (state, dir) = shared(true);
            fake_docker(&state, dir.path(), "exit 0").await;
            let response = post_action(State(state.clone()), headers(), Path(("0123456789ab".into(), "start".into()))).await;
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
            assert!(text(response).await.is_empty());

            fake_docker(&state, dir.path(), "echo 'Error: No such container: abc' >&2\necho more >&2\nexit 1").await;
            let response = post_action(State(state), headers(), Path(("0123456789ab".into(), "remove".into()))).await;
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
            assert_eq!(text(response).await, r#"{"error":"Error: No such container: abc"}"#);
        }

        #[tokio::test]
        async fn the_state_route_reads_the_engine() {
            let (state, dir) = shared(true);
            fake_docker(&state, dir.path(), "case \"$1\" in\n version) echo 27.3.1 ;;\n *) ;;\nesac").await;
            let response = get_state(State(state.clone()), headers(), query(None)).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["cache-control"], "no-store");
            assert_eq!(text(response).await, r#"{"engine":{"status":"ok","version":"27.3.1","error":null},"containers":[]}"#);
        }

        #[tokio::test]
        async fn retry_reads_the_environment_again() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("docker");
            std::fs::write(&path, "#!/bin/sh\necho 27.3.1\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            let good = || async { LoginEnv { vars: vec![("PATH".into(), format!("{}:/bin:/usr/bin", dir.path().display()))] } };
            let nowhere = || async { LoginEnv { vars: vec![("PATH".into(), "/nowhere".into())] } };
            let cache = LoginEnvCache::default();
            assert_eq!(current_state(&cache, false, nowhere).await.engine.status, Status::NoCli);
            assert_eq!(current_state(&cache, false, good).await.engine.status, Status::NoCli, "remembered");
            assert_eq!(current_state(&cache, true, good).await.engine.status, Status::Ok, "retry reads again");
        }
    }

    #[cfg(unix)]
    mod fake {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        use std::path::Path;
        use std::time::Duration;

        const READ: Duration = Duration::from_secs(5);

        /// A folder with an executable `name` that runs `body`.
        fn script(dir: &Path, name: &str, body: &str) {
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        /// An env whose PATH is `dir` plus the system tools a script needs.
        fn env_for(dir: &Path) -> DockerEnv {
            DockerEnv { vars: vec![("PATH".into(), format!("{}:/bin:/usr/bin", dir.display()))] }
        }

        #[tokio::test]
        async fn missing_docker_reads_as_no_cli() {
            let dir = tempfile::tempdir().unwrap();
            let env = DockerEnv { vars: vec![("PATH".into(), dir.path().display().to_string())] };
            let state = read_state(&env, READ).await;
            assert_eq!(state.engine.status, Status::NoCli);
            assert!(state.containers.is_empty());
        }

        #[tokio::test]
        async fn a_stopped_daemon_reads_as_down() {
            let dir = tempfile::tempdir().unwrap();
            script(dir.path(), "docker", "echo 'Cannot connect to the Docker daemon' >&2\necho 'second line' >&2\nexit 1");
            let state = read_state(&env_for(dir.path()), READ).await;
            assert_eq!(state.engine.status, Status::Down);
            assert_eq!(state.engine.error.as_deref(), Some("Cannot connect to the Docker daemon"));
        }

        #[tokio::test]
        async fn a_hung_docker_reads_as_down() {
            let dir = tempfile::tempdir().unwrap();
            script(dir.path(), "docker", "exec sleep 5");
            let started = std::time::Instant::now();
            let state = read_state(&env_for(dir.path()), Duration::from_millis(200)).await;
            assert_eq!(state.engine.status, Status::Down);
            assert_eq!(state.engine.error.as_deref(), Some("timed out"));
            assert!(started.elapsed() < Duration::from_secs(3));
        }

        #[tokio::test]
        async fn a_running_engine_lists_its_containers() {
            let dir = tempfile::tempdir().unwrap();
            let fixture = dir.path().join("inspect.json");
            std::fs::write(&fixture, FIXTURE).unwrap();
            let log = dir.path().join("args");
            let body = format!(
                "case \"$1\" in\n version) echo 27.3.1 ;;\n ps) echo 9f1c2d3e4a5b; echo 0a1b2c3d4e5f ;;\n inspect) echo \"$@\" > '{}'; cat '{}' ;;\nesac",
                log.display(),
                fixture.display()
            );
            script(dir.path(), "docker", &body);
            let state = read_state(&env_for(dir.path()), READ).await;
            assert_eq!(state.engine.status, Status::Ok);
            assert_eq!(state.engine.version.as_deref(), Some("27.3.1"));
            assert_eq!(state.containers.len(), 2);
            assert_eq!(state.containers[0].name, "westerm-site-api-1");
            // `--type container`: a vanished id never matches an image or a volume.
            assert_eq!(std::fs::read_to_string(&log).unwrap(), "inspect --type container 9f1c2d3e4a5b 0a1b2c3d4e5f\n");
        }

        #[tokio::test]
        async fn no_containers_skip_the_inspect() {
            let dir = tempfile::tempdir().unwrap();
            // `inspect` with no ids would fail; the read must not call it.
            script(dir.path(), "docker", "case \"$1\" in\n version) echo 27.3.1 ;;\n ps) ;;\n *) exit 1 ;;\nesac");
            let state = read_state(&env_for(dir.path()), READ).await;
            assert_eq!(state.engine.status, Status::Ok);
            assert!(state.containers.is_empty());
        }

        #[tokio::test]
        async fn only_the_first_200_ids_are_inspected() {
            let dir = tempfile::tempdir().unwrap();
            let body = "case \"$1\" in\n version) echo 27.3.1 ;;\n ps) i=0; while [ $i -lt 250 ]; do echo 0123456789ab; i=$((i+1)); done ;;\n inspect) [ \"$2 $3\" = \"--type container\" ] || exit 4; shift 3; echo \"[]\"; echo $# >&2; [ $# -eq 200 ] || exit 3 ;;\nesac";
            script(dir.path(), "docker", body);
            let state = read_state(&env_for(dir.path()), READ).await;
            assert_eq!(state.engine.status, Status::Ok, "{:?}", state.engine.error);
        }

        #[tokio::test]
        async fn an_action_failure_returns_the_first_error_line() {
            let dir = tempfile::tempdir().unwrap();
            script(dir.path(), "docker", "echo 'Error: No such container: abc' >&2\necho more >&2\nexit 1");
            let result = run_action(&env_for(dir.path()), "0123456789ab", "remove", READ).await;
            assert_eq!(result, Err("Error: No such container: abc".to_owned()));
        }

        #[tokio::test]
        async fn an_action_passes_its_arguments_as_an_array() {
            let dir = tempfile::tempdir().unwrap();
            let log = dir.path().join("args");
            script(dir.path(), "docker", &format!("echo \"$@\" > '{}'", log.display()));
            run_action(&env_for(dir.path()), "0123456789ab", "remove", READ).await.unwrap();
            assert_eq!(std::fs::read_to_string(&log).unwrap(), "rm 0123456789ab\n");
            assert!(run_action(&env_for(dir.path()), "0123456789ab", "kill", READ).await.is_err());
            assert!(run_action(&env_for(dir.path()), "ABC", "start", READ).await.is_err());
        }

        #[tokio::test]
        async fn a_hung_action_times_out() {
            let dir = tempfile::tempdir().unwrap();
            script(dir.path(), "docker", "exec sleep 5");
            let result = run_action(&env_for(dir.path()), "0123456789ab", "stop", Duration::from_millis(200)).await;
            assert_eq!(result, Err("timed out".to_owned()));
        }
    }
}
