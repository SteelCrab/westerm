//! The PTY host: a background process that owns every terminal process, so
//! shells survive dashboard restarts and page reloads. It listens only on a
//! private local channel (see ipc.rs): a Unix socket or a Windows named pipe.

pub(crate) mod agent;
mod registry;
mod session;
pub mod store;

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::sync::{mpsc, oneshot, Notify};

use crate::ipc;
use crate::protocol::{AttachRequest, PROTOCOL};
use crate::state_dir::{private_file, state_dir};
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

    // One host per state directory. Replacing a running host would orphan it
    // with all its shells, so a second one refuses. The lock is released when
    // the process exits, also after a crash. Hosts from before the lock
    // existed are caught by the connect check.
    let lock = private_file().create(true).truncate(false).write(true).open(dir.join("host.lock"))?;
    if lock.try_lock().is_err() || ipc::connect(&dir).await.is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!("another PTY host is already running on {}", ipc::describe(&dir)),
        ));
    }
    let listener = ipc::bind(&dir)?;
    #[cfg(unix)]
    let listener = {
        use axum::serve::ListenerExt;
        listener.tap_io(|stream| ipc::tune(stream))
    };
    println!("pty host {HOST_BUILD} listening on {} (pid {})", ipc::describe(&dir), std::process::id());

    let reason = tokio::select! {
        result = axum::serve(listener, app) => format!("server stopped: {result:?}"),
        () = stop.notified() => "requested".to_owned(),
        signal = termination() => signal.to_owned(),
    };
    println!("pty host stopping ({reason}), saving sessions");
    let (reply, saved) = oneshot::channel();
    if registry.send(RegistryCommand::SaveAll { reply }).is_ok() {
        let _ = saved.await;
    }
    ipc::unbind(&dir);
    drop(lock);
    // Exiting closes the PTY masters, which hangs up the shells.
    std::process::exit(0);
}

/// SIGTERM arrives on logout and reboot; the screens are saved before exit.
#[cfg(unix)]
async fn termination() -> &'static str {
    use tokio::signal::unix::{signal, SignalKind};
    let (Ok(mut terminate), Ok(mut interrupt), Ok(mut hangup)) =
        (signal(SignalKind::terminate()), signal(SignalKind::interrupt()), signal(SignalKind::hangup()))
    else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = terminate.recv() => "SIGTERM",
        _ = interrupt.recv() => "SIGINT",
        _ = hangup.recv() => "SIGHUP",
    }
}

/// A detached host gets these only while it has a console. Screens are also
/// saved 3 s after output, so a shutdown without notice loses little.
#[cfg(windows)]
async fn termination() -> &'static str {
    use tokio::signal::windows::{ctrl_c, ctrl_close, ctrl_logoff, ctrl_shutdown};
    let (Ok(mut interrupt), Ok(mut close), Ok(mut logoff), Ok(mut shutdown)) =
        (ctrl_c(), ctrl_close(), ctrl_logoff(), ctrl_shutdown())
    else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = interrupt.recv() => "Ctrl+C",
        _ = close.recv() => "console closed",
        _ = logoff.recv() => "logoff",
        _ = shutdown.recv() => "shutdown",
    }
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let (reply, count) = oneshot::channel();
    let _ = state.registry.send(RegistryCommand::Count { reply });
    let sessions = count.await.unwrap_or(0);
    Json(serde_json::json!({ "build": HOST_BUILD, "protocol": PROTOCOL, "pid": std::process::id(), "sessions": sessions }))
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
