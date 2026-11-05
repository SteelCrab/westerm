//! `/remote/<target>/…`: what a remote window asks of this dashboard. A
//! remote window is a page whose tabs, layout, and shells live on a server,
//! so its requests go over the link to that server's dashboard, to the same
//! path there. Every route checks the target and the Host header first.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use super::remote::manager::Opened;
use super::{close_message, cross_site, host_allowed, http1, is_json, no_store, origin_allowed, pipe, AppState, Shared, REMOTE_HANDSHAKE_TIMEOUT};
use crate::host::MAX_MESSAGE;
use crate::protocol::{normalize_ssh_target, AttachRequest};
use futures_util::StreamExt;

pub(super) fn routes() -> Router<Shared> {
    Router::new()
        .route("/remote/{target}/api/workspace", get(get_workspace).put(put_workspace))
        .route("/remote/{target}/api/ssh-hosts", get(ssh_hosts))
        .route("/remote/{target}/ws/session", get(session))
}

/// The target from the path, when the Host header is one of this dashboard's
/// own (a page on another name could reach 127.0.0.1 by DNS rebinding).
fn checked_target(state: &AppState, headers: &HeaderMap, raw: &str) -> Result<String, StatusCode> {
    let target = normalize_ssh_target(raw).ok_or(StatusCode::BAD_REQUEST)?;
    if host_allowed(state, headers) { Ok(target) } else { Err(StatusCode::FORBIDDEN) }
}

/// Like `checked_target`, for a GET. A GET reads the server's layout or
/// host list, so a request from another site is refused too.
fn checked_get_target(state: &AppState, headers: &HeaderMap, raw: &str) -> Result<String, StatusCode> {
    let target = checked_target(state, headers, raw)?;
    if cross_site(state, headers) { Err(StatusCode::FORBIDDEN) } else { Ok(target) }
}

/// A stream to the server of `target`, or 503 when its link is not up. The
/// window's status stream keeps the link up and shows why it is not.
async fn open(state: &AppState, target: &str) -> Result<Opened, StatusCode> {
    state.remote.open_stream(target).await.ok_or(StatusCode::SERVICE_UNAVAILABLE)
}

/// Drops `link` apart from the caller, which must not wait for a connection in progress.
fn drop_link(state: &Shared, target: &str, link: u64) {
    let (state, target) = (state.clone(), target.to_owned());
    tokio::spawn(async move { state.remote.invalidate(&target, link).await });
}

/// The server's answer to one request: its status and body. 502 when it does
/// not answer, and the link goes.
async fn forward(state: &Shared, target: &str, method: &str, path: &str, body: Option<&[u8]>) -> Response {
    let opened = match open(state, target).await {
        Ok(opened) => opened,
        Err(status) => return status.into_response(),
    };
    let Some((status, body)) = http1::exchange(opened.stream, method, path, body).await else {
        drop_link(state, target, opened.link);
        return StatusCode::BAD_GATEWAY.into_response();
    };
    match StatusCode::from_u16(status) {
        Ok(StatusCode::OK) => no_store(([(header::CONTENT_TYPE, "application/json")], body)),
        Ok(status) => (status, body).into_response(),
        Err(_) => StatusCode::BAD_GATEWAY.into_response(),
    }
}

async fn get_workspace(State(state): State<Shared>, Path(target): Path<String>, headers: HeaderMap) -> Response {
    match checked_get_target(&state, &headers, &target) {
        Ok(target) => forward(&state, &target, "GET", "/api/workspace", None).await,
        Err(status) => status.into_response(),
    }
}

/// Like the local `put_workspace`: JSON from the dashboard's own page only.
async fn put_workspace(State(state): State<Shared>, Path(target): Path<String>, headers: HeaderMap, body: Bytes) -> Response {
    let target = match checked_target(&state, &headers, &target) {
        Ok(target) => target,
        Err(status) => return status.into_response(),
    };
    if !is_json(&headers) || !origin_allowed(&state, &headers) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    forward(&state, &target, "PUT", "/api/workspace", Some(&body)).await
}

async fn ssh_hosts(State(state): State<Shared>, Path(target): Path<String>, headers: HeaderMap) -> Response {
    match checked_get_target(&state, &headers, &target) {
        Ok(target) => forward(&state, &target, "GET", "/api/ssh-hosts", None).await,
        Err(status) => status.into_response(),
    }
}

/// A pane of a remote window. The server's host opens the session.
async fn session(
    State(state): State<Shared>,
    Path(target): Path<String>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    let target = match checked_target(&state, &headers, &target) {
        Ok(target) => target,
        Err(status) => return status.into_response(),
    };
    let Some(request) = AttachRequest::from_query(&query).filter(|_| origin_allowed(&state, &headers)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let opened = match open(&state, &target).await {
        Ok(opened) => opened,
        Err(status) => return status.into_response(),
    };
    ws.max_message_size(MAX_MESSAGE).on_upgrade(move |browser| relay(browser, request, opened, target, state))
}

/// Pipes the pane to its session on the server. The pane's first message,
/// its fitted size, waits in the socket until the server's host has accepted.
async fn relay(mut browser: WebSocket, request: AttachRequest, opened: Opened, target: String, state: Shared) {
    let url = format!("ws://remote/ws/session?{}", request.to_query());
    let config = WebSocketConfig::default().max_message_size(None).max_frame_size(None);
    let handshake =
        tokio::time::timeout(REMOTE_HANDSHAKE_TIMEOUT, tokio_tungstenite::client_async_with_config(url, opened.stream, Some(config)));
    let Ok(Ok((host, _))) = handshake.await else {
        drop_link(&state, &target, opened.link);
        let _ = browser.send(close_message(1011, "remote host unavailable")).await;
        return;
    };
    let (to_browser, from_browser) = browser.split();
    let end = pipe(to_browser, from_browser, host).await;
    // The pane closed its session but the server never answered: the link may
    // have gone silent with the close in it. End the session at the next
    // connection, and do not reuse this link.
    if end.session_closed && !end.host_answered {
        state.remote.remember_close(&target, &request.id);
        drop_link(&state, &target, opened.link);
    }
}
