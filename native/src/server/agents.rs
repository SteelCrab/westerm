//! `/api/agents/history`: the past conversations of a folder, read from the
//! history files of Claude Code, Codex, and Gemini CLI. The response has ids,
//! titles, and times only, never the text of a conversation.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{no_store, read_allowed, Shared};
use crate::protocol::{is_conversation_id, AgentKind};
use crate::state_dir::home_dir;

/// A conversation file is read up to this many bytes.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// A Claude Code request looks at this many of the newest files, also those of another folder.
const MAX_CLAUDE_FILES: usize = 50;
/// A Codex request looks at this many of the newest files.
const MAX_CODEX_FILES: usize = 500;
/// The first line of a Codex file is read up to this many bytes.
const MAX_FIRST_LINE: usize = 1024 * 1024;
/// Only the last bytes of Codex's title index are read.
const MAX_INDEX_TAIL: u64 = 4 * 1024 * 1024;
/// Codex first lines never change, so they are kept by path, this many.
const MAX_CODEX_MEMO: usize = 4096;
/// Gemini folders are checked for a `.project_root` file, this many.
const MAX_GEMINI_ROOTS: usize = 256;
/// A response lists this many conversations. The UI shows 5 after it drops the running ones.
const MAX_RESULTS: usize = 10;
/// A title is cut to this many characters.
const MAX_TITLE_CHARS: usize = 120;
/// Like the host's start folder: at most 4096 bytes.
const MAX_DIR_BYTES: usize = 4096;

pub(super) fn routes() -> Router<Shared> {
    Router::new().route("/api/agents/history", get(history))
}

#[derive(Deserialize)]
struct HistoryQuery {
    dir: String,
}

/// The newest conversations of a folder, from all three tools.
async fn history(State(state): State<Shared>, headers: HeaderMap, Query(query): Query<HistoryQuery>) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let dir = query.dir;
    if dir.len() > MAX_DIR_BYTES || dir.contains('\0') || !Path::new(&dir).is_absolute() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let listed = tokio::task::spawn_blocking(move || {
        let home = home_dir();
        let folders = ToolFolders {
            claude: crate::host::agent::claude_home().to_path_buf(),
            codex: codex_dir(std::env::var_os("CODEX_HOME").map(PathBuf::from), &home),
            gemini: gemini_dir(std::env::var_os("GEMINI_CLI_HOME").map(PathBuf::from), &home),
        };
        history_json(&folders, &dir)
    })
    .await;
    match listed {
        Ok(body) => no_store(Json(body)),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The state folders of the three tools. The route resolves the real ones; tests pass temp folders.
struct ToolFolders {
    claude: PathBuf,
    codex: PathBuf,
    /// The `.gemini` folder itself.
    gemini: PathBuf,
}

/// The response body: the newest conversations of `dir` from all three tools.
fn history_json(folders: &ToolFolders, dir: &str) -> Value {
    let conversations = merge(
        [
            claude_conversations(&folders.claude, dir, MAX_RESULTS),
            codex_conversations(&folders.codex, dir, MAX_RESULTS),
            gemini_conversations(&folders.gemini, dir, MAX_RESULTS),
        ],
        MAX_RESULTS,
    );
    json!({ "conversations": conversations })
}

/// One past conversation. `updated` is in milliseconds since the epoch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Conversation {
    pub kind: AgentKind,
    pub id: String,
    pub title: Option<String>,
    pub updated: u64,
}

/// Codex's folder: `CODEX_HOME`, else `~/.codex`.
fn codex_dir(env: Option<PathBuf>, home: &Path) -> PathBuf {
    env.filter(|dir| !dir.as_os_str().is_empty()).unwrap_or_else(|| home.join(".codex"))
}

/// Gemini CLI's folder: `$GEMINI_CLI_HOME/.gemini`, else `~/.gemini`.
fn gemini_dir(env: Option<PathBuf>, home: &Path) -> PathBuf {
    env.filter(|dir| !dir.as_os_str().is_empty()).unwrap_or_else(|| home.to_path_buf()).join(".gemini")
}

/// The lists of all tools, newest first, cut to `limit`.
fn merge(lists: [Vec<Conversation>; 3], limit: usize) -> Vec<Conversation> {
    let mut all: Vec<Conversation> = lists.into_iter().flatten().collect();
    all.sort_by_key(|item| std::cmp::Reverse(item.updated));
    all.truncate(limit);
    all
}

/// A title as one line, trimmed and cut; None when nothing is left.
fn clean_title(text: &str) -> Option<String> {
    let line = text.replace("\r\n", " ").replace(['\r', '\n'], " ");
    let title: String = line.trim().chars().take(MAX_TITLE_CHARS).collect();
    let title = title.trim_end();
    if title.is_empty() { None } else { Some(title.to_owned()) }
}

/// A file's modified time in milliseconds since the epoch.
fn modified_ms(path: &Path) -> Option<u64> {
    let time = fs::metadata(path).ok()?.modified().ok()?;
    time.duration_since(UNIX_EPOCH).ok().map(|since| since.as_millis() as u64)
}

/// The first `MAX_FILE_BYTES` of a file.
fn read_capped(path: &Path) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path).ok()?.take(MAX_FILE_BYTES).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// The JSON values of the lines of `bytes`. A line that is not JSON is skipped.
fn json_lines(bytes: &[u8]) -> impl Iterator<Item = Value> + '_ {
    bytes.split(|byte| *byte == b'\n').filter_map(|line| serde_json::from_slice(line).ok())
}

fn text_field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name)?.as_str()
}

/// The paths in `dir` with a file name `keep` accepts, with their modified times, newest first.
fn newest_files(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<(PathBuf, u64)> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<(PathBuf, u64)> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_str().is_some_and(&keep))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| Some((entry.path(), modified_ms(&entry.path())?)))
        .collect();
    files.sort_by_key(|file| std::cmp::Reverse(file.1));
    files
}

/// Claude Code's folder name for a path: every UTF-16 code unit that is not a letter or digit
/// becomes `-`, as its JS regex does, so a character outside the BMP gives two.
pub(crate) fn claude_folder_name(dir: &str) -> String {
    dir.encode_utf16()
        .map(|unit| match char::from_u32(u32::from(unit)) {
            Some(c) if c.is_ascii_alphanumeric() => c,
            _ => '-',
        })
        .collect()
}

/// The title of a Claude Code conversation and the folder it ran in, from its lines.
/// A title kind that appears more than once counts with its last line.
fn read_claude(bytes: &[u8]) -> (Option<String>, Option<String>) {
    let (mut cwd, mut custom, mut ai, mut summary, mut prompt) = (None, None, None, None, None);
    for line in json_lines(bytes) {
        if cwd.is_none() {
            cwd = text_field(&line, "cwd").map(str::to_owned);
        }
        let slot_and_key = match text_field(&line, "type") {
            Some("custom-title") => (&mut custom, "customTitle"),
            Some("ai-title") => (&mut ai, "aiTitle"),
            Some("summary") => (&mut summary, "summary"),
            Some("last-prompt") => (&mut prompt, "lastPrompt"),
            _ => continue,
        };
        if let Some(text) = text_field(&line, slot_and_key.1) {
            *slot_and_key.0 = Some(clean_title(text));
        }
    }
    let title = [custom, ai, summary, prompt].into_iter().flatten().find_map(|title| title);
    (cwd, title)
}

/// The newest Claude Code conversations of `dir`, at most `limit`.
pub(crate) fn claude_conversations(claude_dir: &Path, dir: &str, limit: usize) -> Vec<Conversation> {
    let folder = claude_dir.join("projects").join(claude_folder_name(dir));
    let is_conversation = |name: &str| name.strip_suffix(".jsonl").is_some_and(is_conversation_id);
    let mut found = Vec::new();
    // Files of another folder with the same name count too, so a request reads a bounded amount.
    for (path, updated) in newest_files(&folder, is_conversation).into_iter().take(MAX_CLAUDE_FILES) {
        if found.len() == limit {
            break;
        }
        let Some(bytes) = read_capped(&path) else { continue };
        let (cwd, title) = read_claude(&bytes);
        // Two folders can share one name (`/a/b-c`, `/a/b/c`), so a recorded folder must match.
        if cwd.as_deref().is_some_and(|cwd| cwd != dir) {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else { continue };
        found.push(Conversation { kind: AgentKind::Claude, id: id.to_owned(), title, updated });
    }
    found
}

/// What the first line of a Codex file says: its conversation id and folder.
type CodexMeta = (String, String);

/// Codex first lines by path. The first line of a file never changes.
static CODEX_MEMO: Mutex<Option<HashMap<PathBuf, CodexMeta>>> = Mutex::new(None);

/// The first line of a file, when it ends within `MAX_FIRST_LINE` bytes.
fn first_line(path: &Path) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path).ok()?.take(MAX_FIRST_LINE as u64 + 1).read_to_end(&mut bytes).ok()?;
    match bytes.iter().position(|byte| *byte == b'\n') {
        Some(end) => bytes.truncate(end),
        None if bytes.len() > MAX_FIRST_LINE => return None,
        None => {}
    }
    Some(bytes)
}

fn parse_codex_meta(path: &Path) -> Option<CodexMeta> {
    let line: Value = serde_json::from_slice(&first_line(path)?).ok()?;
    if text_field(&line, "type") != Some("session_meta") {
        return None;
    }
    let payload = line.get("payload")?;
    let id = text_field(payload, "id").filter(|id| is_conversation_id(id))?;
    Some((id.to_owned(), text_field(payload, "cwd")?.to_owned()))
}

/// The id and folder of a Codex file, from the memo or its first line.
fn codex_meta(path: &Path) -> Option<CodexMeta> {
    let known = CODEX_MEMO.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().and_then(|memo| memo.get(path).cloned());
    if known.is_some() {
        return known;
    }
    let meta = parse_codex_meta(path)?;
    let mut guard = CODEX_MEMO.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let memo = guard.get_or_insert_with(HashMap::new);
    if memo.len() >= MAX_CODEX_MEMO {
        memo.clear();
    }
    memo.insert(path.to_path_buf(), meta.clone());
    Some(meta)
}

/// The names in `dir` that `keep` accepts, in descending order.
fn names_descending(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = entries.flatten().filter(|entry| entry.file_name().to_str().is_some_and(&keep)).map(|entry| entry.path()).collect();
    paths.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    paths
}

/// The newest rollout files, by their date folders and names, at most `max`.
fn recent_rollouts(sessions: &Path, max: usize) -> Vec<PathBuf> {
    let numbered = |name: &str| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit());
    let rollout = |name: &str| name.starts_with("rollout-") && name.ends_with(".jsonl");
    let mut files = Vec::new();
    for year in names_descending(sessions, numbered) {
        for month in names_descending(&year, numbered) {
            for day in names_descending(&month, numbered) {
                files.extend(names_descending(&day, rollout));
                if files.len() >= max {
                    files.truncate(max);
                    return files;
                }
            }
        }
    }
    files
}

/// Codex's thread names by conversation id, from the end of its index; the last entry of an id wins.
fn codex_titles(codex_dir: &Path) -> HashMap<String, String> {
    let mut titles = HashMap::new();
    let Ok(mut file) = File::open(codex_dir.join("session_index.jsonl")) else { return titles };
    let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let cut = len > MAX_INDEX_TAIL;
    if cut && file.seek(SeekFrom::Start(len - MAX_INDEX_TAIL)).is_err() {
        return titles;
    }
    let mut bytes = Vec::new();
    if file.take(MAX_INDEX_TAIL).read_to_end(&mut bytes).is_err() {
        return titles;
    }
    // The cut may fall inside a line; that line is dropped.
    let start = if cut { bytes.iter().position(|byte| *byte == b'\n').map_or(bytes.len(), |end| end + 1) } else { 0 };
    for line in json_lines(&bytes[start..]) {
        if let (Some(id), Some(name)) = (text_field(&line, "id"), text_field(&line, "thread_name")) {
            match clean_title(name) {
                Some(title) => titles.insert(id.to_owned(), title),
                None => titles.remove(id),
            };
        }
    }
    titles
}

/// The newest Codex conversations of `dir`, at most `limit`, among the newest files.
pub(crate) fn codex_conversations(codex_dir: &Path, dir: &str, limit: usize) -> Vec<Conversation> {
    let mut found = Vec::new();
    for path in recent_rollouts(&codex_dir.join("sessions"), MAX_CODEX_FILES) {
        if found.len() == limit {
            break;
        }
        let Some((id, cwd)) = codex_meta(&path) else { continue };
        if cwd != dir {
            continue;
        }
        let Some(updated) = modified_ms(&path) else { continue };
        found.push(Conversation { kind: AgentKind::Codex, id, title: None, updated });
    }
    if !found.is_empty() {
        let titles = codex_titles(codex_dir);
        for item in &mut found {
            item.title = titles.get(&item.id).cloned();
        }
    }
    found
}

/// The text of a Gemini message: `content` as a string, or the `text` parts of a list joined.
fn gemini_text(message: &Value) -> Option<String> {
    match message.get("content")? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => Some(parts.iter().filter_map(|part| text_field(part, "text")).collect()),
        _ => None,
    }
}

/// The title from a message list: the text of the first user message.
fn first_user_title<'a>(messages: impl IntoIterator<Item = &'a Value>) -> Option<String> {
    let first = messages.into_iter().find(|message| text_field(message, "type") == Some("user"))?;
    clean_title(&gemini_text(first)?)
}

/// The head line of a Gemini `.jsonl` chat, its last summary, and its first user message's title,
/// read one line at a time up to `MAX_FILE_BYTES`. None when the first line is no JSON.
fn read_gemini_lines(path: &Path) -> Option<(Value, Option<String>, Option<String>)> {
    let mut reader = BufReader::new(File::open(path).ok()?.take(MAX_FILE_BYTES));
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line).ok()?;
    let head: Value = serde_json::from_slice(line.strip_suffix(b"\n").unwrap_or(&line)).ok()?;
    let is_user = |value: &Value| text_field(value, "type") == Some("user");
    // Only the first user message names the chat, even when its text is empty.
    let (mut summary, mut user) = (None, is_user(&head).then(|| first_user_title([&head])));
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let Ok(value) = serde_json::from_slice::<Value>(line.strip_suffix(b"\n").unwrap_or(&line)) else { continue };
        if let Some(text) = text_field(&value, "summary") {
            summary = Some(text.to_owned());
        }
        if user.is_none() && is_user(&value) {
            user = Some(first_user_title([&value]));
        }
    }
    Some((head, summary, user.flatten()))
}

/// A Gemini conversation file: its id and title, or None for a file that is no main conversation.
fn read_gemini(path: &Path) -> Option<(String, Option<String>)> {
    let (head, summary, user) = if path.extension().is_some_and(|extension| extension == "json") {
        let whole: Value = serde_json::from_slice(&read_capped(path)?).ok()?;
        let summary = text_field(&whole, "summary").map(str::to_owned);
        let user = first_user_title(whole.get("messages").and_then(Value::as_array).into_iter().flatten());
        (whole, summary, user)
    } else {
        read_gemini_lines(path)?
    };
    if text_field(&head, "kind") == Some("subagent") {
        return None;
    }
    let id = text_field(&head, "sessionId").filter(|id| is_conversation_id(id))?;
    let title = summary.as_deref().and_then(clean_title).or(user);
    Some((id.to_owned(), title))
}

/// The folders under `tmp` that hold the conversations of `dir`: the name `projects.json`
/// gives it, else every folder whose `.project_root` is `dir`.
fn gemini_projects(gemini_dir: &Path, dir: &str) -> Vec<PathBuf> {
    let tmp = gemini_dir.join("tmp");
    let named = read_capped(&gemini_dir.join("projects.json"))
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| text_field(value.get("projects")?, dir).map(str::to_owned));
    if let Some(name) = named {
        // A name is one plain folder name, never a path out of `tmp`.
        let mut parts = Path::new(&name).components();
        return match (parts.next(), parts.next()) {
            (Some(Component::Normal(part)), None) => vec![tmp.join(part)],
            _ => Vec::new(),
        };
    }
    let Ok(entries) = fs::read_dir(&tmp) else { return Vec::new() };
    let mut folders: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    folders.sort();
    folders.truncate(MAX_GEMINI_ROOTS);
    folders.retain(|folder| fs::read_to_string(folder.join(".project_root")).is_ok_and(|root| root.trim_end() == dir));
    folders
}

/// The newest Gemini CLI conversations of `dir`, at most `limit`. `gemini_dir` is the `.gemini` folder.
pub(crate) fn gemini_conversations(gemini_dir: &Path, dir: &str, limit: usize) -> Vec<Conversation> {
    let is_chat = |name: &str| name.starts_with("session-") && (name.ends_with(".jsonl") || name.ends_with(".json"));
    let mut files: Vec<(PathBuf, u64)> = gemini_projects(gemini_dir, dir).iter().flat_map(|project| newest_files(&project.join("chats"), is_chat)).collect();
    files.sort_by_key(|file| std::cmp::Reverse(file.1));
    let mut found = Vec::new();
    for (path, updated) in files {
        if found.len() == limit {
            break;
        }
        if let Some((id, title)) = read_gemini(&path) {
            found.push(Conversation { kind: AgentKind::Gemini, id, title, updated });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::time::{Duration, UNIX_EPOCH};

    use std::sync::Arc;

    use axum::http::header;

    use super::*;
    use crate::server::tests::test_state;

    /// A conversation id of 8 or more hex digits and dashes.
    fn id(n: u32) -> String {
        format!("{n:08x}-0000-4000-8000-000000000000")
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// Sets the modified time to `secs` seconds after the epoch.
    fn age(path: &Path, secs: u64) {
        let file = File::options().write(true).open(path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(secs)).unwrap();
    }

    fn claude_file(root: &Path, dir: &str, name: &str, text: &str) -> PathBuf {
        let path = root.join("projects").join(claude_folder_name(dir)).join(name);
        write(&path, text);
        path
    }

    fn titles(list: &[Conversation]) -> Vec<Option<&str>> {
        list.iter().map(|item| item.title.as_deref()).collect()
    }

    #[test]
    fn claude_folder_names() {
        assert_eq!(claude_folder_name("/Users/p/westerm/.claude/worktrees/x"), "-Users-p-westerm--claude-worktrees-x");
        assert_eq!(claude_folder_name("/a/b-c"), claude_folder_name("/a/b/c"));
        // Claude Code's JS regex sees UTF-16 code units: an emoji is two of them.
        assert_eq!(claude_folder_name("/a/\u{1F600}b"), "-a---b");
        assert_eq!(claude_folder_name("/a/\u{D55C}b"), "-a--b");
    }

    #[test]
    fn claude_scan_stops_at_its_limit() {
        let root = tempfile::tempdir().unwrap();
        // "/a/b/c" and "/a/b-c" share one folder name; the newest 50 files belong to the other folder.
        for n in 0..=MAX_CLAUDE_FILES as u32 {
            let cwd = if n == 0 { "/a/b/c" } else { "/a/b-c" };
            let line = format!("{{\"cwd\":\"{cwd}\"}}\n");
            let path = claude_file(root.path(), "/a/b/c", &format!("{}.jsonl", id(n + 1)), &line);
            age(&path, 100 + u64::from(n));
        }
        assert!(claude_conversations(root.path(), "/a/b/c", 5).is_empty());
        // One file fewer is still in reach.
        let folder = root.path().join("projects").join(claude_folder_name("/a/b/c"));
        fs::remove_file(folder.join(format!("{}.jsonl", id(2)))).unwrap();
        assert_eq!(claude_conversations(root.path(), "/a/b/c", 5).len(), 1);
    }

    #[test]
    fn claude_titles_follow_their_order() {
        let root = tempfile::tempdir().unwrap();
        let dir = "/w";
        let all = r#"{"type":"summary","summary":"S"}
{"type":"ai-title","aiTitle":"first ai"}
{"type":"last-prompt","lastPrompt":"L"}
{"type":"custom-title","customTitle":"Mine"}
{"type":"ai-title","aiTitle":"last ai"}
"#;
        claude_file(root.path(), dir, &format!("{}.jsonl", id(1)), all);
        let no_custom = all.lines().filter(|line| !line.contains("custom")).collect::<Vec<_>>().join("\n");
        claude_file(root.path(), dir, &format!("{}.jsonl", id(2)), &no_custom);
        claude_file(root.path(), dir, &format!("{}.jsonl", id(3)), "{\"type\":\"summary\",\"summary\":\"S\"}\n{\"type\":\"last-prompt\",\"lastPrompt\":\"L\"}\n");
        claude_file(root.path(), dir, &format!("{}.jsonl", id(4)), "{\"type\":\"last-prompt\",\"lastPrompt\":\"L\"}\n");
        claude_file(root.path(), dir, &format!("{}.jsonl", id(5)), "{\"type\":\"user\"}\n");
        for n in 1..=5 {
            age(&root.path().join("projects").join(claude_folder_name(dir)).join(format!("{}.jsonl", id(n))), 100 - n as u64);
        }
        let list = claude_conversations(root.path(), dir, 10);
        assert_eq!(titles(&list), [Some("Mine"), Some("last ai"), Some("S"), Some("L"), None]);
        assert_eq!(list[0].id, id(1));
        assert_eq!(list[0].kind, AgentKind::Claude);
        assert_eq!(list[0].updated, 99_000);
    }

    #[test]
    fn titles_are_one_short_line() {
        let root = tempfile::tempdir().unwrap();
        let long = "x".repeat(300);
        claude_file(root.path(), "/w", &format!("{}.jsonl", id(1)), "{\"type\":\"ai-title\",\"aiTitle\":\"  one\\r\\ntwo\\nthree \"}\n");
        claude_file(root.path(), "/w", &format!("{}.jsonl", id(2)), &format!("{{\"type\":\"ai-title\",\"aiTitle\":\"{long}\"}}\n"));
        claude_file(root.path(), "/w", &format!("{}.jsonl", id(3)), &format!("{{\"type\":\"ai-title\",\"aiTitle\":\"{}\"}}\n", "y".repeat(120)));
        age(&root.path().join("projects/-w").join(format!("{}.jsonl", id(1))), 3);
        age(&root.path().join("projects/-w").join(format!("{}.jsonl", id(2))), 2);
        age(&root.path().join("projects/-w").join(format!("{}.jsonl", id(3))), 1);
        let list = claude_conversations(root.path(), "/w", 10);
        assert_eq!(list[0].title.as_deref(), Some("one two three"));
        assert_eq!(list[1].title.as_deref(), Some("x".repeat(120).as_str()));
        assert_eq!(list[2].title.as_deref(), Some("y".repeat(120).as_str()));
    }

    #[test]
    fn claude_lists_the_newest_first() {
        let root = tempfile::tempdir().unwrap();
        for (n, secs) in [(1, 10), (2, 30), (3, 20)] {
            let path = claude_file(root.path(), "/w", &format!("{}.jsonl", id(n)), "{}\n");
            age(&path, secs);
        }
        age(&claude_file(root.path(), "/w", "notes.jsonl", "{}\n"), 99);
        let list = claude_conversations(root.path(), "/w", 10);
        assert_eq!(list.iter().map(|item| item.id.clone()).collect::<Vec<_>>(), [id(2), id(3), id(1)]);
        assert_eq!(claude_conversations(root.path(), "/w", 2).len(), 2);
        assert!(claude_conversations(root.path(), "/missing", 5).is_empty());
    }

    #[test]
    fn claude_skips_conversations_of_another_folder() {
        let root = tempfile::tempdir().unwrap();
        // "/a/b-c" and "/a/b/c" share one folder name.
        claude_file(root.path(), "/a/b/c", &format!("{}.jsonl", id(1)), "{\"type\":\"user\",\"cwd\":\"/a/b-c\"}\n{\"cwd\":\"/a/b/c\"}\n");
        claude_file(root.path(), "/a/b/c", &format!("{}.jsonl", id(2)), "{\"type\":\"user\"}\n{\"cwd\":\"/a/b/c\"}\n");
        claude_file(root.path(), "/a/b/c", &format!("{}.jsonl", id(3)), "{\"type\":\"summary\",\"summary\":\"none\"}\n");
        let list = claude_conversations(root.path(), "/a/b/c", 10);
        let mut ids: Vec<String> = list.into_iter().map(|item| item.id).collect();
        ids.sort();
        assert_eq!(ids, [id(2), id(3)]);
    }

    fn codex_file(root: &Path, name: &str, first: &str) -> PathBuf {
        let path = root.join("sessions/2026/11/06").join(name);
        write(&path, first);
        path
    }

    fn meta(id: &str, cwd: &str) -> String {
        format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cwd\":\"{cwd}\"}}}}\n{{\"type\":\"event_msg\"}}\n")
    }

    #[test]
    fn codex_matches_the_folder_and_reads_titles() {
        let root = tempfile::tempdir().unwrap();
        let (mine, other) = (id(1), id(2));
        codex_file(root.path(), &format!("rollout-2026-11-06T10-00-00-{mine}.jsonl"), &meta(&mine, "/w"));
        codex_file(root.path(), &format!("rollout-2026-11-06T11-00-00-{other}.jsonl"), &meta(&other, "/x"));
        codex_file(root.path(), "rollout-bad.jsonl", &meta("nothex", "/w"));
        write(
            &root.path().join("session_index.jsonl"),
            &format!("{{\"id\":\"{mine}\",\"thread_name\":\"Old name\"}}\nnot json\n{{\"id\":\"{other}\",\"thread_name\":\"Other\"}}\n{{\"id\":\"{mine}\",\"thread_name\":\"Fix fonts\"}}\n"),
        );
        let list = codex_conversations(root.path(), "/w", 5);
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].kind, list[0].id.as_str(), list[0].title.as_deref()), (AgentKind::Codex, mine.as_str(), Some("Fix fonts")));
        // Without an index there is no title.
        fs::remove_file(root.path().join("session_index.jsonl")).unwrap();
        assert_eq!(codex_conversations(root.path(), "/w", 5)[0].title, None);
    }

    #[test]
    fn codex_walks_the_newest_days_first_and_honors_the_limit() {
        let root = tempfile::tempdir().unwrap();
        for (day, n) in [("2026/10/30", 1), ("2026/11/06", 2), ("2025/12/31", 3)] {
            let path = root.path().join("sessions").join(day).join(format!("rollout-x-{}.jsonl", id(n)));
            write(&path, &meta(&id(n), "/w"));
        }
        let list = codex_conversations(root.path(), "/w", 2);
        assert_eq!(list.iter().map(|item| item.id.clone()).collect::<Vec<_>>(), [id(2), id(1)]);
    }

    #[test]
    fn codex_scan_stops_at_its_limit() {
        let root = tempfile::tempdir().unwrap();
        // The oldest of 501 files is the only one in the folder.
        for n in 0..501u32 {
            let cwd = if n == 0 { "/w" } else { "/x" };
            codex_file(root.path(), &format!("rollout-2026-11-06T10-00-{n:04}-{}.jsonl", id(n + 1)), &meta(&id(n + 1), cwd));
        }
        assert!(codex_conversations(root.path(), "/w", 5).is_empty());
        // One file fewer is still in reach.
        fs::remove_file(root.path().join("sessions/2026/11/06").join(format!("rollout-2026-11-06T10-00-0500-{}.jsonl", id(501)))).unwrap();
        assert_eq!(codex_conversations(root.path(), "/w", 5).len(), 1);
    }

    fn gemini_chat(root: &Path, project: &str, name: &str, text: &str) -> PathBuf {
        let path = root.join("tmp").join(project).join("chats").join(name);
        write(&path, text);
        path
    }

    fn gemini_head(id: &str, kind: &str) -> String {
        format!("{{\"sessionId\":\"{id}\",\"projectHash\":\"h\",\"startTime\":\"2026-11-06T10:00:00Z\",\"kind\":\"{kind}\"}}\n")
    }

    #[test]
    fn gemini_finds_the_project_by_name_or_root() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("projects.json"), r#"{"projects":{"/w":"w"}}"#);
        gemini_chat(root.path(), "w", &format!("session-2026-11-06T10-00-{}.jsonl", &id(1)[..8]), &gemini_head(&id(1), "main"));
        write(&root.path().join("tmp/other/.project_root"), "/v\n");
        gemini_chat(root.path(), "other", &format!("session-2026-11-06T10-00-{}.jsonl", &id(2)[..8]), &gemini_head(&id(2), "main"));
        let mine = gemini_conversations(root.path(), "/w", 5);
        assert_eq!(mine.len(), 1);
        assert_eq!((mine[0].kind, mine[0].id.as_str()), (AgentKind::Gemini, id(1).as_str()));
        let found = gemini_conversations(root.path(), "/v", 5);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, id(2));
        assert!(gemini_conversations(root.path(), "/nowhere", 5).is_empty());
    }

    #[test]
    fn gemini_titles_and_subagents() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("projects.json"), r#"{"projects":{"/w":"w"}}"#);
        let user = "{\"type\":\"user\",\"content\":[{\"text\":\"fix \"},{\"text\":\"the fonts\"}]}\n";
        let chat = |n: u32, name: &str, text: String| {
            let path = gemini_chat(root.path(), "w", name, &text);
            age(&path, 1000 - n as u64);
        };
        chat(1, "session-a.jsonl", format!("{}{user}", gemini_head(&id(1), "main")));
        chat(2, "session-b.jsonl", format!("{}{user}{{\"summary\":\"Font fix\"}}\n{{\"type\":\"gemini\",\"content\":\"hi\"}}\n", gemini_head(&id(2), "main")));
        chat(3, "session-c.jsonl", format!("{}{user}", gemini_head(&id(3), "subagent")));
        chat(4, "session-d.jsonl", format!("{}{{\"type\":\"user\",\"content\":\"plain\\ntext\"}}\n", gemini_head(&id(4), "main")));
        chat(5, "session-e.json", format!("{{\"sessionId\":\"{}\",\"summary\":\"Old\",\"messages\":[]}}", id(5)));
        chat(6, "session-f.json", format!("{{\"sessionId\":\"{}\",\"messages\":[{{\"type\":\"user\",\"content\":[{{\"text\":\"legacy ask\"}}]}}]}}", id(6)));
        chat(7, "notes.jsonl", gemini_head(&id(7), "main"));
        // A subagent folder under chats/ is not read.
        write(&root.path().join("tmp/w/chats").join(id(1)).join("session-sub.jsonl"), &gemini_head(&id(8), "main"));
        let list = gemini_conversations(root.path(), "/w", 10);
        assert_eq!(list.iter().map(|item| item.id.clone()).collect::<Vec<_>>(), [id(1), id(2), id(4), id(5), id(6)]);
        assert_eq!(titles(&list), [Some("fix the fonts"), Some("Font fix"), Some("plain text"), Some("Old"), Some("legacy ask")]);
    }

    #[test]
    fn broken_lines_and_files_are_skipped() {
        let root = tempfile::tempdir().unwrap();
        claude_file(root.path(), "/w", &format!("{}.jsonl", id(1)), "{broken\n\n{\"type\":\"ai-title\",\"aiTitle\":\"Fine\"}\n[1,2]\n\"x\"\n");
        claude_file(root.path(), "/w", &format!("{}.jsonl", id(2)), "");
        assert_eq!(claude_conversations(root.path(), "/w", 5).len(), 2);

        let big = format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{}\",\"cwd\":\"/w\",\"pad\":\"{}\"}}}}\n", id(3), "p".repeat(1024 * 1024));
        codex_file(root.path(), &format!("rollout-big-{}.jsonl", id(3)), &big);
        codex_file(root.path(), &format!("rollout-empty-{}.jsonl", id(4)), "");
        codex_file(root.path(), &format!("rollout-junk-{}.jsonl", id(5)), "{nope\n");
        assert!(codex_conversations(root.path(), "/w", 5).is_empty());

        write(&root.path().join("projects.json"), "{broken");
        gemini_chat(root.path(), "w", "session-a.jsonl", "");
        gemini_chat(root.path(), "w", "session-b.json", "{broken");
        gemini_chat(root.path(), "w", "session-c.jsonl", "{\"sessionId\":\"nothex!\"}\n");
        assert!(gemini_conversations(root.path(), "/w", 5).is_empty());
    }

    #[test]
    fn a_projects_file_is_read_up_to_its_cap() {
        let root = tempfile::tempdir().unwrap();
        // Cut at the cap, the file is no JSON: the `.project_root` files decide.
        let pad = "p".repeat(MAX_FILE_BYTES as usize);
        write(&root.path().join("projects.json"), &format!("{{\"projects\":{{\"/w\":\"named\"}},\"pad\":\"{pad}\"}}"));
        gemini_chat(root.path(), "named", "session-a.jsonl", &gemini_head(&id(1), "main"));
        write(&root.path().join("tmp/rooted/.project_root"), "/w\n");
        gemini_chat(root.path(), "rooted", "session-b.jsonl", &gemini_head(&id(2), "main"));
        let list = gemini_conversations(root.path(), "/w", 5);
        assert_eq!(list.iter().map(|item| item.id.clone()).collect::<Vec<_>>(), [id(2)]);
    }

    #[test]
    fn gemini_reads_a_long_chat_line_by_line() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("projects.json"), r#"{"projects":{"/w":"w"}}"#);
        let mut text = gemini_head(&id(1), "main");
        text.push_str("not json\n");
        text.push_str("{\"type\":\"user\",\"content\":\"first ask\"}\n");
        for n in 0..2000 {
            text.push_str(&format!("{{\"type\":\"gemini\",\"content\":\"reply {n}\"}}\n"));
        }
        text.push_str("{\"summary\":\"Old summary\"}\n");
        text.push_str("{\"type\":\"user\",\"content\":\"later ask\"}\n{\"summary\":\"New summary\"}");
        gemini_chat(root.path(), "w", "session-a.jsonl", &text);
        let list = gemini_conversations(root.path(), "/w", 5);
        assert_eq!(titles(&list), [Some("New summary")]);
        // Without a summary, the first user message names it.
        let cut = text.find("{\"summary\"").unwrap();
        gemini_chat(root.path(), "w", "session-a.jsonl", &text[..cut]);
        assert_eq!(titles(&gemini_conversations(root.path(), "/w", 5)), [Some("first ask")]);
        // The first line must be the head.
        gemini_chat(root.path(), "w", "session-a.jsonl", &format!("junk\n{text}"));
        assert!(gemini_conversations(root.path(), "/w", 5).is_empty());
    }

    #[test]
    fn a_response_lists_ten() {
        let (claude, codex, gemini) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        for n in 1..=12u32 {
            age(&claude_file(claude.path(), "/w", &format!("{}.jsonl", id(n)), "{}\n"), u64::from(n));
        }
        let folders = ToolFolders { claude: claude.path().to_owned(), codex: codex.path().to_owned(), gemini: gemini.path().to_owned() };
        let body = history_json(&folders, "/w");
        let listed = body["conversations"].as_array().unwrap();
        assert_eq!(listed.len(), 10);
        assert_eq!(listed[0]["id"], id(12));
    }

    #[test]
    fn a_project_name_cannot_leave_the_tmp_folder() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("projects.json"), r#"{"projects":{"/w":"../escape"}}"#);
        gemini_chat(root.path(), "../escape", "session-a.jsonl", &gemini_head(&id(1), "main"));
        assert!(gemini_conversations(root.path(), "/w", 5).is_empty());
    }

    #[test]
    fn tool_folders_follow_their_environment() {
        let home = Path::new("/home/p");
        assert_eq!(codex_dir(None, home), PathBuf::from("/home/p/.codex"));
        assert_eq!(codex_dir(Some("".into()), home), PathBuf::from("/home/p/.codex"));
        assert_eq!(codex_dir(Some("/c".into()), home), PathBuf::from("/c"));
        assert_eq!(gemini_dir(None, home), PathBuf::from("/home/p/.gemini"));
        assert_eq!(gemini_dir(Some("/g".into()), home), PathBuf::from("/g/.gemini"));
    }

    #[test]
    fn the_merge_keeps_the_newest_few() {
        let at = |kind, n: u32, updated| Conversation { kind, id: id(n), title: None, updated };
        let claude = vec![at(AgentKind::Claude, 1, 50), at(AgentKind::Claude, 2, 10)];
        let codex = vec![at(AgentKind::Codex, 3, 40)];
        let gemini = vec![at(AgentKind::Gemini, 4, 60)];
        let merged = merge([claude, codex, gemini], 3);
        assert_eq!(merged.iter().map(|item| item.updated).collect::<Vec<_>>(), [60, 50, 40]);
    }

    #[test]
    fn conversations_serialize_without_their_text() {
        let item = Conversation { kind: AgentKind::Codex, id: id(1), title: None, updated: 5 };
        assert_eq!(serde_json::to_value(&item).unwrap(), json!({"kind":"codex","id":id(1),"title":null,"updated":5}));
    }

    #[tokio::test]
    async fn history_needs_an_absolute_dir() {
        let state: Shared = Arc::new(test_state(&["http://127.0.0.1:4317"]));
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "127.0.0.1:4317".parse().unwrap());
        let ask = |dir: &str, headers: HeaderMap| history(State(state.clone()), headers, Query(HistoryQuery { dir: dir.to_owned() }));
        assert_eq!(ask("relative", headers.clone()).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(ask("", headers.clone()).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(ask("/a\0b", headers.clone()).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(ask(&format!("/{}", "a".repeat(4096)), headers.clone()).await.status(), StatusCode::BAD_REQUEST);
        let mut cross = headers.clone();
        cross.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert_eq!(ask("/w", cross).await.status(), StatusCode::FORBIDDEN);
        let mut evil = headers.clone();
        evil.insert(header::HOST, "evil.example:4317".parse().unwrap());
        assert_eq!(ask("/w", evil).await.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn history_merges_the_three_tools_newest_first() {
        let (claude, codex, gemini) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        age(&claude_file(claude.path(), "/w", &format!("{}.jsonl", id(1)), "{\"type\":\"ai-title\",\"aiTitle\":\"From Claude\"}\n"), 10);
        age(&codex_file(codex.path(), &format!("rollout-x-{}.jsonl", id(2)), &meta(&id(2), "/w")), 30);
        write(&gemini.path().join("projects.json"), r#"{"projects":{"/w":"w"}}"#);
        age(&gemini_chat(gemini.path(), "w", "session-a.jsonl", &gemini_head(&id(3), "main")), 20);
        let folders = ToolFolders { claude: claude.path().to_owned(), codex: codex.path().to_owned(), gemini: gemini.path().to_owned() };
        assert_eq!(
            history_json(&folders, "/w"),
            json!({"conversations": [
                {"kind": "codex", "id": id(2), "title": null, "updated": 30_000},
                {"kind": "gemini", "id": id(3), "title": null, "updated": 20_000},
                {"kind": "claude", "id": id(1), "title": "From Claude", "updated": 10_000},
            ]})
        );
        assert_eq!(history_json(&folders, "/other"), json!({"conversations": []}));
    }
}
