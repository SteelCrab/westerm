//! UDP hole punching types that both ends share: the gate on the server and
//! the dashboard here.

use std::net::SocketAddrV4;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::stun;

/// Sent to the peer's mapped address to open the NAT. At 8 bytes it is too
/// short to be a QUIC packet: a short header with quinn's 8-byte connection
/// id needs 9, so quinn drops it. It holds no secret.
pub const PUNCH_PACKET: &[u8] = &[0x00, b'w', b'e', b's', b't', b'e', b'r', b'm'];
pub const PUNCH_INTERVAL: Duration = Duration::from_millis(200);
/// How long the gate keeps punching and the address open for the dashboard.
/// The gate reads `WESTERM_PUNCH_WAIT_MS` instead when set (for tests).
pub const PUNCH_WAIT: Duration = Duration::from_secs(10);
/// Open punch sockets that wait for a first QUIC packet.
pub const MAX_WAITING: usize = 8;
/// Punched connections alive at once.
pub const MAX_LIVE: usize = 32;

/// Line 2 of `remote-setup` stdin, and the request on the gate's control socket.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PunchRequest {
    /// The dashboard's public address as the NAT shows it.
    pub peer: SocketAddrV4,
    pub stun: Vec<String>,
}

/// Why the server did not offer a punched address.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    /// Two STUN servers saw different ports: the NAT will not punch.
    Symmetric,
    NoStun,
    Busy,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum PunchAnswer {
    /// The server's public address, open for `wait_ms`.
    Ready { addr: SocketAddrV4, wait_ms: u64 },
    Skipped { skipped: SkipReason },
}

/// A request the gate may act on. It sends packets only to `peer`, so a
/// peer that is no real host is refused here.
pub fn parse_request(line: &str) -> Option<PunchRequest> {
    let request: PunchRequest = serde_json::from_str(line).ok()?;
    let ip = request.peer.ip();
    let peer_ok = request.peer.port() != 0 && !ip.is_unspecified() && !ip.is_multicast() && !ip.is_broadcast();
    let stun_ok = (1..=stun::MAX_SERVERS).contains(&request.stun.len()) && request.stun.iter().all(|server| stun::is_server(server));
    (peer_ok && stun_ok).then_some(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_round_trips_as_one_json_line() {
        let line = r#"{"peer":"14.39.31.2:62327","stun":["stun.l.google.com:19302","stun.cloudflare.com:3478"]}"#;
        let request = parse_request(line).unwrap();
        assert_eq!(request.peer, "14.39.31.2:62327".parse().unwrap());
        assert_eq!(request.stun.len(), 2);
        assert_eq!(serde_json::to_string(&request).unwrap(), line);
        assert!(parse_request(r#"{"peer":"127.0.0.1:5000","stun":["127.0.0.1:3478"]}"#).is_some(), "loopback is allowed: tests run on one machine");
    }

    #[test]
    fn refuses_requests_it_must_not_punch_for() {
        let with_peer = |peer: &str| format!(r#"{{"peer":"{peer}","stun":["a.example:3478"]}}"#);
        for peer in ["14.39.31.2:0", "0.0.0.0:5000", "224.0.0.1:5000", "255.255.255.255:5000", "[::1]:5000", "host:5000", ""] {
            assert_eq!(parse_request(&with_peer(peer)), None, "{peer}");
        }
        assert_eq!(parse_request(r#"{"peer":"14.39.31.2:1","stun":[]}"#), None, "no STUN server");
        assert_eq!(parse_request(r#"{"peer":"14.39.31.2:1","stun":["a:1","b:1","c:1","d:1","e:1"]}"#), None, "too many");
        assert_eq!(parse_request(r#"{"peer":"14.39.31.2:1","stun":["not a server"]}"#), None);
        for line in ["", "garbage", "[]", r#"{"peer":"14.39.31.2:1"}"#] {
            assert_eq!(parse_request(line), None, "{line}");
        }
    }

    #[test]
    fn answers_have_two_shapes() {
        let ready = PunchAnswer::Ready { addr: "49.171.219.140:56256".parse().unwrap(), wait_ms: 10_000 };
        assert_eq!(serde_json::to_string(&ready).unwrap(), r#"{"addr":"49.171.219.140:56256","waitMs":10000}"#);
        assert_eq!(serde_json::from_str::<PunchAnswer>(r#"{"addr":"49.171.219.140:56256","waitMs":10000}"#).unwrap(), ready);
        for (reason, text) in [(SkipReason::Symmetric, "symmetric"), (SkipReason::NoStun, "noStun"), (SkipReason::Busy, "busy"), (SkipReason::Error, "error")] {
            let json = format!(r#"{{"skipped":"{text}"}}"#);
            assert_eq!(serde_json::to_string(&PunchAnswer::Skipped { skipped: reason }).unwrap(), json);
            assert_eq!(serde_json::from_str::<PunchAnswer>(&json).unwrap(), PunchAnswer::Skipped { skipped: reason });
        }
    }

    #[test]
    fn the_punch_packet_cannot_pass_for_quic_or_hold_a_secret() {
        assert_eq!(PUNCH_PACKET[0] & 0x40, 0, "the QUIC fixed bit is off");
        assert!(PUNCH_PACKET.len() < 9, "too short for a short header with an 8-byte connection id, so quinn drops it");
    }
}
