//! Remote westerm with the real binary.
//! - The server side: `remote-setup` starts a host and a gate, and a QUIC
//!   client reaches a shell through it. That client is written from the
//!   outside (accept any certificate, then compare its fingerprint with the
//!   report).
//! - The whole path through a dashboard, with a fake `ssh` that runs the
//!   remote commands on this machine as a "server" in a temporary folder.
#![cfg(unix)]

use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
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
    tokio::time::timeout(Duration::from_secs(10), Viewer::attach_over(stream, QUERY)).await.expect("attach in time")
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

fn port_of(report: &serde_json::Value) -> u16 {
    report["port"].as_u64().unwrap() as u16
}

#[tokio::test]
async fn setup_starts_a_gate_that_relays_a_shell() {
    let state = state();
    let _stop = StopHostOnDrop(state.path());
    let id = ClientId::new();
    let report = setup(state.path(), &id, &[]);
    assert_eq!(report["v"], 1);
    assert_eq!(report["host"]["protocol"], 1);
    assert_eq!(report["host"]["build"], host_build());
    let fingerprint = report["fingerprint"].as_str().unwrap().to_owned();
    assert!(fingerprint.len() == 64 && fingerprint.bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(report["socket"].as_str().unwrap().ends_with("host.sock"));

    let connection = quic(port_of(&report), &id, &fingerprint).await.unwrap();
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
    let second = setup(state.path(), &id, &[]);
    assert_eq!(first["port"], second["port"]);
    assert_eq!(first["fingerprint"], second["fingerprint"]);
    assert_eq!(gate_pid(state.path()), pid, "the running gate is kept");
    let authorized = std::fs::read_to_string(state.path().join("remote/authorized_clients")).unwrap();
    assert_eq!(authorized.lines().count(), 1);
}

#[tokio::test]
async fn the_gate_refuses_unknown_clients_and_other_requests() {
    let state = state();
    let _stop = StopHostOnDrop(state.path());
    let id = ClientId::new();
    let report = setup(state.path(), &id, &[]);
    let fingerprint = report["fingerprint"].as_str().unwrap().to_owned();

    let stranger = ClientId::new();
    if let Ok(connection) = quic(port_of(&report), &stranger, &fingerprint).await {
        let stream = stream(&connection).await;
        let attached = tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::client_async(format!("ws://host/session?{QUERY}"), stream),
        )
        .await;
        assert!(!matches!(attached, Ok(Ok(_))), "an unknown client attached");
    }

    let connection = quic(port_of(&report), &id, &fingerprint).await.unwrap();
    let answer = raw(&connection, b"POST /shutdown HTTP/1.1\r\nHost: host\r\nContent-Length: 0\r\n\r\n").await;
    assert!(answer.is_empty(), "got {:?}", String::from_utf8_lossy(&answer));
    let smuggled = b"GET /session?bad HTTP/1.1\r\nHost: host\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n\
POST /shutdown HTTP/1.1\r\nHost: host\r\nContent-Length: 0\r\n\r\n";
    let answer = raw(&connection, smuggled).await;
    assert!(answer.starts_with(b"HTTP/1.1 400"), "got {:?}", String::from_utf8_lossy(&answer));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(host_health(state.path()).await.contains("\"build\""), "the host still runs");
}

#[tokio::test]
async fn stop_ends_the_host_and_the_gate() {
    let state = state();
    let _stop = StopHostOnDrop(state.path());
    let id = ClientId::new();
    setup(state.path(), &id, &[]);
    assert!(state.path().join("gate.json").exists());
    let status = Command::new(BIN)
        .arg("stop")
        .env("WESTERM_STATE_DIR", state.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.path().join("gate.json").exists() || state.path().join("host.sock").exists() {
        assert!(Instant::now() < deadline, "the gate and the host stop");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ---------------------------------------------------------------------------
// Through the dashboard, with a fake ssh.

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use common::Spawned;

const PROBE: &str = "uname -sm; ~/.local/bin/westerm --version 2>/dev/null || true";
const UPLOAD: &str = "mkdir -p ~/.local/bin && cat > ~/.local/bin/.westerm.upload && chmod 755 ~/.local/bin/.westerm.upload && ~/.local/bin/.westerm.upload --version >/dev/null && mv -f ~/.local/bin/.westerm.upload ~/.local/bin/westerm";
const SETUP: &str = "~/.local/bin/westerm remote-setup";
const RESIZE: &str = r#"{"type":"resize","cols":80,"rows":24,"width":640,"height":408}"#;

/// `-G` prints a config pointing at this machine; any other call runs its
/// last argument (the remote command) here, as the "server" user.
const FAKE_SSH: &str = r#"#!/bin/sh
for last; do :; done
case " $* " in *" -G "*) printf 'hostname 127.0.0.1\nport 22\nproxyjump none\n'; exit 0 ;; esac
printf '%s\n' "$last" >> "$FAKE_ROOT/ssh.log"
[ -n "$FAKE_SSH_DELAY" ] && sleep "$FAKE_SSH_DELAY"
HOME="$FAKE_ROOT/home" PATH="$FAKE_ROOT/rbin:$PATH" WESTERM_STATE_DIR="$FAKE_ROOT/rstate" \
  WESTERM_GATE_PORT=0 SHELL=/bin/sh exec sh -c "$last"
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

async fn connect_pane(port: u16, session: &str) -> Socket {
    let url = format!(
        "ws://127.0.0.1:{port}/ws/session?session={session}&kind=remote&target=fake&cols=80&rows=24&width=640&height=408&lang=en"
    );
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert("Origin", format!("http://127.0.0.1:{port}").parse().unwrap());
    tokio_tungstenite::connect_async(request).await.unwrap().0
}

type Pane = Viewer<MaybeTlsStream<tokio::net::TcpStream>>;

/// Opens a remote pane as the browser does and reads until "ready": the
/// pane, the remote stages seen on the way, and the ready message. A socket
/// closed first gives its close code.
async fn open_pane(port: u16, session: &str) -> Result<(Pane, Vec<serde_json::Value>, serde_json::Value), Option<u16>> {
    let mut ws = connect_pane(port, session).await;
    ws.send(Message::text(RESIZE)).await.unwrap();
    let mut stages = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(30), ws.next()).await.expect("the pane answers in time") {
            Some(Ok(Message::Text(text))) => {
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                match value["type"].as_str() {
                    Some("remote") => stages.push(value),
                    Some("ready") => return Ok((Viewer { ws, output: String::new() }, stages, value)),
                    _ => panic!("unexpected message {value}"),
                }
            }
            Some(Ok(Message::Close(frame))) => return Err(frame.map(|frame| u16::from(frame.code))),
            Some(Ok(_)) => {}
            None | Some(Err(_)) => return Err(None),
        }
    }
}

/// A JSON POST to the dashboard; the response status.
async fn post(port: u16, path: &str, with_origin: bool, body: &str) -> u16 {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let origin = if with_origin { format!("Origin: http://127.0.0.1:{port}\r\n") } else { String::new() };
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n{origin}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[tokio::test]
async fn a_remote_pane_installs_westerm_and_survives_a_reconnect() {
    let remote = FakeRemote::start(None);
    let (mut pane, stages, ready) = open_pane(remote.port, "sr1").await.expect("the pane opens");
    assert!(stages.iter().any(|s| s["stage"] == "connected" && s["via"] == "quic"), "{stages:?}");
    assert!(!stages.iter().any(|s| s["stage"] == "failed"), "{stages:?}");
    assert_eq!(ready["mode"], "new");
    assert!(remote.path().join("home/.local/bin/westerm").is_file(), "westerm is installed on the server");
    assert_eq!(remote.count(UPLOAD), 1);
    pane.send(MARK_INPUT).await;
    pane.wait_for("MARK_42").await;
    drop(pane);

    let (mut again, _, ready) = open_pane(remote.port, "sr1").await.expect("the pane opens again");
    assert_eq!(ready["mode"], "reattached");
    again.wait_for("MARK_42").await;
}

#[tokio::test]
async fn two_panes_opening_at_once_install_once() {
    let remote = FakeRemote::start(Some("1"));
    let (first, second) = tokio::join!(open_pane(remote.port, "sa1"), open_pane(remote.port, "sb1"));
    assert!(first.is_ok() && second.is_ok());
    assert_eq!(remote.count(PROBE), 2, "one check, and one after the upload");
    assert_eq!(remote.count(UPLOAD), 1);
    assert_eq!(remote.count(SETUP), 1);
}

#[tokio::test]
async fn closing_a_pane_while_connecting_ends_it_on_the_server() {
    let remote = FakeRemote::start(Some("2"));
    let mut closing = connect_pane(remote.port, "sclose").await;
    closing.send(Message::text(RESIZE)).await.unwrap();
    match tokio::time::timeout(Duration::from_secs(10), closing.next()).await.unwrap() {
        Some(Ok(Message::Text(text))) => assert!(text.contains(r#""type":"remote""#), "{text}"),
        other => panic!("expected a stage, got {other:?}"),
    }
    closing.send(Message::text(r#"{"type":"close"}"#)).await.unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(message)) = closing.next().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "the dashboard closes the pane");

    let (_keep, _, ready) = open_pane(remote.port, "skeep").await.expect("another pane opens");
    assert_eq!(ready["mode"], "new");
    let rstate = remote.path().join("rstate");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let response = host_health(&rstate).await;
        let body = response.split_once("\r\n\r\n").map(|(_, body)| body).unwrap_or_default();
        let health: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        if health["sessions"] == 1 {
            break;
        }
        assert!(Instant::now() < deadline, "the closed pane's session ends on the server: {health}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let pending = std::fs::read_to_string(remote.path().join("lstate/remote/pending-close.json")).unwrap_or_default();
    assert!(!pending.contains("sclose"), "{pending}");
}

#[tokio::test]
async fn a_lost_gate_comes_back_and_the_shell_is_reattached() {
    let remote = FakeRemote::start(None);
    let (mut pane, _, _) = open_pane(remote.port, "sg1").await.expect("the pane opens");
    pane.send(MARK_INPUT).await;
    pane.wait_for("MARK_42").await;
    let pid = gate_pid(&remote.path().join("rstate"));
    assert!(Command::new("kill").args(["-9", &pid.to_string()]).status().unwrap().success());
    drop(pane);

    let deadline = Instant::now() + Duration::from_secs(20);
    let (mut again, ready) = loop {
        match open_pane(remote.port, "sg1").await {
            Ok((pane, _, ready)) => break (pane, ready),
            Err(code) => {
                assert!(Instant::now() < deadline, "the pane reconnects (last close: {code:?})");
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
    };
    assert_eq!(ready["mode"], "reattached");
    again.wait_for("MARK_42").await;
}

/// The pane's socket to the dashboard is fine, but the link to the server
/// has gone silent (a lost network): a close sent then cannot arrive, and
/// the shell must still end once the server is reachable again.
#[tokio::test]
async fn closing_a_pane_over_a_silent_link_ends_it_later() {
    let remote = FakeRemote::start(None);
    let rstate = remote.path().join("rstate");
    let (mut pane, _, _) = open_pane(remote.port, "sdead1").await.expect("the pane opens");
    pane.send(MARK_INPUT).await;
    pane.wait_for("MARK_42").await;
    let gate = gate_pid(&rstate).to_string();
    assert!(Command::new("kill").args(["-STOP", &gate]).status().unwrap().success());
    pane.send(r#"{"type":"close"}"#).await;
    let _ = pane.ws.close(None).await;
    // Give the dashboard time to see that the close went unanswered; then
    // the silent gate is gone for good, with whatever it never read.
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(Command::new("kill").args(["-KILL", &gate]).status().unwrap().success());

    let deadline = Instant::now() + Duration::from_secs(25);
    let _other = loop {
        match open_pane(remote.port, "sdead2").await {
            Ok((pane, _, _)) => break pane,
            Err(code) => {
                assert!(Instant::now() < deadline, "the dashboard reconnects (last close: {code:?})");
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let response = host_health(&rstate).await;
        let body = response.split_once("\r\n\r\n").map(|(_, body)| body).unwrap_or_default();
        let health: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        if health["sessions"] == 1 {
            break;
        }
        assert!(Instant::now() < deadline, "the closed pane's shell ends on the server: {health}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test]
async fn remote_apis_check_the_origin() {
    let remote = FakeRemote::start(None);
    let close = r#"{"target":"fake","session":"sx1"}"#;
    assert_eq!(post(remote.port, "/api/remote/close", false, close).await, 400);
    assert_eq!(post(remote.port, "/api/remote/close", true, close).await, 204);
    assert_eq!(post(remote.port, "/api/remote/close", true, r#"{"target":"-oProxyCommand=x","session":"sx1"}"#).await, 400);
    assert_eq!(post(remote.port, "/api/remote/decision", true, r#"{"target":"fake","replace":true}"#).await, 409);
}
