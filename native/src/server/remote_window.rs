//! `/remote/<target>/…`: what a remote window asks of this dashboard. A
//! remote window is a page whose tabs, layout, and shells live on a server,
//! so its requests go over the link to that server's dashboard, to the same
//! path there. Every route checks the target and the Host header first.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, Query, RawQuery, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use super::plugins::git;
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
        .route(
            "/remote/{target}/api/docs/{name}",
            get(docs_get).put(docs_put).post(docs_post).layer(DefaultBodyLimit::max(super::docs::MAX_BODY_BYTES)),
        )
        .route("/remote/{target}/api/agents/{name}", get(agents_get))
        .route("/remote/{target}/api/plugins/git/{name}", get(git_get).post(git_post).layer(DefaultBodyLimit::max(git::MAX_BODY_BYTES)))
        .route("/remote/{target}/api/images", post(images_post).layer(DefaultBodyLimit::max(super::images::MAX_BODY_BYTES)))
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
    // Git may wait on a network (pull, push), the worktrees wait for gh to ask
    // GitHub, and a picture is a large body: they may take longer than a
    // layout request.
    let answer = if path.starts_with(git::PREFIX) || path.starts_with("/api/agents/worktrees") || path == super::images::PATH {
        http1::exchange_with_timeout(opened.stream, method, path, body, std::time::Duration::from_secs(35)).await
    } else {
        http1::exchange(opened.stream, method, path, body).await
    };
    let Some((status, body)) = answer else {
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

/// The docs routes a remote window may reach, by their last path part.
const DOCS_ROUTES: [&str; 3] = ["tree", "file", "cwd"];

/// The server's path for a docs route, with the page's query as it came.
fn docs_path(name: &str, query: Option<String>) -> String {
    match query {
        Some(query) => format!("/api/docs/{name}?{query}"),
        None => format!("/api/docs/{name}"),
    }
}

/// The server's path for an agents read of a folder, with the page's query as it came.
fn agents_path(name: &str, query: Option<String>) -> String {
    match query {
        Some(query) => format!("/api/agents/{name}?{query}"),
        None => format!("/api/agents/{name}"),
    }
}

/// The sidebar of a remote window lists a folder's past conversations, and
/// its repository's worktrees and pull requests, on the server.
async fn agents_get(
    State(state): State<Shared>, Path((target, name)): Path<(String, String)>,
    RawQuery(query): RawQuery, headers: HeaderMap,
) -> Response {
    if !matches!(name.as_str(), "history" | "worktrees") { return StatusCode::NOT_FOUND.into_response(); }
    match checked_get_target(&state, &headers, &target) {
        Ok(target) => forward(&state, &target, "GET", &agents_path(&name, query), None).await,
        Err(status) => status.into_response(),
    }
}

async fn git_get(
    State(state): State<Shared>, Path((target, name)): Path<(String, String)>,
    RawQuery(query): RawQuery, headers: HeaderMap,
) -> Response {
    if !matches!(name.as_str(), "status" | "diff" | "commit" | "commit-diff") { return StatusCode::NOT_FOUND.into_response(); }
    let path = match query { Some(query) => format!("{}{name}?{query}", git::PREFIX), None => format!("{}{name}", git::PREFIX) };
    match checked_get_target(&state, &headers, &target) {
        Ok(target) => forward(&state, &target, "GET", &path, None).await,
        Err(status) => status.into_response(),
    }
}

async fn git_post(
    State(state): State<Shared>, Path((target, name)): Path<(String, String)>, headers: HeaderMap, body: Bytes,
) -> Response {
    if name != "action" { return StatusCode::NOT_FOUND.into_response(); }
    let target = match checked_target(&state, &headers, &target) {
        Ok(target) => target, Err(status) => return status.into_response(),
    };
    if !is_json(&headers) || !origin_allowed(&state, &headers) { return StatusCode::FORBIDDEN.into_response(); }
    forward(&state, &target, "POST", &format!("{}action", git::PREFIX), Some(&body)).await
}

/// A picture pasted or dropped on a pane of a remote window: the server saves it, where the pane's program runs.
async fn images_post(State(state): State<Shared>, Path(target): Path<String>, headers: HeaderMap, body: Bytes) -> Response {
    let target = match checked_target(&state, &headers, &target) {
        Ok(target) => target,
        Err(status) => return status.into_response(),
    };
    if !is_json(&headers) || !origin_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    forward(&state, &target, "POST", super::images::PATH, Some(&body)).await
}

/// A docs pane of a remote window reads the server's files.
async fn docs_get(
    State(state): State<Shared>,
    Path((target, name)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    if !DOCS_ROUTES.contains(&name.as_str()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match checked_get_target(&state, &headers, &target) {
        Ok(target) => forward(&state, &target, "GET", &docs_path(&name, query), None).await,
        Err(status) => status.into_response(),
    }
}

async fn docs_put(State(state): State<Shared>, Path((target, name)): Path<(String, String)>, headers: HeaderMap, body: Bytes) -> Response {
    docs_change(&state, &target, &name, "PUT", &headers, &body).await
}

async fn docs_post(State(state): State<Shared>, Path((target, name)): Path<(String, String)>, headers: HeaderMap, body: Bytes) -> Response {
    docs_change(&state, &target, &name, "POST", &headers, &body).await
}

/// Like `put_workspace`: JSON from the dashboard's own page only. Only files change.
async fn docs_change(state: &Shared, target: &str, name: &str, method: &str, headers: &HeaderMap, body: &[u8]) -> Response {
    if name != "file" {
        return StatusCode::NOT_FOUND.into_response();
    }
    let target = match checked_target(state, headers, target) {
        Ok(target) => target,
        Err(status) => return status.into_response(),
    };
    if !is_json(headers) || !origin_allowed(state, headers) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    forward(state, &target, method, "/api/docs/file", Some(body)).await
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::server::tests::test_state;

    fn page_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "127.0.0.1:4317".parse().unwrap());
        headers.insert(header::ORIGIN, "http://127.0.0.1:4317".parse().unwrap());
        headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        headers
    }

    #[test]
    fn docs_requests_keep_their_query() {
        assert_eq!(docs_path("tree", Some("root=%2Fr".into())), "/api/docs/tree?root=%2Fr");
        assert_eq!(docs_path("file", None), "/api/docs/file");
    }

    #[test]
    fn agents_requests_keep_their_query() {
        assert_eq!(agents_path("history", Some("dir=%2Fw".into())), "/api/agents/history?dir=%2Fw");
        assert_eq!(agents_path("history", None), "/api/agents/history");
        assert_eq!(agents_path("worktrees", Some("dir=%2Fw".into())), "/api/agents/worktrees?dir=%2Fw");
    }

    /// Refusals answer before any link to the server is opened.
    #[tokio::test]
    async fn agents_routes_refuse_unknown_names_and_other_pages() {
        let state: Shared = Arc::new(test_state(&["http://127.0.0.1:4317"]));
        let get = |name: &str, headers: HeaderMap| {
            agents_get(State(state.clone()), Path(("prod".to_owned(), name.to_owned())), RawQuery(None), headers)
        };
        assert_eq!(get("secret", page_headers()).await.status(), StatusCode::NOT_FOUND);
        let mut evil = page_headers();
        evil.insert(header::HOST, "evil.example:4317".parse().unwrap());
        for name in ["history", "worktrees"] {
            assert_eq!(get(name, evil.clone()).await.status(), StatusCode::FORBIDDEN, "{name}");
            // The own page, with no link to the server up.
            assert_eq!(get(name, page_headers()).await.status(), StatusCode::SERVICE_UNAVAILABLE, "{name}");
        }
    }

    /// Refusals answer before any link to the server is opened.
    #[tokio::test]
    async fn docs_routes_refuse_unknown_names_and_other_pages() {
        let state: Shared = Arc::new(test_state(&["http://127.0.0.1:4317"]));
        let get = |name: &str, headers: HeaderMap| {
            docs_get(State(state.clone()), Path(("prod".to_owned(), name.to_owned())), RawQuery(None), headers)
        };
        assert_eq!(get("secret", page_headers()).await.status(), StatusCode::NOT_FOUND);
        let mut evil = page_headers();
        evil.insert(header::HOST, "evil.example:4317".parse().unwrap());
        assert_eq!(get("tree", evil).await.status(), StatusCode::FORBIDDEN);
        let mut cross = page_headers();
        cross.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert_eq!(get("file", cross).await.status(), StatusCode::FORBIDDEN);

        assert_eq!(docs_change(&state, "prod", "tree", "PUT", &page_headers(), b"{}").await.status(), StatusCode::NOT_FOUND);
        let mut no_origin = page_headers();
        no_origin.remove(header::ORIGIN);
        assert_eq!(docs_change(&state, "prod", "file", "PUT", &no_origin, b"{}").await.status(), StatusCode::BAD_REQUEST);
    }

    /// Refusals answer before any link to the server is opened.
    #[tokio::test]
    async fn pictures_come_from_the_dashboards_own_page_only() {
        let state: Shared = Arc::new(test_state(&["http://127.0.0.1:4317"]));
        let post = |target: &str, headers: HeaderMap| images_post(State(state.clone()), Path(target.to_owned()), headers, Bytes::from_static(b"{}"));
        let mut evil = page_headers();
        evil.insert(header::HOST, "evil.example:4317".parse().unwrap());
        assert_eq!(post("prod", evil).await.status(), StatusCode::FORBIDDEN);
        let mut no_origin = page_headers();
        no_origin.remove(header::ORIGIN);
        assert_eq!(post("prod", no_origin).await.status(), StatusCode::FORBIDDEN);
        let mut form = page_headers();
        form.insert(header::CONTENT_TYPE, "text/plain".parse().unwrap());
        assert_eq!(post("prod", form).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(post("not a target", page_headers()).await.status(), StatusCode::BAD_REQUEST);
        // The own page, with no link to the server up.
        assert_eq!(post("prod", page_headers()).await.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
