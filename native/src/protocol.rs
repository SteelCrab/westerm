//! Messages and validation shared by the dashboard server, the PTY host, and
//! (through the same JSON) the browser.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;

/// Output flow control: the host pauses reading a PTY while this many bytes
/// wait for the browser's acknowledgment, and resumes below the low mark.
pub const FLOW_HIGH_WATER: u64 = 8 * 1024 * 1024;
pub const FLOW_LOW_WATER: u64 = 2 * 1024 * 1024;

/// Another window took this session, or every session; the old one must not
/// reconnect until it is used again.
pub const CLOSE_TAKEN_OVER: u16 = 4001;

/// The rules between a dashboard and a host: attach queries and the JSON
/// messages. Raised only when they change, so a dashboard can keep using an
/// older remote host that speaks the same rules. The host reports it in /health.
pub const PROTOCOL: u32 = 1;

pub const MAX_COLS: u16 = 1024;
pub const MAX_ROWS: u16 = 512;

/// A remote westerm failed in a way that reconnecting does not fix.
pub const CLOSE_REMOTE_FAILED: u16 = 4003;

/// remote: a shell on a server's own westerm host, reached through the
/// dashboard over QUIC (or an SSH tunnel). The local host never runs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    Shell,
    Browser,
    Editor,
    Ssh,
    Remote,
}

impl SessionKind {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "shell" => Some(Self::Shell),
            "browser" => Some(Self::Browser),
            "editor" => Some(Self::Editor),
            "ssh" => Some(Self::Ssh),
            "remote" => Some(Self::Remote),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Browser => "browser",
            Self::Editor => "editor",
            Self::Ssh => "ssh",
            Self::Remote => "remote",
        }
    }

    /// Kinds whose process outlives its viewer and whose screen is replayed.
    /// The others draw with Kitty images and restart on every attach.
    pub fn is_text(self) -> bool {
        matches!(self, Self::Shell | Self::Ssh | Self::Remote)
    }
}

/// How the dashboard reaches a remote westerm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteVia {
    Quic,
    Tunnel,
}

/// Why a remote pane could not connect. The browser shows each in its language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteFailure {
    /// SSH refused the key (or wanted a password).
    Auth,
    /// The server's host key is not in known_hosts.
    HostKey,
    Unreachable,
    /// Not Linux, or a CPU without a release build.
    Unsupported,
    Download,
    /// A source build has no release to install; WESTERM_REMOTE_BINARY_DIR is needed.
    DevBinary,
    NewerRemote,
    /// The user kept an older remote host that speaks another protocol.
    Incompatible,
    /// Neither QUIC nor an SSH tunnel connected.
    Transport,
    Setup,
}

impl RemoteFailure {
    /// False for failures that a later try can get past on its own.
    pub fn is_permanent(self) -> bool {
        !matches!(self, Self::Unreachable | Self::Transport)
    }
}

/// Where a remote pane is while the dashboard prepares the server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "stage", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RemoteStage {
    Checking,
    Installing { version: String },
    Starting,
    Connecting,
    /// The server's host is an older build with running shells: replace it?
    Confirm { sessions: usize },
    Connected { via: RemoteVia, kill_on_logout: bool },
    Failed { reason: RemoteFailure, detail: String },
}

/// Grid and pixel size. Kitty clients read the pixel size with TIOCGWINSZ.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
    pub width: u16,
    pub height: u16,
}

impl PtySize {
    pub fn is_valid(&self) -> bool {
        (1..=MAX_COLS).contains(&self.cols)
            && (1..=MAX_ROWS).contains(&self.rows)
            && self.width >= 1
            && self.height >= 1
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ClientMessage {
    Input { data: String },
    Resize { cols: u16, rows: u16, width: u16, height: u16 },
    Ack { bytes: u64 },
    /// The user closed the pane: end the process and forget the session.
    Close,
}

impl ClientMessage {
    pub fn parse(text: &str) -> Option<Self> {
        let message: Self = serde_json::from_str(text).ok()?;
        match message {
            Self::Resize { cols, rows, width, height } if !(PtySize { cols, rows, width, height }).is_valid() => None,
            other => Some(other),
        }
    }
}

/// new: a fresh process. reattached: the same live process, screen replayed.
/// revived: a new process after the old one ended, saved screen restored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachMode {
    New,
    Reattached,
    Revived,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ServerMessage {
    Ready { pid: u32, mode: AttachMode },
    Exit { code: i64, signal: i64 },
    Error { message: String },
    /// Sent by the dashboard, never the host, while a remote pane connects.
    Remote(RemoteStage),
}

impl ServerMessage {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("server messages serialize")
    }
}

/// Client-made ids: a letter prefix plus base36. They also name files.
pub fn is_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    (2..=41).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Only http(s) URLs or bare hosts reach terminal-browser.
pub fn normalize_browser_url(input: &str) -> Option<String> {
    let text = input.trim();
    if text.is_empty() || text.len() > 2048 || text.chars().any(|c| c.is_control()) {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(text.to_owned());
    }
    // "host:port" is fine; any other scheme (file:, javascript:, ...) is not.
    if let Some(colon) = text.find(':') {
        let scheme = &text[..colon];
        let looks_like_scheme = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c));
        let port_follows = text[colon + 1..].starts_with(|c: char| c.is_ascii_digit());
        if looks_like_scheme && !port_follows {
            return None;
        }
    }
    if text.starts_with('-') {
        return None;
    }
    Some(text.to_owned())
}

/// An ssh destination: a config alias or [user@]host. No leading dash, no
/// spaces or shell characters; commands also pass it after "--".
pub fn normalize_ssh_target(input: &str) -> Option<String> {
    let text = input.trim();
    if text.is_empty() || text.len() > 255 {
        return None;
    }
    let (user, host) = match text.split_once('@') {
        Some((user, host)) => (user, Some(host)),
        None => (text, None),
    };
    let user_ok = !user.is_empty()
        && !user.starts_with('-')
        && user.chars().all(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c));
    let host_ok = host.is_none_or(|host| {
        !host.is_empty()
            && !host.starts_with('-')
            && host.chars().all(|c| c.is_ascii_alphanumeric() || "._:[]-".contains(c))
    });
    (user_ok && host_ok).then(|| text.to_owned())
}

/// What a pane asks the host for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachRequest {
    pub id: String,
    pub kind: SessionKind,
    pub url: Option<String>,
    /// ssh destination: the remote shell, or the proxy for a remote browser.
    pub target: Option<String>,
    /// Start in this session's directory (the focused shell), for an editor.
    pub cwd_from: Option<String>,
    pub size: PtySize,
    /// UI language for text the host writes (restore notice, errors).
    pub lang: Lang,
    /// The page that asks. One window at a time owns every session.
    pub window: Option<String>,
}

impl AttachRequest {
    pub fn from_query(query: &HashMap<String, String>) -> Option<Self> {
        let id = query.get("session").filter(|id| is_id(id))?.clone();
        let kind = SessionKind::parse(query.get("kind")?)?;
        let number = |key: &str| query.get(key)?.parse::<u16>().ok();
        let size = PtySize {
            cols: number("cols")?,
            rows: number("rows")?,
            width: number("width")?,
            height: number("height")?,
        };
        if !size.is_valid() {
            return None;
        }
        let cwd_from = match query.get("cwdFrom") {
            Some(id) if is_id(id) => Some(id.clone()),
            Some(_) => return None,
            None => None,
        };
        let target = match query.get("target") {
            Some(raw) => Some(normalize_ssh_target(raw)?),
            None => None,
        };
        if matches!(kind, SessionKind::Ssh | SessionKind::Remote) && target.is_none() {
            return None;
        }
        let url = if kind == SessionKind::Browser {
            query.get("url").and_then(|url| normalize_browser_url(url))
        } else {
            None
        };
        let lang = Lang::parse(query.get("lang").map(String::as_str));
        let window = match query.get("window") {
            Some(id) if is_id(id) => Some(id.clone()),
            Some(_) => return None,
            None => None,
        };
        Some(Self { id, kind, url, target, cwd_from, size, lang, window })
    }

    /// What the dashboard asks a remote host for: a plain shell there, with
    /// the same id, size, language, and window.
    pub fn for_remote_host(&self) -> AttachRequest {
        AttachRequest {
            id: self.id.clone(),
            kind: SessionKind::Shell,
            url: None,
            target: None,
            cwd_from: None,
            size: self.size,
            lang: self.lang,
            // The server's host follows the same one-window rule as the local one.
            window: self.window.clone(),
        }
    }

    /// The same request as a query string, from validated fields only.
    pub fn to_query(&self) -> String {
        let mut pairs = vec![
            ("session", self.id.clone()),
            ("kind", self.kind.as_str().to_owned()),
            ("cols", self.size.cols.to_string()),
            ("rows", self.size.rows.to_string()),
            ("width", self.size.width.to_string()),
            ("height", self.size.height.to_string()),
            ("lang", self.lang.as_str().to_owned()),
        ];
        if let Some(url) = &self.url {
            pairs.push(("url", url.clone()));
        }
        if let Some(target) = &self.target {
            pairs.push(("target", target.clone()));
        }
        if let Some(cwd_from) = &self.cwd_from {
            pairs.push(("cwdFrom", cwd_from.clone()));
        }
        if let Some(window) = &self.window {
            pairs.push(("window", window.clone()));
        }
        pairs
            .into_iter()
            .map(|(key, value)| format!("{key}={}", percent_encode(&value)))
            .collect::<Vec<_>>()
            .join("&")
    }
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_name_files_safely() {
        assert!(is_id("smw26lu0mc"));
        for bad in ["", "s", "../x", "Sabc", "s-1", &"s".repeat(42)] {
            assert!(!is_id(bad), "{bad}");
        }
    }

    #[test]
    fn client_messages_are_validated() {
        assert_eq!(ClientMessage::parse(r#"{"type":"input","data":"ls\r"}"#), Some(ClientMessage::Input { data: "ls\r".into() }));
        assert_eq!(ClientMessage::parse(r#"{"type":"close"}"#), Some(ClientMessage::Close));
        assert_eq!(ClientMessage::parse(r#"{"type":"resize","cols":0,"rows":24,"width":1,"height":1}"#), None);
        assert_eq!(ClientMessage::parse(r#"{"type":"ack","bytes":-1}"#), None);
        assert_eq!(ClientMessage::parse("not json"), None);
    }

    #[test]
    fn browser_urls_allow_web_addresses_only() {
        for ok in ["https://example.com", "example.com", "localhost:3000", "example.com:8080/x"] {
            assert!(normalize_browser_url(ok).is_some(), "{ok}");
        }
        for bad in ["javascript:alert(1)", "file:///etc/passwd", "--split", "a\nb", ""] {
            assert!(normalize_browser_url(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn ssh_targets_reject_options_and_shell_syntax() {
        for ok in ["prod", "deploy@10.0.0.5", "me@build-box.local", "u@[::1]"] {
            assert_eq!(normalize_ssh_target(ok).as_deref(), Some(ok));
        }
        for bad in ["-oProxyCommand=evil", "a b", "host;rm", "$(id)", "me@-x", ""] {
            assert!(normalize_ssh_target(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn attach_requests_round_trip_through_queries() {
        let query: HashMap<String, String> = [
            ("session", "sabc1"),
            ("kind", "browser"),
            ("cols", "80"),
            ("rows", "24"),
            ("width", "640"),
            ("height", "408"),
            ("url", "localhost:3000"),
            ("target", "prod"),
            ("lang", "ja"),
            ("window", "wabc1"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let request = AttachRequest::from_query(&query).unwrap();
        assert_eq!(request.url.as_deref(), Some("localhost:3000"));
        assert!(request.to_query().contains("url=localhost%3A3000"));
        assert_eq!(request.lang, Lang::Ja);
        assert!(request.to_query().contains("lang=ja"));
        assert_eq!(request.window.as_deref(), Some("wabc1"));
        assert!(request.to_query().contains("window=wabc1"));
        let mut bad_window = query.clone();
        bad_window.insert("window".into(), "../x".into());
        assert!(AttachRequest::from_query(&bad_window).is_none(), "window ids are checked");
        let mut ssh = query.clone();
        ssh.insert("kind".into(), "ssh".into());
        ssh.remove("target");
        assert!(AttachRequest::from_query(&ssh).is_none(), "ssh needs a destination");
    }

    fn shell_query() -> HashMap<String, String> {
        [("session", "sabc1"), ("kind", "shell"), ("cols", "80"), ("rows", "24"), ("width", "640"), ("height", "408"), ("lang", "ko")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }

    #[test]
    fn remote_needs_a_destination_and_becomes_a_plain_shell_remotely() {
        let mut q = shell_query();
        q.insert("kind".into(), "remote".into());
        assert!(AttachRequest::from_query(&q).is_none(), "remote needs a destination");
        q.insert("target".into(), "prod".into());
        q.insert("cwdFrom".into(), "sother1".into());
        q.insert("window".into(), "wabc1".into());
        let request = AttachRequest::from_query(&q).unwrap();
        assert_eq!(request.kind, SessionKind::Remote);
        let remote = request.for_remote_host();
        assert_eq!((remote.kind, remote.target.clone(), remote.url.clone(), remote.cwd_from.clone()), (SessionKind::Shell, None, None, None));
        assert_eq!((remote.id.as_str(), remote.size, remote.lang), (request.id.as_str(), request.size, request.lang));
        assert!(remote.to_query().contains("kind=shell") && !remote.to_query().contains("target"));
        // The server's host follows the same one-window rule as the local one.
        assert_eq!(remote.window.as_deref(), Some("wabc1"));
        assert!(remote.to_query().contains("window=wabc1"));
    }

    #[test]
    fn remote_stages_serialize_for_the_browser() {
        let json = |stage| ServerMessage::Remote(stage).to_json();
        assert_eq!(
            json(RemoteStage::Connected { via: RemoteVia::Quic, kill_on_logout: false }),
            r#"{"type":"remote","stage":"connected","via":"quic","killOnLogout":false}"#
        );
        assert_eq!(
            json(RemoteStage::Failed { reason: RemoteFailure::HostKey, detail: "x".into() }),
            r#"{"type":"remote","stage":"failed","reason":"hostKey","detail":"x"}"#
        );
        assert_eq!(json(RemoteStage::Installing { version: "0.2.0".into() }), r#"{"type":"remote","stage":"installing","version":"0.2.0"}"#);
        assert_eq!(json(RemoteStage::Confirm { sessions: 3 }), r#"{"type":"remote","stage":"confirm","sessions":3}"#);
        assert_eq!(json(RemoteStage::Checking), r#"{"type":"remote","stage":"checking"}"#);
    }

    #[test]
    fn only_transient_remote_failures_retry() {
        assert!(!RemoteFailure::Unreachable.is_permanent() && !RemoteFailure::Transport.is_permanent());
        assert!(RemoteFailure::Auth.is_permanent() && RemoteFailure::NewerRemote.is_permanent());
    }
}
