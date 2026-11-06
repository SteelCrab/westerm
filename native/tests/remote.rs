//! Remote westerm with the real binary.
//! - The server side: `remote-setup` starts a host, a dashboard and a gate, and a QUIC
//!   client reaches a shell through it. That client is written from the
//!   outside (accept any certificate, then compare its fingerprint with the
//!   report).
//! - The whole path through a dashboard, with a fake `ssh` that runs the
//!   remote commands on this machine as a "server" in a temporary folder.
#![cfg(unix)]

use std::io::Write;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod common;
use common::{StopHostOnDrop, Viewer, BIN};

const QUERY: &str = "session=sr1&kind=shell&cols=80&rows=24&width=640&height=408&lang=en";
/// Prints MARK_42 only when the shell runs it: the typed line itself does not contain that text.
const MARK_INPUT: &str = r#"{"type":"input","data":"echo MARK_$((40+2))\r"}"#;

/// A short state folder: the host socket path must fit in 104 bytes.
fn state() -> tempfile::TempDir {
    tempfile::Builder::new().prefix("wr").tempdir_in("/tmp").unwrap()
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

struct ClientId {
    cert: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
    fingerprint: String,
}

impl ClientId {
    fn new() -> Self {
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::new(vec!["client".to_owned()]).unwrap().self_signed(&key).unwrap();
        let fingerprint = hex_sha256(cert.der());
        Self { cert: cert.der().clone(), key: PrivatePkcs8KeyDer::from(key.serialize_der()), fingerprint }
    }
}

/// Runs `westerm remote-setup` as SSH would, and reads its report.
fn setup(state: &Path, id: &ClientId, extra: &[&str]) -> serde_json::Value {
    let mut child = Command::new(BIN)
        .arg("remote-setup")
        .args(extra)
        .env("WESTERM_STATE_DIR", state)
        .env("WESTERM_GATE_PORT", "0")
        .env("SHELL", "/bin/sh")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(format!("{}\n", id.fingerprint).as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "remote-setup failed: {}{stdout}", String::from_utf8_lossy(&output.stderr));
    let line = stdout.lines().rev().find(|line| !line.trim().is_empty()).expect("a report line");
    serde_json::from_str(line).unwrap()
}

/// Accepts any server certificate; the test compares fingerprints itself.
#[derive(Debug)]
struct AnyServer(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for AnyServer {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

/// A QUIC connection to the gate on `port`, whose certificate must have
/// `expected` as its fingerprint.
async fn quic(port: u16, id: &ClientId, expected: &str) -> Result<quinn::Connection, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let algorithms = provider.signature_verification_algorithms;
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyServer(algorithms)))
        .with_client_auth_cert(vec![id.cert.clone()], PrivateKeyDer::Pkcs8(id.key.clone_key()))
        .unwrap();
    tls.alpn_protocols = vec![b"westerm/1".to_vec()];
    let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls).unwrap();
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(crypto)));
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let connecting = endpoint.connect(addr, "westerm").map_err(|e| e.to_string())?;
    let connection = tokio::time::timeout(Duration::from_secs(5), connecting)
        .await
        .map_err(|_| "handshake timed out".to_owned())?
        .map_err(|e| e.to_string())?;
    let certs = connection
        .peer_identity()
        .and_then(|identity| identity.downcast::<Vec<CertificateDer<'static>>>().ok())
        .ok_or("no peer certificate")?;
    let presented = hex_sha256(&certs[0]);
    if presented != expected {
        return Err(format!("the gate presented {presented}, not {expected}"));
    }
    Ok(connection)
}

type QuicStream = tokio::io::Join<quinn::RecvStream, quinn::SendStream>;

async fn stream(connection: &quinn::Connection) -> QuicStream {
    let (send, recv) = connection.open_bi().await.unwrap();
    tokio::io::join(recv, send)
}

async fn attach(connection: &quinn::Connection) -> (Viewer<QuicStream>, serde_json::Value) {
    let stream = stream(connection).await;
    tokio::time::timeout(Duration::from_secs(10), Viewer::attach_path(stream, &format!("/ws/session?{QUERY}")))
        .await
        .expect("attach in time")
}

/// Sends `request` raw on a new stream and returns everything that comes back.
async fn raw(connection: &quinn::Connection, request: &[u8]) -> Vec<u8> {
    let mut stream = stream(connection).await;
    stream.write_all(request).await.unwrap();
    let mut answer = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut answer)).await;
    answer
}

/// GET /health from the host over its Unix socket, as `westerm stop` does.
async fn host_health(state: &Path) -> String {
    let mut socket = tokio::net::UnixStream::connect(state.join("host.sock")).await.unwrap();
    socket.write_all(b"GET /health HTTP/1.1\r\nHost: host\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").await.unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).await.unwrap();
    response
}

fn host_build() -> String {
    let output = Command::new(BIN).arg("--version").output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.split("(host ").nth(1).and_then(|rest| rest.split(')').next()).unwrap().to_owned()
}

fn gate_pid(state: &Path) -> u64 {
    let record: serde_json::Value = serde_json::from_slice(&std::fs::read(state.join("gate.json")).unwrap()).unwrap();
    record["pid"].as_u64().unwrap()
}

fn dashboard_pid(state: &Path) -> u64 {
    let record: serde_json::Value = serde_json::from_slice(&std::fs::read(state.join("dashboard.json")).unwrap()).unwrap();
    record["pid"].as_u64().unwrap()
}

fn port_of(report: &serde_json::Value) -> u16 {
    report["port"].as_u64().unwrap() as u16
}

#[tokio::test]
async fn setup_starts_a_dashboard_behind_the_gate() {
    let state = state();
    let _stop = StopHostOnDrop(state.path());
    let id = ClientId::new();
    let report = setup(state.path(), &id, &[]);
    assert_eq!(report["v"], 2);
    assert_eq!(report["host"]["protocol"], 1);
    assert_eq!(report["host"]["build"], host_build());
    let fingerprint = report["fingerprint"].as_str().unwrap().to_owned();
    assert!(fingerprint.len() == 64 && fingerprint.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(report["dashboard"], state.path().join("dashboard.sock").to_str().unwrap());
    assert!(report.get("socket").is_none(), "the host socket is not reported any more");

    let connection = quic(port_of(&report), &id, &fingerprint).await.unwrap();
    let answer = raw(&connection, b"GET /api/workspace HTTP/1.1\r\nHost: remote\r\nConnection: close\r\n\r\n").await;
    let answer = String::from_utf8_lossy(&answer);
    assert!(answer.starts_with("HTTP/1.1 200") && answer.ends_with("null"), "got {answer:?}");
    let (mut first, ready) = attach(&connection).await;
    assert_eq!(ready["mode"], "new");
    first.send(MARK_INPUT).await;
    first.wait_for("MARK_42").await;
    first.detach().await;
    connection.close(0u32.into(), b"bye");

    let again = quic(port_of(&report), &id, &fingerprint).await.unwrap();
    let (mut second, ready) = attach(&again).await;
    assert_eq!(ready["mode"], "reattached");
    second.wait_for("MARK_42").await;
}

#[tokio::test]
async fn setup_twice_keeps_one_gate_and_one_authorized_line() {
    let state = state();
    let _stop = StopHostOnDrop(state.path());
    let id = ClientId::new();
    let first = setup(state.path(), &id, &[]);
    let pid = gate_pid(state.path());
    let dashboard = dashboard_pid(state.path());
    let second = setup(state.path(), &id, &[]);
    assert_eq!(first["port"], second["port"]);
    assert_eq!(first["fingerprint"], second["fingerprint"]);
    assert_eq!(gate_pid(state.path()), pid, "the running gate is kept");
    assert_eq!(dashboard_pid(state.path()), dashboard, "the running dashboard is kept");
    let authorized = std::fs::read_to_string(state.path().join("remote/authorized_clients")).unwrap();
    assert_eq!(authorized.lines().count(), 1);
}

#[tokio::test]
async fn the_gate_refuses_unknown_clients() {
    let state = state();
    let _stop = StopHostOnDrop(state.path());
    let id = ClientId::new();
    let report = setup(state.path(), &id, &[]);
    let fingerprint = report["fingerprint"].as_str().unwrap().to_owned();

    let stranger = ClientId::new();
    // TLS 1.3 ends the client's handshake before the gate checks its
    // certificate, so the refusal can also close the connection under a stream.
    if let Ok(connection) = quic(port_of(&report), &stranger, &fingerprint).await
        && let Ok((send, recv)) = connection.open_bi().await
    {
        let attached = tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::client_async(format!("ws://remote/ws/session?{QUERY}"), tokio::io::join(recv, send)),
        )
        .await;
        assert!(!matches!(attached, Ok(Ok(_))), "an unknown client attached");
    }

    // A known client reaches the server dashboard, which refuses what a remote window never asks.
    let connection = quic(port_of(&report), &id, &fingerprint).await.unwrap();
    let answer = raw(&connection, b"GET /api/update HTTP/1.1\r\nHost: remote\r\nConnection: close\r\n\r\n").await;
    assert!(answer.starts_with(b"HTTP/1.1 404"), "got {:?}", String::from_utf8_lossy(&answer));
    assert!(host_health(state.path()).await.contains("\"build\""), "the host still runs");
}

#[tokio::test]
async fn stop_ends_the_host_the_gate_and_the_dashboard() {
    let state = state();
    let _stop = StopHostOnDrop(state.path());
    let id = ClientId::new();
    setup(state.path(), &id, &[]);
    assert!(state.path().join("gate.json").exists());
    assert!(state.path().join("dashboard.sock").exists());
    let output = Command::new(BIN).arg("stop").env("WESTERM_STATE_DIR", state.path()).stderr(Stdio::null()).output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("the remote dashboard stopped"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while ["gate.json", "host.sock", "dashboard.json", "dashboard.sock"].iter().any(|name| state.path().join(name).exists()) {
        assert!(Instant::now() < deadline, "the gate, the dashboard and the host stop");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Sends `signal` (such as "-TERM") to `pid`.
fn signal(pid: u64, signal: &str) {
    assert!(Command::new("kill").args([signal, &pid.to_string()]).status().unwrap().success());
}

#[tokio::test]
async fn a_stopping_gate_tells_its_clients() {
    let state = state();
    let _stop = StopHostOnDrop(state.path());
    let id = ClientId::new();
    let report = setup(state.path(), &id, &[]);
    let fingerprint = report["fingerprint"].as_str().unwrap().to_owned();
    let connection = quic(port_of(&report), &id, &fingerprint).await.unwrap();
    signal(gate_pid(state.path()), "-TERM");
    // Without the close, the client would notice only at its idle timeout.
    let closed = tokio::time::timeout(Duration::from_secs(2), connection.closed()).await;
    assert!(
        matches!(closed, Ok(quinn::ConnectionError::ApplicationClosed(_))),
        "the client learns at once that the gate closed: {closed:?}"
    );
}

#[tokio::test]
async fn the_gate_logs_a_stream_the_dashboard_refuses() {
    let state = state();
    let _stop = StopHostOnDrop(state.path());
    let id = ClientId::new();
    let report = setup(state.path(), &id, &[]);
    let fingerprint = report["fingerprint"].as_str().unwrap().to_owned();
    signal(dashboard_pid(state.path()), "-TERM");
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.path().join("dashboard.json").exists() {
        assert!(Instant::now() < deadline, "the dashboard stops");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let connection = quic(port_of(&report), &id, &fingerprint).await.unwrap();
    let answer = raw(&connection, b"GET /api/workspace HTTP/1.1\r\nHost: remote\r\nConnection: close\r\n\r\n").await;
    assert!(answer.is_empty(), "no dashboard answers");
    let log = std::fs::read_to_string(state.path().join("gate.log")).unwrap();
    assert!(log.lines().any(|line| line.contains("dashboard.sock")), "gate.log names the refusal: {log}");
}

// ---------------------------------------------------------------------------
// Through the dashboard, with a fake ssh.

use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use common::Spawned;

const PROBE: &str = "uname -sm; ~/.local/bin/westerm --version 2>/dev/null || true";
const UPLOAD: &str = "mkdir -p ~/.local/bin && cat > ~/.local/bin/.westerm.upload && chmod 755 ~/.local/bin/.westerm.upload && ~/.local/bin/.westerm.upload --version >/dev/null && mv -f ~/.local/bin/.westerm.upload ~/.local/bin/westerm";
const SETUP: &str = "~/.local/bin/westerm remote-setup";
const RESIZE: &str = r#"{"type":"resize","cols":80,"rows":24,"width":640,"height":408}"#;

/// `-G` prints a config pointing at this machine; any other call runs its
/// last argument (the remote command) here, as the "server" user. The
/// server's sh reads `home/.shrc` (`$ENV`) only when interactive, as zsh
/// reads `~/.zshrc`.
const FAKE_SSH: &str = r#"#!/bin/sh
for last; do :; done
case " $* " in *" -G "*) printf 'hostname 127.0.0.1\nport 22\nproxyjump none\n'; exit 0 ;; esac
printf '%s\n' "$last" >> "$FAKE_ROOT/ssh.log"
[ -n "$FAKE_SSH_DELAY" ] && sleep "$FAKE_SSH_DELAY"
HOME="$FAKE_ROOT/home" PATH="$FAKE_ROOT/rbin:${FAKE_SERVER_PATH:-$PATH}" WESTERM_STATE_DIR="$FAKE_ROOT/rstate" \
  WESTERM_GATE_PORT=0 SHELL=/bin/sh ENV="$FAKE_ROOT/home/.shrc" exec sh -c "$last"
"#;

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// A dashboard whose ssh reaches a "server" in a temporary folder:
/// `home/` (where westerm gets installed), `rstate/` (its host and gate),
/// and `lstate/` (the dashboard's own state).
struct FakeRemote {
    root: tempfile::TempDir,
    port: u16,
    server: Option<Spawned>,
}

fn write_script(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

impl FakeRemote {
    fn start(ssh_delay: Option<&str>) -> Self {
        Self::start_with(ssh_delay, &[])
    }

    /// Like `start`, with extra environment for the dashboard.
    fn start_with(ssh_delay: Option<&str>, env: &[(&str, &str)]) -> Self {
        let root = state();
        let r = root.path();
        for dir in ["bin", "rbin", "bins", "home", "lhome", "rstate", "lstate"] {
            std::fs::create_dir_all(r.join(dir)).unwrap();
        }
        write_script(&r.join("bin/ssh"), FAKE_SSH);
        write_script(&r.join("rbin/uname"), "#!/bin/sh\necho 'Linux x86_64'\n");
        std::os::unix::fs::symlink(BIN, r.join("bins/westerm-x86_64-unknown-linux-musl")).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let path = format!("{}:{}", r.join("bin").display(), std::env::var("PATH").unwrap_or_default());
        let log = std::fs::File::create(r.join("dashboard.log")).unwrap();
        let mut command = Command::new(BIN);
        command
            .args(["serve", "--port", &port.to_string()])
            .env("PATH", path)
            .env("HOME", r.join("lhome"))
            .env("WESTERM_STATE_DIR", r.join("lstate"))
            .env("WESTERM_REMOTE_BINARY_DIR", r.join("bins"))
            .env("FAKE_ROOT", r)
            .env("SHELL", "/bin/sh")
            .stdout(log.try_clone().unwrap())
            .stderr(log);
        if let Some(delay) = ssh_delay {
            command.env("FAKE_SSH_DELAY", delay);
        }
        command.envs(env.iter().copied());
        let server = Spawned(command.spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "the dashboard starts");
            std::thread::sleep(Duration::from_millis(50));
        }
        Self { root, port, server: Some(server) }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    /// How many times ssh ran `command` on the "server".
    fn count(&self, command: &str) -> usize {
        std::fs::read_to_string(self.path().join("ssh.log")).unwrap_or_default().lines().filter(|line| *line == command).count()
    }
}

impl Drop for FakeRemote {
    fn drop(&mut self) {
        drop(self.server.take());
        for state in ["rstate", "lstate"] {
            drop(StopHostOnDrop(&self.path().join(state)));
        }
    }
}

type Pane = Viewer<MaybeTlsStream<tokio::net::TcpStream>>;

async fn http(port: u16, method: &str, path: &str, body: Option<&str>, with_origin: bool) -> (u16, String) {
    let origin = if with_origin { format!("Origin: http://127.0.0.1:{port}\r\n") } else { String::new() };
    http_raw(port, method, path, &format!("127.0.0.1:{port}"), &origin, body).await
}

/// A JSON POST to the dashboard; the response status.
async fn post(port: u16, path: &str, with_origin: bool, body: &str) -> u16 {
    http(port, "POST", path, Some(body), with_origin).await.0
}

/// A GET-like request with another Host header (a DNS rebinding attempt), and the dashboard's own Origin.
async fn http_with_host(port: u16, method: &str, path: &str, host: &str) -> (u16, String) {
    http_raw(port, method, path, host, &format!("Origin: http://127.0.0.1:{port}\r\n"), None).await
}

/// A request with one extra header line, e.g. `Origin: http://evil.example`, and no other Origin.
async fn http_with_header(port: u16, method: &str, path: &str, header: &str) -> (u16, String) {
    http_raw(port, method, path, &format!("127.0.0.1:{port}"), &format!("{header}\r\n"), None).await
}

async fn http_raw(port: u16, method: &str, path: &str, host: &str, origin: &str, body: Option<&str>) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\n{origin}Connection: close\r\n");
    match body {
        Some(body) => request.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len())),
        None => request.push_str("Content-Length: 0\r\n\r\n"),
    }
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(30), stream.read_to_end(&mut response)).await.expect("an answer in time").unwrap();
    let text = String::from_utf8_lossy(&response).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").expect("a response head");
    (head.split_whitespace().nth(1).unwrap().parse().unwrap(), body.to_owned())
}

/// The status of a WebSocket upgrade to the dashboard: 101 when it opens.
async fn ws_status(port: u16, path_and_query: &str, origin: Option<&str>) -> u16 {
    let mut request = format!("ws://127.0.0.1:{port}{path_and_query}").into_client_request().unwrap();
    if let Some(origin) = origin {
        request.headers_mut().insert("Origin", origin.parse().unwrap());
    }
    match tokio_tungstenite::connect_async(request).await {
        Ok(_) => 101,
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => response.status().as_u16(),
        Err(error) => panic!("the upgrade failed without a status: {error}"),
    }
}

/// Opens a shell in a remote window as the browser does: the fitted size first, then until "ready".
async fn window_pane(port: u16, target: &str, session: &str) -> (Pane, serde_json::Value) {
    window_pane_of(port, target, session, "shell").await
}

/// Like `window_pane`, for a pane of `kind`.
async fn window_pane_of(port: u16, target: &str, session: &str, kind: &str) -> (Pane, serde_json::Value) {
    let url = format!(
        "ws://127.0.0.1:{port}/remote/{target}/ws/session?session={session}&kind={kind}&cols=80&rows=24&width=640&height=408&lang=en&window=wa"
    );
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert("Origin", format!("http://127.0.0.1:{port}").parse().unwrap());
    let (ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let mut pane = Viewer { ws, output: String::new() };
    pane.send(RESIZE).await;
    let ready = tokio::time::timeout(Duration::from_secs(30), pane.next_json()).await.expect("the pane answers in time");
    assert_eq!(ready["type"], "ready", "{ready}");
    (pane, ready)
}

/// Opens a window's status stream for `target`: the socket and its first message.
async fn open_link(port: u16, target: &str) -> (Socket, serde_json::Value) {
    let mut request = format!("ws://127.0.0.1:{port}/api/remote/link?target={target}").into_client_request().unwrap();
    request.headers_mut().insert("Origin", format!("http://127.0.0.1:{port}").parse().unwrap());
    let (mut link, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let hello = next_message(&mut link).await;
    (link, hello)
}

/// The next JSON message on a status stream, within 30 s.
async fn next_message(link: &mut Socket) -> serde_json::Value {
    loop {
        match tokio::time::timeout(Duration::from_secs(30), link.next()).await.expect("a message in time") {
            Some(Ok(Message::Text(text))) => return serde_json::from_str(&text).unwrap(),
            Some(Ok(_)) => {}
            other => panic!("the status stream ended: {other:?}"),
        }
    }
}

/// The stages a status stream shows, up to and including "connected" (30 s at most).
async fn stages_until_connected(link: &mut Socket) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut stages: Vec<String> = Vec::new();
    while stages.last().is_none_or(|stage| stage != "connected") {
        let left = deadline.saturating_duration_since(Instant::now());
        let message = tokio::time::timeout(left, next_message(link)).await.unwrap_or_else(|_| panic!("connected within 30 s; saw {stages:?}"));
        assert_eq!(message["type"], "remote", "{message}");
        if message["stage"] == "failed" {
            // Shown only when a test fails: why the try failed.
            eprintln!("a try failed: {message}");
        }
        stages.push(message["stage"].as_str().unwrap().to_owned());
    }
    stages
}

/// The first stage message named `name` (30 s at most).
async fn stage_named(link: &mut Socket, name: &str) -> serde_json::Value {
    let wait = async {
        loop {
            let message = next_message(link).await;
            if message["stage"] == name {
                return message;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(30), wait).await.unwrap_or_else(|_| panic!("a {name} stage within 30 s"))
}

#[tokio::test]
async fn remote_apis_check_the_origin() {
    let remote = FakeRemote::start(None);
    let close = r#"{"target":"fake","session":"sx1"}"#;
    assert_eq!(post(remote.port, "/api/remote/close", false, close).await, 400);
    assert_eq!(post(remote.port, "/api/remote/close", true, close).await, 204);
    assert_eq!(post(remote.port, "/api/remote/close", true, r#"{"target":"-oProxyCommand=x","session":"sx1"}"#).await, 400);
    assert_eq!(post(remote.port, "/api/remote/decision", true, r#"{"target":"fake","replace":true}"#).await, 409);
    assert_eq!(post(remote.port, "/api/remote/retry", false, r#"{"target":"fake"}"#).await, 400);
    assert_eq!(ws_status(remote.port, "/api/remote/link?target=fake", None).await, 403);
}

#[tokio::test]
async fn a_watched_link_shows_its_stages_until_connected() {
    let fake = FakeRemote::start(None);
    let (mut link, hello) = open_link(fake.port, "fake").await;
    assert_eq!(hello["type"], "hello");
    assert!(hello["version"].as_str().unwrap().starts_with("westerm "));
    // The stream shows the latest stage, so a quick one (such as "connecting")
    // can be skipped; the ones shown keep this order and end at "connected".
    let stages = stages_until_connected(&mut link).await;
    assert!(keeps_order_per_try(&stages, &STAGE_ORDER), "{stages:?} is not in the order {STAGE_ORDER:?}");
    assert_eq!(stages.last().map(String::as_str), Some("connected"));
}

const STAGE_ORDER: [&str; 5] = ["checking", "installing", "starting", "connecting", "connected"];

/// Each try keeps `order`. A try can fail on a slow machine (the QUIC
/// handshake has 3 s); the link then tries again, from any stage.
fn keeps_order_per_try(stages: &[String], order: &[&str]) -> bool {
    stages.split(|stage| stage == "failed").all(|attempt| {
        let mut rest = order.iter();
        attempt.iter().all(|stage| rest.any(|known| known == stage))
    })
}

#[test]
fn a_failed_try_starts_the_order_again() {
    let stages = |list: &[&str]| list.iter().map(|stage| stage.to_string()).collect::<Vec<_>>();
    assert!(keeps_order_per_try(&stages(&["checking", "connecting", "connected"]), &STAGE_ORDER));
    // Seen on a slow CI machine: the first try failed, and the saved gate answered the next one.
    let retried = ["checking", "installing", "starting", "connecting", "failed", "connecting", "connected"];
    assert!(keeps_order_per_try(&stages(&retried), &STAGE_ORDER));
    assert!(!keeps_order_per_try(&stages(&["connecting", "checking", "connected"]), &STAGE_ORDER));
    assert!(!keeps_order_per_try(&stages(&["checking", "failed", "connected", "connecting"]), &STAGE_ORDER));
}

#[tokio::test]
async fn two_windows_opening_at_once_set_up_once() {
    let fake = FakeRemote::start(Some("1"));
    let (mut a, _) = open_link(fake.port, "fake").await;
    let (mut b, _) = open_link(fake.port, "fake").await;
    stages_until_connected(&mut a).await;
    stages_until_connected(&mut b).await;
    assert_eq!(fake.count(UPLOAD), 1);
    assert_eq!(fake.count(SETUP), 1);
}

#[tokio::test]
async fn remote_window_requests_wait_for_a_watched_link() {
    let fake = FakeRemote::start(None);
    assert_eq!(http(fake.port, "GET", "/remote/fake/api/workspace", None, true).await.0, 503, "no window watches the link yet");
    assert_eq!(fake.count(PROBE), 0, "requests alone never start ssh");
    let (mut link, _) = open_link(fake.port, "fake").await;
    stages_until_connected(&mut link).await;
    assert_eq!(http(fake.port, "GET", "/remote/fake/api/workspace", None, true).await.0, 200);
}

#[tokio::test]
async fn a_permanent_failure_waits_for_retry() {
    let fake = FakeRemote::start(None);
    let newer = fake.path().join("home/.local/bin/westerm");
    std::fs::create_dir_all(newer.parent().unwrap()).unwrap();
    write_script(&newer, "#!/bin/sh\necho 'westerm 99.0.0 (host rs-0)'\n");
    let (mut link, _) = open_link(fake.port, "fake").await;
    let failed = stage_named(&mut link, "failed").await;
    assert_eq!(failed["reason"], "newerRemote");
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(fake.count(PROBE), 1, "no retry on its own");
    std::fs::remove_file(&newer).unwrap();
    // Within 30 s of the failed SSH check: "Try again" still runs SSH at once.
    assert_eq!(post(fake.port, "/api/remote/retry", true, r#"{"target":"fake"}"#).await, 204);
    stages_until_connected(&mut link).await;
    assert!(fake.count(PROBE) > 1, "the retry ran SSH again");
}

#[tokio::test]
async fn the_link_closes_after_the_last_window_leaves() {
    let fake = FakeRemote::start_with(None, &[("WESTERM_REMOTE_LINGER_MS", "200")]);
    let (mut link, _) = open_link(fake.port, "fake").await;
    stages_until_connected(&mut link).await;
    drop(link);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(http(fake.port, "GET", "/remote/fake/api/workspace", None, true).await.0, 503);
}

/// The number of sessions in the host of `state`.
async fn host_sessions(state: &Path) -> serde_json::Value {
    let response = host_health(state).await;
    let body = response.split_once("\r\n\r\n").map(|(_, body)| body).unwrap_or_default();
    let health: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    health["sessions"].clone()
}

/// Waits up to 30 s for a pane's socket to end.
async fn wait_until_closed(pane: &mut Pane) {
    let ended = tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(Ok(message)) = pane.ws.next().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "the pane's socket ends");
}

#[tokio::test]
async fn a_lost_gate_comes_back_and_the_shell_is_reattached() {
    let fake = FakeRemote::start(None);
    let (mut link, _) = open_link(fake.port, "fake").await;
    stages_until_connected(&mut link).await;
    let (mut pane, _) = window_pane(fake.port, "fake", "sw1").await;
    pane.send(MARK_INPUT).await;
    pane.wait_for("MARK_42").await;
    let pid = gate_pid(&fake.path().join("rstate"));
    assert!(Command::new("kill").args(["-9", &pid.to_string()]).status().unwrap().success());
    wait_until_closed(&mut pane).await;
    stages_until_connected(&mut link).await;
    let (mut again, ready) = window_pane(fake.port, "fake", "sw1").await;
    assert_eq!(ready["mode"], "reattached");
    again.wait_for("MARK_42").await; // the screen comes back after "ready"
}

/// The window's socket to the dashboard is fine, but the link to the server
/// has gone silent (a lost network): a close sent then cannot arrive, and
/// the shell must still end once the server is reachable again.
#[tokio::test]
async fn a_session_closed_while_disconnected_ends_after_reconnect() {
    let fake = FakeRemote::start(None);
    let rstate = fake.path().join("rstate");
    let (mut link, _) = open_link(fake.port, "fake").await;
    stages_until_connected(&mut link).await;
    let (pane, ready) = window_pane(fake.port, "fake", "sw2").await;
    assert_eq!(ready["mode"], "new");
    drop(pane);
    let gate = gate_pid(&rstate).to_string();
    assert!(Command::new("kill").args(["-STOP", &gate]).status().unwrap().success());
    assert_eq!(post(fake.port, "/api/remote/close", true, r#"{"target":"fake","session":"sw2"}"#).await, 204);
    // The close goes over the silent link and gets no answer within 5 s;
    // then the silent gate is gone for good, with whatever it never read.
    tokio::time::sleep(Duration::from_secs(6)).await;
    let pending = std::fs::read_to_string(fake.path().join("lstate/remote/pending-close.json")).unwrap_or_default();
    assert!(pending.contains("sw2"), "the close waits for the next connection: {pending}");
    assert!(Command::new("kill").args(["-KILL", &gate]).status().unwrap().success());

    stages_until_connected(&mut link).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    while host_sessions(&rstate).await != 0 {
        assert!(Instant::now() < deadline, "the closed session ends on the server");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let (_pane, ready) = window_pane(fake.port, "fake", "sw2").await;
    assert_eq!(ready["mode"], "new");
}

#[tokio::test]
async fn a_remote_window_saves_and_reads_the_servers_layout() {
    let fake = FakeRemote::start(None);
    let (mut link, _) = open_link(fake.port, "fake").await;
    stages_until_connected(&mut link).await;
    let saved = r#"{"sessions":[],"tabs":[],"version":1}"#; // write_workspace stores keys sorted
    assert_eq!(http(fake.port, "PUT", "/remote/fake/api/workspace", Some(saved), true).await.0, 204);
    assert_eq!(std::fs::read_to_string(fake.path().join("rstate/workspace.json")).unwrap(), saved);
    assert_eq!(http(fake.port, "GET", "/remote/fake/api/workspace", None, true).await, (200, saved.to_owned()));
    assert!(!fake.path().join("lstate/workspace.json").exists(), "the local layout is untouched");
}

#[tokio::test]
async fn a_remote_window_opens_a_shell_on_the_server() {
    let fake = FakeRemote::start(None);
    let (mut link, _) = open_link(fake.port, "fake").await;
    stages_until_connected(&mut link).await;
    let (mut pane, ready) = window_pane(fake.port, "fake", "sw1").await;
    assert_eq!(ready["mode"], "new");
    pane.send(MARK_INPUT).await;
    pane.wait_for("MARK_42").await;
    pane.detach().await;
    let (mut pane, ready) = window_pane(fake.port, "fake", "sw1").await;
    assert_eq!(ready["mode"], "reattached");
    pane.wait_for("MARK_42").await; // the screen comes back after "ready"
}

/// Programs installed where only the user's interactive shell looks (zsh's
/// `~/.zshrc` adds `~/.local/bin` on many servers) still start in a pane.
#[tokio::test]
async fn a_remote_editor_finds_programs_on_the_interactive_path() {
    // The server sees only the system folders, not a real tode of this machine.
    let fake = FakeRemote::start_with(None, &[("FAKE_SERVER_PATH", "/usr/bin:/bin:/usr/sbin:/sbin")]);
    let home = fake.path().join("home");
    std::fs::write(home.join(".shrc"), "PATH=\"$PATH:$HOME/.interactive/bin\"\n").unwrap();
    std::fs::create_dir_all(home.join(".interactive/bin")).unwrap();
    write_script(&home.join(".interactive/bin/tode"), "#!/bin/sh\necho FAKE_TODE_RAN\nsleep 5\n");
    let (mut link, _) = open_link(fake.port, "fake").await;
    stages_until_connected(&mut link).await;
    let (mut pane, _) = window_pane_of(fake.port, "fake", "se1", "editor").await;
    pane.wait_for("FAKE_TODE_RAN").await;
}

#[tokio::test]
async fn remote_window_routes_check_origin_and_host() {
    let fake = FakeRemote::start(None);
    let body = r#"{"version":1,"tabs":[],"sessions":[]}"#;
    assert_eq!(http(fake.port, "PUT", "/remote/fake/api/workspace", Some(body), false).await.0, 400, "no Origin");
    assert_eq!(http_with_host(fake.port, "GET", "/remote/fake/api/workspace", "evil.example:80").await.0, 403, "DNS rebinding");
    assert_eq!(http(fake.port, "GET", "/remote/-oProxyCommand=x/api/workspace", None, true).await.0, 400);
    // A page on another site can fire a GET at 127.0.0.1 (an <img>); it must not make ssh connect.
    for path in ["/remote/fake/api/workspace", "/remote/fake/api/ssh-hosts"] {
        for header in ["Sec-Fetch-Site: cross-site", "Sec-Fetch-Site: same-site", "Origin: http://evil.example"] {
            assert_eq!(http_with_header(fake.port, "GET", path, header).await.0, 403, "{path} with {header}");
        }
    }
    let query = "session=sw2&kind=shell&cols=80&rows=24&width=640&height=408&lang=en";
    assert_eq!(ws_status(fake.port, &format!("/remote/fake/ws/session?{query}"), Some("http://evil.example")).await, 403);
    assert_eq!(fake.count(PROBE), 0, "refused requests never reach ssh");
}

// ---------------------------------------------------------------------------
// The dashboard on a server (`remote-dashboard`).

/// Waits up to 10 s for `path` to exist.
async fn wait_for_file(path: &Path) -> PathBuf {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(Instant::now() < deadline, "{} did not appear", path.display());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    path.to_path_buf()
}

/// One HTTP request over a Unix socket, answered to the end:
/// (status, body). A body is sent as JSON.
async fn unix_http(socket: &Path, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
    let mut stream = tokio::net::UnixStream::connect(socket).await.unwrap();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: host\r\nConnection: close\r\n");
    match body {
        Some(body) => request.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len())),
        None => request.push_str("Content-Length: 0\r\n\r\n"),
    }
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response)).await.expect("an answer in time").unwrap();
    let text = String::from_utf8_lossy(&response).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").expect("a response head");
    let status = head.split_whitespace().nth(1).and_then(|code| code.parse().ok()).expect("a status");
    (status, body.to_owned())
}

/// The addresses a process listens on over TCP. Empty, with a note, when
/// `lsof` is not installed.
fn listening_tcp_ports(pid: u32) -> Vec<String> {
    let output = match Command::new("lsof").args(["-nP", "-a", "-p", &pid.to_string(), "-iTCP", "-sTCP:LISTEN", "-Fn"]).output() {
        Ok(output) => output,
        Err(error) => {
            eprintln!("lsof is not available ({error}): the no-TCP check is skipped");
            return Vec::new();
        }
    };
    String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| line.strip_prefix('n')).map(str::to_owned).collect()
}

#[tokio::test]
async fn the_remote_dashboard_answers_only_its_routes_on_a_private_socket() {
    let state = state();
    let _host = StopHostOnDrop(state.path());
    let dashboard = Spawned(
        Command::new(BIN).arg("remote-dashboard").env("WESTERM_STATE_DIR", state.path()).env("SHELL", "/bin/sh").spawn().unwrap(),
    );
    let socket = wait_for_file(&state.path().join("dashboard.sock")).await;
    assert_eq!(std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(listening_tcp_ports(dashboard.0.id()).is_empty(), "no TCP port on the server");
    assert_eq!(unix_http(&socket, "GET", "/api/workspace", None).await, (200, "null".to_owned()));
    // write_workspace stores the keys sorted, so this order reads back unchanged.
    let saved = r#"{"sessions":[],"tabs":[],"version":1}"#;
    assert_eq!(unix_http(&socket, "PUT", "/api/workspace", Some(saved)).await.0, 204, "no Origin needed on the socket");
    assert_eq!(unix_http(&socket, "GET", "/api/workspace", None).await.1, saved);
    for path in ["/", "/index.html", "/api/update", "/api/terminal-config", "/api/remote/decision"] {
        assert_eq!(unix_http(&socket, "GET", path, None).await.0, 404, "{path}");
    }
    let stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
    let (mut viewer, _) = Viewer::attach_path(stream, &format!("/ws/session?{QUERY}")).await;
    viewer.send(MARK_INPUT).await;
    viewer.wait_for("MARK_42").await;
    viewer.detach().await;
    let stop = Command::new(BIN).arg("stop").env("WESTERM_STATE_DIR", state.path()).output().unwrap();
    assert!(String::from_utf8_lossy(&stop.stdout).contains("the remote dashboard stopped"));
    assert!(!socket.exists());
}
