//! AI agents in shell sessions: which one runs in the foreground, what it is
//! doing, and the command that resumes its conversation after the shell comes
//! back as "restored" (a reboot, `westerm stop`, a host update).
//!
//! Claude Code writes `~/.claude/sessions/<pid>.json` with its conversation id
//! and a busy, waiting, or idle status. Codex keeps no per-process file and starts its
//! conversation file lazily, so it resumes the newest conversation of the
//! folder (`codex resume --last`). Gemini CLI is the same, and its conversation id
//! is known only when the command line names it (`gemini --resume latest`
//! otherwise). The rest comes from the screen.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::protocol::{is_conversation_id, AgentKind, AgentState, AgentStatus, Launch};

impl AgentKind {
    pub(crate) fn program(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
        }
    }

    /// The name in the restored shell's notice line.
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Gemini => "Gemini CLI",
        }
    }
}

/// Saved with the session, so the agent can resume after its process is gone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecord {
    pub kind: AgentKind,
    /// The program's path when the command named one; else found on PATH.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// Conversation id, when the agent tells it.
    #[serde(default)]
    pub session: Option<String>,
    /// Flags from the original command line that the resume keeps.
    #[serde(default)]
    pub args: Vec<String>,
}

impl AgentRecord {
    /// The command line typed into the restored shell.
    pub fn resume_command(&self) -> String {
        let mut words = vec![self.program.clone().unwrap_or_else(|| self.kind.program().to_owned())];
        match (self.kind, &self.session) {
            (AgentKind::Claude, Some(id)) => words.extend(["--resume".to_owned(), id.clone()]),
            (AgentKind::Claude, None) => words.push("--continue".to_owned()),
            (AgentKind::Codex, Some(id)) => words.extend(["resume".to_owned(), id.clone()]),
            (AgentKind::Codex, None) => words.extend(["resume".to_owned(), "--last".to_owned()]),
            (AgentKind::Gemini, Some(id)) => words.extend(["--resume".to_owned(), id.clone()]),
            (AgentKind::Gemini, None) => words.extend(["--resume".to_owned(), "latest".to_owned()]),
        }
        words.extend(self.args.iter().cloned());
        words.iter().map(|word| shell_quote(word)).collect::<Vec<_>>().join(" ")
    }

    /// The model the kept flags name, if any.
    pub fn model(&self) -> Option<&str> {
        let (values, _) = kept_flags(self.kind);
        let mut words = self.args.iter();
        while let Some(word) = words.next() {
            if values.contains(&word.as_str()) {
                let value = words.next()?;
                if word == "--model" || word == "-m" {
                    return Some(value);
                }
            }
        }
        None
    }
}

/// The record of a launch that opens an existing conversation. None for a new one.
pub fn launch_record(launch: &Launch) -> Option<AgentRecord> {
    let session = launch.conversation.clone()?;
    Some(AgentRecord { kind: launch.kind, program: None, session: Some(session), args: Vec::new() })
}

/// The command line typed into a new shell to start an agent: a fresh
/// conversation, or the one the launch names. Built from the launch alone,
/// never from text the browser sent.
pub fn launch_command(launch: &Launch) -> String {
    match launch_record(launch) {
        Some(record) => record.resume_command(),
        None => launch.kind.program().to_owned(),
    }
}

/// One look at the foreground program of a shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub record: AgentRecord,
    pub status: AgentStatus,
    /// Its process: another process is another agent.
    pub pid: u32,
    pub cwd: Option<String>,
    /// Its version, when it tells it.
    pub version: Option<String>,
}

/// A word as sh, bash, zsh, and fish read it literally.
fn shell_quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word.bytes().all(|b| b.is_ascii_alphanumeric() || b"_./:=@%+,-".contains(&b));
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// The last part of a path, without the `.exe` of a Windows program.
fn file_name(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    // get(): a name may end in a character of several bytes.
    match name.len().checked_sub(4).and_then(|at| name.get(at..)) {
        Some(end) if end.eq_ignore_ascii_case(".exe") => &name[..name.len() - 4],
        _ => name,
    }
}

fn named(path: &str) -> Option<AgentKind> {
    // Windows paths, as npm's launchers write them: `...\npm/node_modules/@openai/codex/bin/codex.js`.
    let path = &path.replace('\\', "/");
    if path.contains("@anthropic-ai/claude-code/") {
        return Some(AgentKind::Claude);
    }
    if path.contains("@openai/codex/") {
        return Some(AgentKind::Codex);
    }
    if path.contains("@google/gemini-cli/") {
        return Some(AgentKind::Gemini);
    }
    let name = file_name(path);
    let name = name.strip_suffix(".js").or_else(|| name.strip_suffix(".mjs")).unwrap_or(name);
    match name {
        "claude" => Some(AgentKind::Claude),
        "codex" => Some(AgentKind::Codex),
        "gemini" => Some(AgentKind::Gemini),
        _ => None,
    }
}

/// Programs that run an agent from a script path in the next argument.
fn interpreter(path: &str) -> bool {
    let name = file_name(path);
    ["node", "bun", "deno", "python"].iter().any(|prefix| name.starts_with(prefix))
        || ["sh", "bash", "zsh", "dash"].contains(&name)
}

/// Subcommands that do not open a conversation.
fn tool_command(kind: AgentKind, word: &str) -> bool {
    let commands: &[&str] = match kind {
        AgentKind::Claude => &[
            "agents", "auth", "auto-mode", "config", "doctor", "install", "mcp", "migrate-installer", "plugin",
            "plugins", "setup-token", "update", "upgrade",
        ],
        AgentKind::Codex => &[
            "a", "app-server", "apply", "cloud", "completion", "debug", "e", "exec", "features", "help", "login",
            "logout", "mcp", "mcp-server", "proto", "review", "sandbox",
        ],
        AgentKind::Gemini => &["mcp", "extensions", "extension", "skills", "skill", "hooks", "hook", "gemma"],
    };
    commands.contains(&word)
}

/// Value flags, then flags without a value, that a resume keeps.
fn kept_flags(kind: AgentKind) -> (&'static [&'static str], &'static [&'static str]) {
    match kind {
        AgentKind::Claude => (
            &[
                "--model", "--permission-mode", "--add-dir", "--settings", "--setting-sources", "--mcp-config",
                "--append-system-prompt", "--system-prompt", "--allowedTools", "--allowed-tools",
                "--disallowedTools", "--disallowed-tools", "--agent", "--fallback-model", "--plugin-dir", "--effort",
            ],
            &[
                "--dangerously-skip-permissions", "--allow-dangerously-skip-permissions", "--strict-mcp-config",
                "--verbose", "--ide", "--chrome", "--no-chrome",
            ],
        ),
        AgentKind::Codex => (
            &[
                "-m", "--model", "-s", "--sandbox", "-a", "--ask-for-approval", "-c", "--config", "-p", "--profile",
                "--enable", "--disable", "--add-dir", "--local-provider",
            ],
            &["--dangerously-bypass-approvals-and-sandbox", "--search", "--oss", "--approve-for-me", "--no-alt-screen"],
        ),
        AgentKind::Gemini => (
            &[
                "-m", "--model", "--approval-mode", "--policy", "--admin-policy", "--allowed-mcp-server-names",
                "--allowed-tools", "-e", "--extensions", "--include-directories",
            ],
            &["-y", "--yolo", "-s", "--sandbox", "--skip-trust", "--screen-reader"],
        ),
    }
}

/// Gemini CLI flags that run one task, list, or print instead of opening a conversation.
const GEMINI_NOT_A_CONVERSATION: &[&str] = &[
    "-p", "--prompt", "--acp", "--experimental-acp", "--list-sessions", "--delete-session", "-l", "--list-extensions",
    "-v", "--version", "-h", "--help",
];

/// Gemini CLI flags that a resume drops, with the word after them: the start
/// prompt, a worktree (a resume must not make another), a session file, and output options.
const GEMINI_DROPPED_VALUES: &[&str] =
    &["-i", "--prompt-interactive", "-w", "--worktree", "--session-file", "-o", "--output-format"];

/// The agent a command line runs, its conversation id when the command names
/// one, and the flags a resume keeps. None for other programs, print mode,
/// and tool subcommands such as `claude mcp` or `codex exec`.
pub fn identify(argv: &[String]) -> Option<AgentRecord> {
    let first = argv.first()?;
    let (kind, path, rest) = match named(first) {
        Some(kind) => (kind, first, &argv[1..]),
        None if interpreter(first) => {
            let script = argv[1..].iter().position(|arg| !arg.starts_with('-'))? + 1;
            (named(&argv[script])?, &argv[script], &argv[script + 1..])
        }
        None => return None,
    };
    // A path runs the same program again; a package's script runs through
    // the name on PATH, which is its launcher.
    let program = (path.contains('/') && !path.contains("node_modules")).then(|| path.clone());
    let (values, switches) = kept_flags(kind);
    let mut args = Vec::new();
    let mut session = None;
    let mut positional = 0;
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        let (flag, inline) = match word.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
            _ => (word.as_str(), None),
        };
        match flag {
            "-p" | "--print" if kind == AgentKind::Claude => return None,
            "-r" | "--resume" if kind == AgentKind::Claude => {
                // Without an id, `--resume` opens the picker.
                let value = match inline {
                    Some(value) => Some(value),
                    None => match words.clone().next() {
                        Some(next) if is_conversation_id(next) => {
                            words.next();
                            Some(next.as_str())
                        }
                        _ => None,
                    },
                };
                session = value.filter(|id| is_conversation_id(id)).map(str::to_owned);
            }
            _ if kind == AgentKind::Gemini && GEMINI_NOT_A_CONVERSATION.contains(&flag) => return None,
            "-r" | "--resume" | "--session-id" if kind == AgentKind::Gemini => {
                // The word after it is an id, `latest`, or a number; only an id names a conversation.
                let value = match inline {
                    Some(value) => Some(value),
                    None => words.clone().next().filter(|next| !next.starts_with('-')).map(|next| {
                        words.next();
                        next.as_str()
                    }),
                };
                session = value.filter(|id| is_conversation_id(id)).map(str::to_owned);
            }
            _ if kind == AgentKind::Gemini && GEMINI_DROPPED_VALUES.contains(&flag) => {
                // `-w` takes an optional name, so only a word that is no flag belongs to it.
                if inline.is_none() && words.clone().next().is_some_and(|next| !next.starts_with('-')) {
                    words.next();
                }
            }
            _ if values.contains(&flag) => {
                let value = match inline {
                    Some(value) => Some(value.to_owned()),
                    None => words.next().cloned(),
                };
                if let Some(value) = value {
                    args.extend([flag.to_owned(), value]);
                }
            }
            _ if switches.contains(&flag) && inline.is_none() => args.push(flag.to_owned()),
            _ if flag.starts_with('-') => {}
            _ => {
                // Positional words: a subcommand first, else a prompt (dropped).
                if positional == 0 && tool_command(kind, flag) {
                    return None;
                }
                if kind == AgentKind::Codex && positional == 1 && rest_is_resume(rest) && is_conversation_id(flag) {
                    session = Some(flag.to_owned());
                }
                positional += 1;
            }
        }
    }
    Some(AgentRecord { kind, program, session, args })
}

/// `codex resume <id>`: the first positional word is "resume".
fn rest_is_resume(rest: &[String]) -> bool {
    rest.iter().find(|word| !word.starts_with('-')).is_some_and(|word| word == "resume")
}

/// Claude Code's state folder for this host, read once.
pub fn claude_home() -> &'static Path {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| claude_dir(std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from), &crate::state_dir::home_dir()))
}

/// Claude Code's state folder: `CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn claude_dir(config_dir: Option<PathBuf>, home: &Path) -> PathBuf {
    config_dir.filter(|dir| !dir.as_os_str().is_empty()).unwrap_or_else(|| home.join(".claude"))
}

/// What Claude Code reports in `sessions/<pid>.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaudeReport {
    pub session: Option<String>,
    /// None for a status this build does not know.
    pub status: Option<AgentStatus>,
    pub version: Option<String>,
}

pub fn read_claude_report(dir: &Path, pid: u32) -> Option<ClaudeReport> {
    let bytes = std::fs::read(dir.join("sessions").join(format!("{pid}.json"))).ok()?;
    parse_claude_report(&bytes, pid)
}

fn parse_claude_report(bytes: &[u8], pid: u32) -> Option<ClaudeReport> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    if value.get("pid")?.as_u64()? != u64::from(pid) {
        return None;
    }
    let session = value.get("sessionId").and_then(|id| id.as_str()).filter(|id| is_conversation_id(id));
    let status = match value.get("status").and_then(|status| status.as_str()) {
        Some("busy") => Some(AgentStatus::Working),
        // A question for the user: a permission, a choice, a plan to approve.
        Some("waiting") => Some(AgentStatus::Waiting),
        Some("idle") => Some(AgentStatus::Idle),
        _ => None,
    };
    let version = value
        .get("version")
        .and_then(|version| version.as_str())
        .filter(|version| version.len() <= 32 && version.bytes().all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b)));
    Some(ClaudeReport { session: session.map(str::to_owned), status, version: version.map(str::to_owned) })
}

/// Text of an agent's questions to the user: approvals ("Do you want to",
/// "Would you like to"), Codex's questions with choices, whose footer also
/// reads "esc to interrupt", Codex's end of Plan mode, and Gemini's tool and
/// edit approvals.
const QUESTIONS: &[&str] =
    &["do you want to", "would you like to", "to submit answer", "implement this plan?", "allow execution of", "apply this change?"];

/// Text of an agent's progress line while it works: "esc to interrupt" for
/// Claude Code and Codex, "(esc to cancel, 3s)" for Gemini. Gemini's comma
/// keeps the "Esc to cancel" hint of other dialogs from reading as work.
const WORKING: &[&str] = &["esc to interrupt", "(esc to cancel,"];

/// Reads the last rows of an agent's screen: its status line while it works
/// and its questions, which win.
pub fn screen_status(last: &str) -> Option<AgentStatus> {
    let text = last.to_lowercase();
    if QUESTIONS.iter().any(|question| text.contains(question)) {
        Some(AgentStatus::Waiting)
    } else if WORKING.iter().any(|working| text.contains(working)) {
        Some(AgentStatus::Working)
    } else {
        None
    }
}

/// One look at an agent found by `identify` in process `pid`: Claude's report
/// and the last rows of the screen give its status, and Claude's report its
/// conversation and version.
pub fn look(mut record: AgentRecord, pid: u32, cwd: Option<String>, claude: Option<ClaudeReport>, last: &str) -> Seen {
    let mut reported = None;
    let mut version = None;
    if record.kind == AgentKind::Claude
        && let Some(report) = claude
    {
        reported = report.status;
        version = report.version;
        if report.session.is_some() {
            record.session = report.session;
        }
    }
    // The more urgent of the report and the screen: a question, then work.
    let screen = screen_status(last);
    let either = |status| reported == Some(status) || screen == Some(status);
    let status = [AgentStatus::Waiting, AgentStatus::Working].into_iter().find(|&status| either(status)).unwrap_or(AgentStatus::Idle);
    Seen { record, status, pid, cwd, version }
}

/// Polls in a row without the agent before its record is dropped. On a reboot
/// the agent may end a moment before the host saves; one missing poll must
/// not erase what the next start resumes.
const GONE_AFTER: u8 = 2;
/// Polls a restored shell waits for its resumed agent before the record can
/// go: the shell starts, the command is typed, the agent starts.
const RESUME_POLLS: u8 = 20;

/// What changed after a look.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// Save the session.
    pub record: bool,
    /// Tell the viewer.
    pub state: bool,
}

/// The agent of one session across polls.
#[derive(Default)]
pub struct Tracker {
    record: Option<AgentRecord>,
    state: Option<AgentState>,
    /// The process of the agent in `state`.
    pid: Option<u32>,
    missing: u8,
    /// Polls left while a resume is on its way; an empty foreground meanwhile
    /// keeps the record.
    holding: u8,
}

impl Tracker {
    /// A restored session starts with the record it resumes.
    pub fn resuming(record: AgentRecord) -> Self {
        Self { record: Some(record), holding: RESUME_POLLS, ..Self::default() }
    }

    pub fn record(&self) -> Option<&AgentRecord> {
        self.record.as_ref()
    }

    pub fn state(&self) -> Option<&AgentState> {
        self.state.as_ref()
    }

    /// `now`: milliseconds since the epoch.
    pub fn observe(&mut self, seen: Option<Seen>, now: u64) -> Changes {
        let mut changes = Changes::default();
        let state = seen.as_ref().map(|seen| {
            let same = self.state.as_ref().filter(|_| self.pid == Some(seen.pid));
            AgentState {
                kind: seen.record.kind,
                status: seen.status,
                since: same.filter(|old| old.status == seen.status).map_or(now, |old| old.since),
                started: same.map_or(now, |old| old.started),
                conversation: seen.record.session.clone(),
                resume: seen.record.resume_command(),
                cwd: seen.cwd.clone(),
                version: seen.version.clone(),
                model: seen.record.model().map(str::to_owned),
            }
        });
        self.pid = seen.as_ref().map(|seen| seen.pid);
        if state != self.state {
            self.state = state;
            changes.state = true;
        }
        match seen {
            Some(seen) => {
                self.missing = 0;
                self.holding = 0;
                if self.record.as_ref() != Some(&seen.record) {
                    self.record = Some(seen.record);
                    changes.record = true;
                }
            }
            None if self.holding > 0 => self.holding -= 1,
            None if self.record.is_some() => {
                self.missing = self.missing.saturating_add(1);
                if self.missing >= GONE_AFTER {
                    self.record = None;
                    self.missing = 0;
                    changes.record = true;
                }
            }
            None => {}
        }
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_owned).collect()
    }

    fn record(kind: AgentKind, session: Option<&str>, args: &[&str]) -> AgentRecord {
        AgentRecord {
            kind,
            program: None,
            session: session.map(str::to_owned),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    const ID: &str = "9f00f957-d4e0-4b3b-a88c-a76633c0e762";

    fn look_at(record: AgentRecord, claude: Option<ClaudeReport>, last: &str) -> Seen {
        look(record, 7, None, claude, last)
    }

    fn observe(tracker: &mut Tracker, seen: Option<Seen>) -> Changes {
        tracker.observe(seen, 1_000)
    }

    #[test]
    fn finds_agents_by_program_and_script() {
        assert_eq!(identify(&argv("claude")), Some(record(AgentKind::Claude, None, &[])));
        let path = identify(&argv("/Users/me/.local/bin/claude")).unwrap();
        assert_eq!(path.program.as_deref(), Some("/Users/me/.local/bin/claude"));
        assert_eq!(path.resume_command(), "/Users/me/.local/bin/claude --continue");
        assert_eq!(identify(&argv("codex")).map(|r| r.kind), Some(AgentKind::Codex));
        let npm = argv("node /usr/local/lib/node_modules/@anthropic-ai/claude-code/cli.js --model opus");
        assert_eq!(identify(&npm), Some(record(AgentKind::Claude, None, &["--model", "opus"])));
        let npm = argv("node --no-warnings /opt/lib/node_modules/@openai/codex/bin/codex.js");
        assert_eq!(identify(&npm).map(|r| r.kind), Some(AgentKind::Codex));
        assert_eq!(identify(&argv("/bin/sh /tmp/bin/claude")).unwrap().program.as_deref(), Some("/tmp/bin/claude"));
        for other in ["vim", "node server.js", "cargo test", "claudette", "-zsh"] {
            assert_eq!(identify(&argv(other)), None, "{other}");
        }
    }

    #[test]
    fn finds_agents_in_windows_command_lines() {
        let words = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
        let native = identify(&words(&[r"C:\Users\me\.local\bin\claude.exe", "--model", "opus"]));
        assert_eq!(native, Some(record(AgentKind::Claude, None, &["--model", "opus"])), "resumes by the name on PATH");
        assert_eq!(identify(&words(&["codex.EXE"])).map(|r| r.kind), Some(AgentKind::Codex));
        // npm's PowerShell launcher joins its folder and the script with a `/`.
        let node = r"C:\Program Files\nodejs\node.exe";
        let npm = words(&[node, r"C:\Users\me\AppData\Roaming\npm/node_modules/@google/gemini-cli/dist/index.js"]);
        assert_eq!(identify(&npm), Some(record(AgentKind::Gemini, None, &[])));
        let npm = words(&[node, r"C:\Users\me\AppData\Roaming\npm\node_modules\@openai\codex\bin\codex.js"]);
        assert_eq!(identify(&npm), Some(record(AgentKind::Codex, None, &[])));
        let packaged = r"C:\npm\node_modules\@anthropic-ai\claude-code\bin\claude.exe";
        assert_eq!(identify(&words(&[packaged])), Some(record(AgentKind::Claude, None, &[])));
        assert_eq!(identify(&words(&[r"C:\Program Files\Git\bin\bash.exe", r"C:\tools\claude"])).map(|r| r.kind), Some(AgentKind::Claude));
        for other in [&[r"C:\WINDOWS\system32\cmd.exe", "/d", "/c", r"C:\npm\claude.cmd"][..], &["pwsh.exe", "-NoLogo"], &[node, "server.js"], &["é.exe"], &["éaé"], &[".exe"]] {
            assert_eq!(identify(&words(other)), None, "{other:?}");
        }
    }

    #[test]
    fn keeps_known_flags_and_drops_prompts_and_resume_flags() {
        let line = argv("claude --model opus --permission-mode=plan --dangerously-skip-permissions fix the bug --continue --unknown x");
        assert_eq!(
            identify(&line),
            Some(record(AgentKind::Claude, None, &["--model", "opus", "--permission-mode", "plan", "--dangerously-skip-permissions"]))
        );
        let line = argv("codex -m gpt-5 --sandbox workspace-write -C /tmp --search hello");
        assert_eq!(
            identify(&line),
            Some(record(AgentKind::Codex, None, &["-m", "gpt-5", "--sandbox", "workspace-write", "--search"]))
        );
    }

    #[test]
    fn reads_conversation_ids_from_resume_commands() {
        assert_eq!(identify(&argv(&format!("claude --resume {ID}"))).unwrap().session.as_deref(), Some(ID));
        assert_eq!(identify(&argv(&format!("claude -r {ID} --model opus"))).unwrap().args, ["--model", "opus"]);
        assert_eq!(identify(&argv("claude --resume")).unwrap().session, None, "the picker");
        assert_eq!(identify(&argv(&format!("codex resume {ID}"))).unwrap().session.as_deref(), Some(ID));
        assert_eq!(identify(&argv("codex resume --last")).unwrap().session, None);
    }

    #[test]
    fn finds_gemini_by_program_and_script() {
        assert_eq!(identify(&argv("gemini")), Some(record(AgentKind::Gemini, None, &[])));
        let path = identify(&argv("/opt/homebrew/bin/gemini")).unwrap();
        assert_eq!((path.kind, path.program.as_deref()), (AgentKind::Gemini, Some("/opt/homebrew/bin/gemini")));
        let npm = argv("node /opt/homebrew/lib/node_modules/@google/gemini-cli/bundle/gemini.js");
        assert_eq!(identify(&npm), Some(record(AgentKind::Gemini, None, &[])));
        for other in [
            "gemini -p hi", "gemini --prompt hi", "gemini mcp list", "gemini extensions list", "gemini extension list",
            "gemini skills list", "gemini skill list", "gemini hooks list", "gemini hook list", "gemini gemma",
            "gemini --list-sessions", "gemini --delete-session 2", "gemini --acp", "gemini --experimental-acp",
            "gemini -l", "gemini --list-extensions", "gemini -v", "gemini --version", "gemini -h", "gemini --help",
        ] {
            assert_eq!(identify(&argv(other)), None, "{other}");
        }
    }

    #[test]
    fn gemini_keeps_its_flags_and_drops_prompts_and_worktrees() {
        let line = argv("gemini -m gemini-2.5-pro --yolo -w feat -i \"fix it\" --include-directories ../lib \"start\"");
        let found = identify(&line).unwrap();
        assert_eq!(found, record(AgentKind::Gemini, None, &["-m", "gemini-2.5-pro", "--yolo", "--include-directories", "../lib"]));
        assert_eq!(found.resume_command(), "gemini --resume latest -m gemini-2.5-pro --yolo --include-directories ../lib");
        assert_eq!(found.model(), Some("gemini-2.5-pro"));
        // A worktree name and a prompt with spaces are one word each.
        let quoted: Vec<String> = ["gemini", "--worktree", "feat", "-o", "json", "-d", "fix it", "-s"].map(str::to_owned).to_vec();
        assert_eq!(identify(&quoted), Some(record(AgentKind::Gemini, None, &["-s"])));
    }

    #[test]
    fn gemini_reads_its_conversation_from_the_command_line() {
        let resume = identify(&argv(&format!("gemini --resume {ID}"))).unwrap();
        assert_eq!(resume.session.as_deref(), Some(ID));
        assert_eq!(resume.resume_command(), format!("gemini --resume {ID}"));
        let session = identify(&argv(&format!("gemini --session-id {ID} --yolo"))).unwrap();
        assert_eq!(session.session.as_deref(), Some(ID));
        assert_eq!(session.args, ["--yolo"]);
        assert_eq!(identify(&argv(&format!("gemini --resume={ID}"))).unwrap().session.as_deref(), Some(ID));
        for line in ["gemini -r latest", "gemini -r 3", "gemini --resume", "gemini --resume latest --yolo"] {
            assert_eq!(identify(&argv(line)).unwrap().session, None, "{line}");
        }
        // The word after `-r latest` is not a prompt that sets a conversation.
        assert_eq!(identify(&argv("gemini -r 3 -m x")).unwrap().args, ["-m", "x"]);
    }

    #[test]
    fn gemini_screens_tell_work_and_questions() {
        assert_eq!(screen_status("⠋ Thinking (esc to cancel, 3s)"), Some(AgentStatus::Working));
        assert_eq!(screen_status("Allow execution of [Shell]?"), Some(AgentStatus::Waiting));
        assert_eq!(screen_status("Apply this change?"), Some(AgentStatus::Waiting));
        assert_eq!(screen_status("Do you want to proceed?"), Some(AgentStatus::Waiting));
        assert_eq!(screen_status("Allow execution of [Shell]?\n(esc to cancel, 3s)"), Some(AgentStatus::Waiting), "a question wins");
        let dialog = "Enter to select · ↑/↓ to navigate · Esc to cancel";
        assert_eq!(screen_status(dialog), None, "another agent's dialog hint is not work");
        assert_eq!(AgentKind::Gemini.label(), "Gemini CLI");
    }

    #[test]
    fn print_mode_and_tool_commands_are_not_conversations() {
        for line in ["claude -p hello", "claude --print", "claude mcp list", "claude doctor", "codex exec fix", "codex login"] {
            assert_eq!(identify(&argv(line)), None, "{line}");
        }
    }

    #[test]
    fn resume_commands_quote_their_words() {
        assert_eq!(record(AgentKind::Claude, Some(ID), &["--model", "opus"]).resume_command(), format!("claude --resume {ID} --model opus"));
        assert_eq!(record(AgentKind::Claude, None, &[]).resume_command(), "claude --continue");
        assert_eq!(record(AgentKind::Codex, None, &["-c", "model=\"o3\""]).resume_command(), "codex resume --last -c 'model=\"o3\"'");
        assert_eq!(record(AgentKind::Codex, Some(ID), &[]).resume_command(), format!("codex resume {ID}"));
        let tricky = record(AgentKind::Claude, None, &["--append-system-prompt", "it's $(rm -rf ~); `x`"]);
        assert_eq!(tricky.resume_command(), r"claude --continue --append-system-prompt 'it'\''s $(rm -rf ~); `x`'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(record(AgentKind::Codex, None, &["-c", "x=1", "-m", "o3"]).model(), Some("o3"));
        assert_eq!(record(AgentKind::Claude, None, &["--model", "opus"]).model(), Some("opus"));
        let tricky = record(AgentKind::Claude, None, &["--append-system-prompt", "--model", "--verbose"]);
        assert_eq!(tricky.model(), None, "a value that reads like the flag");
    }

    #[test]
    fn launch_commands() {
        let launch = |kind, conversation: Option<&str>| Launch { kind, conversation: conversation.map(str::to_owned) };
        assert_eq!(launch_command(&launch(AgentKind::Claude, None)), "claude");
        assert_eq!(launch_command(&launch(AgentKind::Codex, None)), "codex");
        assert_eq!(launch_command(&launch(AgentKind::Gemini, None)), "gemini");
        assert_eq!(launch_command(&launch(AgentKind::Claude, Some(ID))), format!("claude --resume {ID}"));
        assert_eq!(launch_command(&launch(AgentKind::Codex, Some(ID))), format!("codex resume {ID}"));
        assert_eq!(launch_command(&launch(AgentKind::Gemini, Some(ID))), format!("gemini --resume {ID}"));
    }

    #[test]
    fn reads_claude_reports() {
        let json = format!(r#"{{"pid":47601,"sessionId":"{ID}","cwd":"/x","status":"busy"}}"#);
        assert_eq!(parse_claude_report(json.as_bytes(), 47601), Some(ClaudeReport { session: Some(ID.into()), status: Some(AgentStatus::Working), version: None }));
        assert_eq!(parse_claude_report(json.as_bytes(), 1), None, "another process");
        let idle = r#"{"pid":7,"sessionId":"../../etc","status":"idle"}"#;
        assert_eq!(parse_claude_report(idle.as_bytes(), 7), Some(ClaudeReport { session: None, status: Some(AgentStatus::Idle), version: None }));
        // Claude Code 2.1.319, while it asks the user a question.
        let asks = r#"{"pid":7,"status":"waiting","waitingFor":"input needed"}"#;
        assert_eq!(parse_claude_report(asks.as_bytes(), 7).unwrap().status, Some(AgentStatus::Waiting));
        let unknown = r#"{"pid":7,"status":"dreaming"}"#;
        assert_eq!(parse_claude_report(unknown.as_bytes(), 7).unwrap().status, None, "a status this build does not know");
        assert_eq!(parse_claude_report(b"not json", 7), None);
        let versioned = r#"{"pid":7,"status":"idle","version":"2.1.319"}"#;
        assert_eq!(parse_claude_report(versioned.as_bytes(), 7).unwrap().version.as_deref(), Some("2.1.319"));
        let odd = r#"{"pid":7,"version":"<b>2</b>"}"#;
        assert_eq!(parse_claude_report(odd.as_bytes(), 7).unwrap().version, None, "only a version number shows");
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sessions")).unwrap();
        std::fs::write(dir.path().join("sessions/7.json"), idle).unwrap();
        assert!(read_claude_report(dir.path(), 7).is_some());
        assert_eq!(claude_dir(None, Path::new("/home/me")), PathBuf::from("/home/me/.claude"));
        assert_eq!(claude_dir(Some("/cfg".into()), Path::new("/home/me")), PathBuf::from("/cfg"));
    }

    #[test]
    fn screen_and_report_give_the_status() {
        let claude = record(AgentKind::Claude, None, &[]);
        let report = |status| Some(ClaudeReport { session: Some(ID.into()), status: Some(status), version: None });
        let seen = look_at(claude.clone(), report(AgentStatus::Working), "✻ Thinking…");
        assert_eq!((seen.status, seen.record.session.as_deref()), (AgentStatus::Working, Some(ID)));
        assert_eq!(look_at(claude.clone(), report(AgentStatus::Working), " Do you want to proceed?\n ❯ 1. Yes").status, AgentStatus::Waiting);
        let menu = "❯ 1. 질문 없이 바로 계획\nEnter to select · ↑/↓ to navigate · Esc to cancel";
        assert_eq!(look_at(claude.clone(), report(AgentStatus::Waiting), menu).status, AgentStatus::Waiting, "Claude says it asks");
        assert_eq!(look_at(claude.clone(), report(AgentStatus::Idle), "* esc to interrupt").status, AgentStatus::Working);
        assert_eq!(look_at(claude, None, "> ").status, AgentStatus::Idle);
        let codex = record(AgentKind::Codex, None, &[]);
        assert_eq!(look_at(codex.clone(), None, "• Working (12s • esc to interrupt)").status, AgentStatus::Working);
        assert_eq!(look_at(codex.clone(), None, "Would you like to run the following command?").status, AgentStatus::Waiting);
        // Codex 0.164 asks the user (request_user_input), with "esc to interrupt" in the same footer.
        let question = "Question 1/3 (3 unanswered)\n어떤 용도의 체크리스트 웹사이트를 만들려고 하는가?\n› 1. 개인 할 일 관리 (Recommended)\n\ntab to add notes | enter to submit answer | ←/→ to navigate questions | esc to interrupt";
        assert_eq!(look_at(codex.clone(), None, question).status, AgentStatus::Waiting);
        assert_eq!(look_at(codex.clone(), None, "Implement this plan?\n› 1. Yes, implement this plan").status, AgentStatus::Waiting);
        let asks = report(AgentStatus::Waiting);
        let seen = look_at(codex, asks, "");
        assert_eq!((seen.record.session, seen.status), (None, AgentStatus::Idle), "Claude's report is Claude's only");
    }

    #[test]
    fn the_state_tells_what_and_since_when() {
        let mut tracker = Tracker::default();
        let mut seen = Seen {
            record: record(AgentKind::Claude, Some(ID), &["--model", "opus", "--verbose"]),
            status: AgentStatus::Working,
            pid: 7,
            cwd: Some("/work".into()),
            version: Some("2.1.319".into()),
        };
        tracker.observe(Some(seen.clone()), 1_000);
        let state = tracker.state().unwrap().clone();
        assert_eq!((state.since, state.started), (1_000, 1_000));
        assert_eq!(state.conversation.as_deref(), Some(ID));
        assert_eq!(state.resume, format!("claude --resume {ID} --model opus --verbose"));
        assert_eq!(state.model.as_deref(), Some("opus"));
        assert_eq!((state.cwd.as_deref(), state.version.as_deref()), (Some("/work"), Some("2.1.319")));
        assert_eq!(tracker.observe(Some(seen.clone()), 2_000), Changes::default(), "nothing new to tell");
        seen.status = AgentStatus::Waiting;
        tracker.observe(Some(seen.clone()), 3_000);
        let state = tracker.state().unwrap();
        assert_eq!((state.since, state.started), (3_000, 1_000), "a new status of the same agent");
        seen.pid = 8;
        tracker.observe(Some(seen), 4_000);
        let state = tracker.state().unwrap();
        assert_eq!((state.since, state.started), (4_000, 4_000), "another process is another agent");
    }

    fn seen(status: AgentStatus) -> Option<Seen> {
        Some(Seen { record: record(AgentKind::Claude, Some(ID), &[]), status, pid: 7, cwd: None, version: None })
    }

    #[test]
    fn records_survive_one_missing_poll() {
        let mut tracker = Tracker::default();
        assert_eq!(observe(&mut tracker, seen(AgentStatus::Working)), Changes { record: true, state: true });
        assert_eq!(observe(&mut tracker, seen(AgentStatus::Idle)), Changes { record: false, state: true });
        assert_eq!(observe(&mut tracker, seen(AgentStatus::Idle)), Changes::default());
        assert_eq!(observe(&mut tracker, None), Changes { record: false, state: true });
        assert!(tracker.record().is_some(), "kept after one miss");
        assert_eq!(observe(&mut tracker, None), Changes { record: true, state: false });
        assert!(tracker.record().is_none());
        assert_eq!(observe(&mut tracker, None), Changes::default());
    }

    #[test]
    fn a_resuming_record_waits_for_its_agent() {
        let mut tracker = Tracker::resuming(record(AgentKind::Codex, None, &[]));
        for _ in 0..RESUME_POLLS {
            assert_eq!(observe(&mut tracker, None), Changes::default());
        }
        assert!(tracker.record().is_some());
        observe(&mut tracker, None);
        assert_eq!(observe(&mut tracker, None), Changes { record: true, state: false });
        assert!(tracker.record().is_none(), "dropped once the resume had its chance");

        let mut tracker = Tracker::resuming(record(AgentKind::Codex, None, &[]));
        observe(&mut tracker, None);
        observe(&mut tracker, Some(Seen { record: record(AgentKind::Codex, None, &[]), status: AgentStatus::Idle, pid: 7, cwd: None, version: None }));
        observe(&mut tracker, None);
        observe(&mut tracker, None);
        assert!(tracker.record().is_none(), "the agent came and went: no more waiting");
    }
}
