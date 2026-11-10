//! The access code, cookie tokens, and the login limiter of one tunnel.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use axum::http::{header, HeaderMap};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// A fresh 6-digit access code from the OS random source, leading zeros kept.
pub fn new_code() -> String {
    // 4_294_000_000 is the largest multiple of 1_000_000 below 2^32: values
    // above it would make low codes slightly more likely.
    loop {
        let mut bytes = [0u8; 4];
        fill(&mut bytes);
        let value = u32::from_le_bytes(bytes);
        if value < 4_294_000_000 {
            return format!("{:06}", value % 1_000_000);
        }
    }
}

/// A fresh cookie token: 256 random bits as lowercase hex.
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn fill(bytes: &mut [u8]) {
    // Without the OS random source no code is safe to hand out.
    getrandom::fill(bytes).expect("the OS random source");
}

/// Compares every byte, so the time taken does not tell how much matched.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// Wrong codes allowed in a row before logins lock.
pub const FREE_TRIES: u32 = 5;
pub const FIRST_LOCK: Duration = Duration::from_secs(30);
pub const MAX_LOCK: Duration = Duration::from_secs(15 * 60);
/// Wrong codes in one tunnel that end it.
pub const MAX_FAILURES: u32 = 20;

/// The answer to a wrong code.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Retry,
    /// Every login is refused for this long.
    Locked(Duration),
    /// The tunnel must end.
    TooMany,
}

/// Counts wrong codes for the whole tunnel, not per address: every request
/// comes from cloudflared on 127.0.0.1, and only one person uses the tunnel.
#[derive(Default)]
pub struct Limiter {
    in_a_row: u32,
    total: u32,
    lock: Option<Duration>,
    locked_until: Option<Instant>,
}

impl Limiter {
    /// How long logins stay locked from `now`, if they are.
    pub fn locked(&self, now: Instant) -> Option<Duration> {
        self.locked_until.and_then(|until| until.checked_duration_since(now)).filter(|left| !left.is_zero())
    }

    pub fn fail(&mut self, now: Instant) -> Verdict {
        self.total += 1;
        if self.total >= MAX_FAILURES {
            return Verdict::TooMany;
        }
        self.in_a_row += 1;
        if self.in_a_row < FREE_TRIES {
            return Verdict::Retry;
        }
        let lock = self.lock.map_or(FIRST_LOCK, |lock| (lock * 2).min(MAX_LOCK));
        self.lock = Some(lock);
        self.locked_until = Some(now + lock);
        Verdict::Locked(lock)
    }

    /// A right code: the count in a row and the lock start over. The total stays.
    pub fn succeed(&mut self) {
        self.in_a_row = 0;
        self.lock = None;
        self.locked_until = None;
    }
}

pub const COOKIE: &str = "westerm_tunnel";

/// The tunnel cookie's value from any `Cookie` header.
pub fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE)
        .map(|(_, value)| value.to_owned())
}

/// The cookie lives for 30 days in the browser; the server forgets the token
/// when the tunnel ends, which is what really ends it.
pub fn set_cookie(token: &str) -> String {
    format!("{COOKIE}={token}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=2592000")
}

/// "iPhone · Safari": product names, the same in every language.
pub fn device_label(user_agent: &str) -> String {
    let os = [("iPhone", "iPhone"), ("iPad", "iPad"), ("Android", "Android"), ("Macintosh", "Mac"), ("Windows", "Windows"), ("Linux", "Linux")]
        .into_iter()
        .find(|(needle, _)| user_agent.contains(needle))
        .map_or("Unknown", |(_, name)| name);
    let browser = [("Edg/", "Edge"), ("Firefox/", "Firefox"), ("FxiOS", "Firefox"), ("Chrome/", "Chrome"), ("CriOS", "Chrome"), ("Safari/", "Safari")]
        .into_iter()
        .find(|(needle, _)| user_agent.contains(needle))
        .map_or("Unknown", |(_, name)| name);
    format!("{os} · {browser}")
}

struct Device {
    label: String,
    /// Milliseconds since the epoch.
    last_seen: u64,
    sockets: u32,
}

/// A signed-in device as the panel shows it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub label: String,
    pub last_seen: u64,
    pub connected: bool,
}

/// The tokens handed out in this tunnel. Only their SHA-256 is kept, so the
/// cookies themselves are nowhere in memory but the request being served.
#[derive(Default)]
pub struct Sessions {
    tokens: HashMap<[u8; 32], Device>,
}

fn key(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

impl Sessions {
    pub fn add(&mut self, token: &str, label: String, now_ms: u64) {
        self.tokens.insert(key(token), Device { label, last_seen: now_ms, sockets: 0 });
    }

    /// True for a token of this tunnel, whose device was seen now.
    pub fn touch(&mut self, token: &str, now_ms: u64) -> bool {
        match self.tokens.get_mut(&key(token)) {
            Some(device) => {
                device.last_seen = device.last_seen.max(now_ms);
                true
            }
            None => false,
        }
    }

    /// A terminal socket of this token opened (+1) or closed (-1).
    pub fn socket(&mut self, token: &str, delta: i32) {
        if let Some(device) = self.tokens.get_mut(&key(token)) {
            device.sockets = device.sockets.saturating_add_signed(delta);
        }
    }

    pub fn clear(&mut self) {
        self.tokens.clear();
    }

    /// Most recently seen first.
    pub fn devices(&self) -> Vec<DeviceView> {
        let mut list: Vec<DeviceView> = self
            .tokens
            .values()
            .map(|device| DeviceView { label: device.label.clone(), last_seen: device.last_seen, connected: device.sockets > 0 })
            .collect();
        list.sort_by_key(|device| std::cmp::Reverse(device.last_seen));
        list
    }

    /// Open terminal sockets of every device.
    pub fn sockets(&self) -> u32 {
        self.tokens.values().map(|device| device.sockets).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header, HeaderMap, HeaderValue};
    use std::time::{Duration, Instant};

    #[test]
    fn codes_are_six_digits() {
        for _ in 0..1000 {
            let code = new_code();
            assert_eq!(code.len(), 6, "{code}");
            assert!(code.bytes().all(|b| b.is_ascii_digit()), "{code}");
        }
    }

    #[test]
    fn tokens_are_64_hex() {
        let token = new_token();
        assert_eq!(token.len(), 64);
        assert!(token.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')), "{token}");
        assert_ne!(token, new_token());
    }

    #[test]
    fn same_compares_whole_slices() {
        assert!(same(b"482913", b"482913"));
        assert!(!same(b"482913", b"482914"));
        assert!(!same(b"48291", b"482913"));
    }

    #[test]
    fn five_failures_lock_for_30_seconds() {
        let t0 = Instant::now();
        let mut limiter = Limiter::default();
        for _ in 0..4 {
            assert_eq!(limiter.fail(t0), Verdict::Retry);
        }
        assert_eq!(limiter.fail(t0), Verdict::Locked(Duration::from_secs(30)));
        assert_eq!(limiter.locked(t0), Some(Duration::from_secs(30)));
        assert_eq!(limiter.locked(t0 + Duration::from_secs(31)), None);
    }

    #[test]
    fn each_failure_after_a_lock_doubles_it_up_to_15_minutes() {
        let t0 = Instant::now();
        let mut limiter = Limiter::default();
        for _ in 0..5 {
            limiter.fail(t0);
        }
        for secs in [60, 120, 240, 480, 900, 900] {
            assert_eq!(limiter.fail(t0), Verdict::Locked(Duration::from_secs(secs)));
        }
    }

    #[test]
    fn success_clears_the_lock_but_not_the_total() {
        let t0 = Instant::now();
        let mut limiter = Limiter::default();
        for _ in 0..5 {
            limiter.fail(t0);
        }
        limiter.succeed();
        assert_eq!(limiter.locked(t0), None);
        assert_eq!(limiter.fail(t0), Verdict::Retry);
        // 6 failures so far; the 20th in total ends the tunnel.
        for _ in 0..13 {
            assert_ne!(limiter.fail(t0), Verdict::TooMany);
        }
        assert_eq!(limiter.fail(t0), Verdict::TooMany);
    }

    #[test]
    fn twenty_failures_end_the_tunnel() {
        let t0 = Instant::now();
        let mut limiter = Limiter::default();
        for n in 1..=20 {
            let verdict = limiter.fail(t0);
            if n % 3 == 0 {
                limiter.succeed();
            }
            assert_eq!(verdict == Verdict::TooMany, n == 20, "failure {n}: {verdict:?}");
        }
    }

    #[test]
    fn cookie_token_reads_any_cookie_header() {
        let mut headers = HeaderMap::new();
        headers.append(header::COOKIE, HeaderValue::from_static("a=1; westerm_tunnel=abc; b=2"));
        assert_eq!(cookie_token(&headers).as_deref(), Some("abc"));

        let mut headers = HeaderMap::new();
        headers.append(header::COOKIE, HeaderValue::from_static("a=1"));
        headers.append(header::COOKIE, HeaderValue::from_static("westerm_tunnel=def"));
        assert_eq!(cookie_token(&headers).as_deref(), Some("def"));

        let mut headers = HeaderMap::new();
        headers.append(header::COOKIE, HeaderValue::from_static("westerm_tunnelx=1; other=2"));
        assert_eq!(cookie_token(&headers), None);
        assert_eq!(cookie_token(&HeaderMap::new()), None);
    }

    #[test]
    fn set_cookie_has_every_attribute() {
        let cookie = set_cookie("abc");
        assert!(cookie.starts_with("westerm_tunnel=abc;"), "{cookie}");
        for attribute in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/", "Max-Age=2592000"] {
            assert!(cookie.split("; ").any(|part| part == attribute), "{attribute} in {cookie}");
        }
    }

    #[test]
    fn device_labels() {
        let cases = [
            ("Mozilla/5.0 (iPhone; CPU iPhone OS 19_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/19.0 Mobile/15E148 Safari/604.1", "iPhone · Safari"),
            ("Mozilla/5.0 (iPhone; CPU iPhone OS 19_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/140.0 Mobile/15E148 Safari/604.1", "iPhone · Chrome"),
            ("Mozilla/5.0 (Linux; Android 16; Pixel 10) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Mobile Safari/537.36", "Android · Chrome"),
            ("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36", "Mac · Chrome"),
            ("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36 Edg/140.0", "Windows · Edge"),
            ("Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0", "Linux · Firefox"),
            ("", "Unknown · Unknown"),
        ];
        for (agent, label) in cases {
            assert_eq!(device_label(agent), label, "{agent}");
        }
    }

    #[test]
    fn sessions_hold_hashes_and_track_devices() {
        let mut sessions = Sessions::default();
        sessions.add("t1", "iPhone · Safari".to_owned(), 10);
        assert!(sessions.touch("t1", 20));
        assert!(!sessions.touch("nope", 20));
        sessions.socket("t1", 1);
        assert_eq!(
            sessions.devices(),
            vec![DeviceView { label: "iPhone · Safari".to_owned(), last_seen: 20, connected: true }]
        );
        assert_eq!(sessions.sockets(), 1);
        assert!(sessions.tokens.keys().all(|key| !key.windows(2).any(|w| w == b"t1")), "only hashes are kept");
        sessions.clear();
        assert!(!sessions.touch("t1", 30));
    }
}
