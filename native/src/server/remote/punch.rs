//! The dashboard's side of UDP hole punching: socket A with its STUN address,
//! a race between direct QUIC and the punched path, and the texts that say
//! why a window uses the SSH tunnel instead.

use std::future::Future;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::pin::{Pin, pin};
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;

use crate::remote::punch::{PunchAnswer, PunchRequest, SkipReason};
use crate::remote::stun::{self, Mapping, StunConfig};

/// The punched handshake: the NAT is already open, so a slow answer means it is not.
pub const PUNCH_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(4);
/// How much longer direct QUIC may take once the punched path is up: direct
/// keeps working when the NAT forgets the punched mapping.
pub const DIRECT_GRACE: Duration = Duration::from_millis(300);
/// The gate stops waiting after `wait_ms`; an offer this close to that is not tried.
pub const OFFER_MARGIN: Duration = Duration::from_secs(1);

const OFF: &str = "hole punching: turned off (WESTERM_REMOTE_STUN)";
const NO_SOCKET: &str = "hole punching: no UDP socket on this computer";
const NO_STUN: &str = "hole punching: no STUN answer on this computer";
const SYMMETRIC: &str = "hole punching: this network gives a new port per destination";

/// Socket A after STUN: what the server needs to punch for it.
pub struct Candidate {
    socket: UdpSocket,
    mapped: SocketAddrV4,
    servers: Vec<String>,
}

impl Candidate {
    pub fn request(&self) -> PunchRequest {
        PunchRequest { peer: self.mapped, stun: self.servers.clone() }
    }

    pub fn into_socket(self) -> std::io::Result<std::net::UdpSocket> {
        self.socket.into_std()
    }
}

/// A new socket A. Err: why hole punching is not possible from here, as shown to the user.
pub async fn prepare(config: &StunConfig) -> Result<Candidate, String> {
    let StunConfig::Servers(servers) = config else { return Err(OFF.to_owned()) };
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.map_err(|_| NO_SOCKET.to_owned())?;
    match stun::query(&socket, servers, stun::QUERY_TIMEOUT).await {
        Mapping::Stable(mapped) => Ok(Candidate { socket, mapped, servers: servers.clone() }),
        Mapping::Symmetric => Err(SYMMETRIC.to_owned()),
        Mapping::Unknown => Err(NO_STUN.to_owned()),
    }
}

/// Which ways the user allows (WESTERM_REMOTE_TRANSPORT).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ways {
    pub direct: bool,
    pub punched: bool,
    pub tunnel: bool,
}

pub fn ways_from(env: &str) -> Ways {
    match env {
        "quic" => Ways { direct: true, punched: true, tunnel: false },
        "punch" => Ways { direct: false, punched: true, tunnel: false },
        "tunnel" => Ways { direct: false, punched: false, tunnel: true },
        _ => Ways { direct: true, punched: true, tunnel: true },
    }
}

/// Why the server gave no punched address. None when it gave one.
pub fn skip_text(answer: Option<&PunchAnswer>) -> Option<&'static str> {
    match answer {
        Some(PunchAnswer::Ready { .. }) => None,
        Some(PunchAnswer::Skipped { skipped: SkipReason::Symmetric }) => {
            Some("hole punching: the server's network gives a new port per destination")
        }
        Some(PunchAnswer::Skipped { skipped: SkipReason::NoStun }) => Some("hole punching: no STUN answer on the server"),
        Some(PunchAnswer::Skipped { skipped: SkipReason::Busy }) => Some("hole punching: the server's gate is busy"),
        // An older server sends no `punch` at all.
        Some(PunchAnswer::Skipped { skipped: SkipReason::Error }) | None => Some("hole punching: the server could not prepare it"),
    }
}

/// The punched address may still be used at `now`.
pub fn offer_fresh(received: Instant, wait_ms: u64, now: Instant) -> bool {
    now.saturating_duration_since(received) + OFFER_MARGIN < Duration::from_millis(wait_ms)
}

/// What the tooltip says when the window uses the SSH tunnel.
pub fn tunnel_note(direct: Option<&str>, punch: Option<&str>) -> String {
    let direct = direct.map(|error| format!("QUIC: {error}"));
    let parts: Vec<String> = direct.into_iter().chain(punch.map(str::to_owned)).collect();
    if parts.is_empty() {
        // Nothing was tried: the user asked for the tunnel only.
        return "WESTERM_REMOTE_TRANSPORT=tunnel".to_owned();
    }
    parts.join("; ")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Won {
    Direct,
    Punched,
}

/// Runs both tries at once. Direct wins whenever it succeeds; a punched
/// success first gives direct `grace` more. Err: each side's error (None: not tried).
pub async fn race<T, E>(
    direct: Option<impl Future<Output = Result<T, E>>>,
    punched: Option<impl Future<Output = Result<T, E>>>,
    grace: Duration,
) -> Result<(T, Won), (Option<E>, Option<E>)> {
    // A side becomes None once it has finished, so it is never polled again.
    let mut direct = pin!(direct);
    let mut punched = pin!(punched);
    let (mut direct_error, mut punched_error) = (None, None);
    // The punched connection and when direct's grace ends.
    let mut ready: Option<(T, tokio::time::Instant)> = None;
    // Once direct is over, a punched result is final at once.
    while direct.is_some() || punched.is_some() {
        let grace_end = ready.as_ref().map(|(_, end)| *end);
        tokio::select! {
            result = run(direct.as_mut().as_pin_mut()), if direct.is_some() => {
                direct.set(None);
                match result {
                    Ok(value) => return Ok((value, Won::Direct)),
                    Err(error) => direct_error = Some(error),
                }
            }
            result = run(punched.as_mut().as_pin_mut()), if punched.is_some() => {
                punched.set(None);
                match result {
                    Ok(value) => ready = Some((value, tokio::time::Instant::now() + grace)),
                    Err(error) => punched_error = Some(error),
                }
            }
            () = sleep_until_some(grace_end) => break,
        }
    }
    // A direct try still on its way after the grace is dropped with `direct`.
    match ready {
        Some((value, _)) => Ok((value, Won::Punched)),
        None => Err((direct_error, punched_error)),
    }
}

/// Awaits a side that is still running. A finished side is None and its
/// select branch is off, so the pending arm is never polled.
async fn run<F: Future>(future: Option<Pin<&mut F>>) -> F::Output {
    match future {
        Some(future) => future.await,
        None => std::future::pending().await,
    }
}

/// Waits for `at`, or for ever when there is nothing to wait for.
async fn sleep_until_some(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transport_setting_picks_the_ways() {
        let all = Ways { direct: true, punched: true, tunnel: true };
        assert_eq!(ways_from(""), all);
        assert_eq!(ways_from("anything else"), all);
        assert_eq!(ways_from("quic"), Ways { tunnel: false, ..all });
        assert_eq!(ways_from("punch"), Ways { direct: false, punched: true, tunnel: false });
        assert_eq!(ways_from("tunnel"), Ways { direct: false, punched: false, tunnel: true });
    }

    #[test]
    fn an_offer_is_fresh_until_a_second_before_the_gate_stops_waiting() {
        let received = Instant::now();
        assert!(offer_fresh(received, 10_000, received + Duration::from_millis(8_900)));
        assert!(!offer_fresh(received, 10_000, received + Duration::from_millis(9_000)));
        assert!(!offer_fresh(received, 600, received), "a wait shorter than the margin is never fresh");
    }

    #[test]
    fn the_server_side_reasons_have_their_text() {
        let ready = PunchAnswer::Ready { addr: "49.171.219.140:56256".parse().unwrap(), wait_ms: 10_000 };
        assert_eq!(skip_text(Some(&ready)), None);
        let skipped = |skipped| PunchAnswer::Skipped { skipped };
        assert_eq!(skip_text(Some(&skipped(SkipReason::Symmetric))), Some("hole punching: the server's network gives a new port per destination"));
        assert_eq!(skip_text(Some(&skipped(SkipReason::NoStun))), Some("hole punching: no STUN answer on the server"));
        assert_eq!(skip_text(Some(&skipped(SkipReason::Busy))), Some("hole punching: the server's gate is busy"));
        assert_eq!(skip_text(Some(&skipped(SkipReason::Error))), Some("hole punching: the server could not prepare it"));
        assert_eq!(skip_text(None), Some("hole punching: the server could not prepare it"));
    }

    #[test]
    fn the_tunnel_note_joins_what_was_tried() {
        assert_eq!(
            tunnel_note(Some("no QUIC answer from 49.171.219.140:4317 within 3 s (is UDP 4317 open?)"), Some("hole punching: no STUN answer on the server")),
            "QUIC: no QUIC answer from 49.171.219.140:4317 within 3 s (is UDP 4317 open?); hole punching: no STUN answer on the server"
        );
        assert_eq!(tunnel_note(None, Some("hole punching: turned off (WESTERM_REMOTE_STUN)")), "hole punching: turned off (WESTERM_REMOTE_STUN)");
        assert_eq!(tunnel_note(None, None), "WESTERM_REMOTE_TRANSPORT=tunnel");
    }

    /// Real time, with wide margins: tokio's paused clock needs a feature this crate does not turn on.
    #[tokio::test]
    async fn direct_wins_when_both_work() {
        let after = |ms: u64, value: Result<&'static str, &'static str>| async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            value
        };
        let grace = Duration::from_millis(200);
        // A long grace where the point is "direct in time" or "no waiting".
        let long = Duration::from_secs(2);
        // Punched is first, direct comes within the grace.
        assert_eq!(race(Some(after(300, Ok("d"))), Some(after(10, Ok("p"))), long).await, Ok(("d", Won::Direct)));
        // Direct is too late: punched is used once the grace is over.
        let started = Instant::now();
        assert_eq!(race(Some(after(10_000, Ok("d"))), Some(after(10, Ok("p"))), grace).await, Ok(("p", Won::Punched)));
        assert!((Duration::from_millis(200)..Duration::from_secs(3)).contains(&started.elapsed()), "{:?}", started.elapsed());
        // Direct already failed: punched is used at once, without the grace.
        let started = Instant::now();
        assert_eq!(race(Some(after(5, Err("no"))), Some(after(30, Ok("p"))), long).await, Ok(("p", Won::Punched)));
        assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        assert_eq!(race(Some(after(10, Ok("d"))), Some(after(50, Err("no"))), grace).await, Ok(("d", Won::Direct)));
        assert_eq!(race(Some(after(10, Err("d-no"))), Some(after(50, Err("p-no"))), grace).await, Err((Some("d-no"), Some("p-no"))));
        let none: Option<std::future::Ready<Result<&str, &str>>> = None;
        assert_eq!(race(none, Some(after(50, Ok("p"))), grace).await, Ok(("p", Won::Punched)));
        let none: Option<std::future::Ready<Result<&str, &str>>> = None;
        assert_eq!(race(Some(after(10, Err("d-no"))), none, grace).await, Err((Some("d-no"), None)));
    }

    #[tokio::test]
    async fn prepare_says_why_it_cannot() {
        assert_eq!(prepare(&StunConfig::Off).await.err(), Some("hole punching: turned off (WESTERM_REMOTE_STUN)".to_owned()));
        let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let config = StunConfig::Servers(vec![silent.local_addr().unwrap().to_string()]);
        assert_eq!(prepare(&config).await.err(), Some("hole punching: no STUN answer on this computer".to_owned()));
    }
}
