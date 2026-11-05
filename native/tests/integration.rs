//! The real binary, in a temp state dir: a shell survives a dropped
//! connection and a host restart, and the dashboard enforces its origin.
//! Runs on macOS, Linux (Unix socket), and Windows (named pipe).

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

mod common;
use common::{Spawned, StopHostOnDrop, Viewer, BIN};

const QUERY: &str = "session=stest1&kind=shell&cols=80&rows=24&width=640&height=408&lang=ko";

/// Prints MARK_42 only when the shell runs it: the typed line itself does not contain that text.
#[cfg(unix)]
const MARK_INPUT: &str = r#"{"type":"input","data":"echo MARK_$((40+2))\r"}"#;
#[cfg(windows)]
const MARK_INPUT: &str = r#"{"type":"input","data":"echo ('MARK_' + (40+2))\r"}"#;

#[cfg(unix)]
type Stream = tokio::net::UnixStream;
#[cfg(windows)]
type Stream = tokio::net::windows::named_pipe::NamedPipeClient;

/// The file that appears once the host listens.
fn endpoint(state: &Path) -> std::path::PathBuf {
    state.join(if cfg!(windows) { "host.pipe" } else { "host.sock" })
}

#[cfg(unix)]
async fn connect(state: &Path) -> Stream {
    tokio::net::UnixStream::connect(endpoint(state)).await.unwrap()
}

#[cfg(windows)]
async fn connect(state: &Path) -> Stream {
    let record = std::fs::read_to_string(endpoint(state)).unwrap();
    let name = record.lines().next().unwrap();
    tokio::net::windows::named_pipe::ClientOptions::new().open(name).unwrap()
}

fn start_host(state: &Path) -> Spawned {
    let child = Command::new(BIN)
        .arg("host")
        .env("WESTERM_STATE_DIR", state)
        .env("SHELL", "/bin/sh")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !endpoint(state).exists() {
        assert!(Instant::now() < deadline, "host endpoint appears");
        std::thread::sleep(Duration::from_millis(30));
    }
    Spawned(child)
}

async fn http(state: &Path, method: &str, path: &str) -> String {
    let mut stream = connect(state).await;
    let head = format!("{method} {path} HTTP/1.1\r\nHost: host\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
    stream.write_all(head.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

async fn stop_host(state: &Path, mut host: Spawned) {
    let _ = http(state, "POST", "/shutdown").await;
    let deadline = Instant::now() + Duration::from_secs(10);
    while host.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "host exits");
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

/// A shell pane in the page `window`, as the dashboard asks for one.
fn window_query(session: &str, window: &str) -> String {
    format!("session={session}&kind=shell&cols=80&rows=24&width=640&height=408&lang=ko&window={window}")
}

async fn attach_query(state: &Path, query: &str) -> (Viewer<Stream>, serde_json::Value) {
    Viewer::attach_over(connect(state).await, query).await
}

async fn attach(state: &Path) -> (Viewer<Stream>, serde_json::Value) {
    attach_query(state, QUERY).await
}

fn session_files(state: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(state.join("sessions"))
        .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

#[tokio::test]
async fn a_shell_survives_a_dropped_connection_and_a_host_restart() {
    let state = tempfile::tempdir().unwrap();
    let host = start_host(state.path());

    let (mut first, ready) = attach(state.path()).await;
    assert_eq!(ready["mode"], "new");
    let pid = ready["pid"].clone();
    first.send(MARK_INPUT).await;
    first.wait_for("MARK_42").await;
    first.detach().await;

    // Same process; the screen is replayed from the host's copy.
    let (mut second, ready) = attach(state.path()).await;
    assert_eq!(ready["mode"], "reattached");
    assert_eq!(ready["pid"], pid, "the same shell process");
    second.wait_for("MARK_42").await;
    second.detach().await;

    // Host restart: the old process is gone; a new one gets the saved screen.
    stop_host(state.path(), host).await;
    assert!(session_files(state.path()).contains(&"stest1.screen".to_owned()), "screen saved on shutdown");
    let host = start_host(state.path());
    let (mut third, ready) = attach(state.path()).await;
    assert_eq!(ready["mode"], "revived");
    assert_ne!(ready["pid"], pid, "a new process");
    third.wait_for("MARK_42").await;
    third.wait_for("이전 세션 복원").await;

    // Closing the pane forgets the session.
    third.send(r#"{"type":"close"}"#).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), third.ws.next()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(session_files(state.path()), Vec::<String>::new(), "saved files removed");
    stop_host(state.path(), host).await;
}

#[tokio::test]
async fn a_second_window_takes_over_and_the_first_is_told() {
    let state = tempfile::tempdir().unwrap();
    let host = start_host(state.path());
    let (mut first, _) = attach(state.path()).await;
    let (_second, ready) = attach(state.path()).await;
    assert_eq!(ready["mode"], "reattached");
    let code = loop {
        match tokio::time::timeout(Duration::from_secs(5), first.ws.next()).await.unwrap() {
            Some(Ok(Message::Close(Some(frame)))) => break u16::from(frame.code),
            Some(Ok(_)) => continue,
            other => panic!("expected a close frame, got {other:?}"),
        }
    };
    assert_eq!(code, 4001);
    stop_host(state.path(), host).await;
}

#[tokio::test]
async fn one_window_at_a_time_owns_every_session() {
    let state = tempfile::tempdir().unwrap();
    let host = start_host(state.path());
    let (mut app_one, _) = attach_query(state.path(), &window_query("sone1", "wapp")).await;
    let (mut app_two, ready) = attach_query(state.path(), &window_query("stwo1", "wapp")).await;
    let two_pid = ready["pid"].clone();
    // More panes in the same window keep the ones before them.
    app_one.send(MARK_INPUT).await;
    app_one.wait_for("MARK_42").await;

    // Another window opens one of them and takes both.
    let (mut web, ready) = attach_query(state.path(), &window_query("sone1", "wweb")).await;
    assert_eq!(ready["mode"], "reattached");
    assert_eq!(app_one.close_code().await, 4001);
    assert_eq!(app_two.close_code().await, 4001, "a session the new window did not open");

    // Back in the first window: the shells kept running, and the other window is told.
    let (_app, ready) = attach_query(state.path(), &window_query("stwo1", "wapp")).await;
    assert_eq!(ready["mode"], "reattached");
    assert_eq!(ready["pid"], two_pid, "the same shell process");
    assert_eq!(web.close_code().await, 4001, "a session this window did not open");
    stop_host(state.path(), host).await;
}

#[tokio::test]
async fn the_dashboard_only_accepts_its_own_origin() {
    let state = tempfile::tempdir().unwrap();
    // Dropped in reverse order: the dashboard is killed, then its host stopped.
    let _host = StopHostOnDrop(state.path());
    let port = 40000 + (std::process::id() % 20000) as u16;
    let _server = Spawned(Command::new(BIN)
        .args(["serve", "--port", &port.to_string()])
        .env("WESTERM_STATE_DIR", state.path())
        .env("SHELL", "/bin/sh")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap());

    let upgrade = |origin: Option<&str>| {
        let origin = origin.map(|o| format!("Origin: {o}\r\n")).unwrap_or_default();
        format!(
            "GET /ws/session?{QUERY} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n{origin}\r\n"
        )
    };
    let status = |request: String| async move {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.ok()?;
        stream.write_all(request.as_bytes()).await.ok()?;
        let mut buf = [0u8; 64];
        let n = stream.read(&mut buf).await.ok()?;
        String::from_utf8_lossy(&buf[..n]).split_whitespace().nth(1).map(str::to_owned)
    };

    let deadline = Instant::now() + Duration::from_secs(10);
    while status(upgrade(None)).await.is_none() {
        assert!(Instant::now() < deadline, "server starts");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(status(upgrade(None)).await.as_deref(), Some("403"));
    assert_eq!(status(upgrade(Some("http://evil.example"))).await.as_deref(), Some("403"));
    let own = format!("http://127.0.0.1:{port}");
    assert_eq!(status(upgrade(Some(&own))).await.as_deref(), Some("101"));
}

#[tokio::test]
async fn a_second_host_refuses_to_start_while_one_is_running() {
    let state = tempfile::tempdir().unwrap();
    let first = start_host(state.path());
    let mut second = Spawned(Command::new(BIN)
        .arg("host")
        .env("WESTERM_STATE_DIR", state.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = second.0.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            panic!("the second host kept running and took over the endpoint");
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    };
    assert!(!status.success(), "the second host exits with an error");
    // The first host still owns the endpoint and answers.
    assert!(http(state.path(), "GET", "/health").await.contains("\"build\""));
    stop_host(state.path(), first).await;
}

#[tokio::test]
async fn health_reports_the_protocol() {
    let state = tempfile::tempdir().unwrap();
    let host = start_host(state.path());
    let response = http(state.path(), "GET", "/health").await;
    let body = response.split_once("\r\n\r\n").map(|(_, body)| body).unwrap_or_default();
    let health: serde_json::Value = serde_json::from_str(body).expect("health is JSON");
    assert_eq!(health["protocol"], 1);
    assert!(health["build"].as_str().is_some_and(|build| build.starts_with("rs-")));
    stop_host(state.path(), host).await;
}
