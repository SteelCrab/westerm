//! The PTY host: a background process that owns every terminal process, so
//! shells survive dashboard restarts and page reloads. It listens only on a
//! Unix socket (0600) inside the private state directory.

mod registry;
mod session;
pub mod store;

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::{mpsc, oneshot, Notify};

use crate::protocol::AttachRequest;
use crate::state_dir::{host_socket, state_dir};
use registry::RegistryCommand;
use store::SessionStore;

/// Build id of this binary's host code; see build.rs.
pub const HOST_BUILD: &str = env!("WESTERM_HOST_BUILD");
/// Browser frames (pastes) up to this size.
pub const MAX_MESSAGE: usize = 8 * 1024 * 1024;

#[derive(Clone)]
struct AppState {
    registry: mpsc::UnboundedSender<RegistryCommand>,
    stop: Arc<Notify>,
}

pub async fn run() -> std::io::Result<()> {
    let dir = state_dir()?;
    let socket = host_socket(&dir);
    let store = Arc::new(SessionStore::new(&dir)?);
    store.prune();
    let registry = registry::start(store);
    let stop = Arc::new(Notify::new());
    let state = AppState { registry: registry.clone(), stop: stop.clone() };

    let app = Router::new()
        .route("/health", get(health))
        .route("/shutdown", post(shutdown))
        .route("/session", get(attach))
        .with_state(state);

    // A host that still answers owns this socket. Deleting the file would
    // orphan it with all its shells, so refuse instead. A file nobody answers
    // on is stale and can go.
    if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!("another PTY host is already running on {}", socket.display()),
        ));
    }
    let _ = fs::remove_file(&socket);
    let listener = tokio::net::UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    println!("pty host {HOST_BUILD} listening on {} (pid {})", socket.display(), std::process::id());

    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut hangup = signal(SignalKind::hangup())?;
    // SIGTERM arrives on logout and reboot; save the screens before exiting.
    let reason = tokio::select! {
        result = axum::serve(listener, app) => format!("server stopped: {result:?}"),
        () = stop.notified() => "requested".to_owned(),
        _ = terminate.recv() => "SIGTERM".to_owned(),
        _ = interrupt.recv() => "SIGINT".to_owned(),
        _ = hangup.recv() => "SIGHUP".to_owned(),
    };
    println!("pty host stopping ({reason}), saving sessions");
    let (reply, saved) = oneshot::channel();
    if registry.send(RegistryCommand::SaveAll { reply }).is_ok() {
        let _ = saved.await;
    }
    let _ = fs::remove_file(&socket);
    // Exiting closes the PTY masters, which hangs up the shells.
    std::process::exit(0);
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let (reply, count) = oneshot::channel();
    let _ = state.registry.send(RegistryCommand::Count { reply });
    let sessions = count.await.unwrap_or(0);
    Json(serde_json::json!({ "build": HOST_BUILD, "pid": std::process::id(), "sessions": sessions }))
}

async fn shutdown(State(state): State<AppState>) -> StatusCode {
    state.stop.notify_one();
    StatusCode::NO_CONTENT
}

async fn attach(State(state): State<AppState>, Query(query): Query<HashMap<String, String>>, ws: WebSocketUpgrade) -> Response {
    let Some(request) = AttachRequest::from_query(&query) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    ws.max_message_size(MAX_MESSAGE).on_upgrade(move |ws| async move {
        let _ = state.registry.send(RegistryCommand::Attach { ws, request });
    })
}
