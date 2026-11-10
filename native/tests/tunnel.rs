//! The Cloudflare tunnel, end to end, without Cloudflare. This test binary is
//! also the fake cloudflared: run as `<this binary> tunnel …` it serves the
//! metrics routes a real one serves. It has no test harness, so `main` can
//! tell the two uses apart. Runs on macOS, Linux, and Windows.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod common;
use common::{Spawned, StopHostOnDrop, Viewer, BIN};

const FAKE_HOSTNAME: &str = "fake-tunnel.trycloudflare.com";

type Test = (&'static str, fn());

const TESTS: &[Test] = &[
    ("the_fake_serves_the_metrics", the_fake_serves_the_metrics),
    ("the_guard_ends_its_child_when_stdin_closes", the_guard_ends_its_child_when_stdin_closes),
    ("the_guard_passes_the_exit_code", the_guard_passes_the_exit_code),
    ("start_reaches_on_with_a_url_and_a_code", start_reaches_on_with_a_url_and_a_code),
    ("a_second_start_does_nothing", a_second_start_does_nothing),
    ("without_a_cookie_nothing_opens", without_a_cookie_nothing_opens),
    ("the_code_opens_a_shell_and_the_layout", the_code_opens_a_shell_and_the_layout),
    ("wrong_codes_lock_the_login", wrong_codes_lock_the_login),
    ("other_hosts_and_origins_are_refused", other_hosts_and_origins_are_refused),
    ("tunnel_side_denies_local_only_routes", tunnel_side_denies_local_only_routes),
    ("stop_closes_sockets_and_cookies_but_not_shells", stop_closes_sockets_and_cookies_but_not_shells),
    ("an_old_cookie_fails_on_a_new_tunnel", an_old_cookie_fails_on_a_new_tunnel),
    ("cancel_while_starting_leaves_nothing", cancel_while_starting_leaves_nothing),
    ("a_slow_cloudflared_fails_after_the_timeout", a_slow_cloudflared_fails_after_the_timeout),
    ("a_crashed_cloudflared_fails", a_crashed_cloudflared_fails),
    ("not_ready_shows_reconnecting", not_ready_shows_reconnecting),
    ("missing_cloudflared_is_missing", missing_cloudflared_is_missing),
    ("the_control_api_checks_origin_and_host", the_control_api_checks_origin_and_host),
    ("killing_the_dashboard_ends_cloudflared", killing_the_dashboard_ends_cloudflared),
    ("idle_tunnels_stop", idle_tunnels_stop),
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("tunnel") {
        fake_cloudflared(&args[2..]);
        return;
    }
    // Like libtest: plain arguments filter by name; flags are ignored.
    let filters: Vec<&str> = args[1..].iter().map(String::as_str).filter(|arg| !arg.starts_with('-')).collect();
    let chosen: Vec<&Test> = TESTS.iter().filter(|(name, _)| filters.is_empty() || filters.iter().any(|f| name.contains(f))).collect();
    println!("\nrunning {} tests", chosen.len());
    for (name, test) in &chosen {
        print!("test {name} ... ");
        let _ = std::io::stdout().flush();
        test();
        println!("ok");
    }
    println!("\ntest result: ok. {} passed; 0 failed", chosen.len());
}

fn env_ms(name: &str) -> Option<Duration> {
    std::env::var(name).ok()?.parse().ok().map(Duration::from_millis)
}

/// A stand-in for `cloudflared tunnel --metrics <addr> …`. Environment:
/// FAKE_CF_HOSTNAME, FAKE_CF_DELAY_MS (before the metrics server opens, like
/// a slow provisioning), FAKE_CF_EXIT_AFTER_MS (then exit 1, like a crash),
/// FAKE_CF_HEARTBEAT (a file that counts up every 100 ms while it lives),
/// FAKE_CF_NOT_READY (while this file exists, /ready answers 503).
fn fake_cloudflared(args: &[String]) {
    let metrics = args.windows(2).find(|pair| pair[0] == "--metrics").map(|pair| pair[1].clone()).expect("--metrics");
    let hostname = std::env::var("FAKE_CF_HOSTNAME").unwrap_or_else(|_| FAKE_HOSTNAME.to_owned());
    if let Some(path) = std::env::var_os("FAKE_CF_HEARTBEAT") {
        std::thread::spawn(move || {
            for beat in 1u64.. {
                let _ = std::fs::write(&path, beat.to_string());
                std::thread::sleep(Duration::from_millis(100));
            }
        });
    }
    if let Some(after) = env_ms("FAKE_CF_EXIT_AFTER_MS") {
        std::thread::spawn(move || {
            std::thread::sleep(after);
            std::process::exit(1);
        });
    }
    if let Some(delay) = env_ms("FAKE_CF_DELAY_MS") {
        std::thread::sleep(delay);
    }
    eprintln!("Your quick Tunnel has been created! Visit it at (it may take some time to be reachable):");
    eprintln!("https://{hostname}");
    let not_ready = std::env::var_os("FAKE_CF_NOT_READY");
    let listener = TcpListener::bind(&metrics).expect("metrics address");
    for stream in listener.incoming().flatten() {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            continue;
        }
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        // Read the rest of the head.
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                break;
            }
        }
        let (status, body) = match path.as_str() {
            "/quicktunnel" => ("200 OK", format!(r#"{{"hostname":"{hostname}"}}"#)),
            "/ready" if not_ready.as_ref().is_some_and(|file| Path::new(file).exists()) => {
                ("503 Service Unavailable", r#"{"status":503,"readyConnections":0}"#.to_owned())
            }
            "/ready" => ("200 OK", r#"{"status":200,"readyConnections":1}"#.to_owned()),
            _ => ("404 Not Found", String::new()),
        };
        let mut stream = reader.into_inner();
        let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    }
}

/// A free port on 127.0.0.1.
fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port()
}

/// One GET to 127.0.0.1:`port`: (status, body), or None when nothing answers.
fn get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(stream, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").ok()?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).ok()?;
    let status = raw.split_whitespace().nth(1)?.parse().ok()?;
    let body = raw.split_once("\r\n\r\n").map(|(_, body)| body.to_owned()).unwrap_or_default();
    Some((status, body))
}

/// Calls `ready` until it gives a value or `within` passes.
fn wait_for<T>(within: Duration, mut ready: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + within;
    loop {
        if let Some(value) = ready() {
            return Some(value);
        }
        if Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn beats(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// The fake's heartbeat file stopped counting: the fake is gone.
fn heartbeat_stops(path: &Path, within: Duration) -> bool {
    wait_for(within, || {
        let before = beats(path);
        std::thread::sleep(Duration::from_millis(400));
        (beats(path) == before).then_some(())
    })
    .is_some()
}

/// The fake's heartbeat counts up: the fake runs.
fn heartbeat_runs(path: &Path) -> bool {
    wait_for(Duration::from_secs(10), || beats(path).filter(|&n| n >= 2)).is_some()
}

fn fake_args(metrics: u16) -> Vec<String> {
    vec!["tunnel".to_owned(), "--metrics".to_owned(), format!("127.0.0.1:{metrics}")]
}

fn this_binary() -> std::path::PathBuf {
    std::env::current_exe().unwrap()
}

fn the_fake_serves_the_metrics() {
    let dir = tempfile::tempdir().unwrap();
    let not_ready = dir.path().join("not-ready");
    let metrics = free_port();
    let _fake = Spawned(
        Command::new(this_binary())
            .args(fake_args(metrics))
            .env("FAKE_CF_NOT_READY", &not_ready)
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let (status, body) = wait_for(Duration::from_secs(10), || get(metrics, "/quicktunnel")).expect("metrics answer");
    assert_eq!(status, 200);
    assert!(body.contains(FAKE_HOSTNAME), "{body}");
    assert_eq!(get(metrics, "/ready").map(|(status, _)| status), Some(200));
    std::fs::write(&not_ready, b"").unwrap();
    assert_eq!(get(metrics, "/ready").map(|(status, _)| status), Some(503));
}

fn start_guard(heartbeat: &Path, env: &[(&str, &str)]) -> std::process::Child {
    Command::new(BIN)
        .arg("tunnel-guard")
        .arg("--")
        .arg(this_binary())
        .args(fake_args(free_port()))
        .env("FAKE_CF_HEARTBEAT", heartbeat)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn wait_exit(child: &mut std::process::Child, within: Duration) -> Option<std::process::ExitStatus> {
    wait_for(within, || child.try_wait().ok().flatten())
}

fn the_guard_ends_its_child_when_stdin_closes() {
    let dir = tempfile::tempdir().unwrap();
    let heartbeat = dir.path().join("beat");
    let mut guard = start_guard(&heartbeat, &[]);
    assert!(heartbeat_runs(&heartbeat), "the fake runs under the guard");
    // What the OS does when the dashboard ends, however it ends.
    drop(guard.stdin.take());
    let status = wait_exit(&mut guard, Duration::from_secs(3)).expect("the guard ends");
    assert_eq!(status.code(), Some(0));
    assert!(heartbeat_stops(&heartbeat, Duration::from_secs(3)), "the fake ends with it");
}

fn the_guard_passes_the_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let heartbeat = dir.path().join("beat");
    let mut guard = start_guard(&heartbeat, &[("FAKE_CF_EXIT_AFTER_MS", "300")]);
    let status = wait_exit(&mut guard, Duration::from_secs(10)).expect("the guard ends with its child");
    assert_eq!(status.code(), Some(1));
    // The test still holds the guard's stdin: the guard ended because its child did.
    drop(guard.stdin.take());
}

// ---- The dashboard with a tunnel -------------------------------------------

const TUNNEL_ORIGIN: &str = "https://fake-tunnel.trycloudflare.com";
const QUERY: &str = "session=stun1&kind=shell&cols=80&rows=24&width=640&height=408&lang=ko";

/// A dashboard on a free port in its own state folder, with this binary as
/// its cloudflared. Dropped: killed, then its host stopped.
struct Dashboard {
    child: Option<Spawned>,
    port: u16,
    heartbeat: std::path::PathBuf,
    not_ready: std::path::PathBuf,
    state: tempfile::TempDir,
}

impl Drop for Dashboard {
    fn drop(&mut self) {
        drop(self.child.take());
        drop(StopHostOnDrop(self.state.path()));
    }
}

impl Dashboard {
    fn kill(&mut self) {
        drop(self.child.take());
    }
}

fn serve(env: &[(&str, &str)]) -> Dashboard {
    let state = tempfile::tempdir().unwrap();
    let heartbeat = state.path().join("fake-beat");
    let not_ready = state.path().join("fake-not-ready");
    let mut command = Command::new(BIN);
    command
        .args(["serve", "--port", "0"])
        .env("WESTERM_STATE_DIR", state.path())
        .env("SHELL", "/bin/sh")
        .env("WESTERM_CLOUDFLARED", this_binary())
        .env("FAKE_CF_HEARTBEAT", &heartbeat)
        .env("FAKE_CF_NOT_READY", &not_ready)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    // "> pty host … started" may come first.
    let port = loop {
        let mut line = String::new();
        assert_ne!(stdout.read_line(&mut line).unwrap(), 0, "the dashboard ended before its address");
        if let Some(address) = line.trim().strip_prefix("> westerm: http://127.0.0.1:") {
            break address.parse::<u16>().unwrap();
        }
    };
    // Keep reading, so the dashboard never blocks on a full pipe.
    std::thread::spawn(move || std::io::copy(&mut stdout, &mut std::io::sink()));
    Dashboard { child: Some(Spawned(child)), port, heartbeat, not_ready, state }
}

/// One HTTP/1.1 request: (status, head, body), or None when nothing answers.
fn http(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Option<(u16, String, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let mut request = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).ok()?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).ok()?;
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let status = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((status, head.to_owned(), body.to_owned()))
}

impl Dashboard {
    fn local(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// A POST from the dashboard's own page.
    fn control(&self, action: &str) -> u16 {
        let origin = format!("http://{}", self.local());
        let headers = [("Host", self.local()), ("Origin", origin), ("Content-Type", "application/json".to_owned())];
        let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        http(self.port, "POST", &format!("/api/tunnel/{action}"), &headers, "{}").expect("the dashboard answers").0
    }

    /// The first status on the stream that `wanted` accepts, within 10 s.
    fn status_until(&self, wanted: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
        let origin = format!("http://{}", self.local());
        status_stream(self.port, &self.local(), &origin, Duration::from_secs(10), wanted).expect("a matching status")
    }

    fn status_now(&self) -> serde_json::Value {
        self.status_until(|_| true)
    }

    fn start_on(&self) -> serde_json::Value {
        assert_eq!(self.control("start"), 204);
        self.status_until(|s| s["phase"] == "on")
    }
}

/// Reads the status stream until `wanted` accepts a status. Err(status code)
/// when the upgrade is refused.
fn status_stream(
    port: u16,
    host: &str,
    origin: &str,
    within: Duration,
    wanted: impl Fn(&serde_json::Value) -> bool,
) -> Result<serde_json::Value, u16> {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message;
    runtime().block_on(async {
        let mut request = format!("ws://{host}/api/tunnel/status").into_client_request().unwrap();
        request.headers_mut().insert("Origin", origin.parse().unwrap());
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (mut ws, _) = match tokio_tungstenite::client_async(request, stream).await {
            Ok(ok) => ok,
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => return Err(response.status().as_u16()),
            Err(error) => panic!("status stream: {error}"),
        };
        let last = std::cell::RefCell::new(serde_json::Value::Null);
        let read = async {
            while let Some(Ok(message)) = ws.next().await {
                if let Message::Text(text) = message {
                    let status: serde_json::Value = serde_json::from_str(&text).unwrap();
                    if wanted(&status) {
                        return status;
                    }
                    *last.borrow_mut() = status;
                }
            }
            panic!("the status stream closed");
        };
        let found = tokio::time::timeout(within, read).await;
        Ok(found.unwrap_or_else(|_| panic!("no matching status in time; the last: {}", last.borrow())))
    })
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

/// Requests to the tunnel listener, as cloudflared forwards them.
struct Tunnel {
    port: u16,
}

impl Tunnel {
    fn of(status: &serde_json::Value) -> Self {
        Self { port: status["localPort"].as_u64().expect("localPort") as u16 }
    }

    fn request(&self, method: &str, path: &str, cookie: Option<&str>, extra: &[(&str, &str)], body: &str) -> Option<(u16, String, String)> {
        let cookie = cookie.map(|token| format!("westerm_tunnel={token}"));
        let mut headers = vec![("Host", FAKE_HOSTNAME), ("Origin", TUNNEL_ORIGIN)];
        if let Some(cookie) = &cookie {
            headers.push(("Cookie", cookie));
        }
        headers.extend_from_slice(extra);
        http(self.port, method, path, &headers, body)
    }

    fn get(&self, path: &str, cookie: Option<&str>) -> u16 {
        self.request("GET", path, cookie, &[], "").expect("the tunnel answers").0
    }

    fn login(&self, code: &str) -> (u16, Option<String>) {
        let body = format!(r#"{{"code":"{code}"}}"#);
        let (status, head, _) = self.request("POST", "/api/tunnel-auth/login", None, &[("Content-Type", "application/json")], &body).unwrap();
        let token = head
            .lines()
            .find_map(|line| line.to_ascii_lowercase().starts_with("set-cookie:").then(|| line.split_once(": ").unwrap().1.to_owned()))
            .and_then(|cookie| cookie.strip_prefix("westerm_tunnel=").map(|rest| rest.split(';').next().unwrap().to_owned()));
        (status, token)
    }

    fn sign_in(&self, status: &serde_json::Value) -> String {
        let (code, token) = self.login(status["code"].as_str().unwrap());
        assert_eq!(code, 204);
        token.unwrap()
    }

    fn closed(&self) -> bool {
        TcpStream::connect(("127.0.0.1", self.port)).is_err()
    }
}

/// A terminal through the tunnel. Err(status) when the upgrade is refused.
async fn tunnel_pane(port: u16, cookie: Option<&str>, origin: &str) -> Result<Viewer<tokio::net::TcpStream>, u16> {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message;
    let mut request = format!("ws://{FAKE_HOSTNAME}/ws/session?{QUERY}").into_client_request().unwrap();
    request.headers_mut().insert("Origin", origin.parse().unwrap());
    if let Some(token) = cookie {
        request.headers_mut().insert("Cookie", format!("westerm_tunnel={token}").parse().unwrap());
    }
    let stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (mut ws, _) = match tokio_tungstenite::client_async(request, stream).await {
        Ok(ok) => ok,
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => return Err(response.status().as_u16()),
        Err(error) => panic!("tunnel pane: {error}"),
    };
    ws.send(Message::text(r#"{"type":"resize","cols":80,"rows":24,"width":640,"height":408}"#)).await.unwrap();
    Ok(Viewer { ws, output: String::new() })
}

fn start_reaches_on_with_a_url_and_a_code() {
    let dashboard = serve(&[]);
    let status = dashboard.start_on();
    assert_eq!(status["url"], TUNNEL_ORIGIN);
    let code = status["code"].as_str().unwrap();
    assert!(code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()), "{code}");
    assert!(status["localPort"].as_u64().is_some());
}

fn a_second_start_does_nothing() {
    let dashboard = serve(&[]);
    let first = dashboard.start_on();
    assert_eq!(dashboard.control("start"), 204);
    std::thread::sleep(Duration::from_secs(2));
    let now = dashboard.status_now();
    assert_eq!(now["phase"], "on");
    assert_eq!(now["localPort"], first["localPort"]);
    assert_eq!(now["code"], first["code"]);
}

fn without_a_cookie_nothing_opens() {
    let dashboard = serve(&[]);
    let tunnel = Tunnel::of(&dashboard.start_on());
    assert_eq!(tunnel.get("/api/workspace", None), 401);
    let refused = runtime().block_on(tunnel_pane(tunnel.port, None, TUNNEL_ORIGIN)).err();
    assert_eq!(refused, Some(401));
}

fn the_code_opens_a_shell_and_the_layout() {
    let dashboard = serve(&[]);
    let status = dashboard.start_on();
    let tunnel = Tunnel::of(&status);
    let token = tunnel.sign_in(&status);
    let layout = r#"{"version":1,"tabs":[],"sessions":[],"test":"tunnel"}"#;
    let put = tunnel.request("PUT", "/api/workspace", Some(&token), &[("Content-Type", "application/json")], layout).unwrap();
    assert_eq!(put.0, 204, "{put:?}");
    let (code, _, body) = tunnel.request("GET", "/api/workspace", Some(&token), &[], "").unwrap();
    assert_eq!(code, 200);
    let read: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(read, serde_json::from_str::<serde_json::Value>(layout).unwrap());
    // One runtime keeps the pane open while the status is read on another.
    let rt = runtime();
    let mut pane = rt.block_on(tunnel_pane(tunnel.port, Some(&token), TUNNEL_ORIGIN)).expect("the pane opens");
    assert_eq!(rt.block_on(pane.next_json())["type"], "ready");
    let status = dashboard.status_until(|s| s["devices"].as_array().is_some_and(|d| d.len() == 1 && d[0]["connected"] == true));
    assert_eq!(status["phase"], "on");
    rt.block_on(pane.detach());
}

fn wrong_codes_lock_the_login() {
    let dashboard = serve(&[]);
    let status = dashboard.start_on();
    let tunnel = Tunnel::of(&status);
    let wrong = if status["code"] == "000000" { "111111" } else { "000000" };
    for _ in 0..4 {
        assert_eq!(tunnel.login(wrong).0, 401);
    }
    assert_eq!(tunnel.login(wrong).0, 429);
}

fn other_hosts_and_origins_are_refused() {
    let dashboard = serve(&[]);
    let status = dashboard.start_on();
    let tunnel = Tunnel::of(&status);
    let token = tunnel.sign_in(&status);
    let evil = http(tunnel.port, "GET", "/", &[("Host", "evil.example")], "").unwrap().0;
    assert_eq!(evil, 403);
    let refused = runtime().block_on(tunnel_pane(tunnel.port, Some(&token), "https://evil.example")).err();
    assert_eq!(refused, Some(403));
}

fn tunnel_side_denies_local_only_routes() {
    let dashboard = serve(&[]);
    let status = dashboard.start_on();
    let tunnel = Tunnel::of(&status);
    let token = tunnel.sign_in(&status);
    for path in ["/api/update", "/api/remote/link", "/api/tunnel/start", "/api/tunnel/status"] {
        assert_eq!(tunnel.get(path, Some(&token)), 404, "{path}");
    }
}

fn stop_closes_sockets_and_cookies_but_not_shells() {
    let dashboard = serve(&[]);
    let status = dashboard.start_on();
    let tunnel = Tunnel::of(&status);
    let token = tunnel.sign_in(&status);
    runtime().block_on(async {
        let mut pane = tunnel_pane(tunnel.port, Some(&token), TUNNEL_ORIGIN).await.expect("the pane opens");
        assert_eq!(pane.next_json().await["type"], "ready");
        let stop = tokio::task::spawn_blocking({
            let port = dashboard.port;
            move || {
                let local = format!("127.0.0.1:{port}");
                let origin = format!("http://{local}");
                let headers = [("Host", local.as_str()), ("Origin", origin.as_str()), ("Content-Type", "application/json")];
                http(port, "POST", "/api/tunnel/stop", &headers, "{}").unwrap().0
            }
        });
        assert_eq!(pane.close_code().await, 4010);
        assert_eq!(stop.await.unwrap(), 204);
    });
    let status = dashboard.status_until(|s| s["phase"] == "off");
    assert_eq!(status["reason"], "stopped");
    assert!(status["code"].is_null() && status["url"].is_null());
    assert!(tunnel.closed(), "the listener is gone");
    assert!(heartbeat_stops(&dashboard.heartbeat, Duration::from_secs(5)), "cloudflared is gone");
    // The shell still runs: the dashboard reattaches it.
    runtime().block_on(async {
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", dashboard.port)).await.unwrap();
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut request = format!("ws://127.0.0.1:{}/ws/session?{QUERY}", dashboard.port).into_client_request().unwrap();
        request.headers_mut().insert("Origin", format!("http://127.0.0.1:{}", dashboard.port).parse().unwrap());
        let (ws, _) = tokio_tungstenite::client_async(request, stream).await.unwrap();
        let mut pane = Viewer { ws, output: String::new() };
        pane.send(r#"{"type":"resize","cols":80,"rows":24,"width":640,"height":408}"#).await;
        let ready = pane.next_json().await;
        assert_eq!(ready["mode"], "reattached", "{ready}");
    });
}

fn an_old_cookie_fails_on_a_new_tunnel() {
    let dashboard = serve(&[]);
    let status = dashboard.start_on();
    let token = Tunnel::of(&status).sign_in(&status);
    assert_eq!(dashboard.control("stop"), 204);
    let again = dashboard.start_on();
    assert_eq!(Tunnel::of(&again).get("/api/workspace", Some(&token)), 401);
}

fn cancel_while_starting_leaves_nothing() {
    let dashboard = serve(&[("FAKE_CF_DELAY_MS", "5000")]);
    assert_eq!(dashboard.control("start"), 204);
    dashboard.status_until(|s| s["phase"] == "starting");
    assert!(heartbeat_runs(&dashboard.heartbeat));
    assert_eq!(dashboard.control("stop"), 204);
    let status = dashboard.status_until(|s| s["phase"] == "off");
    assert_eq!(status["reason"], "stopped");
    assert!(heartbeat_stops(&dashboard.heartbeat, Duration::from_secs(5)));
}

fn a_slow_cloudflared_fails_after_the_timeout() {
    let dashboard = serve(&[("FAKE_CF_DELAY_MS", "10000"), ("WESTERM_TUNNEL_START_SECS", "2")]);
    assert_eq!(dashboard.control("start"), 204);
    let status = dashboard.status_until(|s| s["phase"] == "failed");
    assert_eq!(status["reason"], "timeout");
    assert!(heartbeat_stops(&dashboard.heartbeat, Duration::from_secs(5)));
}

fn a_crashed_cloudflared_fails() {
    let dashboard = serve(&[("FAKE_CF_EXIT_AFTER_MS", "1500")]);
    let tunnel = Tunnel::of(&dashboard.start_on());
    let status = dashboard.status_until(|s| s["phase"] == "failed");
    assert_eq!(status["reason"], "exited");
    assert!(status["log"].as_array().is_some_and(|log| log.iter().any(|line| line.as_str().unwrap().contains(FAKE_HOSTNAME))), "{status}");
    assert!(tunnel.closed());
}

fn not_ready_shows_reconnecting() {
    let dashboard = serve(&[]);
    let on = dashboard.start_on();
    std::fs::write(&dashboard.not_ready, b"").unwrap();
    let reconnecting = dashboard.status_until(|s| s["phase"] == "reconnecting");
    assert_eq!(reconnecting["code"], on["code"]);
    assert_eq!(reconnecting["url"], on["url"]);
    std::fs::remove_file(&dashboard.not_ready).unwrap();
    dashboard.status_until(|s| s["phase"] == "on");
}

fn missing_cloudflared_is_missing() {
    let nowhere = std::env::temp_dir().join("westerm-no-cloudflared-here");
    let dashboard = serve(&[("WESTERM_CLOUDFLARED", nowhere.to_str().unwrap())]);
    assert_eq!(dashboard.status_now()["phase"], "missing");
    assert_eq!(dashboard.control("start"), 204);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(dashboard.status_now()["phase"], "missing");
}

fn the_control_api_checks_origin_and_host() {
    let dashboard = serve(&[]);
    let local = dashboard.local();
    let evil = [("Host", local.as_str()), ("Origin", "http://evil.example"), ("Content-Type", "application/json")];
    assert_eq!(http(dashboard.port, "POST", "/api/tunnel/start", &evil, "{}").unwrap().0, 400);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(dashboard.status_now()["phase"], "off");
    let origin = format!("http://{local}");
    assert_eq!(status_stream(dashboard.port, &local, "http://evil.example", Duration::from_secs(5), |_| true).err(), Some(403));
    assert_eq!(status_stream(dashboard.port, "evil.example", &origin, Duration::from_secs(5), |_| true).err(), Some(403));
}

fn killing_the_dashboard_ends_cloudflared() {
    let mut dashboard = serve(&[]);
    dashboard.start_on();
    assert!(heartbeat_runs(&dashboard.heartbeat));
    dashboard.kill();
    assert!(heartbeat_stops(&dashboard.heartbeat, Duration::from_secs(5)));
}

fn idle_tunnels_stop() {
    let dashboard = serve(&[("WESTERM_TUNNEL_IDLE_SECS", "2")]);
    dashboard.start_on();
    let status = dashboard.status_until(|s| s["phase"] == "off");
    assert_eq!(status["reason"], "idle");

    let status = dashboard.start_on();
    let tunnel = Tunnel::of(&status);
    let token = tunnel.sign_in(&status);
    let rt = runtime();
    let mut pane = rt.block_on(tunnel_pane(tunnel.port, Some(&token), TUNNEL_ORIGIN)).expect("the pane opens");
    assert_eq!(rt.block_on(pane.next_json())["type"], "ready");
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(dashboard.status_now()["phase"], "on", "a connected device keeps it on");
    rt.block_on(pane.detach());
    let status = dashboard.status_until(|s| s["phase"] == "off");
    assert_eq!(status["reason"], "idle");
}
