//! STUN binding requests (RFC 5389): ask a public server which address and
//! port the NAT gave a UDP socket. Hole punching needs that address, and two
//! servers that disagree show a NAT that is no use for it. IPv4 only.
//!
//! The packet code reads untrusted bytes, so it never indexes and never
//! panics. Nothing is sent when the setting is `off`.

use std::future::Future;
use std::net::{SocketAddr, SocketAddrV4};
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::task::JoinSet;
use tokio::time::Instant;

pub const DEFAULT_SERVERS: [&str; 2] = ["stun.l.google.com:19302", "stun.cloudflare.com:3478"];
pub const MAX_SERVERS: usize = 4;
/// The whole query, name lookup included. Punching is an extra: it must not
/// make a connection visibly slower.
pub const QUERY_TIMEOUT: Duration = Duration::from_millis(1000);

const COOKIE: [u8; 4] = [0x21, 0x12, 0xA4, 0x42];
const BINDING_REQUEST: [u8; 2] = [0x00, 0x01];
const BINDING_SUCCESS: [u8; 2] = [0x01, 0x01];
const MAPPED_ADDRESS: u16 = 0x0001;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;
const FAMILY_IPV4: u8 = 0x01;

pub fn binding_request(transaction: &[u8; 12]) -> [u8; 20] {
    let mut packet = [0u8; 20];
    packet[..2].copy_from_slice(&BINDING_REQUEST);
    // Bytes 2..4 (the length) stay 0: a request has no attributes.
    packet[4..8].copy_from_slice(&COOKIE);
    packet[8..].copy_from_slice(transaction);
    packet
}

/// The address a success response reports, if it answers `transaction`.
pub fn mapped_address(response: &[u8], transaction: &[u8; 12]) -> Option<SocketAddrV4> {
    let header = response.get(..20)?;
    if header.get(..2)? != BINDING_SUCCESS || header.get(4..8)? != COOKIE || header.get(8..20)? != transaction {
        return None;
    }
    let length = usize::from(u16::from_be_bytes(header.get(2..4)?.try_into().ok()?));
    let mut rest = response.get(20..)?.get(..length)?;
    let mut plain = None;
    while !rest.is_empty() {
        let kind = u16::from_be_bytes(rest.get(..2)?.try_into().ok()?);
        let size = usize::from(u16::from_be_bytes(rest.get(2..4)?.try_into().ok()?));
        let value = rest.get(4..)?.get(..size)?;
        match kind {
            // XOR-MAPPED-ADDRESS is what servers should send; it wins.
            XOR_MAPPED_ADDRESS => return decode(value, true),
            MAPPED_ADDRESS if plain.is_none() => plain = decode(value, false),
            _ => {}
        }
        // Attributes are padded to 4 bytes. The last one may lack its padding.
        rest = rest.get(4 + size.next_multiple_of(4)..).unwrap_or(&[]);
    }
    plain
}

fn decode(value: &[u8], xor: bool) -> Option<SocketAddrV4> {
    let [_, family, port0, port1, ip0, ip1, ip2, ip3] = *<&[u8; 8]>::try_from(value.get(..8)?).ok()?;
    if family != FAMILY_IPV4 {
        return None;
    }
    let mut port = u16::from_be_bytes([port0, port1]);
    let mut ip = u32::from_be_bytes([ip0, ip1, ip2, ip3]);
    if xor {
        port ^= u16::from_be_bytes([COOKIE[0], COOKIE[1]]);
        ip ^= u32::from_be_bytes(COOKIE);
    }
    Some(SocketAddrV4::new(ip.into(), port))
}

/// `host:port` with a name or an IPv4 address. Strict: this text ends up in a
/// name lookup and the user may have typed anything.
pub fn is_server(text: &str) -> bool {
    if text.len() > 255 {
        return false;
    }
    let Some((host, port)) = text.rsplit_once(':') else { return false };
    let host_ok = !host.is_empty()
        && !host.starts_with('-')
        && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let port_ok = !port.is_empty()
        && port.bytes().all(|b| b.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|port| port != 0);
    host_ok && port_ok
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StunConfig {
    Off,
    Servers(Vec<String>),
}

/// `WESTERM_REMOTE_STUN`: unset or empty is the default servers, `off` is no
/// STUN, anything else is a comma list of `host:port`. A list that cannot be
/// read means off, never the public servers: the user wanted other ones.
pub fn config_from(env: Option<&str>) -> StunConfig {
    let text = env.unwrap_or("").trim();
    if text.is_empty() {
        return StunConfig::Servers(DEFAULT_SERVERS.iter().map(|server| server.to_string()).collect());
    }
    if text.eq_ignore_ascii_case("off") {
        return StunConfig::Off;
    }
    let servers: Vec<String> = text.split(',').map(|server| server.trim().to_string()).collect();
    if (1..=MAX_SERVERS).contains(&servers.len()) && servers.iter().all(|server| is_server(server)) {
        StunConfig::Servers(servers)
    } else {
        StunConfig::Off
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mapping {
    /// Every server saw this address (a single answer cannot be checked).
    Stable(SocketAddrV4),
    /// The servers saw different ports: the peer cannot guess ours.
    Symmetric,
    /// No answer.
    Unknown,
}

pub fn classify(answers: &[SocketAddrV4]) -> Mapping {
    match answers.split_first() {
        None => Mapping::Unknown,
        Some((first, rest)) if rest.iter().all(|answer| answer == first) => Mapping::Stable(*first),
        Some(_) => Mapping::Symmetric,
    }
}

struct Peer {
    address: SocketAddrV4,
    transaction: [u8; 12],
    answer: Option<SocketAddrV4>,
    resent: bool,
    resend_at: Instant,
}

/// Asks `servers` from `socket` (the socket the punch will use, so the NAT
/// mapping is the one that counts). Always ends within `timeout`; the answers
/// that came in by then are classified.
pub async fn query(socket: &UdpSocket, servers: &[String], timeout: Duration) -> Mapping {
    query_with(socket, servers, timeout, lookup_ipv4).await
}

/// The first IPv4 address of a `host:port`.
async fn lookup_ipv4(server: String) -> Option<SocketAddrV4> {
    tokio::net::lookup_host(server).await.ok()?.find_map(|address| match address {
        SocketAddr::V4(address) => Some(address),
        SocketAddr::V6(_) => None,
    })
}

// `lookup` is a parameter so a test can make a name hang.
async fn query_with<L, F>(socket: &UdpSocket, servers: &[String], timeout: Duration, lookup: L) -> Mapping
where
    L: Fn(String) -> F,
    F: Future<Output = Option<SocketAddrV4>> + Send + 'static,
{
    let mut answers = Vec::new();
    // Dropping the future at the deadline keeps what `answers` already holds.
    let _ = tokio::time::timeout(timeout, collect(socket, servers, timeout, lookup, &mut answers)).await;
    classify(&answers)
}

/// Each name is looked up on its own, and its request goes out the moment it
/// resolves. A name that hangs costs only its own answer.
async fn collect<L, F>(socket: &UdpSocket, servers: &[String], timeout: Duration, lookup: L, answers: &mut Vec<SocketAddrV4>)
where
    L: Fn(String) -> F,
    F: Future<Output = Option<SocketAddrV4>> + Send + 'static,
{
    let mut lookups = JoinSet::new();
    for server in servers.iter().take(MAX_SERVERS) {
        lookups.spawn(lookup(server.clone()));
    }
    let mut peers: Vec<Peer> = Vec::new();
    let mut buffer = [0u8; 512];
    while !lookups.is_empty() || peers.iter().any(|peer| peer.answer.is_none()) {
        // The next resend: once per server, half a timeout after its own first send.
        let resend = peers.iter().filter(|peer| peer.answer.is_none() && !peer.resent).map(|peer| peer.resend_at).min();
        tokio::select! {
            Some(done) = lookups.join_next() => {
                let Ok(Some(address)) = done else { continue };
                let mut transaction = [0u8; 12];
                if getrandom::fill(&mut transaction).is_err() {
                    continue;
                }
                let _ = socket.send_to(&binding_request(&transaction), address).await;
                peers.push(Peer { address, transaction, answer: None, resent: false, resend_at: Instant::now() + timeout / 2 });
            }
            () = sleep_until_some(resend) => {
                let now = Instant::now();
                for peer in peers.iter_mut().filter(|peer| peer.answer.is_none() && !peer.resent && peer.resend_at <= now) {
                    peer.resent = true;
                    let _ = socket.send_to(&binding_request(&peer.transaction), peer.address).await;
                }
            }
            received = socket.recv_from(&mut buffer) => {
                let (size, from) = match received {
                    Ok(received) => received,
                    // Windows reports an ICMP "port unreachable" from an earlier send here.
                    Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => continue,
                    Err(_) => return,
                };
                let SocketAddr::V4(from) = from else { continue };
                let Some(datagram) = buffer.get(..size) else { continue };
                // Only a server we asked may answer, and only to its own transaction.
                if let Some(peer) = peers.iter_mut().find(|peer| peer.answer.is_none() && peer.address == from)
                    && let Some(mapped) = mapped_address(datagram, &peer.transaction)
                {
                    peer.answer = Some(mapped);
                    answers.push(mapped);
                }
            }
        }
    }
}

/// Waits for `at`, or for ever when there is nothing to wait for.
async fn sleep_until_some(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use std::net::SocketAddr;

    /// A STUN success response with `attributes` (type, value).
    pub fn response(transaction: &[u8; 12], attributes: &[(u16, &[u8])]) -> Vec<u8> {
        let mut body = Vec::new();
        for (kind, value) in attributes {
            body.extend_from_slice(&kind.to_be_bytes());
            body.extend_from_slice(&(value.len() as u16).to_be_bytes());
            body.extend_from_slice(value);
            body.resize(body.len() + (4 - value.len() % 4) % 4, 0);
        }
        let mut packet = vec![0x01, 0x01];
        packet.extend_from_slice(&(body.len() as u16).to_be_bytes());
        packet.extend_from_slice(&[0x21, 0x12, 0xA4, 0x42]);
        packet.extend_from_slice(transaction);
        packet.extend_from_slice(&body);
        packet
    }

    /// A local STUN server that answers with the sender's address, plus `lie` on the port.
    pub async fn fake_stun(lie: u16) -> SocketAddr {
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buffer = [0u8; 64];
            while let Ok((size, from)) = socket.recv_from(&mut buffer).await {
                let SocketAddr::V4(from) = from else { continue };
                if size != 20 {
                    continue;
                }
                let transaction: [u8; 12] = buffer[8..20].try_into().unwrap();
                let port = (from.port().wrapping_add(lie)) ^ 0x2112;
                let ip = u32::from(*from.ip()) ^ 0x2112_A442;
                let mut value = vec![0x00, 0x01];
                value.extend_from_slice(&port.to_be_bytes());
                value.extend_from_slice(&ip.to_be_bytes());
                let _ = socket.send_to(&response(&transaction, &[(0x0020, &value)]), from).await;
            }
        });
        addr
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;
    use std::net::SocketAddr;

    const TXN: [u8; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

    // 192.0.2.1:32853, as in RFC 5769.
    const XOR_MAPPED: [u8; 8] = [0x00, 0x01, 0xA1, 0x47, 0xE1, 0x12, 0xA6, 0x43];
    const MAPPED: [u8; 8] = [0x00, 0x01, 0x80, 0x55, 192, 0, 2, 1];

    fn addr() -> SocketAddrV4 {
        "192.0.2.1:32853".parse().unwrap()
    }

    #[test]
    fn a_request_is_a_binding_request_with_the_cookie() {
        let request = binding_request(&TXN);
        assert_eq!(request[..8], [0x00, 0x01, 0x00, 0x00, 0x21, 0x12, 0xA4, 0x42]);
        assert_eq!(request[8..], TXN);
    }

    #[test]
    fn reads_the_mapped_address() {
        assert_eq!(mapped_address(&response(&TXN, &[(0x0020, &XOR_MAPPED)]), &TXN), Some(addr()));
        assert_eq!(mapped_address(&response(&TXN, &[(0x0001, &MAPPED)]), &TXN), Some(addr()));
        // An unknown attribute first, with padding; XOR-MAPPED-ADDRESS wins over MAPPED-ADDRESS.
        let both = response(&TXN, &[(0x8022, b"abc"), (0x0001, &[0, 1, 0, 80, 10, 0, 0, 1]), (0x0020, &XOR_MAPPED)]);
        assert_eq!(mapped_address(&both, &TXN), Some(addr()));
    }

    #[test]
    fn refuses_answers_that_are_not_ours() {
        let good = response(&TXN, &[(0x0020, &XOR_MAPPED)]);
        assert_eq!(mapped_address(&good, &[9; 12]), None, "another transaction");
        let mut cookie = good.clone();
        cookie[4] ^= 1;
        assert_eq!(mapped_address(&cookie, &TXN), None, "a wrong magic cookie");
        let mut request = good.clone();
        request[1] = 0x00;
        assert_eq!(mapped_address(&request, &TXN), None, "not a success response");
        let ipv6 = [0x00, 0x02, 0xA1, 0x47, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(mapped_address(&response(&TXN, &[(0x0020, &ipv6)]), &TXN), None, "IPv4 only");
    }

    #[test]
    fn never_panics_on_broken_input() {
        let good = response(&TXN, &[(0x8022, b"abc"), (0x0020, &XOR_MAPPED)]);
        for end in 0..good.len() {
            let _ = mapped_address(&good[..end], &TXN);
        }
        // An attribute whose length points past the end.
        let mut lying = good.clone();
        lying[22] = 0xFF;
        lying[23] = 0xFF;
        assert_eq!(mapped_address(&lying, &TXN), None);
        // Every single-bit change of a good answer.
        for at in 0..good.len() {
            for bit in 0..8 {
                let mut changed = good.clone();
                changed[at] ^= 1 << bit;
                let _ = mapped_address(&changed, &TXN);
            }
        }
    }

    #[test]
    fn the_setting_picks_the_servers() {
        let defaults = StunConfig::Servers(DEFAULT_SERVERS.iter().map(|s| s.to_string()).collect());
        assert_eq!(config_from(None), defaults);
        assert_eq!(config_from(Some("")), defaults);
        assert_eq!(config_from(Some("off")), StunConfig::Off);
        assert_eq!(config_from(Some(" OFF ")), StunConfig::Off);
        assert_eq!(
            config_from(Some("a.example:3478, 10.0.0.1:19302")),
            StunConfig::Servers(vec!["a.example:3478".into(), "10.0.0.1:19302".into()])
        );
        // A value that cannot be read turns STUN off: it must not fall back to the public servers.
        for broken in ["a.example", "a.example:0", "a.example:99999", "a b:1", "-x:1", "a:1,b:2,c:3,d:4,e:5"] {
            assert_eq!(config_from(Some(broken)), StunConfig::Off, "{broken}");
        }
        assert!(!is_server(&format!("{}:1", "a".repeat(254))), "too long");
    }

    #[test]
    fn two_equal_answers_are_a_stable_mapping() {
        let other: SocketAddrV4 = "192.0.2.1:40000".parse().unwrap();
        assert_eq!(classify(&[]), Mapping::Unknown);
        assert_eq!(classify(&[addr()]), Mapping::Stable(addr()), "one server: no check possible");
        assert_eq!(classify(&[addr(), addr()]), Mapping::Stable(addr()));
        assert_eq!(classify(&[addr(), other]), Mapping::Symmetric);
    }

    #[tokio::test]
    async fn a_query_reports_this_sockets_address() {
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let SocketAddr::V4(own) = socket.local_addr().unwrap() else { unreachable!() };
        let servers = vec![fake_stun(0).await.to_string(), fake_stun(0).await.to_string()];
        assert_eq!(query(&socket, &servers, QUERY_TIMEOUT).await, Mapping::Stable(own));
        let lying = vec![fake_stun(0).await.to_string(), fake_stun(1).await.to_string()];
        assert_eq!(query(&socket, &lying, QUERY_TIMEOUT).await, Mapping::Symmetric);
    }

    #[tokio::test]
    async fn a_silent_server_ends_within_the_timeout() {
        let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let started = std::time::Instant::now();
        assert_eq!(
            query(&socket, &[silent.local_addr().unwrap().to_string()], Duration::from_millis(300)).await,
            Mapping::Unknown
        );
        assert!(started.elapsed() < Duration::from_millis(800), "{:?}", started.elapsed());
    }

    /// A name that does not resolve must not hold the query past its timeout.
    #[tokio::test]
    async fn an_unresolvable_server_ends_within_the_timeout() {
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let started = std::time::Instant::now();
        assert_eq!(
            query(&socket, &["no-such-host.invalid:3478".into()], Duration::from_millis(300)).await,
            Mapping::Unknown
        );
        assert!(started.elapsed() < Duration::from_millis(800), "{:?}", started.elapsed());
    }

    /// A name that never resolves must not take the other servers' answers away.
    #[tokio::test]
    async fn a_hanging_name_does_not_cost_the_other_answer() {
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let SocketAddr::V4(own) = socket.local_addr().unwrap() else { unreachable!() };
        let good = fake_stun(0).await;
        let servers = vec!["hangs.example:3478".to_string(), good.to_string()];
        let lookup = |server: String| async move {
            if server.starts_with("hangs.") {
                std::future::pending::<()>().await;
            }
            server.parse::<SocketAddrV4>().ok()
        };
        let started = std::time::Instant::now();
        assert_eq!(query_with(&socket, &servers, Duration::from_millis(300), lookup).await, Mapping::Stable(own));
        assert!(started.elapsed() < Duration::from_millis(800), "{:?}", started.elapsed());
    }
}
