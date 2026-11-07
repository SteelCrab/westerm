//! Requests from the dashboard page: `window.westermApp.request(message)`
//! (src/appBridge.ts). Only the dashboard may ask.

use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Url, Webview};

use crate::state::shared;
use crate::update::{self, Outcome};
use crate::windows;

/// src/appBridge.ts AppMessage.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AppMessage {
    /// Whether a newer app exists; `refresh` asks GitHub again.
    UpdateStatus {
        #[serde(default)]
        refresh: bool,
    },
    Update,
    RemoteWindow { target: String },
}

/// Whether `page` is the dashboard the windows show.
fn from_dashboard(page: &Url, dashboard: Option<u16>) -> bool {
    page.scheme() == "http" && page.host_str() == Some("127.0.0.1") && dashboard.is_some() && page.port() == dashboard
}

/// Async: on Windows a window made in a synchronous command blocks the app.
#[tauri::command]
pub async fn app_request(app: AppHandle, webview: Webview, message: AppMessage) -> Result<Value, String> {
    let page = webview.url().map_err(|e| e.to_string())?;
    if !from_dashboard(&page, shared(&app).dashboard_port()) {
        return Err("only the westerm dashboard may ask the app".into());
    }
    match message {
        AppMessage::RemoteWindow { target } => windows::open_remote(&app, &target).map(|()| Value::Null),
        AppMessage::UpdateStatus { refresh } => {
            let handle = app.clone();
            tauri::async_runtime::spawn_blocking(move || update::status(&handle, refresh)).await.map_err(|e| e.to_string())
        }
        AppMessage::Update => {
            let handle = app.clone();
            let outcome = tauri::async_runtime::spawn_blocking(move || update::install(&handle))
                .await
                .map_err(|e| e.to_string())??;
            if outcome == Outcome::Relaunching {
                // Answer the page first, then quit; the new copy is on its way.
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(300));
                    app.exit(0);
                });
            }
            Ok(json!(outcome.as_str()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_from_other_pages_are_refused() {
        let page = |text: &str| text.parse::<Url>().unwrap();
        assert!(from_dashboard(&page("http://127.0.0.1:4317/"), Some(4317)));
        assert!(from_dashboard(&page("http://127.0.0.1:4317/?remote=prod"), Some(4317)));
        assert!(!from_dashboard(&page("http://127.0.0.1:4318/"), Some(4317)), "another westerm or program");
        assert!(!from_dashboard(&page("http://localhost:4317/"), Some(4317)));
        assert!(!from_dashboard(&page("tauri://localhost/index.html"), Some(4317)), "the start page");
        assert!(!from_dashboard(&page("http://127.0.0.1:4317/"), None), "before the dashboard");
    }

    #[test]
    fn messages_read_as_the_page_writes_them() {
        let read = |text: &str| serde_json::from_str::<AppMessage>(text);
        assert_eq!(read(r#"{"type":"update"}"#).unwrap(), AppMessage::Update);
        assert_eq!(read(r#"{"type":"updateStatus","refresh":true}"#).unwrap(), AppMessage::UpdateStatus { refresh: true });
        assert_eq!(read(r#"{"type":"updateStatus"}"#).unwrap(), AppMessage::UpdateStatus { refresh: false });
        assert_eq!(
            read(r#"{"type":"remoteWindow","target":"prod"}"#).unwrap(),
            AppMessage::RemoteWindow { target: "prod".into() }
        );
        assert!(read(r#"{"type":"shell","command":"rm"}"#).is_err());
    }
}
