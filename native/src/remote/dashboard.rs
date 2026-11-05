//! `westerm remote-dashboard`: the dashboard a remote window talks to, on the
//! server. It listens on a private Unix socket only, never on TCP, and it
//! answers just what a remote window needs (see `server::remote_router`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::daemon::{termination, Daemon, Record};
use crate::{ipc, server};

pub static DASHBOARD: Daemon = Daemon::new("dashboard");

/// What a running remote dashboard wrote to `dashboard.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DashboardRecord {
    pub pid: u32,
    /// `version_line()` of the binary that runs it.
    pub version: String,
}

pub fn socket_path(state: &Path) -> PathBuf {
    state.join("dashboard.sock")
}

impl Record for DashboardRecord {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn version(&self) -> &str {
        &self.version
    }
}

/// Asks the running remote dashboard to stop. False when none ran.
pub async fn stop_running(state: &Path) -> io::Result<bool> {
    DASHBOARD.stop_running(state).await
}

/// `westerm remote-dashboard` for the state directory `state` (see state_dir()).
pub async fn run(state: &Path) -> io::Result<()> {
    let lock = DASHBOARD.lock(state)?;
    let socket = socket_path(state);
    let listener = ipc::bind_socket(&socket)?;
    let version = crate::version_line();
    DASHBOARD.write_record(state, &DashboardRecord { pid: std::process::id(), version: version.clone() })?;
    println!("remote dashboard {version} listening on {} (pid {})", socket.display(), std::process::id());

    // Not a graceful shutdown: that would wait for open panes, and a pane
    // stays open for as long as its remote window does.
    let app = server::remote_router(state.to_path_buf());
    let reason = tokio::select! {
        result = axum::serve(listener, app) => {
            if let Err(error) = result {
                eprintln!("remote dashboard: {error}");
            }
            "the listener failed"
        }
        signal = termination() => signal,
    };
    println!("remote dashboard stopping ({reason})");
    // The record goes before the lock, as in the gate: whoever waits for the
    // lock may start a new copy, which writes its own record.
    DASHBOARD.remove_record(state);
    let _ = fs::remove_file(&socket);
    drop(lock);
    Ok(())
}
