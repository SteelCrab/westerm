//! The app's windows: the main one with the dashboard, and one per remote
//! server. Each window stays on the dashboard. A link with a target
//! (`target="_blank"`, `window.open`) to another page opens in the default
//! browser; any other navigation away from the dashboard is refused.

use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window, WindowEvent, Wry};
use tauri_plugin_opener::OpenerExt;

use crate::state::shared;

pub const MAIN: &str = "main";
const REMOTE_PREFIX: &str = "remote-";

/// Gives each page `window.westermApp` (src/appBridge.ts). The dashboard on
/// 127.0.0.1 may call `app_request` (capabilities/dashboard.json); the app
/// checks the page's address again (bridge.rs).
const BRIDGE: &str = r#"(() => {
  if (window.westermApp) return;
  const request = (message) => window.__TAURI_INTERNALS__.invoke("app_request", { message });
  Object.defineProperty(window, "westermApp", { value: Object.freeze({ request }) });
})();"#;

/// A message on the start page (ui/index.html).
#[derive(Clone, Copy, Debug)]
pub enum Page {
    CannotStart,
    Stopped,
}

impl Page {
    fn state(self) -> &'static str {
        match self {
            Self::CannotStart => "cannot-start",
            Self::Stopped => "stopped",
        }
    }
}

fn configure<'a, M: Manager<Wry>>(
    app: &AppHandle,
    builder: WebviewWindowBuilder<'a, Wry, M>,
) -> WebviewWindowBuilder<'a, Wry, M> {
    let navigating = app.clone();
    let opening = app.clone();
    builder
        .inner_size(1280.0, 820.0)
        .min_inner_size(640.0, 400.0)
        .center()
        .initialization_script(BRIDGE)
        // The page moves tabs and panes with HTML drag and drop; the native
        // file drop handler would take those events (Windows).
        .disable_drag_drop_handler()
        .on_navigation(move |url| follow(&navigating, url))
        .on_new_window(move |url, _| {
            // Not inside the webview's own handler: a window made there can
            // block the app on Windows.
            let app = opening.clone();
            std::thread::spawn(move || open_new(&app, &url));
            NewWindowResponse::Deny
        })
}

/// Makes the main window with the start page ("Starting westerm…").
pub fn create_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = configure(app, WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into())))
        .title("westerm")
        .build()?;
    *shared(app).start_page.lock().unwrap() = window.url().ok();
    Ok(window)
}

/// Shows the dashboard on `port` in the main window.
pub fn show_dashboard(app: &AppHandle, port: u16) {
    let shared = shared(app);
    shared.set_dashboard(Some(port));
    if let (Some(window), Some(url)) = (app.get_webview_window(MAIN), shared.dashboard_url()) {
        let _ = window.navigate(url);
    }
}

/// Shows a message on the start page in the main window.
pub fn show_page(app: &AppHandle, page: Page) {
    let shared = shared(app);
    let Some(mut url) = shared.start_page.lock().unwrap().clone() else { return };
    url.query_pairs_mut()
        .clear()
        .append_pair("state", page.state())
        .append_pair("log", &shared.log.path.to_string_lossy());
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.navigate(url);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Navigation {
    Allow,
    /// Not the dashboard. A new window (`target="_blank"`) opens it in the
    /// default browser; a navigation of the page or a frame is refused.
    External,
    Refuse,
}

/// Where a window may go: the app's start page and the dashboard on its port.
fn navigation(url: &Url, dashboard: Option<u16>) -> Navigation {
    let host = url.host_str();
    match url.scheme() {
        "tauri" => Navigation::Allow,
        "http" | "https" if host == Some("tauri.localhost") => Navigation::Allow,
        "http" if host == Some("127.0.0.1") && dashboard.is_some() && url.port() == dashboard => Navigation::Allow,
        "http" | "https" | "mailto" => Navigation::External,
        "about" | "blob" => Navigation::Allow,
        _ => Navigation::Refuse,
    }
}

/// Whether a navigation in a window goes ahead. wry asks for every
/// navigation, frames included, and does not say which frame it is, so an
/// outside address is refused without opening it: a plugin's iframe could
/// otherwise open any address in the user's browser. The dashboard opens
/// outside pages only through new windows (`on_new_window`).
fn may_follow(url: &Url, dashboard: Option<u16>) -> bool {
    navigation(url, dashboard) == Navigation::Allow
}

fn follow(app: &AppHandle, url: &Url) -> bool {
    may_follow(url, shared(app).dashboard_port())
}

/// `window.open` and links with a target: a dashboard's remote window
/// (`/?remote=<target>`) opens as an app window, anything else in the browser.
fn open_new(app: &AppHandle, url: &Url) {
    let dashboard = shared(app).dashboard_port();
    if navigation(url, dashboard) == Navigation::Allow && url.scheme() == "http" {
        if let Some((_, target)) = url.query_pairs().find(|(key, _)| key == "remote") {
            let _ = open_remote(app, &target);
        }
        return;
    }
    if navigation(url, dashboard) == Navigation::External {
        open_external(app, url);
    }
}

pub fn open_external(app: &AppHandle, url: &Url) {
    if let Err(error) = app.opener().open_url(url.as_str(), None::<&str>) {
        shared(app).log.write(&format!("westerm-app: could not open {url}: {error}"));
    }
}

/// An ssh destination exactly as the dashboard checks it
/// (shared/workspace-file.ts isSshTarget, native/src/protocol.rs).
fn is_ssh_target(text: &str) -> bool {
    if text.is_empty() || text.len() > 255 || text.trim() != text {
        return false;
    }
    let (user, host) = match text.split_once('@') {
        Some((user, host)) => (user, Some(host)),
        None => (text, None),
    };
    let user_ok = !user.is_empty()
        && !user.starts_with('-')
        && user.chars().all(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c));
    let host_ok = host.is_none_or(|host| {
        !host.is_empty() && !host.starts_with('-') && host.chars().all(|c| c.is_ascii_alphanumeric() || "._:[]-".contains(c))
    });
    user_ok && host_ok
}

/// One window per server. Labels allow only some characters, so the target is
/// written in hexadecimal; the same server always gets the same label, and
/// the window keeps its size and place.
fn remote_label(target: &str) -> String {
    let hex: String = target.bytes().map(|byte| format!("{byte:02x}")).collect();
    format!("{REMOTE_PREFIX}{hex}")
}

/// `/?remote=<target>` with everything but unreserved characters encoded, so
/// "+" cannot turn into a space.
fn remote_url(port: u16, target: &str) -> String {
    let encoded: String = target
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect();
    format!("http://127.0.0.1:{port}/?remote={encoded}")
}

/// Opens `target`'s window, or brings it to the front when it is open. The
/// page cannot raise a window it opened, so it asks the app every time.
pub fn open_remote(app: &AppHandle, target: &str) -> Result<(), String> {
    if !is_ssh_target(target) {
        return Err(format!("not a server: {target}"));
    }
    let label = remote_label(target);
    if let Some(window) = app.get_webview_window(&label) {
        raise(&window);
        return Ok(());
    }
    let port = shared(app).dashboard_port().ok_or("westerm is not connected yet")?;
    let url: Url = remote_url(port, target).parse().map_err(|e| format!("{e}"))?;
    configure(app, WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url)))
        .title(format!("westerm — {target}"))
        .on_document_title_changed(|window, title| {
            if !title.is_empty() {
                let _ = window.set_title(&title);
            }
        })
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn raise(window: &WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        raise(&window);
    }
}

fn remote_windows(app: &AppHandle, except: &str) -> usize {
    app.webview_windows().keys().filter(|label| label.starts_with(REMOTE_PREFIX) && label.as_str() != except).count()
}

/// The window with the keyboard, or the main one.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn active(app: &AppHandle) -> Option<WebviewWindow> {
    let windows = app.webview_windows();
    windows
        .values()
        .find(|window| window.is_focused().unwrap_or(false))
        .cloned()
        .or_else(|| windows.get(MAIN).cloned())
}

/// The main window loads the dashboard again (its panes reattach); a remote
/// window reloads its own page.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn reload(app: &AppHandle, window: &WebviewWindow) {
    let url = if window.label() == MAIN {
        let shared = shared(app);
        shared.dashboard_url().or_else(|| shared.start_page.lock().unwrap().clone())
    } else {
        window.url().ok()
    };
    if let Some(url) = url {
        let _ = window.navigate(url);
    }
}

/// The page in the default browser: the dashboard for the main window.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn open_in_browser(app: &AppHandle, window: &WebviewWindow) {
    let url = if window.label() == MAIN { shared(app).dashboard_url() } else { window.url().ok() };
    if let Some(url) = url {
        open_external(app, &url);
    }
}

/// Closing the main window:
/// - macOS: hides it while remote windows are open; the Dock icon brings it
///   back. The app quits with its last window, as Mac apps of one window do.
/// - Windows and Linux: quits, with the remote windows. There is no Dock to
///   bring a hidden window back.
pub fn on_event(window: &Window, event: &WindowEvent) {
    let app = window.app_handle();
    match event {
        WindowEvent::CloseRequested { api, .. } if window.label() == MAIN => {
            if cfg!(target_os = "macos") {
                if remote_windows(app, "") > 0 {
                    api.prevent_close();
                    let _ = window.hide();
                }
            } else {
                app.exit(0);
            }
        }
        WindowEvent::Destroyed if window.label().starts_with(REMOTE_PREFIX) => {
            let main_hidden = app.get_webview_window(MAIN).is_some_and(|main| !main.is_visible().unwrap_or(true));
            if main_hidden && remote_windows(app, window.label()) == 0 {
                app.exit(0);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        text.parse().unwrap()
    }

    #[test]
    fn windows_stay_on_the_dashboard() {
        let port = Some(4317);
        assert_eq!(navigation(&url("http://127.0.0.1:4317/?remote=prod"), port), Navigation::Allow);
        assert_eq!(navigation(&url("tauri://localhost/index.html?state=stopped"), port), Navigation::Allow);
        assert_eq!(navigation(&url("http://tauri.localhost/index.html"), port), Navigation::Allow, "Windows");
        assert_eq!(navigation(&url("http://127.0.0.1:8080/"), port), Navigation::External, "another local server");
        assert_eq!(navigation(&url("http://127.0.0.1:4317/"), None), Navigation::External, "before the dashboard");
        assert_eq!(navigation(&url("http://localhost:4317/"), port), Navigation::External);
        assert_eq!(navigation(&url("https://github.com/SteelCrab/westerm"), port), Navigation::External);
        assert_eq!(navigation(&url("file:///etc/passwd"), port), Navigation::Refuse);
        assert_eq!(navigation(&url("javascript:alert(1)"), port), Navigation::Refuse);
    }

    #[test]
    fn navigations_away_from_the_dashboard_are_refused_not_opened() {
        let port = Some(4317);
        assert!(may_follow(&url("http://127.0.0.1:4317/?remote=prod"), port));
        assert!(may_follow(&url("tauri://localhost/index.html"), port));
        assert!(may_follow(&url("about:blank"), port), "an empty frame");
        assert!(!may_follow(&url("https://evil.example/?d=secret"), port), "a frame leaving its page");
        assert!(!may_follow(&url("mailto:someone@example.com"), port));
        assert!(!may_follow(&url("http://127.0.0.1:8080/"), port));
        assert!(!may_follow(&url("file:///etc/passwd"), port));
    }

    #[test]
    fn server_names_follow_the_dashboard_rules() {
        for ok in ["prod", "deploy@10.0.0.5", "me@build-box.local", "u@[::1]", "pista@msi"] {
            assert!(is_ssh_target(ok), "{ok}");
        }
        for bad in ["-oProxyCommand=evil", "a b", "host;rm", "$(id)", "me@-x", "", " prod", "x".repeat(256).as_str()] {
            assert!(!is_ssh_target(bad), "{bad}");
        }
    }

    #[test]
    fn remote_labels_are_stable_and_valid() {
        let label = remote_label("pista@msi.local");
        assert_eq!(label, remote_label("pista@msi.local"));
        assert_ne!(label, remote_label("pista@msi"));
        assert!(label.starts_with(REMOTE_PREFIX));
        // Tauri labels: letters, digits, '-', '/', ':', '_'.
        assert!(label.chars().all(|c| c.is_ascii_alphanumeric() || "-/:_".contains(c)), "{label}");
    }

    #[test]
    fn remote_urls_encode_the_target() {
        assert_eq!(remote_url(4317, "pista@msi"), "http://127.0.0.1:4317/?remote=pista%40msi");
        assert_eq!(remote_url(4317, "a+b@[::1]"), "http://127.0.0.1:4317/?remote=a%2Bb%40%5B%3A%3A1%5D");
        let parsed = url(&remote_url(4317, "a+b@h"));
        assert_eq!(parsed.query_pairs().find(|(key, _)| key == "remote").unwrap().1, "a+b@h");
    }
}
