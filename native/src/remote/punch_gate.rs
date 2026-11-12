//! The gate's side of UDP hole punching. `remote-setup` asks over the
//! control socket `gate.sock` (0600, so only this user). For each request the
//! gate opens a new UDP socket ("socket B"), learns its public address from
//! STUN, sends punch packets to the client's address only, and serves QUIC on
//! it for that client, for a limited time. Authentication is the same TLS
//! client certificate check as on the gate's main port.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UdpSocket, UnixListener, UnixStream};
use tokio::sync::Notify;
use tokio::task::JoinSet;

use super::gate;
use super::punch::{self, PunchAnswer, PunchRequest, SkipReason, MAX_LIVE, MAX_WAITING, PUNCH_INTERVAL, PUNCH_PACKET, PUNCH_WAIT};
use super::stun::{self, Mapping};

/// How long remote-setup waits for the gate's answer (above the STUN
/// timeout), and so how long the gate waits for the request line.
pub const ASK_TIMEOUT: Duration = Duration::from_millis(1500);
/// A request or an answer is one short JSON line.
const MAX_LINE: u64 = 4096;

pub fn socket_path(state: &Path) -> PathBuf {
    state.join("gate.sock")
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Sockets B that wait for their first connection.
    pub waiting: usize,
    /// Sockets B in all, waiting or connected.
    pub live: usize,
    /// How long a socket B punches and waits for its first connection.
    pub wait: Duration,
}

impl Limits {
    pub fn from_env() -> Self {
        Self { waiting: MAX_WAITING, live: MAX_LIVE, wait: wait_from(std::env::var("WESTERM_PUNCH_WAIT_MS").ok().as_deref()) }
    }
}

/// WESTERM_PUNCH_WAIT_MS (for tests) can make the wait shorter, never longer.
fn wait_from(env: Option<&str>) -> Duration {
    env.and_then(|text| text.trim().parse::<u64>().ok()).map_or(PUNCH_WAIT, |ms| Duration::from_millis(ms).min(PUNCH_WAIT))
}

pub struct Puncher {
    config: quinn::ServerConfig,
    state: PathBuf,
    limits: Limits,
    sockets: Mutex<Sockets>,
}

/// Every socket B, from the limit check until it is closed: the limits count these.
#[derive(Default)]
struct Sockets {
    next: u64,
    open: HashMap<u64, Open>,
}

struct Open {
    peer: SocketAddrV4,
    /// No connection yet. Only a waiting socket is replaced by a new request.
    waiting: bool,
    /// Set once QUIC runs on the socket, for close_all.
    endpoint: Option<quinn::Endpoint>,
    /// Ends the socket early: a new request came for the same peer.
    replaced: Arc<Notify>,
}

/// Holds one entry of `Sockets` and gives it back however the socket ends.
struct Slot {
    puncher: Arc<Puncher>,
    id: u64,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.puncher.sockets().open.remove(&self.id);
    }
}

impl Puncher {
    pub fn new(config: quinn::ServerConfig, state: PathBuf, limits: Limits) -> Arc<Self> {
        Arc::new(Self { config, state, limits, sockets: Mutex::new(Sockets::default()) })
    }

    fn sockets(&self) -> MutexGuard<'_, Sockets> {
        self.sockets.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Makes socket B for `request` and starts punching. Never fails: a problem is a `Skipped`.
    pub async fn punch(self: &Arc<Self>, request: PunchRequest) -> PunchAnswer {
        let peer = request.peer;
        let answer = self.open(request).await;
        match &answer {
            PunchAnswer::Ready { addr, .. } => println!("gate: punch for {peer}: {addr}"),
            PunchAnswer::Skipped { skipped } => println!("gate: punch for {peer}: skipped ({})", reason_text(*skipped)),
        }
        answer
    }

    async fn open(self: &Arc<Self>, request: PunchRequest) -> PunchAnswer {
        let skipped = |skipped| PunchAnswer::Skipped { skipped };
        // The slot is taken before the STUN query, so requests that come
        // together cannot all pass the check.
        let Some((slot, replaced)) = self.reserve(request.peer) else { return skipped(SkipReason::Busy) };
        let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await else { return skipped(SkipReason::Error) };
        let mapping = tokio::select! {
            mapping = stun::query(&socket, &request.stun, stun::QUERY_TIMEOUT) => mapping,
            () = replaced.notified() => return skipped(SkipReason::Error),
        };
        let addr = match mapping {
            Mapping::Stable(addr) => addr,
            Mapping::Symmetric => return skipped(SkipReason::Symmetric),
            Mapping::Unknown => return skipped(SkipReason::NoStun),
        };
        // The std socket stays non-blocking; quinn takes one handle, the punch packets use the other.
        let Ok(socket) = socket.into_std() else { return skipped(SkipReason::Error) };
        let Ok(punching) = socket.try_clone() else { return skipped(SkipReason::Error) };
        let runtime = Arc::new(quinn::TokioRuntime);
        let Ok(endpoint) = quinn::Endpoint::new(quinn::EndpointConfig::default(), Some(self.config.clone()), socket, runtime) else {
            return skipped(SkipReason::Error);
        };
        if let Some(open) = self.sockets().open.get_mut(&slot.id) {
            open.endpoint = Some(endpoint.clone());
        }
        let wait = self.limits.wait;
        tokio::spawn(run(slot, endpoint, punching, request.peer, replaced, wait));
        PunchAnswer::Ready { addr, wait_ms: u64::try_from(wait.as_millis()).unwrap_or(u64::MAX) }
    }

    /// Takes a slot within the limits, and ends the socket that still waits for the same peer.
    fn reserve(self: &Arc<Self>, peer: SocketAddrV4) -> Option<(Slot, Arc<Notify>)> {
        let mut sockets = self.sockets();
        let waiting = sockets.open.values().filter(|open| open.waiting).count();
        if waiting >= self.limits.waiting || sockets.open.len() >= self.limits.live {
            return None;
        }
        for open in sockets.open.values().filter(|open| open.waiting && open.peer == peer) {
            open.replaced.notify_one();
        }
        let id = sockets.next;
        sockets.next += 1;
        let replaced = Arc::new(Notify::new());
        sockets.open.insert(id, Open { peer, waiting: true, endpoint: None, replaced: replaced.clone() });
        Some((Slot { puncher: self.clone(), id }, replaced))
    }

    /// Answers requests on the control socket until the listener ends.
    pub async fn serve(self: Arc<Self>, listener: UnixListener) {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    tokio::spawn(self.clone().answer(stream));
                }
                Err(error) => {
                    eprintln!("gate: control socket: {error}");
                    // Such as too many open files: try again later, without spinning.
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    async fn answer(self: Arc<Self>, stream: UnixStream) {
        let (read, mut write) = stream.into_split();
        let mut line = String::new();
        let read = tokio::time::timeout(ASK_TIMEOUT, BufReader::new(read.take(MAX_LINE)).read_line(&mut line)).await;
        let request = matches!(read, Ok(Ok(_))).then(|| punch::parse_request(line.trim_end())).flatten();
        let answer = match request {
            Some(request) => self.punch(request).await,
            None => {
                println!("gate: punch request refused: not a valid request");
                PunchAnswer::Skipped { skipped: SkipReason::Error }
            }
        };
        let Ok(mut text) = serde_json::to_string(&answer) else { return };
        text.push('\n');
        let _ = write.write_all(text.as_bytes()).await;
        let _ = write.shutdown().await;
    }

    /// Closes every socket B, telling connected clients.
    pub async fn close_all(&self) {
        let endpoints: Vec<quinn::Endpoint> = self.sockets().open.values().filter_map(|open| open.endpoint.clone()).collect();
        for endpoint in &endpoints {
            endpoint.close(0u32.into(), b"gate stopping");
        }
        // As on the main port: the close reaches the clients only while the endpoints run.
        let _ = tokio::time::timeout(Duration::from_secs(1), async {
            for endpoint in &endpoints {
                endpoint.wait_idle().await;
            }
        })
        .await;
    }
}

/// The text of `reason` in the JSON answer, such as `noStun`.
fn reason_text(reason: SkipReason) -> String {
    match serde_json::to_value(reason) {
        Ok(serde_json::Value::String(text)) => text,
        _ => format!("{reason:?}"),
    }
}

/// The life of one socket B: punch and wait, then serve the one connection.
async fn run(slot: Slot, endpoint: quinn::Endpoint, punching: std::net::UdpSocket, peer: SocketAddrV4, replaced: Arc<Notify>, wait: Duration) {
    let first = tokio::select! {
        connection = first_connection(&endpoint, &punching, peer, wait) => connection,
        () = replaced.notified() => None,
    };
    // Socket B closes only when every handle is gone.
    drop(punching);
    if let Some(connection) = first {
        if let Some(open) = slot.puncher.sockets().open.get_mut(&slot.id) {
            open.waiting = false;
        }
        tokio::select! {
            () = gate::serve_connection(connection, slot.puncher.state.clone()) => {}
            () = refuse_all(&endpoint) => {}
        }
    }
    endpoint.close(0u32.into(), b"done");
    // `slot` drops here and takes its endpoint handle out of `Sockets`.
}

/// Punches toward `peer` and returns the first connection from its IP that
/// passes TLS, within `wait`. The port is not checked: a NAT may map
/// another port for QUIC than for STUN.
async fn first_connection(endpoint: &quinn::Endpoint, punching: &std::net::UdpSocket, peer: SocketAddrV4, wait: Duration) -> Option<quinn::Connection> {
    let deadline = tokio::time::sleep(wait);
    tokio::pin!(deadline);
    let mut ticks = tokio::time::interval(PUNCH_INTERVAL);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Handshakes run side by side, so one that stalls cannot hold up the real client.
    let mut handshakes = JoinSet::new();
    loop {
        tokio::select! {
            () = &mut deadline => return None,
            _ = ticks.tick() => {
                // Non-blocking; a lost punch packet is no problem, the next one follows.
                let _ = punching.send_to(PUNCH_PACKET, peer);
            }
            incoming = endpoint.accept() => {
                let incoming = incoming?;
                if incoming.remote_address().ip().to_canonical() != IpAddr::V4(*peer.ip()) {
                    incoming.refuse();
                } else if !incoming.remote_address_validated() {
                    // A Retry proves the client owns its address, as on the main port.
                    let _ = incoming.retry();
                } else {
                    handshakes.spawn(async move { incoming.await });
                }
            }
            Some(done) = handshakes.join_next() => {
                if let Ok(Ok(connection)) = done {
                    return Some(connection);
                }
            }
        }
    }
}

/// Socket B serves one connection: any other is refused until it closes.
async fn refuse_all(endpoint: &quinn::Endpoint) {
    while let Some(incoming) = endpoint.accept().await {
        incoming.refuse();
    }
}

/// For remote-setup: asks the running gate. Any failure (no socket, no answer in `timeout`) is `Skipped { Error }`.
pub async fn ask(state: &Path, request: &PunchRequest, timeout: Duration) -> PunchAnswer {
    let exchange = async {
        let mut stream = UnixStream::connect(socket_path(state)).await.ok()?;
        let mut line = serde_json::to_string(request).ok()?;
        line.push('\n');
        stream.write_all(line.as_bytes()).await.ok()?;
        let mut answer = String::new();
        BufReader::new(stream.take(MAX_LINE)).read_line(&mut answer).await.ok()?;
        serde_json::from_str(answer.trim_end()).ok()
    };
    tokio::time::timeout(timeout, exchange).await.ok().flatten().unwrap_or(PunchAnswer::Skipped { skipped: SkipReason::Error })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn puncher(dir: &Path, limits: Limits) -> Arc<Puncher> {
        let identity = crate::remote::identity::load_or_create(dir, "gate").unwrap();
        let config = crate::remote::tls::server_config(&identity, dir.join("authorized_clients")).unwrap();
        Puncher::new(config, dir.to_path_buf(), limits)
    }

    /// Review Focus 2: a socket B that ended (nobody came) gives its slot back.
    #[tokio::test]
    async fn a_finished_punch_frees_its_slot() {
        let dir = tempfile::tempdir().unwrap();
        let identity = crate::remote::identity::load_or_create(dir.path(), "gate").unwrap();
        let config = crate::remote::tls::server_config(&identity, dir.path().join("authorized_clients")).unwrap();
        let limits = Limits { waiting: 1, live: 1, wait: Duration::from_millis(200) };
        let puncher = Puncher::new(config, dir.path().to_path_buf(), limits);
        let stun = vec![crate::remote::stun::fake::fake_stun(0).await.to_string()];
        let request = |port: u16| PunchRequest { peer: SocketAddrV4::new(Ipv4Addr::LOCALHOST, port), stun: stun.clone() };
        assert!(matches!(puncher.punch(request(5001)).await, PunchAnswer::Ready { wait_ms: 200, .. }));
        assert_eq!(puncher.punch(request(5002)).await, PunchAnswer::Skipped { skipped: SkipReason::Busy });
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(matches!(puncher.punch(request(5002)).await, PunchAnswer::Ready { .. }), "the slot is free again");
    }

    #[tokio::test]
    async fn ask_gets_the_answer_over_the_control_socket() {
        let dir = tempfile::tempdir().unwrap();
        let request = PunchRequest { peer: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 5001), stun: vec![crate::remote::stun::fake::fake_stun(0).await.to_string()] };
        assert_eq!(ask(dir.path(), &request, ASK_TIMEOUT).await, PunchAnswer::Skipped { skipped: SkipReason::Error }, "no gate");
        let limits = Limits { waiting: 1, live: 1, wait: Duration::from_millis(200) };
        let listener = crate::ipc::bind_socket(&socket_path(dir.path())).unwrap();
        tokio::spawn(puncher(dir.path(), limits).serve(listener));
        assert!(matches!(ask(dir.path(), &request, ASK_TIMEOUT).await, PunchAnswer::Ready { wait_ms: 200, .. }));
    }

    #[test]
    fn the_wait_can_only_get_shorter() {
        assert_eq!(wait_from(None), PUNCH_WAIT);
        assert_eq!(wait_from(Some("600")), Duration::from_millis(600));
        assert_eq!(wait_from(Some("600000")), PUNCH_WAIT);
        assert_eq!(wait_from(Some("x")), PUNCH_WAIT);
    }
}
