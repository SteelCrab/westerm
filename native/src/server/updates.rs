//! /api/update: whether a newer release exists, and, when the user asks,
//! installing it and restarting the dashboard on the new executable.
//! WESTERM_NO_UPDATE_CHECK turns the check off.

use std::path::Path;
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use tokio::sync::Mutex;

use super::{no_store, origin_allowed, Shared};
use crate::update::{self, Release};

/// A successful check is reused this long; a failed one is retried sooner.
const FRESH: Duration = Duration::from_secs(6 * 60 * 60);
const RETRY: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
pub struct UpdateCheck(Mutex<Option<(Instant, Result<Release, String>)>>);

impl UpdateCheck {
    pub async fn latest(&self, refresh: bool) -> Result<Release, String> {
        let mut cached = self.0.lock().await;
        if let Some((at, result)) = cached.as_ref() {
            let keep = if result.is_ok() { FRESH } else { RETRY };
            if !refresh && at.elapsed() < keep {
                return result.clone();
            }
        }
        let result = tokio::task::spawn_blocking(update::latest).await.unwrap_or_else(|e| Err(e.to_string()));
        *cached = Some((Instant::now(), result.clone()));
        result
    }
}

pub fn disabled() -> bool {
    std::env::var_os("WESTERM_NO_UPDATE_CHECK").is_some()
}

#[derive(Deserialize)]
pub struct CheckQuery {
    refresh: Option<String>,
}

pub async fn get_update(State(state): State<Shared>, Query(query): Query<CheckQuery>) -> Response {
    let mut body = serde_json::json!({ "current": update::CURRENT, "channel": update::channel() });
    if disabled() {
        body["disabled"] = true.into();
        return no_store(Json(body));
    }
    match state.updates.latest(query.refresh.is_some()).await {
        Ok(release) => {
            body["newer"] = update::is_newer(&release.version, update::CURRENT).into();
            body["latest"] = release.version.into();
            body["page"] = release.page.into();
        }
        Err(error) => body["error"] = error.into(),
    }
    no_store(Json(body))
}

fn failure(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}

/// Installs the newest release over this executable, answers, then restarts
/// on it. The new dashboard replaces the PTY host, so open shells come back
/// once as "restored".
pub async fn post_update(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if !origin_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if disabled() {
        return failure(StatusCode::CONFLICT, "updates are turned off (WESTERM_NO_UPDATE_CHECK)");
    }
    let release = match state.updates.latest(true).await {
        Ok(release) => release,
        Err(error) => return failure(StatusCode::BAD_GATEWAY, error),
    };
    if !update::is_newer(&release.version, update::CURRENT) {
        return failure(StatusCode::CONFLICT, "westerm is up to date");
    }
    // Before the swap: afterwards Linux reports this executable as deleted.
    let exe = match update::executable() {
        Ok(exe) => exe,
        Err(error) => return failure(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let installed = {
        let (release, exe) = (release.clone(), exe.clone());
        tokio::task::spawn_blocking(move || update::install(&release, &exe)).await.unwrap_or_else(|e| Err(e.to_string()))
    };
    if let Err(error) = installed {
        return failure(StatusCode::CONFLICT, error);
    }
    println!("> updated to {}; restarting", release.version);
    tokio::spawn(async move {
        // Let this answer reach the browser first.
        tokio::time::sleep(Duration::from_millis(300)).await;
        restart(&exe);
    });
    no_store(Json(serde_json::json!({ "status": "restarting", "version": release.version })))
}

/// The arguments to start again with: the same, minus --port, since the
/// port goes through PORT (a --port 0 dashboard keeps its actual port).
fn restart_args() -> Vec<std::ffi::OsString> {
    let mut args = Vec::new();
    let mut given = std::env::args_os().skip(1);
    while let Some(arg) = given.next() {
        if arg == "--port" {
            given.next();
        } else {
            args.push(arg);
        }
    }
    args
}

/// Runs the updated executable in place of this one, with the same arguments.
fn restart(exe: &Path) -> ! {
    let mut command = std::process::Command::new(exe);
    command.args(restart_args()).env("WESTERM_RESTARTING", "1");
    if let Some(port) = super::BOUND_PORT.get() {
        command.env("PORT", port.to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Same process id; `npm start` and terminals keep their child.
        let error = command.exec();
        eprintln!("> restart failed: {error}");
        std::process::exit(1);
    }
    #[cfg(windows)]
    {
        // The new process waits for this one to free the port.
        match command.spawn() {
            Ok(_) => std::process::exit(0),
            Err(error) => {
                eprintln!("> restart failed: {error}");
                std::process::exit(1);
            }
        }
    }
}
