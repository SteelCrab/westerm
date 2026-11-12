//! `openBrowser`: which addresses a plugin may open in a browser pane. A
//! browser pane's requests carry the dashboard's origin, so opening the
//! dashboard itself would hand a plugin westerm's own powers. The host is
//! read the way a browser reads it, so another spelling of 127.0.0.1 or
//! localhost does not slip past.

use std::net::{Ipv4Addr, Ipv6Addr};

/// The reason for a scheme other than `http` or `https`.
pub(super) const NOT_HTTP: &str = "only http and https";
/// The reason for the dashboard's own address.
pub(super) const OWN_ADDRESS: &str = "this is westerm's own address";
/// The reason for an address a browser would not open.
pub(super) const INVALID: &str = "not a valid address";

/// Why `url` must not be opened, or `None` when it may. `origins` are the
/// dashboard's own origins (`http://127.0.0.1:<port>`, `http://localhost:<port>`,
/// and every `--allow-origin`). Refused: a loopback host on the port of any
/// loopback origin, any address whose origin is one of `origins`, and a name
/// this check cannot fold to ASCII (IDNA) on the port of a loopback origin
/// or on the scheme and port of any own origin: a browser may map it to
/// localhost or to an `--allow-origin` host.
pub(super) fn refused(url: &str, origins: &[String]) -> Option<&'static str> {
    let target = match parse(url) {
        Ok(target) => target,
        Err(reason) => return Some(reason),
    };
    let own: Vec<Address> = origins.iter().filter_map(|origin| parse(origin).ok()).collect();
    let guarded_port = own.iter().any(|origin| origin.host.is_loopback() && origin.port == target.port);
    let own_scheme_and_port = own.iter().any(|origin| origin.scheme == target.scheme && origin.port == target.port);
    let unchecked = matches!(target.host, Host::Unchecked(_));
    let refuse = (target.host.is_loopback() && guarded_port)
        || (unchecked && (guarded_port || own_scheme_and_port))
        || own.contains(&target);
    refuse.then_some(OWN_ADDRESS)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scheme {
    Http,
    Https,
}

impl Scheme {
    fn default_port(self) -> u16 {
        match self {
            Self::Http => 80,
            Self::Https => 443,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Host {
    /// A lower-case ASCII name without trailing dots.
    Domain(String),
    /// Also an IPv4-mapped IPv6 address.
    V4(Ipv4Addr),
    V6(Ipv6Addr),
    /// A name with characters this check does not fold (IDNA).
    Unchecked(String),
}

impl Host {
    /// The host reaches this computer: `localhost` and its subdomains,
    /// 127.0.0.0/8, ::1, and the "any address" 0.0.0.0 and ::, which connect
    /// to this computer too.
    fn is_loopback(&self) -> bool {
        match self {
            Self::Domain(name) => name == "localhost" || name.ends_with(".localhost"),
            Self::V4(ip) => ip.is_loopback() || ip.is_unspecified(),
            Self::V6(ip) => ip.is_loopback() || ip.is_unspecified() || ip.to_ipv4().is_some_and(|ip| ip.is_loopback() || ip.is_unspecified()),
            Self::Unchecked(_) => false,
        }
    }
}

/// An address's origin, the way a browser sees it.
#[derive(Debug, PartialEq, Eq)]
struct Address {
    scheme: Scheme,
    host: Host,
    port: u16,
}

/// Reads the origin of `url` the way a browser's URL parser does (WHATWG),
/// as far as it matters for the host and the port.
fn parse(url: &str) -> Result<Address, &'static str> {
    // A browser drops leading and trailing controls and spaces, and every
    // tab and newline inside.
    let url: String = url.trim_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    let (scheme, rest) = url.split_once(':').ok_or(NOT_HTTP)?;
    let scheme = match scheme.to_ascii_lowercase().as_str() {
        "http" => Scheme::Http,
        "https" => Scheme::Https,
        _ => return Err(NOT_HTTP),
    };
    // Any number of slashes or backslashes may follow; the authority ends at
    // the path, the query, or the fragment. Only what follows the last `@` is
    // the host.
    let rest = rest.trim_start_matches(['/', '\\']);
    let authority = rest.split(['/', '\\', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, after)| after);
    let (host, port) = split_port(host_port)?;
    let port = match port {
        None | Some("") => scheme.default_port(),
        Some(digits) => parse_port(digits)?,
    };
    Ok(Address { scheme, host: parse_host(host)?, port })
}

/// The host text (brackets kept) and the port text after `:`, if any.
fn split_port(host_port: &str) -> Result<(&str, Option<&str>), &'static str> {
    if host_port.starts_with('[') {
        let end = host_port.find(']').ok_or(INVALID)? + 1;
        return match &host_port[end..] {
            "" => Ok((&host_port[..end], None)),
            after => Ok((&host_port[..end], Some(after.strip_prefix(':').ok_or(INVALID)?))),
        };
    }
    Ok(match host_port.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (host_port, None),
    })
}

/// Decimal digits only, leading zeros allowed, at most 65535.
fn parse_port(digits: &str) -> Result<u16, &'static str> {
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(INVALID);
    }
    let significant = digits.trim_start_matches('0');
    if significant.is_empty() {
        return Ok(0);
    }
    if significant.len() > 5 {
        return Err(INVALID);
    }
    significant.parse().map_err(|_| INVALID)
}

fn parse_host(text: &str) -> Result<Host, &'static str> {
    if let Some(inner) = text.strip_prefix('[') {
        let ip: Ipv6Addr = inner.strip_suffix(']').ok_or(INVALID)?.parse().map_err(|_| INVALID)?;
        return Ok(ip.to_ipv4_mapped().map_or(Host::V6(ip), Host::V4));
    }
    let decoded = percent_decode(text);
    let name: String = String::from_utf8_lossy(&decoded).chars().map(fold_width).collect::<String>().to_lowercase();
    if name.chars().any(forbidden) {
        return Err(INVALID);
    }
    // `localhost.` and `127.0.0.1.` name the same host.
    let name = name.trim_end_matches('.');
    if name.is_empty() {
        return Err(INVALID);
    }
    if !name.is_ascii() {
        return Ok(Host::Unchecked(name.to_owned()));
    }
    // A name whose last label is a number is an IPv4 address, or invalid.
    let last = name.rsplit('.').next().unwrap_or_default();
    if last.bytes().all(|byte| byte.is_ascii_digit()) || ipv4_number(last).is_some() {
        return parse_ipv4(name).map(Host::V4).ok_or(INVALID);
    }
    Ok(Host::Domain(name.to_owned()))
}

/// `%XX` escapes become bytes; a `%` without two hex digits stays.
fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = (bytes[at] == b'%').then(|| bytes.get(at + 1..at + 3)).flatten().and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    out
}

/// Full-width ASCII and the ideographic full stops, as IDNA maps them.
fn fold_width(c: char) -> char {
    match c {
        '\u{ff01}'..='\u{ff5e}' => char::from_u32(u32::from(c) - 0xfee0).unwrap_or(c),
        '\u{3002}' | '\u{ff61}' => '.',
        _ => c,
    }
}

/// A character no host may hold.
fn forbidden(c: char) -> bool {
    c <= ' ' || c == '\u{7f}' || matches!(c, '#' | '%' | '/' | ':' | '<' | '>' | '?' | '@' | '[' | '\\' | ']' | '^' | '|')
}

/// One to four numbers, each decimal, `0x` hex, or `0` octal. The last
/// fills the bytes that are left: `127.1` is 127.0.0.1, `2130706433` too.
fn parse_ipv4(name: &str) -> Option<Ipv4Addr> {
    let parts: Vec<u64> = name.split('.').map(ipv4_number).collect::<Option<_>>()?;
    let (last, first) = parts.split_last()?;
    if parts.len() > 4 || first.iter().any(|part| *part > 255) || *last >= 1_u64 << (8 * (5 - parts.len())) {
        return None;
    }
    let value = first.iter().enumerate().fold(*last, |value, (at, part)| value + (part << (8 * (3 - at))));
    u32::try_from(value).ok().map(Ipv4Addr::from)
}

/// One IPv4 number; `None` for an empty part or a bad digit.
fn ipv4_number(part: &str) -> Option<u64> {
    let (digits, radix) = match part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
        Some(hex) => (hex, 16),
        None if part.len() > 1 && part.starts_with('0') => (&part[1..], 8),
        None => (part, 10),
    };
    if digits.is_empty() {
        return (radix == 16).then_some(0);
    }
    if !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    u64::from_str_radix(digits, radix).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origins() -> Vec<String> {
        ["http://127.0.0.1:4317", "http://localhost:4317", "http://localhost:5173"].map(String::from).to_vec()
    }

    #[test]
    fn dashboard_aliases_are_refused() {
        let origins = origins();
        for url in [
            "http://127.0.0.1:4317/",
            "http://localhost:4317/api/workspace",
            "http://127.1:4317",
            "http://0x7f.0.0.1:4317",
            "http://2130706433:4317",
            "http://LOCALHOST.:4317",
            "http://[::ffff:127.0.0.1]:4317",
            "http://[::1]:4317",
            "http://0.0.0.0:4317",
            "http://app.localhost:4317",
            "http://127.0.0.2:4317",
            "http://localhost:5173/",
            "https://localhost:4317",
        ] {
            assert!(refused(url, &origins).is_some(), "{url}");
        }
        for url in ["http://localhost:3000", "http://127.0.0.1:8080/x", "https://example.com", "http://example.com:4317"] {
            assert_eq!(refused(url, &origins), None, "{url}");
        }
        for url in ["javascript:alert(1)", "file:///etc/passwd", "ftp://x"] {
            assert!(refused(url, &origins).is_some(), "{url}");
        }
    }

    #[test]
    fn more_spellings_a_browser_accepts_are_refused() {
        let origins = origins();
        for url in [
            // Octal, hex without dots, a trailing dot on an IP, a zero-padded port.
            "http://0177.0.0.1:4317",
            "http://0x7f000001:4317",
            "http://127.0.0.1.:4317",
            "http://127.0.0.1:04317",
            // Percent-escapes in the host, upper-case hex, user info.
            "http://%6c%6fcalhost:4317",
            "http://user:pw@127.0.0.1:4317",
            "http://evil.example@localhost:4317/",
            // Backslashes and missing or extra slashes, tabs and newlines inside.
            "http:\\\\127.0.0.1:4317",
            "http:127.0.0.1:4317",
            "http:///localhost:4317",
            "ht\ttp://loc\nalhost:4317",
            "  HTTP://LocalHost:4317  ",
            // IPv6 spellings of loopback and of "this computer".
            "http://[0:0:0:0:0:0:0:1]:4317",
            "http://[::]:4317",
            "http://[::ffff:7f00:1]:4317",
            "http://[0:0:0:0:0:ffff:127.0.0.1]:4317",
            // Full-width characters a browser maps to ASCII.
            "http://\u{ff4c}\u{ff4f}\u{ff43}\u{ff41}\u{ff4c}\u{ff48}\u{ff4f}\u{ff53}\u{ff54}:4317",
            "http://127\u{3002}0\u{3002}0\u{3002}1:4317",
        ] {
            assert!(refused(url, &origins).is_some(), "{url}");
        }
    }

    #[test]
    fn an_allowed_origin_on_another_host_is_refused() {
        let mut origins = origins();
        origins.push("http://devbox.lan:8080".into());
        origins.push("https://dash.example".into());
        for url in ["http://devbox.lan:8080/", "http://DEVBOX.LAN.:8080", "https://dash.example:443/x", "https://dash.example"] {
            assert_eq!(refused(url, &origins), Some(OWN_ADDRESS), "{url}");
        }
        for url in ["http://devbox.lan:8081", "https://devbox.lan:8080", "http://dash.example"] {
            assert_eq!(refused(url, &origins), None, "{url}");
        }
    }

    #[test]
    fn unfolded_spellings_of_an_allowed_origin_are_refused() {
        let mut origins = origins();
        origins.push("https://dash.example".into());
        origins.push("http://devbox.lan:8080".into());
        // A browser maps U+217E (small roman numeral 500) to `d` and drops a
        // soft hyphen (IDNA), so both reach dash.example; this check does not
        // fold them, so a name it cannot check on an own scheme and port is refused.
        for url in [
            "https://\u{217e}ash.example/",
            "https://da\u{ad}sh.example/",
            "https://dash.exampl\u{0435}/",
            "http://devb\u{ad}ox.lan:8080/",
        ] {
            assert_eq!(refused(url, &origins), Some(OWN_ADDRESS), "{url}");
        }
        // The same names on another scheme or port are not an own origin.
        for url in ["http://\u{217e}ash.example/", "https://\u{217e}ash.example:8443/", "https://devb\u{ad}ox.lan:8080/"] {
            assert_eq!(refused(url, &origins), None, "{url}");
        }
    }

    #[test]
    fn reasons_say_what_is_wrong() {
        let origins = origins();
        assert_eq!(refused("ftp://x", &origins), Some(NOT_HTTP));
        assert_eq!(refused("example.com", &origins), Some(NOT_HTTP), "no scheme");
        assert_eq!(refused("http://127.0.0.1:4317", &origins), Some(OWN_ADDRESS));
        for url in ["http://", "http://:4317", "http://localhost:99999", "http://localhost:43a7", "http://[::1", "http://1.2.3.4.5", "http://256.0.0.0.1", "http://0x7g.1", "http://foo.09"] {
            assert_eq!(refused(url, &origins), Some(INVALID), "{url}");
        }
        assert_eq!(refused("http://example.com:80/", &origins), None);
        assert_eq!(refused("http://\u{4f8b}\u{3048}.jp/", &origins), None, "a non-ASCII name on another port is fine");
        assert!(refused("http://\u{4f8b}\u{3048}.jp:4317/", &origins).is_some(), "a name that cannot be checked on a dashboard port is not");
    }
}
