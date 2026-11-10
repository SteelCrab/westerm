//! The dashboard server: the web UI (embedded in the binary), small APIs, and
//! a WebSocket relay from each browser pane to its session in the PTY host.
//! Listens on 127.0.0.1 only. On a server, `remote_router` serves the few
//! routes a remote window needs, on a private Unix socket instead.

mod agents;
mod cloudflare;
mod config;
mod docs;
mod git;
mod fonts;
pub(crate) mod host_client;
mod http1;
mod remote;
mod remote_window;
mod updates;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use rust_embed::RustEmbed;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message as HostMessage;
use tokio_tungstenite::WebSocketStream;

use crate::host::MAX_MESSAGE;
use crate::protocol::{
    is_id, normalize_ssh_target, AttachRequest, ClientMessage, ServerMessage, REMOTE_LINK_PATH, REMOTE_RETRY_PATH,
};
use crate::state_dir::{home_dir, state_dir};
use host_client::HostClient;
use remote::RemoteManager;

// Keep the canonical path intact: appending '/' breaks Windows verbatim paths.
#[derive(RustEmbed)]
#[folder = "$WESTERM_WEB_DIST"]
struct Assets;

pub struct Options {
    pub port: u16,
    /// Extra allowed page origins, e.g. the Vite dev server.
    pub allow_origins: Vec<String>,
}

struct AppState {
    /// Shared with the tunnel listener's state: one connection to one host.
    host: Arc<HostClient>,
    origins: Vec<String>,
    state_dir: PathBuf,
    updates: updates::UpdateCheck,
    /// Links to remote westerm servers, one per server.
    remote: Arc<RemoteManager>,
    /// True when every caller is already trusted, as on the server's private
    /// socket: no Origin is checked.
    trusted: bool,
    /// The Cloudflare tunnel's controls; only the local dashboard has them.
    tunnel: Option<Arc<cloudflare::TunnelManager>>,
}

type Shared = Arc<AppState>;

/// The port this dashboard listens on; a restart after an update keeps it,
/// also when it was chosen with --port 0.
static BOUND_PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();

/// `westerm stop`: the host saves every session and exits. Shells come back
/// as "restored" when westerm starts again.
pub async fn stop_host() -> std::io::Result<()> {
    // On a server, the gate and the server dashboard go with the host, and
    // before it: a remote window's pane that retries in between would make
    // the dashboard start a new host.
    #[cfg(unix)]
    if crate::remote::gate::stop_running(&state_dir()?).await? {
        println!("westerm: the remote gate stopped");
    }
    #[cfg(unix)]
    if crate::remote::dashboard::stop_running(&state_dir()?).await? {
        println!("westerm: the remote dashboard stopped");
    }
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
    let host = Arc::new(HostClient::default());
    let remote = Arc::new(RemoteManager::new(state_dir()?));
    let tunnel = cloudflare::TunnelManager::new(Arc::clone(&host), Arc::clone(&remote), state_dir()?);
    let state = Arc::new(AppState {
        host,
        origins,
        state_dir: state_dir()?,
        updates: updates::UpdateCheck::default(),
        remote,
        trusted: false,
        tunnel: Some(tunnel),
    });
    // Start the PTY host now so the first pane does not wait for it.
    if let Err(error) = state.host.ensure().await {
        eprintln!("> {error}");
    }

    let app = local_router(state);
    println!("> westerm: http://127.0.0.1:{port}");
    axum::serve(listener, app).await
}

/// Every route of the dashboard on 127.0.0.1.
fn local_router(state: Shared) -> Router {
    Router::new()
        .route("/api/terminal-config", get(terminal_config))
        .route("/api/update", get(updates::get_update).post(updates::post_update))
        .route("/api/workspace", get(get_workspace).put(put_workspace))
        .route("/api/ssh-hosts", get(ssh_hosts))
        .route(REMOTE_LINK_PATH, get(remote_link))
        .route(REMOTE_RETRY_PATH, post(remote_retry))
        .route("/api/remote/decision", post(remote_decision))
        .route("/api/remote/close", post(remote_close))
        .route("/api/remote/update-host", post(remote_update_host))
        .route("/ws/session", get(session))
        // The page asks this first; the tunnel listener answers it differently.
        .route(cloudflare::AUTH_PATH, get(|| async { no_store(Json(serde_json::json!({ "tunnel": false }))) }))
        .merge(docs::routes())
        .merge(agents::routes())
        .merge(git::routes())
        .merge(fonts::routes())
        .merge(remote_window::routes())
        .merge(cloudflare::control_routes())
        .fallback(asset)
        .with_state(state)
}

/// The server's dashboard for remote windows, served on a Unix socket only the
/// user can open. It answers the layout, the SSH host list, and sessions; every
/// other path is 404. It leaves a running host as it is.
#[cfg(unix)]
pub fn remote_router(state_dir: PathBuf) -> Router {
    let state = Arc::new(AppState {
        host: Arc::new(HostClient::keeping_running()),
        origins: Vec::new(),
        remote: Arc::new(RemoteManager::new(state_dir.clone())),
        state_dir,
        updates: updates::UpdateCheck::default(),
        trusted: true,
        tunnel: None,
    });
    Router::new()
        .route("/api/workspace", get(get_workspace).put(put_workspace))
        .route("/api/ssh-hosts", get(ssh_hosts))
        .route("/ws/session", get(session))
        .merge(docs::routes())
        .merge(agents::routes())
        .merge(git::routes())
        .fallback(|| async { StatusCode::NOT_FOUND })
        .with_state(state)
}

/// `westerm tunnel-guard -- <program> <args…>` (see cloudflare/guard.rs).
pub fn tunnel_guard(program: std::ffi::OsString, args: Vec<std::ffi::OsString>) -> i32 {
    cloudflare::guard::run(program, args)
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
    state.trusted
        || headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| state.origins.iter().any(|allowed| allowed == origin))
}

/// The Host header names one of this dashboard's own origins. A page on
/// another name can point at 127.0.0.1 (DNS rebinding) and then pass the
/// browser's same-origin checks; only the names above are served.
fn host_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| {
            state
                .origins
                .iter()
                .filter_map(|origin| origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://")))
                .any(|allowed| allowed == host)
        })
}

/// A browser says the request comes from another site: `Sec-Fetch-Site` is
/// `cross-site` or `same-site`, or an `Origin` is present that is not ours.
/// A request with neither header (not from a browser) is not cross-site.
/// For routes that do work on a plain GET, which any page can fire at 127.0.0.1.
fn cross_site(state: &AppState, headers: &HeaderMap) -> bool {
    let fetch_site = headers.get("sec-fetch-site").and_then(|value| value.to_str().ok());
    matches!(fetch_site, Some("cross-site" | "same-site")) || (headers.contains_key(header::ORIGIN) && !origin_allowed(state, headers))
}

/// A read from the dashboard's own page. The server's private socket trusts every caller.
fn read_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    state.trusted || (host_allowed(state, headers) && !cross_site(state, headers))
}

/// The request says its body is JSON.
fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"))
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
    if is_json(&headers) && origin_allowed(&state, &headers) && config::write_workspace(&state.state_dir, &body) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::BAD_REQUEST
    }
}

/// A JSON body from the dashboard's own page; None for anything else.
fn json_body<T: DeserializeOwned>(state: &AppState, headers: &HeaderMap, body: &Bytes) -> Option<T> {
    if !is_json(headers) || !origin_allowed(state, headers) {
        return None;
    }
    serde_json::from_slice(body).ok()
}

#[derive(Deserialize)]
struct DecisionBody {
    target: String,
    replace: bool,
}

#[derive(Deserialize)]
struct CloseBody {
    target: String,
    session: String,
}

#[derive(Deserialize)]
struct TargetBody {
    target: String,
}

/// A remote window's status stream: "hello" with this westerm's version,
/// then the link's stage each time it changes, the current one first. The
/// link to the server stays up while a window keeps this open. A page on
/// another site must not make ssh run, so the Origin is checked too.
async fn remote_link(
    State(state): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(target) = query.get("target").and_then(|target| normalize_ssh_target(target)) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !host_allowed(&state, &headers) || !origin_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| link_status(socket, state, target))
}

/// Sends the stages until the window closes the stream, which drops its `Watch`.
async fn link_status(mut socket: WebSocket, state: Shared, target: String) {
    let mut watch = state.remote.watch(&target);
    let hello = ServerMessage::Hello { version: crate::version_line() }.to_json();
    if socket.send(Message::Text(hello.into())).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            changed = watch.stages.changed() => {
                if changed.is_err() {
                    return;
                }
                let Some(stage) = watch.stages.borrow_and_update().clone() else { continue };
                if socket.send(Message::Text(ServerMessage::Remote(stage).to_json().into())).await.is_err() {
                    return;
                }
            }
            message = socket.recv() => match message {
                Some(Ok(Message::Close(_)) | Err(_)) | None => return,
                Some(Ok(_)) => {}
            },
        }
    }
}

/// "Try again" after a permanent failure. 409 when no link waits for it.
async fn remote_retry(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let Some(TargetBody { target }) = json_body(&state, &headers, &body) else {
        return StatusCode::BAD_REQUEST;
    };
    let Some(target) = normalize_ssh_target(&target) else { return StatusCode::BAD_REQUEST };
    if state.remote.retry(&target) { StatusCode::NO_CONTENT } else { StatusCode::CONFLICT }
}

/// The answer to "replace the server's older host?". 409 when nothing asks.
async fn remote_decision(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let Some(DecisionBody { target, replace }) = json_body(&state, &headers, &body) else {
        return StatusCode::BAD_REQUEST;
    };
    let Some(target) = normalize_ssh_target(&target) else { return StatusCode::BAD_REQUEST };
    if state.remote.decide(&target, replace) { StatusCode::NO_CONTENT } else { StatusCode::CONFLICT }
}

/// A remote session closed while its socket could not say so.
async fn remote_close(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let Some(CloseBody { target, session }) = json_body(&state, &headers, &body) else {
        return StatusCode::BAD_REQUEST;
    };
    let Some(target) = normalize_ssh_target(&target).filter(|_| is_id(&session)) else {
        return StatusCode::BAD_REQUEST;
    };
    state.remote.forget_session(&target, &session).await;
    StatusCode::NO_CONTENT
}

/// Replaces a server's older host now (after the user kept it once).
async fn remote_update_host(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(TargetBody { target }) = json_body(&state, &headers, &body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(target) = normalize_ssh_target(&target) else { return StatusCode::BAD_REQUEST.into_response() };
    match state.remote.update_host(&target).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(failure) => {
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({ "reason": failure.reason, "detail": failure.detail }))).into_response()
        }
    }
}

async fn session(
    State(state): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(request) = session_request(&state, &headers, &query) else {
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

/// A pane's attach request, from this dashboard's own page only.
fn session_request(state: &AppState, headers: &HeaderMap, query: &HashMap<String, String>) -> Option<AttachRequest> {
    AttachRequest::from_query(query).filter(|_| origin_allowed(state, headers))
}

/// Codes a server may send in a close frame; others become 1011.
fn sendable(code: u16) -> u16 {
    if code == 1000 || (3000..=4999).contains(&code) { code } else { 1011 }
}

/// Pipes a browser pane to its host session, frame for frame. A closed
/// browser only detaches: the host keeps text sessions running.
async fn relay(browser: WebSocket, dir: PathBuf, query: String, state: Shared) {
    let Some((host, browser)) = connect_host(browser, &dir, &query, &state).await else { return };
    let (to_browser, from_browser) = browser.split();
    pipe(to_browser, from_browser, host).await;
}

/// The pane's session in the host. When the host cannot be reached, the
/// browser is told with 1011 and None comes back.
async fn connect_host(
    mut browser: WebSocket,
    dir: &std::path::Path,
    query: &str,
    state: &AppState,
) -> Option<(WebSocketStream<crate::ipc::Stream>, WebSocket)> {
    let host = async {
        let stream = crate::ipc::connect(dir).await.ok()?;
        let config = WebSocketConfig::default().max_message_size(None).max_frame_size(None);
        let url = format!("ws://host/session?{query}");
        tokio_tungstenite::client_async_with_config(url, stream, Some(config)).await.ok().map(|(ws, _)| ws)
    };
    match host.await {
        Some(host) => Some((host, browser)),
        None => {
            eprintln!("> pty host connection failed");
            state.host.invalidate().await;
            let _ = browser.send(close_message(1011, "pty host unavailable")).await;
            None
        }
    }
}

/// What a tunnel pane's relay adds to a local one.
struct RelayExtra {
    /// True (or dropped) when the tunnel ends: the browser gets CLOSE_TUNNEL_ENDED.
    ended: tokio::sync::watch::Receiver<bool>,
    /// Cloudflare closes WebSockets that stay quiet; a quiet shell is common.
    ping: std::time::Duration,
    /// Runs once when the relay ends, however it ends.
    on_end: Box<dyn FnOnce() + Send>,
}

/// Like `relay`, for the tunnel listener. A closed browser or an ended tunnel
/// only detaches: the shell keeps running in the host.
async fn relay_with(browser: WebSocket, dir: PathBuf, query: String, state: Shared, extra: RelayExtra) {
    let RelayExtra { mut ended, ping, on_end } = extra;
    let Some((host, browser)) = connect_host(browser, &dir, &query, &state).await else {
        on_end();
        return;
    };
    let (mut to_browser, mut from_browser) = browser.split();
    let (mut to_host, mut from_host) = host.split();
    let mut pings = tokio::time::interval_at(tokio::time::Instant::now() + ping, ping);
    loop {
        tokio::select! {
            message = from_browser.next() => {
                let forwarded = match message {
                    Some(Ok(Message::Text(text))) => HostMessage::text(text.as_str()),
                    Some(Ok(Message::Binary(bytes))) => HostMessage::Binary(bytes),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    _ => break,
                };
                if to_host.send(forwarded).await.is_err() {
                    break;
                }
            }
            message = from_host.next() => {
                let forwarded = match message {
                    Some(Ok(HostMessage::Text(text))) => Message::Text(text.as_str().into()),
                    Some(Ok(HostMessage::Binary(bytes))) => Message::Binary(bytes),
                    Some(Ok(HostMessage::Close(frame))) => {
                        let (code, reason) = frame.map(|f| (u16::from(f.code), f.reason.to_string())).unwrap_or((1000, String::new()));
                        let _ = to_browser.send(close_message(sendable(code), &reason)).await;
                        break;
                    }
                    Some(Ok(_)) => continue,
                    _ => break,
                };
                if to_browser.send(forwarded).await.is_err() {
                    break;
                }
            }
            _ = pings.tick() => {
                if to_browser.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
            }
            // The guard that wait_for returns must not live across the send below.
            () = async { drop(ended.wait_for(|ended| *ended).await) } => {
                let _ = to_browser.send(close_message(cloudflare::CLOSE_TUNNEL_ENDED, "tunnel ended")).await;
                break;
            }
        }
    }
    let _ = to_host.close().await;
    on_end();
}

/// How long a remote host may take to accept a session on a fresh stream.
/// A silent one (a dead gate, a lost network) gets the link replaced.
const REMOTE_HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn close_message(code: u16, reason: &str) -> Message {
    Message::Close(Some(CloseFrame { code, reason: reason.into() }))
}

/// How long the host may take to answer a pane's close before a remote
/// pane treats the close as lost.
const CLOSE_ANSWER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// How a pipe ended. `session_closed`: the pane sent "close" (the user
/// closed it) before leaving. `host_answered`: the host then answered
/// within CLOSE_ANSWER_TIMEOUT, so the close arrived.
struct PipeEnd {
    session_closed: bool,
    host_answered: bool,
}

/// Pipes a browser pane to its host session, frame for frame. A closed
/// browser only detaches: the host keeps text sessions running.
async fn pipe<S>(
    mut to_browser: SplitSink<WebSocket, Message>,
    mut from_browser: SplitStream<WebSocket>,
    host: WebSocketStream<S>,
) -> PipeEnd
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut to_host, mut from_host) = host.split();
    let upstream = async {
        let mut session_closed = false;
        while let Some(Ok(message)) = from_browser.next().await {
            let forwarded = match message {
                Message::Text(text) => {
                    session_closed |= matches!(ClientMessage::parse(&text), Some(ClientMessage::Close));
                    HostMessage::text(text.as_str())
                }
                Message::Binary(bytes) => HostMessage::Binary(bytes),
                Message::Close(_) => break,
                _ => continue,
            };
            if to_host.send(forwarded).await.is_err() {
                break;
            }
        }
        let _ = to_host.close().await;
        session_closed
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
    tokio::pin!(upstream, downstream);
    let session_closed = tokio::select! {
        closed = &mut upstream => closed,
        () = &mut downstream => return PipeEnd { session_closed: false, host_answered: true },
    };
    // The host answers a close by ending the session and its socket.
    let host_answered = !session_closed || tokio::time::timeout(CLOSE_ANSWER_TIMEOUT, &mut downstream).await.is_ok();
    PipeEnd { session_closed, host_answered }
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

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn test_state(origins: &[&str]) -> AppState {
        AppState {
            host: Arc::new(HostClient::default()),
            origins: origins.iter().map(|origin| (*origin).to_owned()).collect(),
            state_dir: PathBuf::new(),
            updates: updates::UpdateCheck::default(),
            remote: Arc::new(RemoteManager::new(PathBuf::new())),
            trusted: false,
            tunnel: None,
        }
    }

    fn headers_with_host(host: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, host.parse().unwrap());
        headers
    }

    #[test]
    fn host_allowed_follows_the_allowed_origins() {
        let state = test_state(&["http://127.0.0.1:4317", "http://localhost:4317", "http://localhost:5173"]);
        for host in ["127.0.0.1:4317", "localhost:4317", "localhost:5173"] {
            assert!(host_allowed(&state, &headers_with_host(host)), "{host}");
        }
        assert!(!host_allowed(&state, &headers_with_host("evil.example:4317")));
        assert!(!host_allowed(&state, &HeaderMap::new()));
    }

    #[test]
    fn host_allowed_accepts_https_origins() {
        let state = test_state(&["https://t.example"]);
        assert!(host_allowed(&state, &headers_with_host("t.example")));
        assert!(!host_allowed(&state, &headers_with_host("t.example:443")));
        assert!(!host_allowed(&state, &headers_with_host("evil.example")));
    }

    #[tokio::test]
    async fn the_local_dashboard_says_it_is_not_a_tunnel() {
        use tower::ServiceExt;
        let app = local_router(Arc::new(test_state(&["http://127.0.0.1:4317"])));
        let request = axum::http::Request::builder()
            .uri("/api/tunnel-auth")
            .header("host", "127.0.0.1:4317")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = http_body_util::BodyExt::collect(response.into_body()).await.unwrap().to_bytes();
        assert_eq!(&body[..], br#"{"tunnel":false}"#);
    }

    #[test]
    fn cross_site_follows_fetch_metadata_and_origin() {
        let state = test_state(&["http://127.0.0.1:4317"]);
        let with = |name: &'static str, value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(name, value.parse().unwrap());
            headers
        };
        assert!(cross_site(&state, &with("sec-fetch-site", "cross-site")));
        assert!(cross_site(&state, &with("sec-fetch-site", "same-site")));
        assert!(cross_site(&state, &with("origin", "http://evil.example")));
        assert!(!cross_site(&state, &with("sec-fetch-site", "same-origin")));
        assert!(!cross_site(&state, &with("sec-fetch-site", "none")));
        assert!(!cross_site(&state, &with("origin", "http://127.0.0.1:4317")));
        assert!(!cross_site(&state, &HeaderMap::new()), "not a browser");
    }
}
