//! Starts the PTY host once and reuses it. A host from another build is asked
//! to save and stop first; its shells are then revived from saved screens.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::host::HOST_BUILD;
use crate::ipc;
use crate::state_dir::{legacy_state_dir, private_file, state_dir};

/// Caches the state directory of a host known to run the current build.
#[derive(Default)]
pub struct HostClient {
    ready: Mutex<Option<PathBuf>>,
}

/// Status and body of an HTTP/1.1 response read to the end. Handles chunked
/// bodies: Node writes them when it sends no Content-Length, and the
/// Node-era host answers /health that way.
fn parse_response(raw: &[u8]) -> Option<(u16, String)> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n")?;
    let status = head.split_whitespace().nth(1)?.parse().ok()?;
    let chunked = head
        .lines()
        .any(|line| line.to_ascii_lowercase().replace(' ', "") == "transfer-encoding:chunked");
    if !chunked {
        return Some((status, body.to_owned()));
    }
    let mut decoded = String::new();
    let mut rest = body;
    loop {
        let (size_line, after) = rest.split_once("\r\n")?;
        let size = usize::from_str_radix(size_line.split(';').next()?.trim(), 16).ok()?;
        if size == 0 {
            return Some((status, decoded));
        }
        decoded.push_str(after.get(..size)?);
        rest = after.get(size..)?.strip_prefix("\r\n")?;
    }
}

/// One HTTP/1.1 request to the host of the state directory `dir`. Returns (status, body).
async fn request(dir: &Path, method: &str, path: &str) -> Option<(u16, String)> {
    let exchange = async {
        let mut stream = ipc::connect(dir).await.ok()?;
        let head = format!("{method} {path} HTTP/1.1\r\nHost: host\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
        stream.write_all(head.as_bytes()).await.ok()?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.ok()?;
        parse_response(&response)
    };
    tokio::time::timeout(Duration::from_millis(1500), exchange).await.ok().flatten()
}

async fn running_build(dir: &Path) -> Option<String> {
    let (status, body) = request(dir, "GET", "/health").await?;
    if status != 200 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(&body).ok()?;
    value.get("build")?.as_str().map(str::to_owned)
}

impl HostClient {
    /// The state directory of a running host, started if needed; pass it to `ipc::connect`.
    pub async fn ensure(&self) -> Result<PathBuf, String> {
        let mut ready = self.ready.lock().await;
        if let Some(dir) = ready.as_ref() {
            return Ok(dir.clone());
        }
        let dir = start().await?;
        *ready = Some(dir.clone());
        Ok(dir)
    }

    /// Forget the cached host, e.g. after a failed connection.
    pub async fn invalidate(&self) {
        *self.ready.lock().await = None;
    }
}

/// Moves state from the folder used before the rename (wterm-dashboard) into
/// this one. A host still running there is asked to save and stop first, so
/// its shells come back with their screens ("revived") under the new name.
pub async fn migrate_legacy_state() {
    let Some(legacy) = legacy_state_dir() else { return };
    if running_build(&legacy).await.is_some() {
        println!("> stopping the PTY host in {} to move its sessions", legacy.display());
        stop_and_wait(&legacy).await;
    }
    match state_dir().and_then(|new| move_legacy_files(&legacy, &new)) {
        Ok(0) => {}
        Ok(moved) => println!("> moved {moved} saved files from {}", legacy.display()),
        Err(error) => eprintln!("> could not move state from {}: {error}", legacy.display()),
    }
}

/// Moves the layout and saved sessions without overwriting anything already
/// in the new folder. Leaves a note in the old one. Returns how many moved.
fn move_legacy_files(legacy: &Path, new: &Path) -> std::io::Result<usize> {
    let mut moved = 0;
    let layout = legacy.join("workspace.json");
    if layout.is_file() && !new.join("workspace.json").exists() {
        fs::rename(&layout, new.join("workspace.json"))?;
        moved += 1;
    }
    let old_sessions = legacy.join("sessions");
    if old_sessions.is_dir() {
        let new_sessions = new.join("sessions");
        fs::create_dir_all(&new_sessions)?;
        for entry in fs::read_dir(&old_sessions)?.flatten() {
            let target = new_sessions.join(entry.file_name());
            if !target.exists() {
                fs::rename(entry.path(), target)?;
                moved += 1;
            }
        }
        let _ = fs::remove_dir(&old_sessions); // Only when empty.
    }
    if moved > 0 {
        let note = format!("Moved to {} when wterm-dashboard was renamed to westerm.\n", new.display());
        let _ = fs::write(legacy.join("MOVED.txt"), note);
    }
    Ok(moved)
}

/// Stops the host of this user's state directory. False when none was running.
pub async fn stop_running() -> std::io::Result<bool> {
    let dir = state_dir()?;
    if running_build(&dir).await.is_none() {
        return Ok(false);
    }
    stop_and_wait(&dir).await;
    if ipc::is_gone(&dir) {
        Ok(true)
    } else {
        Err(std::io::Error::other("the PTY host did not stop within 5 s"))
    }
}

/// Asks the host to save and stop, and waits up to 5 s for its endpoint to go.
async fn stop_and_wait(dir: &Path) {
    let _ = request(dir, "POST", "/shutdown").await;
    for _ in 0..50 {
        if ipc::is_gone(dir) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn start() -> Result<PathBuf, String> {
    let dir = state_dir().map_err(|e| e.to_string())?;
    match running_build(&dir).await {
        Some(build) if build == HOST_BUILD => return Ok(dir),
        Some(build) => {
            println!("> pty host {build} is outdated; restarting it as {HOST_BUILD}");
            stop_and_wait(&dir).await;
        }
        None => {}
    }

    let log = private_file()
        .create(true)
        .append(true)
        .open(dir.join("host.log"))
        .map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = std::process::Command::new(exe);
    command
        .arg("host")
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    let child = spawn_detached(&mut command).map_err(|e| format!("could not start the PTY host: {e}"))?;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if running_build(&dir).await.as_deref() == Some(HOST_BUILD) {
            println!("> pty host {HOST_BUILD} started (pid {})", child.id());
            return Ok(dir);
        }
    }
    Err(format!("the PTY host did not start; see {}", dir.join("host.log").display()))
}

/// Starts the host apart from the dashboard: closing the terminal that runs
/// the dashboard must not end the host and its shells.
#[cfg(unix)]
fn spawn_detached(command: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    // SAFETY: setsid is async-signal-safe and touches no memory of the parent.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command.spawn()
}

/// Starts the host with no console and in its own process group. It also
/// leaves the parent's job when the job allows that, so a terminal that ends
/// its job on close does not take the host along.
#[cfg(windows)]
fn spawn_detached(command: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
    match command.spawn() {
        Ok(child) => Ok(child),
        // A job without breakaway rights refuses that flag.
        Err(_) => {
            command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
            command.spawn()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_response;

    #[test]
    fn reads_plain_and_chunked_responses() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\n{\"build\":\"x\"}";
        assert_eq!(parse_response(plain), Some((200, r#"{"build":"x"}"#.to_owned())));
        // What the Node host sends for res.end(JSON.stringify(...)).
        let chunked = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n26\r\n{\"build\":\"8ff41cb4215e\",\"pid\":1,\"x\":2}\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked), Some((200, r#"{"build":"8ff41cb4215e","pid":1,"x":2}"#.to_owned())));
        assert_eq!(parse_response(b"HTTP/1.1 204 No Content\r\n\r\n"), Some((204, String::new())));
        assert_eq!(parse_response(b"garbage"), None);
    }
}

#[cfg(test)]
mod migration_tests {
    use super::move_legacy_files;
    use std::fs;

    #[test]
    fn moves_layout_and_sessions_without_overwriting() {
        let root = tempfile::tempdir().unwrap();
        let (legacy, new) = (root.path().join("wterm-dashboard"), root.path().join("westerm"));
        fs::create_dir_all(legacy.join("sessions")).unwrap();
        fs::create_dir_all(new.join("sessions")).unwrap();
        fs::write(legacy.join("workspace.json"), "old layout").unwrap();
        fs::write(legacy.join("sessions/sa.json"), "a").unwrap();
        fs::write(legacy.join("sessions/sb.json"), "old b").unwrap();
        fs::write(new.join("sessions/sb.json"), "new b").unwrap();

        assert_eq!(move_legacy_files(&legacy, &new).unwrap(), 2);
        assert_eq!(fs::read_to_string(new.join("workspace.json")).unwrap(), "old layout");
        assert_eq!(fs::read_to_string(new.join("sessions/sa.json")).unwrap(), "a");
        assert_eq!(fs::read_to_string(new.join("sessions/sb.json")).unwrap(), "new b", "newer state wins");
        assert!(legacy.join("MOVED.txt").exists());
        assert_eq!(move_legacy_files(&legacy, &new).unwrap(), 0, "running again moves nothing");
    }
}
