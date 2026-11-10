//! The tunnel listener: what cloudflared forwards to. Every request is a
//! tunnel request, so no check can be skipped by a header that says
//! otherwise. Order: the Host must be this tunnel's name, the path must be on
//! a fixed list, and the API paths need the cookie that the access code gives.
//! Behind those checks the dashboard's own handlers answer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use super::auth::{cookie_token, device_label, new_token, same, set_cookie, DeviceView, Limiter, Sessions, Verdict};
use super::{AUTH_PATH, LOGIN_PATH, PING_EVERY};
use crate::host::MAX_MESSAGE;
use crate::server::{
    agents, asset, docs, get_workspace, git, is_json, no_store, origin_allowed, put_workspace, relay_with, session_request, ssh_hosts,
    terminal_config, RelayExtra, Shared,
};

/// What a path needs on the tunnel listener.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Access {
    /// The page files and the login: nothing secret.
    Public,
    /// Needs the tunnel cookie.
    Cookie,
    /// 404 on the tunnel, whatever the cookie: local-only or unknown.
    Denied,
}

/// The fixed list of the spec (7.6). A new API path is Denied until it is
/// listed here, so nothing opens to the tunnel by accident.
pub(super) fn access(path: &str) -> Access {
    match path {
        AUTH_PATH | LOGIN_PATH => Access::Public,
        "/api/terminal-config" | "/api/workspace" | "/api/ssh-hosts" | "/ws/session" | "/api/agents/history" => Access::Cookie,
        _ if path.starts_with("/api/docs/") || path.starts_with("/api/git/") => Access::Cookie,
        _ if path.starts_with("/api/") || path.starts_with("/ws/") || path == "/remote" || path.starts_with("/remote/") => {
            Access::Denied
        }
        _ => Access::Public,
    }
}

/// What the manager hears from the listener.
#[derive(Debug)]
pub(super) enum AdmissionEvent {
    /// A device was seen, signed in, or opened or closed a terminal.
    Changed,
    /// The 20th wrong code: the tunnel must end.
    TooManyCodes,
}

/// The answer to a login attempt.
pub(super) enum Login {
    /// The new cookie token.
    Ok(String),
    Wrong,
    Locked(Duration),
    TooMany,
}

/// Who may come in through one tunnel: its host name, its code, and the
/// devices that gave the code.
pub(super) struct Admission {
    pub hostname: String,
    code: String,
    inner: Mutex<(Limiter, Sessions)>,
    pub ended: watch::Receiver<bool>,
    events: mpsc::UnboundedSender<AdmissionEvent>,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_millis() as u64)
}

impl Admission {
    pub fn new(hostname: String, code: String, ended: watch::Receiver<bool>, events: mpsc::UnboundedSender<AdmissionEvent>) -> Self {
        Self { hostname, code, inner: Mutex::new((Limiter::default(), Sessions::default())), ended, events }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, (Limiter, Sessions)> {
        // A panic while holding it leaves plain data; keep serving.
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn notify(&self, event: AdmissionEvent) {
        let _ = self.events.send(event);
    }

    pub fn login(&self, code: &str, label: String, now: Instant) -> Login {
        let mut inner = self.lock();
        let (limiter, sessions) = &mut *inner;
        if let Some(left) = limiter.locked(now) {
            return Login::Locked(left);
        }
        if same(code.as_bytes(), self.code.as_bytes()) {
            limiter.succeed();
            let token = new_token();
            sessions.add(&token, label, now_ms());
            drop(inner);
            self.notify(AdmissionEvent::Changed);
            return Login::Ok(token);
        }
        match limiter.fail(now) {
            Verdict::Retry => Login::Wrong,
            Verdict::Locked(lock) => Login::Locked(lock),
            Verdict::TooMany => {
                drop(inner);
                self.notify(AdmissionEvent::TooManyCodes);
                Login::TooMany
            }
        }
    }

    fn locked_for(&self) -> Option<u64> {
        self.lock().0.locked(Instant::now()).map(|left| left.as_secs_f64().ceil() as u64)
    }

    /// A request with `token`: true when the token belongs to this tunnel.
    fn touch(&self, token: &str) -> bool {
        let known = self.lock().1.touch(token, now_ms());
        if known {
            self.notify(AdmissionEvent::Changed);
        }
        known
    }

    fn socket(&self, token: &str, delta: i32) {
        self.lock().1.socket(token, delta);
        self.notify(AdmissionEvent::Changed);
    }

    pub fn devices(&self) -> Vec<DeviceView> {
        self.lock().1.devices()
    }

    pub fn sockets(&self) -> u32 {
        self.lock().1.sockets()
    }

    /// Every cookie of this tunnel stops working.
    pub fn clear(&self) {
        self.lock().1.clear();
    }
}

/// The cookie token of a request that passed the check.
#[derive(Clone)]
struct Token(String);

/// The tunnel listener's routes. `state.origins` is `["https://<hostname>"]`,
/// so the dashboard's own Origin checks hold here too.
pub(super) fn router(state: Shared, admission: Arc<Admission>) -> Router {
    let status = {
        let admission = Arc::clone(&admission);
        move |headers: HeaderMap| async move { auth_status(&admission, &headers) }
    };
    let login = {
        let admission = Arc::clone(&admission);
        move |State(state): State<Shared>, headers: HeaderMap, body: Bytes| async move {
            login_route(&state, &admission, &headers, &body)
        }
    };
    let session = {
        let admission = Arc::clone(&admission);
        move |State(state): State<Shared>,
              Extension(Token(token)): Extension<Token>,
              headers: HeaderMap,
              Query(query): Query<HashMap<String, String>>,
              ws: WebSocketUpgrade| async move { session_route(state, admission, token, headers, query, ws).await }
    };
    Router::new()
        .route(AUTH_PATH, get(status))
        .route(LOGIN_PATH, axum::routing::post(login))
        .route("/api/terminal-config", get(terminal_config))
        .route("/api/workspace", get(get_workspace).put(put_workspace))
        .route("/api/ssh-hosts", get(ssh_hosts))
        .route("/ws/session", get(session))
        .merge(docs::routes())
        .merge(agents::routes())
        .merge(git::routes())
        .fallback(fallback)
        .layer(middleware::from_fn_with_state(admission, check))
        .with_state(state)
}

/// Host first, then the path list, then the cookie. Every answer, refusals
/// included, gets the frame and referrer headers.
async fn check(State(admission): State<Arc<Admission>>, mut request: Request, next: Next) -> Response {
    let host = request.headers().get(header::HOST).and_then(|value| value.to_str().ok());
    let mut response = if host != Some(admission.hostname.as_str()) {
        StatusCode::FORBIDDEN.into_response()
    } else {
        match access(request.uri().path()) {
            Access::Denied => StatusCode::NOT_FOUND.into_response(),
            Access::Public => next.run(request).await,
            Access::Cookie => match cookie_token(request.headers()).filter(|token| admission.touch(token)) {
                Some(token) => {
                    request.extensions_mut().insert(Token(token));
                    next.run(request).await
                }
                None => StatusCode::UNAUTHORIZED.into_response(),
            },
        }
    };
    let headers = response.headers_mut();
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    response
}

/// Unknown API paths are 404; anything else is the single-page app.
async fn fallback(uri: Uri) -> Response {
    let path = uri.path();
    if path.starts_with("/api/") || path.starts_with("/ws/") {
        StatusCode::NOT_FOUND.into_response()
    } else {
        asset(uri).await
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthAnswer {
    tunnel: bool,
    signed_in: bool,
    locked_for: Option<u64>,
}

fn auth_status(admission: &Admission, headers: &HeaderMap) -> Response {
    let signed_in = cookie_token(headers).is_some_and(|token| admission.touch(&token));
    no_store(Json(AuthAnswer { tunnel: true, signed_in, locked_for: admission.locked_for() }))
}

#[derive(Deserialize)]
struct LoginBody {
    code: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LockAnswer {
    locked_for: Option<u64>,
}

fn locked_answer(status: StatusCode, locked_for: Option<Duration>) -> Response {
    let locked_for = locked_for.map(|left| left.as_secs_f64().ceil() as u64);
    (status, no_store(Json(LockAnswer { locked_for }))).into_response()
}

fn login_route(state: &crate::server::AppState, admission: &Admission, headers: &HeaderMap, body: &Bytes) -> Response {
    if !is_json(headers) || !origin_allowed(state, headers) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(LoginBody { code }) = serde_json::from_slice(body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let agent = headers.get(header::USER_AGENT).and_then(|value| value.to_str().ok()).unwrap_or("");
    match admission.login(&code, device_label(agent), Instant::now()) {
        Login::Ok(token) => (StatusCode::NO_CONTENT, [(header::SET_COOKIE, set_cookie(&token))]).into_response(),
        Login::Wrong | Login::TooMany => locked_answer(StatusCode::UNAUTHORIZED, None),
        Login::Locked(left) => locked_answer(StatusCode::TOO_MANY_REQUESTS, Some(left)),
    }
}

async fn session_route(
    state: Shared,
    admission: Arc<Admission>,
    token: String,
    headers: HeaderMap,
    query: HashMap<String, String>,
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
    let query = request.to_query();
    ws.max_message_size(MAX_MESSAGE).on_upgrade(move |browser| async move {
        admission.socket(&token, 1);
        let ended = admission.ended.clone();
        let on_end = {
            let admission = Arc::clone(&admission);
            Box::new(move || admission.socket(&token, -1))
        };
        relay_with(browser, dir, query, state, RelayExtra { ended, ping: PING_EVERY, on_end }).await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::tests::test_state;
    use axum::body::Body;
    use axum::http::{HeaderMap, Request, StatusCode};
    use http_body_util::BodyExt;
    use tokio::sync::{mpsc, watch};
    use tower::ServiceExt;

    const HOST: &str = "t.example";
    const ORIGIN: &str = "https://t.example";
    const CODE: &str = "482913";

    struct Setup {
        app: Router,
        admission: Arc<Admission>,
        events: mpsc::UnboundedReceiver<AdmissionEvent>,
        _ended: watch::Sender<bool>,
    }

    fn setup() -> Setup {
        let state = Arc::new(test_state(&[ORIGIN]));
        let (ended_tx, ended) = watch::channel(false);
        let (events_tx, events) = mpsc::unbounded_channel();
        let admission = Arc::new(Admission::new(HOST.to_owned(), CODE.to_owned(), ended, events_tx));
        Setup { app: router(state, Arc::clone(&admission)), admission, events, _ended: ended_tx }
    }

    fn request(method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(Body::from(body.to_owned())).unwrap()
    }

    async fn send(app: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, String) {
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, headers, String::from_utf8_lossy(&body).into_owned())
    }

    async fn get(app: &Router, path: &str, cookie: Option<&str>) -> (StatusCode, HeaderMap, String) {
        let cookie = cookie.map(|token| format!("westerm_tunnel={token}"));
        let mut headers = vec![("host", HOST)];
        if let Some(cookie) = &cookie {
            headers.push(("cookie", cookie));
        }
        send(app, request("GET", path, &headers, "")).await
    }

    async fn login(app: &Router, code: &str) -> (StatusCode, HeaderMap, String) {
        let body = format!(r#"{{"code":"{code}"}}"#);
        let headers = [("host", HOST), ("origin", ORIGIN), ("content-type", "application/json"), ("user-agent", "Mozilla/5.0 (iPhone) Safari/604.1")];
        send(app, request("POST", "/api/tunnel-auth/login", &headers, &body)).await
    }

    /// The token from a successful login's Set-Cookie.
    async fn signed_in(app: &Router) -> String {
        let (status, headers, _) = login(app, CODE).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let cookie = headers.get("set-cookie").unwrap().to_str().unwrap();
        cookie.strip_prefix("westerm_tunnel=").unwrap().split(';').next().unwrap().to_owned()
    }

    #[test]
    fn access_table() {
        for path in ["/", "/assets/index-x.js", "/api/tunnel-auth", "/api/tunnel-auth/login"] {
            assert_eq!(access(path), Access::Public, "{path}");
        }
        for path in [
            "/api/terminal-config",
            "/api/workspace",
            "/api/ssh-hosts",
            "/ws/session",
            "/api/docs/tree",
            "/api/agents/history",
            "/api/git/status",
            "/api/git/action",
        ] {
            assert_eq!(access(path), Access::Cookie, "{path}");
        }
        // The computer's installed fonts mean nothing to another device's browser.
        for path in ["/api/update", "/api/remote/link", "/api/tunnel/start", "/remote/x/api/workspace", "/remote", "/ws/other", "/api/new-thing", "/api/system-fonts"] {
            assert_eq!(access(path), Access::Denied, "{path}");
        }
    }

    #[tokio::test]
    async fn another_host_is_refused() {
        let s = setup();
        let (status, _, _) = send(&s.app, request("GET", "/", &[("host", "evil.example")], "")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = send(&s.app, request("GET", "/", &[("host", "t.example:443")], "")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(get(&s.app, "/", None).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn cookie_routes_need_a_valid_cookie() {
        let s = setup();
        assert_eq!(get(&s.app, "/api/workspace", None).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(get(&s.app, "/api/workspace", Some("wrong")).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(get(&s.app, "/api/terminal-config", None).await.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn denied_routes_are_404() {
        let s = setup();
        let token = signed_in(&s.app).await;
        for path in ["/api/update", "/api/remote/link", "/remote/x/api/workspace", "/api/tunnel/start", "/api/docs/nothing-here"] {
            assert_eq!(get(&s.app, path, Some(&token)).await.0, StatusCode::NOT_FOUND, "{path}");
        }
    }

    #[tokio::test]
    async fn the_git_panel_works_with_a_cookie() {
        let s = setup();
        let token = signed_in(&s.app).await;
        let (status, _, _) = get(&s.app, "/api/git/status?dir=/no/such/folder", Some(&token)).await;
        assert_ne!(status, StatusCode::NOT_FOUND, "the Git routes are served");
        assert_ne!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(get(&s.app, "/api/git/status?dir=/tmp", None).await.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn login_sets_a_cookie_that_opens_the_api() {
        let s = setup();
        let (_, _, body) = get(&s.app, "/api/tunnel-auth", None).await;
        assert_eq!(body, r#"{"tunnel":true,"signedIn":false,"lockedFor":null}"#);
        let (status, headers, _) = login(&s.app, CODE).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let cookie = headers.get("set-cookie").unwrap().to_str().unwrap();
        assert!(cookie.starts_with("westerm_tunnel=") && cookie.contains("Secure"), "{cookie}");
        let token = cookie.strip_prefix("westerm_tunnel=").unwrap().split(';').next().unwrap();
        assert_eq!(get(&s.app, "/api/workspace", Some(token)).await.0, StatusCode::OK);
        let (_, _, body) = get(&s.app, "/api/tunnel-auth", Some(token)).await;
        assert_eq!(body, r#"{"tunnel":true,"signedIn":true,"lockedFor":null}"#);
        assert_eq!(s.admission.devices()[0].label, "iPhone · Safari");
    }

    #[tokio::test]
    async fn login_checks_origin_and_json() {
        let s = setup();
        let body = format!(r#"{{"code":"{CODE}"}}"#);
        let plain_origin = [("host", HOST), ("origin", "http://t.example"), ("content-type", "application/json")];
        let (status, headers, _) = send(&s.app, request("POST", "/api/tunnel-auth/login", &plain_origin, &body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(headers.get("set-cookie").is_none());
        let not_json = [("host", HOST), ("origin", ORIGIN), ("content-type", "text/plain")];
        let (status, headers, _) = send(&s.app, request("POST", "/api/tunnel-auth/login", &not_json, &body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(headers.get("set-cookie").is_none());
    }

    #[tokio::test]
    async fn five_wrong_codes_lock_and_twenty_end() {
        let mut s = setup();
        for _ in 0..4 {
            let (status, _, body) = login(&s.app, "000000").await;
            assert_eq!((status, body.as_str()), (StatusCode::UNAUTHORIZED, r#"{"lockedFor":null}"#));
        }
        let (status, _, body) = login(&s.app, "000000").await;
        assert_eq!((status, body.as_str()), (StatusCode::TOO_MANY_REQUESTS, r#"{"lockedFor":30}"#));
        let (status, headers, _) = login(&s.app, CODE).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "a right code waits out the lock too");
        assert!(headers.get("set-cookie").is_none());

        // The rest past each lock, at a later time.
        let mut now = Instant::now();
        for _ in 5..19 {
            now += Duration::from_secs(3600);
            assert!(!matches!(s.admission.login("000000", String::new(), now), Login::TooMany));
        }
        now += Duration::from_secs(3600);
        assert!(matches!(s.admission.login("000000", String::new(), now), Login::TooMany));
        let mut too_many = false;
        while let Ok(event) = s.events.try_recv() {
            too_many |= matches!(event, AdmissionEvent::TooManyCodes);
        }
        assert!(too_many, "the manager hears it");
    }

    #[tokio::test]
    async fn responses_carry_the_frame_and_referrer_headers() {
        let s = setup();
        for (status, headers) in [get(&s.app, "/", None).await, get(&s.app, "/api/workspace", None).await].map(|(status, headers, _)| (status, headers)) {
            assert_eq!(headers.get("x-frame-options").map(|v| v.to_str().unwrap()), Some("DENY"), "{status}");
            assert_eq!(headers.get("referrer-policy").map(|v| v.to_str().unwrap()), Some("no-referrer"), "{status}");
        }
    }
}
