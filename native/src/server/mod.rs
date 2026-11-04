//! The dashboard server: the web UI (embedded in the binary), small APIs, and
//! a WebSocket relay from each browser pane to its session in the PTY host.
//! Listens on 127.0.0.1 only.

mod config;
mod host_client;
mod updates;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use rust_embed::RustEmbed;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message as HostMessage;

use crate::host::MAX_MESSAGE;
use crate::protocol::AttachRequest;
use crate::state_dir::{home_dir, state_dir};
use host_client::HostClient;

#[derive(RustEmbed)]
#[folder = "../dist/"]
struct Assets;

pub struct Options {
    pub port: u16,
    /// Extra allowed page origins, e.g. the Vite dev server.
    pub allow_origins: Vec<String>,
}

struct AppState {
    host: HostClient,
    origins: Vec<String>,
    state_dir: PathBuf,
    updates: updates::UpdateCheck,
}

type Shared = Arc<AppState>;

/// The port this dashboard listens on; a restart after an update keeps it,
/// also when it was chosen with --port 0.
static BOUND_PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();

/// `westerm stop`: the host saves every session and exits. Shells come back
/// as "restored" when westerm starts again.
pub async fn stop_host() -> std::io::Result<()> {
    if host_client::stop_running().await? {
        println!("westerm: the PTY host saved its sessions and stopped");
    } else {
        println!("westerm: no PTY host is running");
    }
    Ok(())
}

pub async fn run(options: Options) -> std::io::Result<()> {
    // Local shells must never listen on a public interface. Port 0 asks the
    // OS for any free port; the origins below use the port actually bound.
    let listener = bind(options.port).await?;
    let port = listener.local_addr()?.port();
    let _ = BOUND_PORT.set(port);
    let mut origins = vec![format!("http://127.0.0.1:{port}"), format!("http://localhost:{port}")];
    origins.extend(options.allow_origins);
    // Before the rename the state lived elsewhere; move it before serving.
    host_client::migrate_legacy_state().await;
    let state = Arc::new(AppState {
        host: HostClient::default(),
        origins,
        state_dir: state_dir()?,
        updates: updates::UpdateCheck::default(),
    });
    // Start the PTY host now so the first pane does not wait for it.
    if let Err(error) = state.host.ensure().await {
        eprintln!("> {error}");
    }

    let app = Router::new()
        .route("/api/terminal-config", get(terminal_config))
        .route("/api/update", get(updates::get_update).post(updates::post_update))
        .route("/api/workspace", get(get_workspace).put(put_workspace))
        .route("/api/ssh-hosts", get(ssh_hosts))
        .route("/ws/session", get(session))
        .fallback(asset)
        .with_state(state);
    println!("> westerm: http://127.0.0.1:{port}");
    axum::serve(listener, app).await
}

/// Binds 127.0.0.1:`port`. After an update on Windows the new process starts
/// before the old one has let go of the port, so it waits a little.
async fn bind(port: u16) -> std::io::Result<tokio::net::TcpListener> {
    let restarting = std::env::var_os("WESTERM_RESTARTING").is_some();
    let mut attempts = 0;
    loop {
        match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse && restarting && attempts < 25 => {
                attempts += 1;
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                return Err(std::io::Error::new(
                    error.kind(),
                    format!("port {port} is in use; choose another with --port N, or --port 0 for any free port"),
                ));
            }
            Err(error) => return Err(error),
        }
    }
}

/// A page on any other site can reach 127.0.0.1. Only the dashboard's own
/// origin may open sessions or change the layout.
fn origin_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| state.origins.iter().any(|allowed| allowed == origin))
}

fn no_store(body: impl IntoResponse) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], body).into_response()
}

async fn terminal_config() -> Response {
    no_store(Json(config::read_terminal_config()))
}

async fn ssh_hosts() -> Response {
    // Aliases and host names only; nothing else from the config leaves.
    no_store(Json(config::read_ssh_hosts(&home_dir().join(".ssh"))))
}

async fn get_workspace(State(state): State<Shared>) -> Response {
    let saved = std::fs::read(config::workspace_path(&state.state_dir)).unwrap_or_else(|_| b"null".to_vec());
    no_store(([(header::CONTENT_TYPE, "application/json")], saved))
}

/// A form on another site can POST; a cross-site JSON PUT needs a CORS
/// preflight this server never answers. The Origin check covers the rest.
async fn put_workspace(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    if json && origin_allowed(&state, &headers) && config::write_workspace(&state.state_dir, &body) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::BAD_REQUEST
    }
}

async fn session(
    State(state): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(request) = AttachRequest::from_query(&query).filter(|_| origin_allowed(&state, &headers)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let dir = match state.host.ensure().await {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("> {error}");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    // Only validated fields reach the host.
    let query = request.to_query();
    ws.max_message_size(MAX_MESSAGE).on_upgrade(move |browser| relay(browser, dir, query, state))
}

/// Codes a server may send in a close frame; others become 1011.
fn sendable(code: u16) -> u16 {
    if code == 1000 || (3000..=4999).contains(&code) { code } else { 1011 }
}

/// Pipes a browser pane to its host session, frame for frame. A closed
/// browser only detaches: the host keeps text sessions running.
async fn relay(browser: WebSocket, dir: PathBuf, query: String, state: Shared) {
    let host = async {
        let stream = crate::ipc::connect(&dir).await.ok()?;
        let config = WebSocketConfig::default().max_message_size(None).max_frame_size(None);
        let url = format!("ws://host/session?{query}");
        tokio_tungstenite::client_async_with_config(url, stream, Some(config)).await.ok().map(|(ws, _)| ws)
    };
    let Some(host) = host.await else {
        eprintln!("> pty host connection failed");
        state.host.invalidate().await;
        let mut browser = browser;
        let _ = browser.send(Message::Close(Some(CloseFrame { code: 1011, reason: "pty host unavailable".into() }))).await;
        return;
    };

    let (mut to_browser, mut from_browser) = browser.split();
    let (mut to_host, mut from_host) = host.split();
    let upstream = async {
        while let Some(Ok(message)) = from_browser.next().await {
            let forwarded = match message {
                Message::Text(text) => HostMessage::text(text.as_str()),
                Message::Binary(bytes) => HostMessage::Binary(bytes),
                Message::Close(_) => break,
                _ => continue,
            };
            if to_host.send(forwarded).await.is_err() {
                break;
            }
        }
        let _ = to_host.close().await;
    };
    let downstream = async {
        while let Some(Ok(message)) = from_host.next().await {
            let forwarded = match message {
                HostMessage::Text(text) => Message::Text(text.as_str().into()),
                HostMessage::Binary(bytes) => Message::Binary(bytes),
                HostMessage::Close(frame) => {
                    let (code, reason) = frame.map(|f| (u16::from(f.code), f.reason.to_string())).unwrap_or((1000, String::new()));
                    let close = CloseFrame { code: sendable(code), reason: reason.into() };
                    let _ = to_browser.send(Message::Close(Some(close))).await;
                    break;
                }
                _ => continue,
            };
            if to_browser.send(forwarded).await.is_err() {
                break;
            }
        }
    };
    tokio::select! {
        () = upstream => {}
        () = downstream => {}
    }
}

/// The web UI, with index.html for unknown paths (a single-page app).
async fn asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let (path, file) = match Assets::get(path) {
        Some(file) => (path, file),
        None => match Assets::get("index.html") {
            Some(file) => ("index.html", file),
            None => return (StatusCode::NOT_FOUND, "web UI not built: run `npm run build`").into_response(),
        },
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    // Hashed asset names never change content; the page itself must stay fresh.
    let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    ([(header::CONTENT_TYPE, mime.as_ref().to_owned()), (header::CACHE_CONTROL, cache.to_owned())], file.data).into_response()
}
