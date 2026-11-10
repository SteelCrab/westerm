//! Cloudflare Quick Tunnel: another device's browser reaches this westerm
//! through `https://<name>.trycloudflare.com`. cloudflared forwards to a
//! listener that exists only while the tunnel is on; that listener checks the
//! Host, a cookie from the access code, and a fixed list of paths before it
//! hands a request to the dashboard's own handlers. Port 4317 stays as it is.

mod auth;
mod awake;
mod cloudflared;
pub(super) mod guard;
mod manager;

pub(super) use manager::TunnelManager;
mod router;

/// The close code a tunnel page's terminal gets when the tunnel ends on
/// purpose; the page then stops reconnecting. Same value in shared/protocol.ts.
/// Not in protocol.rs: that file is a host source, and the host never sees it.
pub(super) const CLOSE_TUNNEL_ENDED: u16 = 4010;
/// GET: is this page a tunnel page, signed in, locked? Both listeners answer it.
pub(super) const AUTH_PATH: &str = "/api/tunnel-auth";
/// POST {code}: the access code for a cookie.
pub(super) const LOGIN_PATH: &str = "/api/tunnel-auth/login";
/// How often a tunnel page's terminal socket gets a ping.
pub(super) const PING_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// WebSocket: the tunnel's status as JSON, now and on every change.
pub(super) const STATUS_PATH: &str = "/api/tunnel/status";

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;

use super::{host_allowed, json_body, origin_allowed, AppState, Shared};

/// The tunnel's controls on 4317, for the dashboard's own page only: a page
/// on another site must neither start a tunnel nor read its code.
pub(super) fn control_routes() -> Router<Shared> {
    Router::new()
        .route(STATUS_PATH, get(status_stream))
        .route("/api/tunnel/start", post(|state: State<Shared>, headers: HeaderMap, body: Bytes| control(state, headers, body, Control::Start)))
        .route("/api/tunnel/stop", post(|state: State<Shared>, headers: HeaderMap, body: Bytes| control(state, headers, body, Control::Stop)))
        .route("/api/tunnel/check", post(|state: State<Shared>, headers: HeaderMap, body: Bytes| control(state, headers, body, Control::Check)))
}

enum Control {
    Start,
    Stop,
    Check,
}

fn manager(state: &AppState) -> Option<Arc<TunnelManager>> {
    state.tunnel.clone()
}

async fn control(State(state): State<Shared>, headers: HeaderMap, body: Bytes, action: Control) -> StatusCode {
    let Some(manager) = manager(&state) else { return StatusCode::NOT_FOUND };
    if !host_allowed(&state, &headers) || json_body::<serde_json::Value>(&state, &headers, &body).is_none() {
        return StatusCode::BAD_REQUEST;
    }
    match action {
        Control::Start => manager.start().await,
        Control::Stop => manager.stop(manager::Reason::Stopped).await,
        Control::Check => manager.check(),
    }
    StatusCode::NO_CONTENT
}

async fn status_stream(State(state): State<Shared>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    let Some(manager) = manager(&state) else { return StatusCode::NOT_FOUND.into_response() };
    // The status holds the access code: the Host and the Origin must be ours.
    if !host_allowed(&state, &headers) || !origin_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| send_status(socket, manager))
}

async fn send_status(mut socket: WebSocket, manager: Arc<TunnelManager>) {
    let mut watch = manager.watch();
    loop {
        let text = serde_json::to_string(&*watch.borrow_and_update()).unwrap_or_default();
        if socket.send(Message::Text(text.into())).await.is_err() {
            return;
        }
        tokio::select! {
            changed = watch.changed() => if changed.is_err() { return },
            message = socket.recv() => if !matches!(message, Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Text(_) | Message::Binary(_)))) { return },
        }
    }
}
