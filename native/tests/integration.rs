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
    start_host_with(state, &[])
}

fn start_host_with(state: &Path, env: &[(&str, std::ffi::OsString)]) -> Spawned {
    let child = Command::new(BIN)
        .arg("host")
        .env("WESTERM_STATE_DIR", state)
        .env("SHELL", "/bin/sh")
        .envs(env.iter().map(|(key, value)| (*key, value)))
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

/// A fake Claude Code: it reports its conversation where Claude Code does,
/// prints its arguments, and keeps running in the foreground.
#[cfg(unix)]
const FAKE_CLAUDE: &str = r#"#!/bin/sh
mkdir -p "$CLAUDE_CONFIG_DIR/sessions"
printf '{"pid":%s,"sessionId":"11111111-2222-3333-4444-555555555555","status":"busy"}' "$$" > "$CLAUDE_CONFIG_DIR/sessions/$$.json"
echo "FAKE_CLAUDE $*"
while :; do sleep 1; done
"#;

/// The next "agent" message, skipping output.
#[cfg(unix)]
async fn agent_message(viewer: &mut Viewer<Stream>, wanted: &str) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let message = tokio::time::timeout(left, viewer.next_json()).await.expect("an agent message in time");
        if message["type"] == "agent" && message["agent"]["status"] == wanted {
            return message;
        }
    }
}

/// A host whose pixel app is a fake terminal-code (`tode`) that draws one
/// red 2 x 2 image at row 2, column 3 inside a synchronized update.
#[cfg(unix)]
fn start_pixel_host(state: &Path) -> Spawned {
    use std::os::unix::fs::PermissionsExt;
    let bin = state.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let app = bin.join("tode");
    std::fs::write(
        &app,
        "#!/bin/sh\nsleep 1\nprintf '\\033[?1049h\\033[?2026h\\033[2;3H\\033_Ga=T,f=32,s=2,v=2,i=1,p=1,C=1,q=2;/wAA//8AAP//AAD//wAA/w==\\033\\\\\\033[?2026lDRAWN'\nexec sleep 30\n",
    )
    .unwrap();
    std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).unwrap();
    let home = state.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let child = Command::new(BIN)
        .arg("host")
        .env("WESTERM_STATE_DIR", state)
        .env("SHELL", "/bin/sh")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
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

/// Binary messages until one satisfies `done`, acknowledging each as wterm does.
#[cfg(unix)]
async fn binary_until(viewer: &mut Viewer<Stream>, mut done: impl FnMut(&[u8]) -> bool) -> Vec<Vec<u8>> {
    use futures_util::SinkExt;
    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "the expected message arrives; got {} binary messages", seen.len());
        let Ok(Some(Ok(message))) = tokio::time::timeout(left, viewer.ws.next()).await else { continue };
        let Message::Binary(bytes) = message else { continue };
        let ack = format!(r#"{{"type":"ack","bytes":{}}}"#, bytes.len());
        let _ = viewer.ws.send(Message::text(ack)).await;
        let finished = done(&bytes);
        seen.push(bytes.to_vec());
        if finished {
            return seen;
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_restored_shell_resumes_its_agent() {
    use std::os::unix::fs::PermissionsExt;

    let state = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let claude = bin.path().join("claude");
    std::fs::write(&claude, FAKE_CLAUDE).unwrap();
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    let env = [("CLAUDE_CONFIG_DIR", state.path().join("claude").into_os_string())];
    let host = start_host_with(state.path(), &env);
    let query = QUERY.replace("stest1", "sagent1");

    let (mut pane, ready) = attach_query(state.path(), &query).await;
    assert_eq!(ready["mode"], "new");
    assert_eq!(pane.next_json().await, serde_json::json!({ "type": "agent", "agent": null }), "no agent at first");
    // The path keeps a real `claude` on PATH out of the test.
    let line = format!("{} --model opus fix-it", claude.display());
    pane.send(&serde_json::json!({ "type": "input", "data": format!("{line}\r") }).to_string()).await;
    pane.wait_for("FAKE_CLAUDE --model opus fix-it").await;
    let message = agent_message(&mut pane, "working").await;
    assert_eq!(message["agent"]["kind"], "claude");

    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(state.path().join("sessions/sagent1.json")).unwrap()).unwrap();
    assert_eq!(meta["agent"]["session"], "11111111-2222-3333-4444-555555555555");
    assert_eq!(meta["agent"]["args"], serde_json::json!(["--model", "opus"]), "the prompt is not kept");
    pane.detach().await;

    // As after a reboot: the host saves and stops, and everything in the shell ends.
    stop_host(state.path(), host).await;
    let host = start_host_with(state.path(), &env);
    let (mut pane, ready) = attach_query(state.path(), &query).await;
    assert_eq!(ready["mode"], "revived");
    pane.wait_for("에이전트 이어가기 · Claude Code · 11111111").await;
    pane.wait_for("FAKE_CLAUDE --resume 11111111-2222-3333-4444-555555555555 --model opus").await;
    pane.detach().await;

    // resume=off: the shell comes back without its agent.
    stop_host(state.path(), host).await;
    let host = start_host_with(state.path(), &env);
    let (mut pane, ready) = attach_query(state.path(), &format!("{query}&resume=off")).await;
    assert_eq!(ready["mode"], "revived");
    pane.send(MARK_INPUT).await;
    pane.wait_for("MARK_42").await;
    assert_eq!(pane.output.matches("FAKE_CLAUDE --resume").count(), 1, "only the old screen has it");
    pane.detach().await;
    stop_host(state.path(), host).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_pixel_app_reaches_a_pixel_pane_as_rectangles_not_kitty_images() {
    let state = tempfile::tempdir().unwrap();
    let _stop = StopHostOnDrop(state.path());
    let host = start_pixel_host(state.path());
    let query = "session=spix1&kind=editor&cols=80&rows=24&width=640&height=408&lang=en&pixels=raw";
    let (mut viewer, ready) = attach_query(state.path(), query).await;
    assert_eq!(ready["pixels"], true);
    let messages = binary_until(&mut viewer, |bytes| bytes.first() == Some(&1)).await;
    let update = messages.last().unwrap();
    // Kind, flags, a 640 x 408 frame (8 x 17 cells), one rectangle.
    assert_eq!(&update[..8], &[1, 0, 0x80, 0x02, 0x98, 0x01, 1, 0]);
    // At column 3 and row 2: x 16, y 17, 2 x 2, 16 bytes of red.
    assert_eq!(&update[8..20], &[16, 0, 17, 0, 2, 0, 2, 0, 16, 0, 0, 0]);
    assert_eq!(&update[20..], [255, 0, 0, 255].repeat(4));
    for text in messages.iter().filter(|bytes| bytes.first() == Some(&0)) {
        assert!(!String::from_utf8_lossy(text).contains("\x1b_G"), "no Kitty image reaches the page");
    }
    viewer.detach().await;
    stop_host(state.path(), host).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_page_that_does_not_ask_for_pixels_still_gets_kitty_images() {
    let state = tempfile::tempdir().unwrap();
    let _stop = StopHostOnDrop(state.path());
    let host = start_pixel_host(state.path());
    let query = "session=spix2&kind=editor&cols=80&rows=24&width=640&height=408&lang=en";
    let (mut viewer, ready) = attach_query(state.path(), query).await;
    assert!(ready.get("pixels").is_none());
    viewer.wait_for("DRAWN").await;
    assert!(viewer.output.contains("\x1b_Ga=T,f=32"));
    viewer.detach().await;
    stop_host(state.path(), host).await;
}

/// A fake `claude` that the host's shell finds first. The shell is a login
/// shell, and its profiles reorder PATH (macOS `path_helper`, a user's
/// `~/.profile`), so HOME is an empty folder whose `.profile` puts the fake
/// folder first.
#[cfg(unix)]
fn fake_claude_env(state: &Path, bin: &Path, home: &Path) -> Vec<(&'static str, std::ffi::OsString)> {
    use std::os::unix::fs::PermissionsExt;

    let claude = bin.join("claude");
    std::fs::write(&claude, FAKE_CLAUDE).unwrap();
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(home.join(".profile"), format!("PATH='{}':\"$PATH\"\nexport PATH\n", bin.display())).unwrap();
    vec![("CLAUDE_CONFIG_DIR", state.join("claude").into_os_string()), ("HOME", home.as_os_str().to_owned())]
}

/// Collects output for `time`, or until the host closes the stream.
#[cfg(unix)]
async fn drain_output(pane: &mut Viewer<Stream>, time: Duration) {
    let deadline = Instant::now() + time;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match tokio::time::timeout(left, pane.ws.next()).await {
            Ok(Some(Ok(Message::Binary(bytes)))) => pane.output.push_str(&String::from_utf8_lossy(&bytes)),
            Ok(Some(Ok(_))) => {}
            Ok(None | Some(Err(_))) => break,
            Err(_) => {}
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_launch_starts_the_agent_in_its_folder() {
    let state = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let work_path = work.path().canonicalize().unwrap();
    let env = fake_claude_env(state.path(), bin.path(), home.path());
    let host = start_host_with(state.path(), &env);
    let query = format!("{}&agent=claude&cwd={}", QUERY.replace("stest1", "slaunch1"), work_path.display());

    let (mut pane, ready) = attach_query(state.path(), &query).await;
    assert_eq!(ready["mode"], "new");
    let message = agent_message(&mut pane, "working").await;
    assert_eq!(message["agent"]["kind"], "claude");
    assert_eq!(message["agent"]["cwd"], work_path.to_str().unwrap());
    pane.wait_for("FAKE_CLAUDE").await;
    assert_eq!(pane.output.matches("FAKE_CLAUDE").count(), 1, "typed once, not twice");
    pane.detach().await;

    // The browser sends the same query on every reconnect: the agent is not typed again.
    let (mut pane, ready) = attach_query(state.path(), &query).await;
    assert_eq!(ready["mode"], "reattached");
    drain_output(&mut pane, Duration::from_secs(2)).await;
    assert_eq!(pane.output.matches("FAKE_CLAUDE").count(), 1, "only the replayed screen has it");
    pane.detach().await;
    stop_host(state.path(), host).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_launch_is_typed_once() {
    let state = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let env = fake_claude_env(state.path(), bin.path(), home.path());
    let host = start_host_with(state.path(), &env);
    let query = format!("{}&agent=claude&cwd={}", QUERY.replace("stest1", "slaunch2"), work.path().display());

    let (mut pane, _) = attach_query(state.path(), &query).await;
    pane.wait_for("FAKE_CLAUDE").await;
    pane.detach().await;
    // A revived shell (the host restarted) ignores the launch; only its saved agent resumes.
    stop_host(state.path(), host).await;
    let host = start_host_with(state.path(), &env);
    let (mut pane, ready) = attach_query(state.path(), &format!("{query}&resume=off")).await;
    assert_eq!(ready["mode"], "revived");
    pane.send(MARK_INPUT).await;
    pane.wait_for("MARK_42").await;
    // A typed launch would come after 300 ms of quiet, later than MARK_42.
    drain_output(&mut pane, Duration::from_secs(2)).await;
    assert_eq!(pane.output.matches("FAKE_CLAUDE").count(), 1, "only the old screen has it");
    pane.detach().await;
    stop_host(state.path(), host).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_launch_into_a_missing_folder_opens_a_plain_shell() {
    let state = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let gone = tempfile::tempdir().unwrap();
    let gone_path = gone.path().join("removed");
    let env = fake_claude_env(state.path(), bin.path(), home.path());
    let host = start_host_with(state.path(), &env);
    let query = format!("{}&agent=claude&cwd={}", QUERY.replace("stest1", "slaunch3"), gone_path.display());

    let (mut pane, ready) = attach_query(state.path(), &query).await;
    assert_eq!(ready["mode"], "new");
    pane.send(MARK_INPUT).await;
    pane.wait_for("MARK_42").await;
    // A typed launch would come after 300 ms of quiet, later than MARK_42.
    drain_output(&mut pane, Duration::from_secs(2)).await;
    assert_eq!(pane.output.matches("FAKE_CLAUDE").count(), 0, "the shell opens in the home folder without the agent");
    pane.detach().await;
    stop_host(state.path(), host).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_shell_reports_its_place() {
    let state = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let work_path = work.path().canonicalize().unwrap();
    std::fs::create_dir(work_path.join(".git")).unwrap();
    std::fs::write(work_path.join(".git/HEAD"), "ref: refs/heads/topic\n").unwrap();
    let host = start_host(state.path());
    let query = format!("{}&cwd={}", QUERY.replace("stest1", "splace1"), work_path.display());

    let (mut pane, _) = attach_query(state.path(), &query).await;
    let deadline = Instant::now() + Duration::from_secs(15);
    let place = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let message = tokio::time::timeout(left, pane.next_json()).await.expect("a place message in time");
        if message["type"] == "place" && !message["place"].is_null() {
            break message;
        }
    };
    assert_eq!(place, serde_json::json!({ "type": "place", "place": { "cwd": work_path.to_str().unwrap(), "branch": "topic" } }));
    pane.detach().await;
    stop_host(state.path(), host).await;
}
