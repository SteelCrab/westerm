//! `westerm gate`: the server's QUIC entrance to its PTY host. It accepts
//! only clients in `authorized_clients`, and on each stream it lets through
//! only a WebSocket upgrade to `/session`; the host's other routes (such as
//! `/shutdown`) stay local.

use std::fs;
use std::future::Future;
use std::io::{self, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::{identity, remote_dir, tls, DEFAULT_PORT, PORT_SPAN};
use crate::ipc;
use crate::state_dir::private_file;

/// A request or response head longer than this is refused.
const HEAD_LIMIT: usize = 8 * 1024;
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);

/// What a running gate wrote to `gate.json` in its state directory.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GateRecord {
    pub pid: u32,
    pub port: u16,
    /// `version_line()` of the binary that runs it.
    pub version: String,
}

fn record_path(state: &Path) -> PathBuf {
    state.join("gate.json")
}

fn lock_path(state: &Path) -> PathBuf {
    state.join("gate.lock")
}

pub fn read_record(state: &Path) -> Option<GateRecord> {
    serde_json::from_slice(&fs::read(record_path(state)).ok()?).ok()
}

fn write_record(state: &Path, record: &GateRecord) -> io::Result<()> {
    let temp = state.join(format!("gate.json.{}", std::process::id()));
    let mut file = private_file().create(true).truncate(true).write(true).open(&temp)?;
    file.write_all(serde_json::to_string(record).map_err(io::Error::other)?.as_bytes())?;
    fs::rename(&temp, record_path(state))
}

/// A gate holds `gate.lock` for as long as it runs.
pub fn is_running(state: &Path) -> bool {
    let Ok(file) = private_file().create(true).truncate(false).write(true).open(lock_path(state)) else {
        return false;
    };
    file.try_lock().is_err()
}

/// Asks the running gate to stop and waits up to 5 s. False when none ran.
pub async fn stop_running(state: &Path) -> io::Result<bool> {
    if !is_running(state) {
        let _ = fs::remove_file(record_path(state));
        return Ok(false);
    }
    let record = read_record(state).ok_or_else(|| io::Error::other("a gate holds gate.lock but gate.json is missing"))?;
    // SAFETY: kill only sends a signal to the process the record names.
    unsafe { libc::kill(record.pid as libc::pid_t, libc::SIGTERM) };
    for _ in 0..50 {
        if !is_running(state) {
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(io::Error::other("the remote gate did not stop within 5 s"))
}

/// WESTERM_GATE_PORT picks one port (0: any free one); otherwise the gate
/// tries 4317 and the next ports.
pub fn ports_from(env: Option<&str>) -> Vec<u16> {
    match env.and_then(|text| text.trim().parse::<u16>().ok()) {
        Some(port) => vec![port],
        None => (DEFAULT_PORT..DEFAULT_PORT + PORT_SPAN).collect(),
    }
}

/// The first port that binds, on IPv6 (dual-stack where the system allows)
/// and else IPv4. Another user's gate may hold a port.
pub fn bind_first(config: quinn::ServerConfig, ports: &[u16]) -> io::Result<(quinn::Endpoint, u16)> {
    for &port in ports {
        for addr in [SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)), SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))] {
            if let Ok(endpoint) = quinn::Endpoint::server(config.clone(), addr) {
                let bound = endpoint.local_addr()?.port();
                return Ok((endpoint, bound));
            }
        }
    }
    let range = match ports {
        [only] => only.to_string(),
        [first, .., last] => format!("{first}–{last}"),
        [] => "the list".to_owned(),
    };
    Err(io::Error::new(io::ErrorKind::AddrInUse, format!("no free UDP port in {range}")))
}

/// True only for `GET /session?… HTTP/1.1` with `Upgrade: websocket`: the
/// one request a dashboard sends through the gate.
pub fn request_allowed(head: &str) -> bool {
    let mut lines = head.split("\r\n");
    let Some(request_line) = lines.next() else { return false };
    let mut parts = request_line.split(' ');
    let (Some("GET"), Some(target), Some("HTTP/1.1"), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    if !target.starts_with("/session?") {
        return false;
    }
    lines.take_while(|line| !line.is_empty()).any(|line| {
        line.split_once(':')
            .is_some_and(|(name, value)| name.trim().eq_ignore_ascii_case("upgrade") && value.trim().eq_ignore_ascii_case("websocket"))
    })
}

/// Reads up to and including the blank line that ends an HTTP head.
/// Returns (head, bytes after it); None on EOF, error, or an oversized head.
async fn read_head<R: AsyncRead + Unpin>(reader: &mut R) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    loop {
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let rest = buffer.split_off(end + 4);
            return Some((buffer, rest));
        }
        if buffer.len() > HEAD_LIMIT {
            return None;
        }
        let read = reader.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

/// One stream from a dashboard. The host sees the request head only when it
/// is a session upgrade, and anything after it only once the host answered
/// 101. A refused upgrade therefore cannot carry a second request.
pub(crate) async fn proxy<C, H, F, Fut>(mut client: C, connect_host: F) -> io::Result<()>
where
    C: AsyncRead + AsyncWrite + Unpin,
    H: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce() -> Fut,
    Fut: Future<Output = io::Result<H>>,
{
    let Ok(Some((head, rest))) = tokio::time::timeout(HEAD_TIMEOUT, read_head(&mut client)).await else {
        return client.shutdown().await;
    };
    if !request_allowed(&String::from_utf8_lossy(&head)) {
        return client.shutdown().await;
    }
    let mut host = connect_host().await?;
    host.write_all(&head).await?;
    let Ok(Some((response, after))) = tokio::time::timeout(HEAD_TIMEOUT, read_head(&mut host)).await else {
        return client.shutdown().await;
    };
    client.write_all(&response).await?;
    if !response.starts_with(b"HTTP/1.1 101") {
        return client.shutdown().await;
    }
    client.write_all(&after).await?;
    host.write_all(&rest).await?;
    tokio::io::copy_bidirectional(&mut client, &mut host).await?;
    Ok(())
}

/// `westerm gate` for the state directory `state` (see state_dir()).
pub async fn run(state: &Path) -> io::Result<()> {
    let lock = private_file().create(true).truncate(false).write(true).open(lock_path(state))?;
    if lock.try_lock().is_err() {
        return Err(io::Error::new(io::ErrorKind::AddrInUse, "another gate is already running"));
    }
    let dir = remote_dir(state)?;
    let identity = identity::load_or_create(&dir, "gate")?;
    let config = tls::server_config(&identity, dir.join("authorized_clients")).map_err(io::Error::other)?;
    let ports = ports_from(std::env::var("WESTERM_GATE_PORT").ok().as_deref());
    let (endpoint, port) = bind_first(config, &ports)?;
    let version = crate::version_line();
    write_record(state, &GateRecord { pid: std::process::id(), port, version: version.clone() })?;
    println!("gate {version} listening on UDP {port} (pid {})", std::process::id());

    let reason = tokio::select! {
        () = accept(endpoint.clone(), state.to_path_buf()) => "the endpoint closed",
        signal = termination() => signal,
    };
    println!("gate stopping ({reason})");
    endpoint.close(0u32.into(), b"gate stopping");
    // gate.json goes before the lock: whoever waits for the lock may start
    // a new gate, which writes its own record.
    let _ = fs::remove_file(record_path(state));
    drop(lock);
    Ok(())
}

async fn accept(endpoint: quinn::Endpoint, state: PathBuf) {
    while let Some(incoming) = endpoint.accept().await {
        // A Retry proves the client owns its address before any work is done.
        if !incoming.remote_address_validated() {
            let _ = incoming.retry();
            continue;
        }
        let state = state.clone();
        tokio::spawn(async move {
            let Ok(connection) = incoming.await else { return };
            while let Ok((send, recv)) = connection.accept_bi().await {
                let state = state.clone();
                tokio::spawn(async move {
                    let _ = proxy(tokio::io::join(recv, send), || ipc::connect(&state)).await;
                });
            }
        });
    }
}

async fn termination() -> &'static str {
    use tokio::signal::unix::{signal, SignalKind};
    let (Ok(mut terminate), Ok(mut interrupt), Ok(mut hangup)) =
        (signal(SignalKind::terminate()), signal(SignalKind::interrupt()), signal(SignalKind::hangup()))
    else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = terminate.recv() => "SIGTERM",
        _ = interrupt.recv() => "SIGINT",
        _ = hangup.recv() => "SIGHUP",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn only_websocket_session_upgrades_pass() {
        let ok = "GET /session?session=sa&kind=shell&cols=80 HTTP/1.1\r\nHost: host\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n";
        assert!(request_allowed(ok));
        assert!(request_allowed(&format!("{ok}\r\n")), "with the blank line that ends the head");
        assert!(request_allowed(&ok.replace("Upgrade: websocket", "upgrade: WebSocket")));
        for bad in [
            "GET /session?session=sa HTTP/1.1\r\nHost: host\r\n",
            "POST /shutdown HTTP/1.1\r\nHost: host\r\n",
            "GET /health HTTP/1.1\r\nUpgrade: websocket\r\n",
            "GET /session HTTP/1.1\r\nUpgrade: websocket\r\n",
            "GET /session?x HTTP/1.0\r\nUpgrade: websocket\r\n",
            "GET /session?x y HTTP/1.1\r\nUpgrade: websocket\r\n",
            "GET /session?x HTTP/1.1\r\n\r\nUpgrade: websocket\r\n",
            "",
        ] {
            assert!(!request_allowed(bad), "{bad:?}");
        }
    }

    #[test]
    fn ports_follow_the_override() {
        assert_eq!(ports_from(None), (4317..=4326).collect::<Vec<_>>());
        assert_eq!(ports_from(Some("0")), vec![0]);
        assert_eq!(ports_from(Some("5000")), vec![5000]);
        assert_eq!(ports_from(Some("x")), ports_from(None));
    }

    #[tokio::test]
    async fn binds_the_next_port_when_one_is_taken() {
        let v4 = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        let taken = v4.local_addr().unwrap().port();
        let _v6 = std::net::UdpSocket::bind(("::", taken));
        let dir = tempfile::tempdir().unwrap();
        let identity = crate::remote::identity::load_or_create(dir.path(), "gate").unwrap();
        let config = crate::remote::tls::server_config(&identity, dir.path().join("authorized_clients")).unwrap();
        let (_endpoint, port) = bind_first(config, &[taken, 0]).unwrap();
        assert_ne!(port, taken);
        assert_ne!(port, 0);
    }

    const UPGRADE: &[u8] = b"GET /session?session=sa HTTP/1.1\r\nHost: host\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n";

    /// Reads until `needle` has arrived (or the stream ends).
    async fn read_until(stream: &mut tokio::io::DuplexStream, needle: &[u8]) -> Vec<u8> {
        let mut got = Vec::new();
        let mut chunk = [0u8; 256];
        while !got.windows(needle.len()).any(|window| window == needle) {
            let n = stream.read(&mut chunk).await.unwrap();
            if n == 0 {
                break;
            }
            got.extend_from_slice(&chunk[..n]);
        }
        got
    }

    #[tokio::test]
    async fn an_accepted_upgrade_flows_both_ways() {
        let (gate_side, mut dashboard) = tokio::io::duplex(4096);
        let (host_side, mut host) = tokio::io::duplex(4096);
        let task = tokio::spawn(proxy(gate_side, move || async move { Ok(host_side) }));
        dashboard.write_all(&[UPGRADE, b"FRAME-UP"].concat()).await.unwrap();
        let head = read_until(&mut host, b"\r\n\r\n").await;
        assert!(head.starts_with(b"GET /session?session=sa HTTP/1.1"));
        host.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\nFRAME-DOWN").await.unwrap();
        let down = read_until(&mut dashboard, b"FRAME-DOWN").await;
        assert!(down.starts_with(b"HTTP/1.1 101"));
        let up = read_until(&mut host, b"FRAME-UP").await;
        assert!(up.ends_with(b"FRAME-UP"), "bytes sent with the request reach the host after the upgrade");
        drop(dashboard);
        drop(host);
        task.await.unwrap().ok();
    }

    #[tokio::test]
    async fn a_refused_upgrade_lets_nothing_else_through() {
        let (gate_side, mut dashboard) = tokio::io::duplex(4096);
        let (host_side, mut host) = tokio::io::duplex(4096);
        let task = tokio::spawn(proxy(gate_side, move || async move { Ok(host_side) }));
        let smuggled = b"POST /shutdown HTTP/1.1\r\nHost: host\r\nContent-Length: 0\r\n\r\n";
        dashboard.write_all(&[UPGRADE, smuggled].concat()).await.unwrap();
        read_until(&mut host, b"\r\n\r\n").await;
        host.write_all(b"HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\n\r\n").await.unwrap();
        let answer = read_until(&mut dashboard, b"\r\n\r\n").await;
        assert!(answer.starts_with(b"HTTP/1.1 400"));
        task.await.unwrap().ok();
        let mut rest = Vec::new();
        host.read_to_end(&mut rest).await.unwrap();
        assert!(rest.is_empty(), "the host got more: {:?}", String::from_utf8_lossy(&rest));
    }

    #[tokio::test]
    async fn other_requests_never_reach_the_host() {
        let (gate_side, mut dashboard) = tokio::io::duplex(4096);
        let task = tokio::spawn(proxy(gate_side, || async {
            panic!("the host must not be contacted");
            #[allow(unreachable_code)]
            Ok(tokio::io::duplex(1).0)
        }));
        dashboard.write_all(b"POST /shutdown HTTP/1.1\r\nHost: host\r\n\r\n").await.unwrap();
        let mut answer = Vec::new();
        dashboard.read_to_end(&mut answer).await.unwrap();
        assert!(answer.is_empty());
        task.await.unwrap().ok();
    }
}
