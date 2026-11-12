//! Messages and validation shared by the dashboard server, the PTY host, and
//! (through the same JSON) the browser.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;
use crate::plugins::PluginLaunch;

/// Output flow control: the host pauses reading a PTY while this many bytes
/// wait for the browser's acknowledgment, and resumes below the low mark.
pub const FLOW_HIGH_WATER: u64 = 8 * 1024 * 1024;
pub const FLOW_LOW_WATER: u64 = 2 * 1024 * 1024;

/// Another window took this session, or every session; the old one must not
/// reconnect until it is used again.
pub const CLOSE_TAKEN_OVER: u16 = 4001;

/// WebSocket `?target=`: a remote window's status stream ("hello", then
/// each stage). The dashboard keeps the link up while a window watches it.
pub const REMOTE_LINK_PATH: &str = "/api/remote/link";
/// POST `{target}`: try again after a permanent failure.
pub const REMOTE_RETRY_PATH: &str = "/api/remote/retry";

/// The rules between a dashboard and a host: attach queries and the JSON
/// messages. Raised only when they change, so a dashboard can keep using an
/// older remote host that speaks the same rules. The host reports it in /health.
pub const PROTOCOL: u32 = 1;

pub const MAX_COLS: u16 = 1024;
pub const MAX_ROWS: u16 = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    Shell,
    Browser,
    Editor,
    Ssh,
}

impl SessionKind {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "shell" => Some(Self::Shell),
            "browser" => Some(Self::Browser),
            "editor" => Some(Self::Editor),
            "ssh" => Some(Self::Ssh),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Browser => "browser",
            Self::Editor => "editor",
            Self::Ssh => "ssh",
        }
    }

    /// Kinds whose process outlives its viewer and whose screen is replayed.
    /// The others draw with Kitty images and restart on every attach.
    pub fn is_text(self) -> bool {
        matches!(self, Self::Shell | Self::Ssh)
    }
}

/// How the dashboard reaches a remote westerm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteVia {
    Quic,
    Tunnel,
}

/// Why the dashboard could not connect to a remote westerm. The browser shows each in its language.
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

/// Where a remote connection is while the dashboard prepares the server.
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
    /// The page drew one pixel update; the host may send the next.
    Painted,
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

/// An AI agent in a shell pane; see host/agent.rs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
    Gemini,
}

impl AgentKind {
    /// Its JSON name, which is also its `agent` query value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "gemini" => Some(Self::Gemini),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Working,
    /// Asks the user: a permission or approval question.
    Waiting,
    Idle,
}

/// The agent a pane shows, if any, with what its info card shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AgentState {
    pub kind: AgentKind,
    pub status: AgentStatus,
    /// When this status began, in milliseconds since the epoch.
    pub since: u64,
    /// When the host first saw this agent, in milliseconds since the epoch.
    pub started: u64,
    /// Its conversation id, when it tells it.
    pub conversation: Option<String>,
    /// The command a restored shell types to resume it.
    pub resume: String,
    /// Its current folder.
    pub cwd: Option<String>,
    /// Its version, when it tells it (Claude Code).
    pub version: Option<String>,
    /// The model its command line names.
    pub model: Option<String>,
}

/// A shell's current folder and the git branch there, if any.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Place {
    pub cwd: String,
    pub branch: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ServerMessage {
    /// `pixels`: graphics come as pixel updates, and every binary message
    /// starts with a kind byte (see pixels/mod.rs).
    Ready {
        pid: u32,
        mode: AttachMode,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        pixels: bool,
    },
    /// After `ready` and on every change in a shell: the agent in its foreground.
    Agent { agent: Option<AgentState> },
    /// After `ready` and when the folder or branch changes in a shell.
    Place { place: Option<Place> },
    Exit { code: i64, signal: i64 },
    Error { message: String },
    /// The first message on a remote window's status stream: the local
    /// westerm's version line, so a window can tell that westerm changed.
    Hello { version: String },
    /// On a remote window's status stream: where the link to the server is.
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

/// How a pane takes pixel updates from a pixel app: raw rectangles for the
/// local dashboard, deflated ones for a remote window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelCodec {
    Raw,
    Deflate,
}

impl PixelCodec {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "raw" => Some(Self::Raw),
            "deflate" => Some(Self::Deflate),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Deflate => "deflate",
        }
    }
}

/// An agent a new shell starts by typing its command: a fresh conversation,
/// or the one with this id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub kind: AgentKind,
    pub conversation: Option<String>,
}

/// A conversation id an agent prints, as the hex-and-dash text it is made of.
pub fn is_conversation_id(text: &str) -> bool {
    (8..=64).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// A start folder: absolute, no NUL, at most 4096 bytes. A `/` path counts on
/// every OS: a remote window on Windows relays folders from a Unix server, and
/// the host that starts the shell checks the folder on its own OS.
fn is_start_folder(text: &str) -> bool {
    text.len() <= 4096 && !text.contains('\0') && (text.starts_with('/') || Path::new(text).is_absolute())
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
    /// Start a new shell in this folder.
    pub cwd: Option<String>,
    /// Type this agent's command into a new shell.
    pub launch: Option<Launch>,
    /// Type this plugin's command into a new shell.
    pub plugin: Option<PluginLaunch>,
    pub size: PtySize,
    /// UI language for text the host writes (restore notice, errors).
    pub lang: Lang,
    /// The page that asks. One window at a time owns every session.
    pub window: Option<String>,
    /// A restored shell resumes the AI agent it ran (`resume=off` turns it off).
    pub resume: bool,
    /// The page takes pixel updates. An unknown codec reads as none, so a
    /// newer page still gets the Kitty images it can draw.
    pub pixels: Option<PixelCodec>,
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
        let cwd = match query.get("cwd") {
            Some(path) if is_start_folder(path) => Some(path.clone()),
            Some(_) => return None,
            None => None,
        };
        let conversation = match query.get("conversation") {
            Some(id) if is_conversation_id(id) => Some(id.clone()),
            Some(_) => return None,
            None => None,
        };
        let launch = match query.get("agent") {
            Some(name) if kind == SessionKind::Shell => {
                Some(Launch { kind: AgentKind::parse(name)?, conversation })
            }
            Some(_) => return None,
            None if conversation.is_some() => return None,
            None => None,
        };
        // `plugin`, `action`, and `target` come together: a plugin launch is
        // for a new shell and reads `target` as its own, never as an ssh destination.
        // An external plugin's target is its argv as JSON (plugins/external.rs).
        let plugin = match (query.get("plugin"), query.get("action")) {
            (Some(plugin), Some(action)) if kind == SessionKind::Shell && launch.is_none() => {
                Some(PluginLaunch::parse(plugin, action, query.get("target")?)?)
            }
            (None, None) => None,
            _ => return None,
        };
        let target = match query.get("target") {
            Some(_) if plugin.is_some() => None,
            Some(raw) => Some(normalize_ssh_target(raw)?),
            None => None,
        };
        if kind == SessionKind::Ssh && target.is_none() {
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
        let resume = query.get("resume").map(String::as_str) != Some("off");
        let pixels = query.get("pixels").and_then(|codec| PixelCodec::parse(codec));
        Some(Self { id, kind, url, target, cwd_from, cwd, launch, plugin, size, lang, window, resume, pixels })
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
        if let Some(cwd) = &self.cwd {
            pairs.push(("cwd", cwd.clone()));
        }
        if let Some(launch) = &self.launch {
            pairs.push(("agent", launch.kind.as_str().to_owned()));
            if let Some(conversation) = &launch.conversation {
                pairs.push(("conversation", conversation.clone()));
            }
        }
        if let Some(plugin) = &self.plugin {
            pairs.push(("plugin", plugin.plugin().to_owned()));
            pairs.push(("action", plugin.action().to_owned()));
            pairs.push(("target", plugin.target()));
        }
        if let Some(window) = &self.window {
            pairs.push(("window", window.clone()));
        }
        if !self.resume {
            pairs.push(("resume", "off".to_owned()));
        }
        if let Some(pixels) = self.pixels {
            pairs.push(("pixels", pixels.as_str().to_owned()));
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

    fn percent_decode(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                out.push(u8::from_str_radix(&text[i + 1..i + 3], 16).unwrap());
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }

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
        assert!(request.resume && !request.to_query().contains("resume"), "on unless turned off");
        let mut off = query.clone();
        off.insert("resume".into(), "off".into());
        let off = AttachRequest::from_query(&off).unwrap();
        assert!(!off.resume && off.to_query().ends_with("&resume=off"));
        let mut bad_window = query.clone();
        bad_window.insert("window".into(), "../x".into());
        assert!(AttachRequest::from_query(&bad_window).is_none(), "window ids are checked");
        let mut ssh = query.clone();
        ssh.insert("kind".into(), "ssh".into());
        ssh.remove("target");
        assert!(AttachRequest::from_query(&ssh).is_none(), "ssh needs a destination");
    }

    #[test]
    fn attach_requests_carry_the_pixel_codec() {
        let mut query = shell_query();
        query.insert("kind".into(), "browser".into());
        assert_eq!(AttachRequest::from_query(&query).unwrap().pixels, None);
        query.insert("pixels".into(), "deflate".into());
        let request = AttachRequest::from_query(&query).unwrap();
        assert_eq!(request.pixels, Some(PixelCodec::Deflate));
        assert!(request.to_query().contains("pixels=deflate"));
        // A codec this host does not know: the old way, not a refusal.
        query.insert("pixels".into(), "zstd".into());
        assert_eq!(AttachRequest::from_query(&query).unwrap().pixels, None);
    }

    #[test]
    fn pixel_messages_serialize_and_parse() {
        assert_eq!(ClientMessage::parse(r#"{"type":"painted"}"#), Some(ClientMessage::Painted));
        let ready = ServerMessage::Ready { pid: 7, mode: AttachMode::New, pixels: true }.to_json();
        assert_eq!(ready, r#"{"type":"ready","pid":7,"mode":"new","pixels":true}"#);
        let old = ServerMessage::Ready { pid: 7, mode: AttachMode::New, pixels: false }.to_json();
        assert_eq!(old, r#"{"type":"ready","pid":7,"mode":"new"}"#, "older pages see the message they know");
    }

    fn shell_query() -> HashMap<String, String> {
        [("session", "sabc1"), ("kind", "shell"), ("cols", "80"), ("rows", "24"), ("width", "640"), ("height", "408"), ("lang", "ko")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }

    #[test]
    fn remote_is_no_longer_a_session_kind() {
        assert_eq!(SessionKind::parse("remote"), None);
        let mut q = shell_query();
        q.insert("kind".into(), "remote".into());
        q.insert("target".into(), "prod".into());
        assert_eq!(AttachRequest::from_query(&q), None);
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
        let agent = AgentState {
            kind: AgentKind::Claude,
            status: AgentStatus::Waiting,
            since: 2,
            started: 1,
            conversation: None,
            resume: "claude --continue".into(),
            cwd: Some("/work".into()),
            version: None,
            model: None,
        };
        assert_eq!(
            ServerMessage::Agent { agent: Some(agent) }.to_json(),
            r#"{"type":"agent","agent":{"kind":"claude","status":"waiting","since":2,"started":1,"conversation":null,"resume":"claude --continue","cwd":"/work","version":null,"model":null}}"#
        );
        assert_eq!(ServerMessage::Agent { agent: None }.to_json(), r#"{"type":"agent","agent":null}"#);
        let hello = ServerMessage::Hello { version: "westerm 0.3.0 (host rs-a)".into() }.to_json();
        assert_eq!(hello, r#"{"type":"hello","version":"westerm 0.3.0 (host rs-a)"}"#);
    }

    #[test]
    fn place_messages_are_json() {
        let place = Place { cwd: "/w".into(), branch: Some("main".into()) };
        assert_eq!(ServerMessage::Place { place: Some(place) }.to_json(), r#"{"type":"place","place":{"cwd":"/w","branch":"main"}}"#);
        assert_eq!(ServerMessage::Place { place: None }.to_json(), r#"{"type":"place","place":null}"#);
    }

    #[test]
    fn attach_checks_plugin_launches() {
        let with = |extra: &[(&str, &str)]| {
            let mut q = shell_query();
            for (k, v) in extra {
                q.insert((*k).to_owned(), (*v).to_owned());
            }
            AttachRequest::from_query(&q)
        };
        let ok = with(&[("plugin", "docker"), ("action", "logs"), ("target", "0123456789ab")]).unwrap();
        assert_eq!(
            ok.plugin.as_ref().map(|p| (p.plugin(), p.action(), p.target())),
            Some(("docker", "logs", "0123456789ab".to_owned()))
        );
        assert!(ok.target.is_none(), "the plugin target is not an ssh destination");
        let query = ok.to_query();
        assert!(query.contains("plugin=docker&action=logs&target=0123456789ab"), "{query}");
        assert_eq!(query.matches("target=").count(), 1, "{query}");
        let shell = with(&[("plugin", "docker"), ("action", "shell"), ("target", "0123456789ab"), ("cwd", "/work")]).unwrap();
        assert_eq!(shell.cwd.as_deref(), Some("/work"));
        assert!(with(&[]).unwrap().plugin.is_none());
        for bad in [
            vec![("plugin", "docker"), ("action", "logs")],
            vec![("plugin", "docker"), ("target", "0123456789ab")],
            vec![("action", "logs"), ("target", "0123456789ab")],
            vec![("plugin", "nope"), ("action", "logs"), ("target", "0123456789ab")],
            vec![("plugin", "docker"), ("action", "rm"), ("target", "0123456789ab")],
            vec![("plugin", "docker"), ("action", "logs"), ("target", "x;rm -rf ~")],
            vec![("plugin", "docker"), ("action", "logs"), ("target", "0123456789ab"), ("agent", "claude")],
            vec![("plugin", "docker"), ("action", "logs"), ("target", "0123456789ab"), ("kind", "browser")],
            vec![("plugin", "docker"), ("action", "logs"), ("target", "0123456789ab"), ("kind", "ssh")],
        ] {
            assert!(with(&bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn attach_reads_external_launches() {
        let argv = r#"["kubectl","get","pods"]"#;
        let query = format!("{}&plugin=k8s-tools.kubernetes&action=run&target={}", to_pairs(&shell_query()), percent_encode(argv));
        let request = AttachRequest::from_query(&parse_query(&query)).unwrap();
        let plugin = request.plugin.as_ref().unwrap();
        assert_eq!((plugin.plugin(), plugin.action(), plugin.target().as_str()), ("k8s-tools.kubernetes", "run", argv));
        assert!(request.target.is_none(), "the argv is not an ssh destination");
        let again = AttachRequest::from_query(&parse_query(&request.to_query())).unwrap();
        assert_eq!(again.plugin, request.plugin);
        assert_eq!(again.to_query(), request.to_query());
        let path = format!("{}&plugin=k8s-tools.kubernetes&action=run&target={}", to_pairs(&shell_query()), percent_encode(r#"["/bin/sh"]"#));
        assert!(AttachRequest::from_query(&parse_query(&path)).is_none());
    }

    fn to_pairs(query: &HashMap<String, String>) -> String {
        query.iter().map(|(key, value)| format!("{key}={}", percent_encode(value))).collect::<Vec<_>>().join("&")
    }

    /// A query string as the server reads it: split on `&` and `=`, then decoded.
    fn parse_query(query: &str) -> HashMap<String, String> {
        query
            .split('&')
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap();
                (percent_decode(key), percent_decode(value))
            })
            .collect()
    }

    #[test]
    fn attach_checks_cwd_and_launch() {
        let with = |pairs: &[(&str, &str)]| {
            let mut q = shell_query();
            for (k, v) in pairs {
                q.insert((*k).to_owned(), (*v).to_owned());
            }
            AttachRequest::from_query(&q)
        };
        let claude = with(&[("cwd", "/work"), ("agent", "claude")]).unwrap();
        assert_eq!(claude.cwd.as_deref(), Some("/work"));
        assert_eq!(claude.launch, Some(Launch { kind: AgentKind::Claude, conversation: None }));
        let id = "9f00f957-d4e0-4b3b-a88c-a76633c0e762";
        let codex = with(&[("agent", "codex"), ("conversation", id)]).unwrap();
        assert_eq!(codex.launch, Some(Launch { kind: AgentKind::Codex, conversation: Some(id.into()) }));
        let gemini = with(&[("agent", "gemini")]).unwrap();
        assert_eq!(gemini.launch, Some(Launch { kind: AgentKind::Gemini, conversation: None }));
        let plain = with(&[]).unwrap();
        assert!(plain.cwd.is_none() && plain.launch.is_none());
        let long = format!("/{}", "a".repeat(4096));
        for bad in [
            vec![("cwd", "work")],
            vec![("cwd", "/a\0b")],
            vec![("cwd", long.as_str())],
            vec![("agent", "bard")],
            vec![("agent", "claude"), ("conversation", "x;rm")],
            vec![("conversation", id)],
            vec![("kind", "browser"), ("agent", "claude")],
        ] {
            assert!(with(&bad).is_none(), "{bad:?}");
        }
        for request in [claude, codex] {
            let mut q = HashMap::new();
            for pair in request.to_query().split('&') {
                let (k, v) = pair.split_once('=').unwrap();
                q.insert(k.to_owned(), percent_decode(v));
            }
            assert_eq!(AttachRequest::from_query(&q), Some(request));
        }
    }

    #[test]
    fn start_folders_from_a_unix_server_pass_on_every_os() {
        // A remote window on a Windows client relays a folder that a Linux server reported.
        assert!(is_start_folder("/home/x"));
        assert!(is_start_folder("/"));
        #[cfg(windows)]
        assert!(is_start_folder(r"C:\Users\x"));
        for bad in ["home/x", "", "./x"] {
            assert!(!is_start_folder(bad), "{bad}");
        }
    }

    #[test]
    fn agent_kind_names_match_their_json() {
        for kind in [AgentKind::Claude, AgentKind::Codex, AgentKind::Gemini] {
            assert_eq!(serde_json::to_value(kind).unwrap(), serde_json::Value::String(kind.as_str().to_owned()));
            assert_eq!(AgentKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(AgentKind::parse("Claude"), None);
        assert_eq!(AgentKind::parse("bard"), None);
    }

    #[test]
    fn conversation_ids_are_hex_and_dashes() {
        assert!(is_conversation_id("9f00f957-d4e0-4b3b-a88c-a76633c0e762"));
        for bad in ["", "abc", "x;rm -rf", &"a".repeat(65)] {
            assert!(!is_conversation_id(bad), "{bad}");
        }
    }

    #[test]
    fn only_transient_remote_failures_retry() {
        assert!(!RemoteFailure::Unreachable.is_permanent() && !RemoteFailure::Transport.is_permanent());
        assert!(RemoteFailure::Auth.is_permanent() && RemoteFailure::NewerRemote.is_permanent());
    }
}
