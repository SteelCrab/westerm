//! `/plugin-assets/<id>/…` serves the files of an installed external plugin,
//! and `/plugin-sdk/v1.js` the SDK its panel loads. Both are static files,
//! so only the Host header is checked: the panel's frame has an opaque
//! origin, and its requests are cross-site.
//!
//! Every answer under `/plugin-assets` is a sandboxed document
//! (`Content-Security-Policy: sandbox allow-scripts; …`), so a plugin page
//! opened outside its frame (a link, a browser pane) still runs in an opaque
//! origin and never gets the dashboard's. A served file may load scripts,
//! styles, and images only from its own plugin's folder and the SDK, and may
//! open no connection.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::{Path as UrlPath, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::map_response;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use super::manifest::is_package_path;
use super::store;
use crate::server::{host_allowed, Shared};

/// The SDK a panel loads with `<script src="/plugin-sdk/v1.js">`.
const SDK_V1: &str = include_str!("sdk_v1.js");

/// The policy of every answer that is not a plugin's file: the SDK, a
/// refusal, a missing file. A sandboxed document that may load nothing. It
/// names no host or id, so nothing from an unchecked request ends up in it.
const CLOSED_CSP: &str = "sandbox allow-scripts; default-src 'none'";

pub(super) fn routes() -> Router<Shared> {
    Router::new()
        .route("/plugin-assets/{id}/{*path}", get(serve_asset))
        // Paths that name no file get a sandboxed 404, not the dashboard page.
        .route("/plugin-assets", get(not_found))
        .route("/plugin-assets/", get(not_found))
        .route("/plugin-assets/{id}", get(not_found))
        .route("/plugin-assets/{id}/", get(not_found))
        .route("/plugin-sdk/v1.js", get(serve_sdk))
        // Answers the handlers do not make, such as a 405 for a POST.
        .layer(map_response(sandbox_if_missing))
}

/// The policy of a plugin file served from `host` (a checked Host header).
/// The frame's origin is opaque, so `'self'` would match nothing: the
/// plugin's folder and the SDK are named in full. Only the dashboard's own
/// origins (`ancestors`, `--allow-origin` included) may frame the page, so
/// another site cannot be its parent and answer its calls.
pub(super) fn asset_csp(host: &str, id: &str, ancestors: &[String]) -> String {
    let files = format!("http://{host}/plugin-assets/{id}/");
    // An origin given on the command line that would end the directive or
    // the policy is left out.
    let fits = |origin: &&String| !origin.is_empty() && origin.bytes().all(|b| b.is_ascii_graphic() && b != b';' && b != b',');
    let ancestors: Vec<&str> = ancestors.iter().filter(fits).map(String::as_str).collect();
    let ancestors = if ancestors.is_empty() { "'none'".to_owned() } else { ancestors.join(" ") };
    format!(
        "sandbox allow-scripts; default-src 'none'; script-src {files} http://{host}/plugin-sdk/v1.js; \
         style-src {files} 'unsafe-inline'; img-src {files}; connect-src 'none'; frame-src 'none'; \
         form-action 'none'; base-uri 'none'; object-src 'none'; frame-ancestors {ancestors}"
    )
}

/// The file `path` names in the package at `root`. `None` unless `path` is a
/// package path (the rule `plugin.json` paths follow) and it resolves, links
/// followed, to a regular file inside `root`. That also holds for a folder
/// source, which skips the package checks for links.
pub(super) fn resolve_asset(root: &Path, path: &str) -> Option<PathBuf> {
    if !is_package_path(path) {
        return None;
    }
    let real_root = root.canonicalize().ok()?;
    let file = real_root.join(path).canonicalize().ok()?;
    (file.starts_with(&real_root) && file.is_file()).then_some(file)
}

/// A sandboxed answer: the policy, `nosniff`, and `no-cache`.
fn sandboxed(status: StatusCode, csp: &str, content_type: Option<&str>, body: impl Into<Body>) -> Response {
    let csp = HeaderValue::from_str(csp).unwrap_or(HeaderValue::from_static(CLOSED_CSP));
    let mut response = (status, body.into()).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_SECURITY_POLICY, csp);
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if let Some(value) = content_type.and_then(|value| HeaderValue::from_str(value).ok()) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    response
}

fn closed(status: StatusCode) -> Response {
    sandboxed(status, CLOSED_CSP, None, Body::empty())
}

async fn not_found() -> Response {
    closed(StatusCode::NOT_FOUND)
}

async fn sandbox_if_missing(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.entry(header::CONTENT_SECURITY_POLICY).or_insert(HeaderValue::from_static(CLOSED_CSP));
    headers.entry(header::X_CONTENT_TYPE_OPTIONS).or_insert(HeaderValue::from_static("nosniff"));
    response
}

/// The Host header, when it names one of the dashboard's own origins.
fn checked_host<'a>(state: &Shared, headers: &'a HeaderMap) -> Option<&'a str> {
    if !host_allowed(state, headers) {
        return None;
    }
    headers.get(header::HOST)?.to_str().ok()
}

/// Whether the request loads a script, style, image, or font
/// (`Sec-Fetch-Dest`). The frame's origin is opaque, so a module script (what
/// bundlers emit, often with `crossorigin`) is a CORS request from `null` and
/// needs `Access-Control-Allow-Origin`. A `fetch()` (`empty`) gets none, so
/// another website cannot read a plugin's files; the browser sets the header,
/// a page cannot.
fn subresource_load(headers: &HeaderMap) -> bool {
    headers
        .get("sec-fetch-dest")
        .is_some_and(|dest| matches!(dest.as_bytes(), b"script" | b"style" | b"image" | b"font"))
}

/// A file of an installed external plugin.
async fn serve_asset(State(state): State<Shared>, headers: HeaderMap, UrlPath((id, path)): UrlPath<(String, String)>) -> Response {
    let Some(host) = checked_host(&state, &headers) else { return closed(StatusCode::FORBIDDEN) };
    let Some(record) = state.plugins.external(&id) else { return closed(StatusCode::NOT_FOUND) };
    let root = store::package_dir(&state.plugins.state_dir, &record);
    let read = tokio::task::spawn_blocking(move || {
        let file = resolve_asset(&root, &path)?;
        std::fs::read(&file).ok().map(|bytes| (file, bytes))
    });
    match read.await.ok().flatten() {
        Some((file, bytes)) => {
            let mime = mime_guess::from_path(&file).first_or_octet_stream();
            let mut response = sandboxed(StatusCode::OK, &asset_csp(host, &id, &state.origins), Some(mime.as_ref()), bytes);
            if subresource_load(&headers) {
                response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
            }
            response
        }
        None => closed(StatusCode::NOT_FOUND),
    }
}

/// The SDK. Loaded as a script its policy does nothing; opened as a page it
/// is a sandboxed document that loads nothing.
async fn serve_sdk(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if !host_allowed(&state, &headers) {
        return closed(StatusCode::FORBIDDEN);
    }
    sandboxed(StatusCode::OK, CLOSED_CSP, Some("text/javascript; charset=utf-8"), SDK_V1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::plugins::PluginState;
    use crate::server::plugins::external::manifest;
    use crate::server::plugins::external::store::{plugin_dir, ExternalRecord, Source, SourceKind};
    use crate::server::tests::test_state;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const ID: &str = "k8s-tools.kubernetes";
    const HOST: &str = "127.0.0.1:4318";
    const SANDBOX: &str = "sandbox allow-scripts;";

    fn headers(host: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(axum::http::header::HOST, host.parse().unwrap());
        // A sandboxed frame has an opaque origin, so its requests are cross-site.
        headers.insert("sec-fetch-site", "cross-site".parse().unwrap());
        headers
    }

    /// `headers(host)` for a request of the kind `dest` (`Sec-Fetch-Dest`).
    fn headers_for(host: &str, dest: &str) -> HeaderMap {
        let mut headers = headers(host);
        headers.insert("sec-fetch-dest", dest.parse().unwrap());
        headers
    }

    /// Writes a plugin package into `folder`: `plugin.json`, a panel, an icon, and `js/app.js`.
    fn package(folder: &Path, version: &str) {
        std::fs::create_dir_all(folder.join("js")).unwrap();
        let manifest = serde_json::json!({
            "id": ID, "name": "Kubernetes", "version": version, "description": "See pods.", "panel": "panel.html", "icon": "icon.svg",
        });
        std::fs::write(folder.join("plugin.json"), manifest.to_string()).unwrap();
        std::fs::write(folder.join("panel.html"), "<p>panel</p>").unwrap();
        std::fs::write(folder.join("icon.svg"), "<svg/>").unwrap();
        std::fs::write(folder.join("js/app.js"), "1;").unwrap();
    }

    /// A server whose state folder holds `plugins.json`, with `ID` installed
    /// from a local folder unless `installed` is false.
    struct Env {
        state_dir: tempfile::TempDir,
        work: tempfile::TempDir,
        state: Shared,
    }

    impl Env {
        fn new() -> Self {
            let state_dir = tempfile::tempdir().unwrap();
            let work = tempfile::tempdir().unwrap();
            let mut state = test_state(&[&format!("http://{HOST}")]);
            state.plugins = std::sync::Arc::new(PluginState::load(state_dir.path()));
            Self { state_dir, work, state: Arc::new(state) }
        }

        fn install(&self, kind: SourceKind, root: &Path) {
            let manifest = manifest::read(root, crate::update::CURRENT).unwrap();
            let path = (kind == SourceKind::Folder).then(|| root.display().to_string());
            let record = ExternalRecord {
                id: ID.into(),
                version: manifest.version.clone(),
                source: Source { kind, url: None, path },
                sha256: None,
                granted: manifest::Permissions::default(),
                installed_at: 0,
            };
            self.state.plugins.put_external(record, manifest).unwrap();
        }

        /// `ID` installed from a folder in the work folder; returns the folder.
        fn with_folder() -> (Self, PathBuf) {
            let env = Self::new();
            let folder = env.work.path().join("k8s");
            package(&folder, "0.3.1");
            let folder = folder.canonicalize().unwrap();
            env.install(SourceKind::Folder, &folder);
            (env, folder)
        }

        async fn get(&self, host: &str, id: &str, path: &str) -> Response {
            self.get_as(headers(host), id, path).await
        }

        async fn get_as(&self, headers: HeaderMap, id: &str, path: &str) -> Response {
            serve_asset(State(self.state.clone()), headers, UrlPath((id.to_owned(), path.to_owned()))).await
        }
    }

    async fn body(response: Response) -> Vec<u8> {
        axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap().to_vec()
    }

    fn assert_sandboxed(response: &Response, what: &str) {
        let csp = response.headers().get("content-security-policy").map(|value| value.to_str().unwrap());
        assert!(csp.is_some_and(|csp| csp.starts_with(SANDBOX)), "{what}: {csp:?}");
        assert_eq!(response.headers().get("x-content-type-options").map(|value| value.to_str().unwrap()), Some("nosniff"), "{what}");
    }

    #[tokio::test]
    async fn every_asset_response_is_sandboxed() {
        let (env, _folder) = Env::with_folder();

        let panel = env.get(HOST, ID, "panel.html").await;
        assert_eq!(panel.status(), StatusCode::OK);
        assert_sandboxed(&panel, "panel.html");
        let csp = panel.headers()["content-security-policy"].to_str().unwrap().to_owned();
        assert_eq!(csp, asset_csp(HOST, ID, &env.state.origins));
        assert!(csp.ends_with(&format!("; frame-ancestors http://{HOST}")), "{csp}");
        assert_eq!(panel.headers()["content-type"], "text/html");
        assert_eq!(panel.headers()["cache-control"], "no-cache");
        assert_eq!(body(panel).await, b"<p>panel</p>");

        let script = env.get(HOST, ID, "js/app.js").await;
        assert_eq!(script.status(), StatusCode::OK);
        assert!(script.headers()["content-type"].to_str().unwrap().contains("javascript"));

        for (id, path) in [(ID, "missing.js"), (ID, "../plugins.json"), (ID, "js"), (ID, "js/"), ("a.b", "panel.html"), ("a;b", "panel.html")] {
            let response = env.get(HOST, id, path).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{id}/{path}");
            assert_sandboxed(&response, &format!("{id}/{path}"));
            assert!(response.headers().get("access-control-allow-origin").is_none(), "{id}/{path}");
        }
    }

    #[tokio::test]
    async fn only_subresource_loads_may_read_a_file_across_origins() {
        let (env, _folder) = Env::with_folder();
        let acao = |response: &Response| response.headers().get("access-control-allow-origin").map(|value| value.to_str().unwrap().to_owned());
        // A module script in the opaque-origin frame is a CORS request from
        // `null`; so are styles, images, and fonts with `crossorigin`.
        for dest in ["script", "style", "image", "font"] {
            let response = env.get_as(headers_for(HOST, dest), ID, "js/app.js").await;
            assert_eq!(response.status(), StatusCode::OK, "{dest}");
            assert_eq!(acao(&response).as_deref(), Some("*"), "{dest}");
        }
        // `fetch()` from any website (`empty`), the frame's own page, or a
        // request that does not say what it is gets the file but cannot read it.
        for dest in ["empty", "iframe", "document", "SCRIPT", "script, empty"] {
            let response = env.get_as(headers_for(HOST, dest), ID, "js/app.js").await;
            assert_eq!(response.status(), StatusCode::OK, "{dest}");
            assert_eq!(acao(&response), None, "{dest}");
        }
        assert_eq!(acao(&env.get(HOST, ID, "panel.html").await), None, "no Sec-Fetch-Dest");
    }

    #[tokio::test]
    async fn dot_files_of_a_folder_source_are_not_served() {
        let (env, folder) = Env::with_folder();
        std::fs::write(folder.join(".env"), "TOKEN=secret").unwrap();
        std::fs::create_dir_all(folder.join(".git")).unwrap();
        std::fs::write(folder.join(".git/config"), "[remote]").unwrap();
        std::fs::create_dir_all(folder.join("js/.cache")).unwrap();
        std::fs::write(folder.join("js/.cache/x.js"), "1;").unwrap();
        for path in [".env", ".git/config", "js/.cache/x.js", ".git"] {
            let response = env.get_as(headers_for(HOST, "script"), ID, path).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
            assert_sandboxed(&response, path);
            assert!(body(response).await.is_empty(), "{path}");
        }
        assert_eq!(env.get(HOST, ID, "js/app.js").await.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_downloaded_package_is_served_from_its_version_folder() {
        let env = Env::new();
        let root = plugin_dir(env.state_dir.path(), ID).join("0.3.1");
        package(&root, "0.3.1");
        env.install(SourceKind::Url, &root);
        let response = env.get(HOST, ID, "icon.svg").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "image/svg+xml");
        assert_eq!(body(response).await, b"<svg/>");
    }

    #[test]
    fn the_csp_names_only_this_plugin_and_the_sdk() {
        let origins = ["http://127.0.0.1:4318".to_owned(), "http://localhost:4318".to_owned()];
        assert_eq!(
            asset_csp("127.0.0.1:4318", "k8s-tools.kubernetes", &origins),
            "sandbox allow-scripts; default-src 'none'; script-src http://127.0.0.1:4318/plugin-assets/k8s-tools.kubernetes/ http://127.0.0.1:4318/plugin-sdk/v1.js; style-src http://127.0.0.1:4318/plugin-assets/k8s-tools.kubernetes/ 'unsafe-inline'; img-src http://127.0.0.1:4318/plugin-assets/k8s-tools.kubernetes/; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'; object-src 'none'; frame-ancestors http://127.0.0.1:4318 http://localhost:4318"
        );
    }

    #[test]
    fn only_the_dashboard_may_frame_a_plugin_page() {
        let ancestors = |origins: &[&str]| {
            let origins: Vec<String> = origins.iter().map(|origin| (*origin).to_owned()).collect();
            let csp = asset_csp(HOST, ID, &origins);
            csp.rsplit_once("; frame-ancestors ").map(|(_, list)| list.to_owned()).unwrap()
        };
        assert_eq!(ancestors(&["http://127.0.0.1:4318", "http://localhost:5173"]), "http://127.0.0.1:4318 http://localhost:5173");
        // An --allow-origin value that would break the policy is left out.
        assert_eq!(ancestors(&["http://127.0.0.1:4318", "http://a; script-src *", "http://b,c", ""]), "http://127.0.0.1:4318");
        assert_eq!(ancestors(&[]), "'none'");
    }

    #[test]
    fn asset_paths_stay_inside_the_package() {
        let work = tempfile::tempdir().unwrap();
        let root = work.path().join("pkg");
        package(&root, "0.3.1");
        std::fs::write(work.path().join("plugins.json"), "{}").unwrap();

        assert_eq!(resolve_asset(&root, "js/app.js"), Some(root.join("js/app.js").canonicalize().unwrap()));
        assert!(resolve_asset(&root, "panel.html").is_some());
        for bad in [
            "../plugins.json", "js/../../x", "js/../app.js", "/etc/passwd", r"..\x", r"js\app.js", "./panel.html", "", "js/", "js",
            "js//app.js", "C:/x", "panel.html:stream", "a\0b", "missing.js", ".env", ".git/config", "js/.hidden.js",
        ] {
            assert!(resolve_asset(&root, bad).is_none(), "{bad:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn links_out_of_the_package_are_refused() {
        let work = tempfile::tempdir().unwrap();
        let root = work.path().join("pkg");
        package(&root, "0.3.1");
        std::fs::write(work.path().join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink("/etc", root.join("out")).unwrap();
        std::os::unix::fs::symlink(work.path().join("secret.txt"), root.join("secret.txt")).unwrap();
        std::os::unix::fs::symlink(root.join("panel.html"), root.join("inside.html")).unwrap();
        assert!(resolve_asset(&root, "out/passwd").is_none());
        assert!(resolve_asset(&root, "secret.txt").is_none());
        assert!(resolve_asset(&root, "inside.html").is_some(), "a link that stays in the folder is fine");
    }

    #[tokio::test]
    async fn a_foreign_host_is_refused() {
        let (env, _folder) = Env::with_folder();
        let response = env.get("evil.example:4317", ID, "panel.html").await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_sandboxed(&response, "foreign host");
        assert!(response.headers().get("access-control-allow-origin").is_none());
        assert!(body(response).await.is_empty());

        let mut no_host = headers(HOST);
        no_host.remove(axum::http::header::HOST);
        let response = serve_asset(State(env.state.clone()), no_host, UrlPath((ID.into(), "panel.html".into()))).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_sandboxed(&response, "no host");
    }

    #[tokio::test]
    async fn the_sdk_is_served_as_sandboxed_javascript() {
        let env = Env::new();
        let response = serve_sdk(State(env.state.clone()), headers(HOST)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "text/javascript; charset=utf-8");
        assert_eq!(response.headers()["content-security-policy"], "sandbox allow-scripts; default-src 'none'");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["cache-control"], "no-cache");
        assert!(String::from_utf8(body(response).await).unwrap().contains("window.westerm"));

        let foreign = serve_sdk(State(env.state.clone()), headers("evil.example:4318")).await;
        assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
        assert_sandboxed(&foreign, "sdk, foreign host");
    }

    /// Sends one raw HTTP/1.1 request to `address` and returns the head
    /// (status line and headers) and the body, lowercased.
    async fn request(address: std::net::SocketAddr, method: &str, path: &str) -> (String, String) {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let text = format!("{method} {path} HTTP/1.1\r\nHost: {HOST}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        stream.write_all(text.as_bytes()).await.unwrap();
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).await.unwrap();
        let answer = String::from_utf8_lossy(&answer).to_ascii_lowercase();
        let (head, body) = answer.split_once("\r\n\r\n").unwrap();
        (head.to_owned(), body.to_owned())
    }

    #[tokio::test]
    async fn every_path_under_plugin_assets_is_sandboxed() {
        let (env, _folder) = Env::with_folder();
        // As in the app: everything else falls back to the dashboard page,
        // which has no sandbox. `Router::layer` would cover a fallback of
        // `routes()` alone, so the test needs one of its own.
        let app = Router::new().merge(routes()).fallback(|| async { "dashboard" }).with_state(env.state.clone());
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await });

        let (ok, _) = request(address, "GET", &format!("/plugin-assets/{ID}/panel.html")).await;
        assert!(ok.starts_with("http/1.1 200"), "{ok}");
        let (_, outside) = request(address, "GET", "/somewhere").await;
        assert_eq!(outside, "dashboard", "the fallback is reachable");
        for (method, path) in [
            ("GET", "/plugin-assets"),
            ("GET", "/plugin-assets/"),
            ("GET", "/plugin-assets/a.b"),
            ("GET", "/plugin-assets/a.b/"),
            ("GET", "/plugin-assets//x"),
            ("GET", "/plugin-assets//"),
            ("GET", "/plugin-assets/k8s-tools.kubernetes/%2e%2e/plugin.json"),
            ("GET", "/plugin-assets/k8s-tools.kubernetes/js%2f..%2f..%2fplugin.json"),
            ("POST", "/plugin-assets/k8s-tools.kubernetes/panel.html"),
        ] {
            let (head, body) = request(address, method, path).await;
            assert!(!head.starts_with("http/1.1 200"), "{method} {path}: {head}");
            assert_ne!(body, "dashboard", "{method} {path}");
            assert!(!head.contains("access-control-allow-origin"), "{method} {path}: {head}");
            assert!(head.contains("\r\ncontent-security-policy: sandbox allow-scripts;"), "{method} {path}: {head}");
            assert!(head.contains("\r\nx-content-type-options: nosniff"), "{method} {path}: {head}");
        }
        let (sdk, _) = request(address, "GET", "/plugin-sdk/v1.js").await;
        assert!(sdk.starts_with("http/1.1 200"), "{sdk}");
        server.abort();
    }
}
