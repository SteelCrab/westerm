//! What the app keeps while it runs, and finding or starting the dashboard.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, Url};

use crate::server::{self, Log, Server};
use crate::windows::{self, Page};

pub struct Shared {
    pub log: Arc<Log>,
    /// The port of the dashboard the windows show, once there is one.
    dashboard: Mutex<Option<u16>>,
    /// The server this app started; None when it uses another westerm.
    server: Mutex<Option<Server>>,
    /// The start page's address, where messages are shown.
    pub start_page: Mutex<Option<Url>>,
    /// Set while an update downloads and installs.
    pub updating: AtomicBool,
    /// Set when the app stops its server on purpose (quit, update), so its
    /// end is not shown as a failure.
    stopping: AtomicBool,
}

impl Shared {
    pub fn new(log: Log) -> Self {
        Self {
            log: Arc::new(log),
            dashboard: Mutex::new(None),
            server: Mutex::new(None),
            start_page: Mutex::new(None),
            updating: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
        }
    }

    pub fn dashboard_port(&self) -> Option<u16> {
        *self.dashboard.lock().unwrap()
    }

    pub fn dashboard_url(&self) -> Option<Url> {
        self.dashboard_port().and_then(|port| format!("http://127.0.0.1:{port}/").parse().ok())
    }

    pub fn set_dashboard(&self, port: Option<u16>) {
        *self.dashboard.lock().unwrap() = port;
    }

    /// Stops the server this app started, if any. The PTY host lives on.
    pub fn stop_server(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(server) = self.server.lock().unwrap().take() {
            server.stop();
        }
    }
}

pub fn shared(app: &AppHandle) -> Arc<Shared> {
    Arc::clone(&app.state::<Arc<Shared>>())
}

/// Opens the westerm on the preferred port, or starts one there, or on any
/// free port when another program holds it. Runs off the main thread.
pub fn connect(app: AppHandle) {
    std::thread::spawn(move || {
        let shared = shared(&app);
        let port = server::preferred_port();
        if server::probe(port) {
            shared.log.write(&format!("westerm-app: using the westerm on port {port}"));
            windows::show_dashboard(&app, port);
            return;
        }
        match start(&app, port).or_else(|first| {
            shared.log.write(&format!("westerm-app: {first}; trying any free port"));
            start(&app, 0)
        }) {
            Ok(port) => windows::show_dashboard(&app, port),
            Err(error) => {
                shared.log.write(&format!("westerm-app: {error}"));
                windows::show_page(&app, Page::CannotStart);
            }
        }
    });
}

/// Finds or starts a dashboard again, after the app stopped its own for an
/// update that failed.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub fn reconnect(app: &AppHandle) {
    let shared = shared(app);
    shared.stopping.store(false, Ordering::SeqCst);
    shared.set_dashboard(None);
    connect(app.clone());
}

fn start(app: &AppHandle, port: u16) -> Result<u16, String> {
    let shared = shared(app);
    let exe = server::sidecar()?;
    let handle = app.clone();
    let (server, port) = Server::start(&exe, port, Arc::clone(&shared.log), move |_| {
        let shared = self::shared(&handle);
        if !shared.stopping.load(Ordering::SeqCst) {
            shared.set_dashboard(None);
            windows::show_page(&handle, Page::Stopped);
        }
    })?;
    *shared.server.lock().unwrap() = Some(server);
    Ok(port)
}
