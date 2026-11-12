//! The Git plugin's server routes, under `/api/plugins/git`. Commands run in
//! the selected worktree, with literal path arguments and bounded output. No
//! shell is involved.
//!
//! Unlike the other plugins' routes, these answer whether or not the plugin
//! is installed: a remote window's server and a tunnel page serve them too,
//! and neither has a plugin manager. Installing only shows the panel.

use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::server::{docs, host_allowed, is_json, no_store, origin_allowed, read_allowed, Shared};

pub(in crate::server) const MAX_BODY_BYTES: usize = 64 * 1024;
const MAX_OUTPUT: u64 = 2 * 1024 * 1024;
const MAX_FILES: usize = 2000;
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
struct GitError(StatusCode, String);

impl IntoResponse for GitError {
    fn into_response(self) -> Response {
        (self.0, no_store(Json(json!({ "error": self.1 })))).into_response()
    }
}

fn invalid(message: &str) -> GitError {
    GitError(StatusCode::BAD_REQUEST, message.to_owned())
}

fn failed(message: &str) -> GitError {
    GitError(StatusCode::INTERNAL_SERVER_ERROR, message.to_owned())
}

/// The path every route starts with; the remote window and the tunnel name it too.
pub(in crate::server) const PREFIX: &str = "/api/plugins/git/";

pub(in crate::server) fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/plugins/git/status", get(status))
        .route("/api/plugins/git/diff", get(diff))
        .route("/api/plugins/git/commit", get(commit_detail))
        .route("/api/plugins/git/commit-diff", get(commit_diff))
        .route("/api/plugins/git/action", post(action).layer(DefaultBodyLimit::max(MAX_BODY_BYTES)))
}

#[derive(Deserialize)]
struct DirectoryQuery {
    dir: String,
}

async fn status(State(state): State<Shared>, headers: HeaderMap, Query(query): Query<DirectoryQuery>) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match repository_status(&query.dir).await {
        Ok(body) => no_store(Json(body)),
        Err(error) => error.into_response(),
    }
}

#[derive(Deserialize)]
struct DiffQuery {
    dir: String,
    path: String,
    #[serde(default)]
    staged: bool,
}

async fn diff(State(state): State<Shared>, headers: HeaderMap, Query(query): Query<DiffQuery>) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match repository_diff(&query.dir, &query.path, query.staged).await {
        Ok(text) => no_store(Json(json!({ "text": text }))),
        Err(error) => error.into_response(),
    }
}

#[derive(Deserialize)]
struct CommitQuery {
    dir: String,
    hash: String,
}

async fn commit_detail(State(state): State<Shared>, headers: HeaderMap, Query(query): Query<CommitQuery>) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let result = async {
        let repo = Repository::open(&query.dir).await?;
        read_commit(&repo, &query.hash).await
    }
    .await;
    match result {
        Ok(body) => no_store(Json(body)),
        Err(error) => error.into_response(),
    }
}

#[derive(Deserialize)]
struct CommitDiffQuery {
    dir: String,
    hash: String,
    path: String,
}

async fn commit_diff(State(state): State<Shared>, headers: HeaderMap, Query(query): Query<CommitDiffQuery>) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match committed_diff(&query.dir, &query.hash, &query.path).await {
        Ok(text) => no_store(Json(json!({ "text": text }))),
        Err(error) => error.into_response(),
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
enum Operation {
    Stage { paths: Vec<String> },
    Unstage { paths: Vec<String> },
    Commit { message: String },
    Switch { branch: String },
    Pull,
    Push,
}

#[derive(Deserialize)]
struct ActionBody {
    dir: String,
    #[serde(flatten)]
    operation: Operation,
}

async fn action(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    if !is_json(&headers) || !(state.trusted || (host_allowed(&state, &headers) && origin_allowed(&state, &headers))) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let body: ActionBody = match serde_json::from_slice(&body) {
        Ok(body) => body,
        Err(_) => return invalid("Invalid Git operation").into_response(),
    };
    match change_repository(&body.dir, body.operation).await {
        Ok(()) => no_store(Json(json!({}))),
        Err(error) => error.into_response(),
    }
}

struct Repository(PathBuf);

impl Repository {
    async fn open(dir: &str) -> Result<Self, GitError> {
        if dir.len() > 4096 || dir.contains('\0') || !Path::new(dir).is_absolute() {
            return Err(invalid("Choose an absolute repository folder"));
        }
        let folder = tokio::fs::canonicalize(dir).await.map_err(|_| invalid("Folder does not exist"))?;
        let repo = Self(folder);
        let output = repo.run(&["rev-parse", "--show-toplevel"]).await?;
        if !output.status.success() {
            return Err(GitError(StatusCode::NOT_FOUND, "This folder is not in a Git worktree".to_owned()));
        }
        let root = utf8(output.stdout)?;
        let root = root.strip_suffix('\n').unwrap_or(&root);
        let root = if cfg!(windows) { root.strip_suffix('\r').unwrap_or(root) } else { root };
        // Git and canonicalize spell Windows roots differently (D:/... vs \\?\D:\...).
        // Compare canonical paths on both sides of the repository boundary check.
        let root = tokio::fs::canonicalize(root).await.map_err(|_| invalid("Repository folder is no longer available"))?;
        Ok(Self(root))
    }

    async fn run(&self, args: &[&str]) -> Result<std::process::Output, GitError> {
        let mut command = Command::new("git");
        command
            .current_dir(&self.0)
            .args(["--no-pager", "--literal-pathspecs", "-c", "color.ui=false"])
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "Never")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // A dashboard started from a Git hook must still use the chosen worktree.
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
        ] {
            command.env_remove(name);
        }
        let mut child = command.spawn().map_err(|error| failed(&format!("Cannot start Git: {error}")))?;
        let stdout = child.stdout.take().ok_or_else(|| failed("Git stdout unavailable"))?;
        let stderr = child.stderr.take().ok_or_else(|| failed("Git stderr unavailable"))?;
        let work = async {
            let (status, stdout, stderr) = tokio::try_join!(
                async { child.wait().await.map_err(|error| failed(&error.to_string())) },
                read_output(stdout),
                read_output(stderr),
            )?;
            Ok(std::process::Output { status, stdout, stderr })
        };
        tokio::time::timeout(TIMEOUT, work).await.map_err(|_| {
            GitError(StatusCode::GATEWAY_TIMEOUT, "Git timed out. Check the repository and refresh before retrying.".to_owned())
        })?
    }

    async fn checked(&self, args: &[&str]) -> Result<Vec<u8>, GitError> {
        let output = self.run(args).await?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr).chars().take(2000).collect();
            return Err(GitError(StatusCode::CONFLICT, message));
        }
        Ok(output.stdout)
    }

    async fn has_head(&self) -> Result<bool, GitError> {
        Ok(self.run(&["rev-parse", "--verify", "HEAD"]).await?.status.success())
    }

    async fn files(&self) -> Result<Vec<ChangedFile>, GitError> {
        parse_status(&self.checked(&["status", "--porcelain=v1", "-z", "--untracked-files=all", "--ignore-submodules=none"]).await?)
    }
}

async fn read_output(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>, GitError> {
    let mut bytes = Vec::new();
    reader.take(MAX_OUTPUT + 1).read_to_end(&mut bytes).await.map_err(|error| failed(&error.to_string()))?;
    if bytes.len() as u64 > MAX_OUTPUT {
        return Err(GitError(StatusCode::PAYLOAD_TOO_LARGE, "Git output is too large to display".to_owned()));
    }
    Ok(bytes)
}

fn utf8(bytes: Vec<u8>) -> Result<String, GitError> {
    String::from_utf8(bytes).map_err(|_| invalid("Git output contains a filename that is not UTF-8"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangedFile {
    path: String,
    old_path: Option<String>,
    index: String,
    worktree: String,
    conflict: bool,
}

fn parse_status(bytes: &[u8]) -> Result<Vec<ChangedFile>, GitError> {
    let mut entries = bytes.split(|byte| *byte == 0).filter(|entry| !entry.is_empty());
    let mut files = Vec::new();
    while let Some(entry) = entries.next() {
        if entry.len() < 4 || entry[2] != b' ' {
            return Err(failed("Invalid Git status output"));
        }
        let index = entry[0] as char;
        let worktree = entry[1] as char;
        let old_path = if matches!(index, 'R' | 'C') || matches!(worktree, 'R' | 'C') {
            Some(utf8(entries.next().ok_or_else(|| failed("Missing rename source"))?.to_vec())?)
        } else {
            None
        };
        files.push(ChangedFile {
            path: utf8(entry[3..].to_vec())?,
            old_path,
            index: index.to_string(),
            worktree: worktree.to_string(),
            conflict: index == 'U' || worktree == 'U' || matches!((index, worktree), ('A', 'A') | ('D', 'D')),
        });
        if files.len() > MAX_FILES {
            return Err(GitError(StatusCode::PAYLOAD_TOO_LARGE, "Too many changed files to display".to_owned()));
        }
    }
    Ok(files)
}

#[derive(Serialize)]
struct Commit {
    hash: String,
    message: String,
    author: String,
    time: String,
}

#[derive(Serialize)]
struct RepositoryStatus {
    root: String,
    branch: Option<String>,
    head: Option<String>,
    branches: Vec<String>,
    upstream: Option<String>,
    ahead: usize,
    behind: usize,
    files: Vec<ChangedFile>,
    commits: Vec<Commit>,
}

async fn repository_status(dir: &str) -> Result<RepositoryStatus, GitError> {
    let repo = Repository::open(dir).await?;
    let branch = repo.run(&["symbolic-ref", "--quiet", "HEAD"]).await?;
    let branch = if branch.status.success() {
        Some(utf8(branch.stdout)?.trim_end().strip_prefix("refs/heads/").ok_or_else(|| failed("Invalid branch reference"))?.to_owned())
    } else {
        None
    };
    let has_head = repo.has_head().await?;
    let head = if has_head { Some(utf8(repo.checked(&["rev-parse", "--short", "HEAD"]).await?)?.trim_end().to_owned()) } else { None };
    let branches =
        utf8(repo.checked(&["for-each-ref", "--format=%(refname:lstrip=2)", "refs/heads/"]).await?)?.lines().map(str::to_owned).collect();
    let mut result = RepositoryStatus {
        root: docs::shown_path(&repo.0),
        branch,
        head,
        branches,
        upstream: None,
        ahead: 0,
        behind: 0,
        files: repo.files().await?,
        commits: Vec::new(),
    };
    if !has_head {
        return Ok(result);
    }
    let upstream = repo.run(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]).await?;
    if upstream.status.success() {
        result.upstream = Some(utf8(upstream.stdout)?.trim_end().to_owned());
        let counts = utf8(repo.checked(&["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]).await?)?;
        let mut counts = counts.split_whitespace();
        result.ahead = counts.next().and_then(|value| value.parse().ok()).unwrap_or(0);
        result.behind = counts.next().and_then(|value| value.parse().ok()).unwrap_or(0);
    }
    let log = repo.checked(&["log", "-15", "--format=%H%x00%s%x00%an%x00%aI%x00", "--no-show-signature"]).await?;
    let text = utf8(log)?;
    let mut fields = text.split('\0');
    while let (Some(hash), Some(message), Some(author), Some(time)) = (fields.next(), fields.next(), fields.next(), fields.next()) {
        result.commits.push(Commit {
            hash: hash.trim_start_matches('\n').to_owned(),
            message: message.to_owned(),
            author: author.to_owned(),
            time: time.to_owned(),
        });
    }
    Ok(result)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommitFile {
    path: String,
    old_path: Option<String>,
    status: String,
}

#[derive(Serialize)]
struct CommitDetail {
    hash: String,
    message: String,
    author: String,
    email: String,
    time: String,
    parents: Vec<String>,
    files: Vec<CommitFile>,
}

async fn read_commit(repo: &Repository, hash: &str) -> Result<CommitDetail, GitError> {
    if !(7..=64).contains(&hash.len()) || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("Choose a commit hash"));
    }
    let resolved = repo.run(&["rev-parse", "--verify", &format!("{hash}^{{commit}}")]).await?;
    if !resolved.status.success() {
        return Err(GitError(StatusCode::NOT_FOUND, "Commit not found".to_owned()));
    }
    let hash = utf8(resolved.stdout)?.trim_end().to_owned();
    let output = utf8(
        repo.checked(&["show", "--no-patch", "--no-show-signature", "--format=%H%x00%B%x00%an%x00%ae%x00%aI%x00%P", &hash, "--"]).await?,
    )?;
    let fields: Vec<_> = output.split('\0').collect();
    if fields.len() != 6 {
        return Err(failed("Invalid commit metadata"));
    }
    let mut detail = CommitDetail {
        hash,
        message: fields[1].trim_end_matches('\n').to_owned(),
        author: fields[2].to_owned(),
        email: fields[3].to_owned(),
        time: fields[4].to_owned(),
        parents: fields[5].split_whitespace().map(str::to_owned).collect(),
        files: Vec::new(),
    };
    // Compare merge commits with their first parent, consistently for the list and patches.
    let mut args = vec!["diff-tree", "--no-commit-id", "--name-status", "-z", "-r", "-M", "--root"];
    if let Some(parent) = detail.parents.first() {
        args.push(parent);
    }
    args.extend([detail.hash.as_str(), "--"]);
    detail.files = commit_files(&repo.checked(&args).await?)?;
    Ok(detail)
}

fn commit_files(bytes: &[u8]) -> Result<Vec<CommitFile>, GitError> {
    let mut fields = bytes.split(|byte| *byte == 0).filter(|field| !field.is_empty());
    let mut files = Vec::new();
    while let Some(status) = fields.next() {
        let status = status[0] as char;
        let path = utf8(fields.next().ok_or_else(|| failed("Missing commit file path"))?.to_vec())?;
        let (path, old_path) = if matches!(status, 'R' | 'C') {
            (utf8(fields.next().ok_or_else(|| failed("Missing commit rename path"))?.to_vec())?, Some(path))
        } else {
            (path, None)
        };
        files.push(CommitFile { path, old_path, status: status.to_string() });
        if files.len() > MAX_FILES {
            return Err(GitError(StatusCode::PAYLOAD_TOO_LARGE, "Too many changed files to display".to_owned()));
        }
    }
    Ok(files)
}

async fn committed_diff(dir: &str, hash: &str, path: &str) -> Result<String, GitError> {
    validate_path(path)?;
    let repo = Repository::open(dir).await?;
    let detail = read_commit(&repo, hash).await?;
    let file = detail.files.iter().find(|file| file.path == path).ok_or_else(|| invalid("File is not changed in this commit"))?;
    let mut args = vec!["diff-tree", "--no-commit-id", "--no-ext-diff", "--no-textconv", "--no-color", "--root", "-r", "-p", "-M"];
    if let Some(parent) = detail.parents.first() {
        args.push(parent);
    }
    args.extend([&detail.hash, "--", path]);
    if let Some(old) = &file.old_path {
        args.push(old);
    }
    utf8(repo.checked(&args).await?)
}

fn validate_path(path: &str) -> Result<(), GitError> {
    if path.is_empty()
        || path.len() > 4096
        || path.contains('\0')
        || path.contains('\\')
        || !Path::new(path).components().all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(invalid("Choose a file inside the repository"));
    }
    Ok(())
}

async fn repository_diff(dir: &str, path: &str, staged: bool) -> Result<String, GitError> {
    validate_path(path)?;
    let repo = Repository::open(dir).await?;
    let files = repo.files().await?;
    let file = files.iter().find(|file| file.path == path).ok_or_else(|| invalid("File is no longer changed; refresh the repository"))?;
    if !staged && file.index == "?" {
        let full = repo.0.join(path);
        let metadata = tokio::fs::symlink_metadata(&full).await.map_err(|_| invalid("File is no longer available"))?;
        if !metadata.is_file() {
            return Err(invalid("Only regular untracked files can be previewed"));
        }
        let real = tokio::fs::canonicalize(&full).await.map_err(|_| invalid("File is no longer available"))?;
        if !real.starts_with(&repo.0) {
            return Err(invalid("File is outside the repository"));
        }
        let file = tokio::fs::File::open(real).await.map_err(|_| failed("Cannot read file"))?;
        let bytes = read_output(file).await?;
        if bytes.contains(&0) {
            return Ok("Binary file".to_owned());
        }
        let text = String::from_utf8_lossy(&bytes);
        return Ok(text.split_inclusive('\n').map(|line| format!("+{line}")).collect());
    }
    let mut args = vec!["diff", "--no-ext-diff", "--no-textconv", "--no-color"];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", path]);
    if let Some(old) = file.old_path.as_deref() {
        args.push(old);
    }
    utf8(repo.checked(&args).await?)
}

async fn change_repository(dir: &str, operation: Operation) -> Result<(), GitError> {
    let repo = Repository::open(dir).await?;
    match operation {
        Operation::Stage { paths } | Operation::Unstage { paths } if paths.is_empty() || paths.len() > MAX_FILES => {
            return Err(invalid("Choose between 1 and 2000 changed files"));
        }
        Operation::Stage { paths } => {
            let paths = changed_paths(&repo, paths, false).await?;
            let mut args = vec!["add", "--all", "--"];
            args.extend(paths.iter().map(String::as_str));
            repo.checked(&args).await?;
        }
        Operation::Unstage { paths } => {
            let paths = changed_paths(&repo, paths, true).await?;
            // An unborn branch has no HEAD to restore from. Remove only index entries.
            let mut args = if repo.has_head().await? {
                vec!["reset", "--quiet", "HEAD", "--"]
            } else {
                vec!["rm", "--cached", "--force", "--ignore-unmatch", "--"]
            };
            args.extend(paths.iter().map(String::as_str));
            repo.checked(&args).await?;
        }
        Operation::Commit { message } => {
            let message = message.trim();
            if message.is_empty() || message.len() > 16384 || message.contains('\0') {
                return Err(invalid("Enter a commit message"));
            }
            repo.checked(&["commit", "-m", message]).await?;
        }
        Operation::Switch { branch } => {
            if branch.is_empty() || branch.starts_with('-') || branch.contains('\0') {
                return Err(invalid("Choose a local branch"));
            }
            let branches = utf8(repo.checked(&["for-each-ref", "--format=%(refname:lstrip=2)", "refs/heads/"]).await?)?;
            if !branches.lines().any(|name| name == branch) {
                return Err(invalid("Choose a local branch"));
            }
            repo.checked(&["switch", "--no-guess", &branch]).await?;
        }
        Operation::Pull => {
            repo.checked(&["pull", "--ff-only"]).await?;
        }
        Operation::Push => {
            repo.checked(&["push"]).await?;
        }
    }
    Ok(())
}

async fn changed_paths(repo: &Repository, paths: Vec<String>, unstage: bool) -> Result<Vec<String>, GitError> {
    let files = repo.files().await?;
    let mut result = Vec::new();
    for path in paths {
        validate_path(&path)?;
        let file =
            files.iter().find(|file| file.path == path).ok_or_else(|| invalid("File is no longer changed; refresh the repository"))?;
        if !result.contains(&path) {
            result.push(path);
        }
        if let Some(old) = file.old_path.as_ref().filter(|_| unstage || matches!(file.worktree.as_str(), "R" | "C")) {
            validate_path(old)?;
            if !result.contains(old) {
                result.push(old.to_owned());
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(root: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git").current_dir(root).args(args).output().unwrap();
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap()
    }

    fn init() -> (tempfile::TempDir, String) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        fs::create_dir(&root).unwrap();
        git(&root, &["init", "--quiet", "--initial-branch=main"]);
        for (key, value) in [
            ("user.name", "Sidebar Test"),
            ("user.email", "sidebar@example.test"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
            ("core.hooksPath", ".git/test-hooks"),
        ] {
            git(&root, &["config", key, value]);
        }
        (temp, root.to_str().unwrap().to_owned())
    }

    async fn stage(root: &str, paths: &[&str]) {
        change_repository(root, Operation::Stage { paths: paths.iter().map(|path| (*path).to_owned()).collect() }).await.unwrap();
    }

    async fn commit(root: &str, message: &str) {
        change_repository(root, Operation::Commit { message: message.to_owned() }).await.unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn untracked_preview_accepts_normal_and_verbatim_windows_roots() {
        let (_temp, root) = init();
        let canonical = Path::new(&root).canonicalize().unwrap();
        let shown = docs::shown_path(&canonical);
        fs::write(canonical.join("preview.txt"), "inside the repository\n").unwrap();
        fs::create_dir(canonical.join("nested")).unwrap();
        fs::write(canonical.join("nested").join("preview.txt"), "inside the repository\n").unwrap();
        for dir in [shown.as_str(), canonical.to_str().unwrap()] {
            for path in ["preview.txt", "nested/preview.txt"] {
                assert_eq!(repository_diff(dir, path, false).await.unwrap(), "+inside the repository\n");
            }
            assert_eq!(repository_status(dir).await.unwrap().root, shown);
            assert!(repository_diff(dir, "../outside.txt", false).await.is_err());
        }
    }

    #[tokio::test]
    async fn commit_details_and_patches_show_the_committed_content_even_with_dirty_files() {
        let (_temp, root) = init();
        let folder = Path::new(&root);
        fs::write(folder.join("a[1] 한글.txt"), "committed content\n").unwrap();
        fs::write(folder.join("binary"), b"a\0b").unwrap();
        stage(&root, &["a[1] 한글.txt", "binary"]).await;
        commit(&root, "Subject\n\nFull message body\nwith another line").await;
        let hash = git(folder, &["rev-parse", "HEAD"]).trim().to_owned();
        let repo = Repository::open(&root).await.unwrap();
        let detail = read_commit(&repo, &hash[..7]).await.unwrap();
        assert_eq!(detail.hash, hash);
        assert_eq!(detail.message, "Subject\n\nFull message body\nwith another line");
        assert_eq!(detail.author, "Sidebar Test");
        assert_eq!(detail.email, "sidebar@example.test");
        assert!(detail.parents.is_empty());
        assert_eq!(detail.files.len(), 2);
        assert!(detail.files.iter().all(|file| file.status == "A"));
        fs::write(folder.join("a[1] 한글.txt"), "dirty worktree\n").unwrap();
        let patch = committed_diff(&root, &hash, "a[1] 한글.txt").await.unwrap();
        assert!(patch.contains("+committed content"), "{patch}");
        assert!(!patch.contains("dirty worktree"));
        assert!(committed_diff(&root, &hash, "binary").await.unwrap().contains("Binary files"));
        assert!(committed_diff(&root, &hash, "not-in-commit").await.is_err());
        for value in ["HEAD", "--help", "abc1234^", "abc1234:path", "0000000000000000000000000000000000000000"] {
            assert!(read_commit(&repo, value).await.is_err(), "{value}");
        }
        let blob = git(folder, &["rev-parse", "HEAD:binary"]);
        assert!(read_commit(&repo, blob.trim()).await.is_err());
    }

    #[tokio::test]
    async fn committed_renames_and_deletions_list_both_paths_and_show_their_patches() {
        let (_temp, root) = init();
        let folder = Path::new(&root);
        fs::write(folder.join("old name.txt"), "preserved\n").unwrap();
        fs::write(folder.join("deleted.txt"), "removed\n").unwrap();
        stage(&root, &["old name.txt", "deleted.txt"]).await;
        commit(&root, "initial").await;
        git(folder, &["mv", "old name.txt", "new name.txt"]);
        git(folder, &["rm", "deleted.txt"]);
        commit(&root, "rename and delete").await;
        let hash = git(folder, &["rev-parse", "HEAD"]);
        let repo = Repository::open(&root).await.unwrap();
        let detail = read_commit(&repo, hash.trim()).await.unwrap();
        assert_eq!(detail.parents.len(), 1);
        let renamed = detail.files.iter().find(|file| file.status == "R").unwrap();
        assert_eq!(renamed.path, "new name.txt");
        assert_eq!(renamed.old_path.as_deref(), Some("old name.txt"));
        assert!(committed_diff(&root, hash.trim(), "new name.txt").await.unwrap().contains("rename from old name.txt"));
        assert!(committed_diff(&root, hash.trim(), "deleted.txt").await.unwrap().contains("-removed"));
    }

    #[tokio::test]
    async fn merge_commit_file_list_and_patch_compare_with_the_first_parent() {
        let (_temp, root) = init();
        let folder = Path::new(&root);
        fs::write(folder.join("base"), "base\n").unwrap();
        stage(&root, &["base"]).await;
        commit(&root, "initial").await;
        git(folder, &["branch", "topic"]);
        fs::write(folder.join("main"), "main\n").unwrap();
        stage(&root, &["main"]).await;
        commit(&root, "main changes").await;
        git(folder, &["switch", "topic"]);
        fs::write(folder.join("topic"), "topic\n").unwrap();
        stage(&root, &["topic"]).await;
        commit(&root, "topic changes").await;
        git(folder, &["switch", "main"]);
        git(folder, &["merge", "--no-ff", "-m", "merge topic", "topic"]);
        let hash = git(folder, &["rev-parse", "HEAD"]);
        let repo = Repository::open(&root).await.unwrap();
        let detail = read_commit(&repo, hash.trim()).await.unwrap();
        assert_eq!(detail.parents.len(), 2);
        assert_eq!(detail.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["topic"]);
        assert!(committed_diff(&root, hash.trim(), "topic").await.unwrap().contains("+topic"));
    }

    #[tokio::test]
    async fn unborn_repository_stages_unstages_and_commits_without_losing_working_files() {
        let (_temp, root) = init();
        let initial = repository_status(&root).await.unwrap();
        assert_eq!(initial.branch.as_deref(), Some("main"));
        assert!(initial.commits.is_empty());
        assert!(initial.head.is_none());
        fs::write(Path::new(&root).join("first.txt"), "one\n").unwrap();
        assert_eq!(repository_diff(&root, "first.txt", false).await.unwrap(), "+one\n");
        stage(&root, &["first.txt"]).await;
        change_repository(&root, Operation::Unstage { paths: vec!["first.txt".into()] }).await.unwrap();
        assert_eq!(fs::read_to_string(Path::new(&root).join("first.txt")).unwrap(), "one\n");
        assert_eq!(repository_status(&root).await.unwrap().files[0].index, "?");
        stage(&root, &["first.txt"]).await;
        commit(&root, "first commit").await;
        let status = repository_status(&root).await.unwrap();
        assert!(status.files.is_empty());
        assert_eq!(status.commits[0].message, "first commit");
        assert_eq!(status.commits[0].author, "Sidebar Test");
    }

    #[tokio::test]
    async fn partially_staged_files_have_independent_diffs_and_commit_only_the_index() {
        let (_temp, root) = init();
        let file = Path::new(&root).join("file.txt");
        fs::write(&file, "original\n").unwrap();
        stage(&root, &["file.txt"]).await;
        commit(&root, "initial").await;
        fs::write(&file, "staged\n").unwrap();
        stage(&root, &["file.txt"]).await;
        fs::write(&file, "unstaged\n").unwrap();
        let status = repository_status(&root).await.unwrap();
        assert_eq!((&status.files[0].index, &status.files[0].worktree), (&"M".to_owned(), &"M".to_owned()));
        assert!(repository_diff(&root, "file.txt", true).await.unwrap().contains("+staged"));
        assert!(repository_diff(&root, "file.txt", false).await.unwrap().contains("+unstaged"));
        commit(&root, "index only").await;
        assert_eq!(git(Path::new(&root), &["show", "HEAD:file.txt"]), "staged\n");
        assert_eq!(fs::read_to_string(file).unwrap(), "unstaged\n");
    }

    #[tokio::test]
    async fn literal_paths_renames_and_deletions_can_be_staged_and_unstaged() {
        let (_temp, root) = init();
        let folder = Path::new(&root);
        // Characters which would otherwise be options or pathspec wildcards.
        for name in ["-option.txt", "a[1].txt", "a1.txt", "old name.txt", "한글.txt"] {
            fs::write(folder.join(name), "original\n").unwrap();
        }
        stage(&root, &["-option.txt", "a[1].txt", "a1.txt", "old name.txt", "한글.txt"]).await;
        commit(&root, "initial").await;
        fs::write(folder.join("a[1].txt"), "literal\n").unwrap();
        fs::write(folder.join("a1.txt"), "leave unstaged\n").unwrap();
        stage(&root, &["a[1].txt"]).await;
        let status = repository_status(&root).await.unwrap();
        assert_eq!(status.files.iter().find(|file| file.path == "a1.txt").unwrap().index, " ");
        git(folder, &["mv", "old name.txt", "new name.txt"]);
        let status = repository_status(&root).await.unwrap();
        let renamed = status.files.iter().find(|file| file.path == "new name.txt").unwrap();
        assert_eq!(renamed.old_path.as_deref(), Some("old name.txt"));
        assert!(repository_diff(&root, "new name.txt", true).await.unwrap().contains("rename from"));
        fs::write(folder.join("new name.txt"), "changed after rename\n").unwrap();
        stage(&root, &["new name.txt"]).await;
        change_repository(&root, Operation::Unstage { paths: vec!["new name.txt".into()] }).await.unwrap();
        assert!(folder.join("new name.txt").exists());
        assert!(!folder.join("old name.txt").exists());
        fs::remove_file(folder.join("한글.txt")).unwrap();
        stage(&root, &["한글.txt"]).await;
        assert_eq!(repository_status(&root).await.unwrap().files.iter().find(|file| file.path == "한글.txt").unwrap().index, "D");
    }

    #[tokio::test]
    async fn linked_worktree_uses_its_own_branch_and_index() {
        let (temp, root) = init();
        fs::write(Path::new(&root).join("file.txt"), "base\n").unwrap();
        stage(&root, &["file.txt"]).await;
        commit(&root, "initial").await;
        let linked = temp.path().join("linked");
        git(Path::new(&root), &["worktree", "add", "-b", "topic", linked.to_str().unwrap()]);
        fs::create_dir(linked.join("nested")).unwrap();
        fs::write(linked.join("file.txt"), "linked\n").unwrap();
        let nested = linked.join("nested");
        let status = repository_status(nested.to_str().unwrap()).await.unwrap();
        assert_eq!(status.branch.as_deref(), Some("topic"));
        stage(nested.to_str().unwrap(), &["file.txt"]).await;
        assert_eq!(repository_status(linked.to_str().unwrap()).await.unwrap().files[0].index, "M");
        assert!(repository_status(&root).await.unwrap().files.is_empty());
        let error = change_repository(linked.to_str().unwrap(), Operation::Switch { branch: "main".into() }).await.unwrap_err();
        assert_eq!(error.0, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn local_branch_switch_and_detached_head_are_reported() {
        let (_temp, root) = init();
        fs::write(Path::new(&root).join("file.txt"), "base\n").unwrap();
        stage(&root, &["file.txt"]).await;
        commit(&root, "initial").await;
        git(Path::new(&root), &["branch", "topic"]);
        git(Path::new(&root), &["tag", "topic"]);
        change_repository(&root, Operation::Switch { branch: "topic".into() }).await.unwrap();
        assert_eq!(repository_status(&root).await.unwrap().branch.as_deref(), Some("topic"));
        assert!(change_repository(&root, Operation::Switch { branch: "--detach".into() }).await.is_err());
        git(Path::new(&root), &["checkout", "--detach"]);
        let status = repository_status(&root).await.unwrap();
        assert!(status.branch.is_none());
        assert!(status.head.is_some());
    }

    #[tokio::test]
    async fn push_and_fast_forward_pull_use_a_disposable_local_remote() {
        let (temp, root) = init();
        let remote = temp.path().join("remote.git");
        git(temp.path(), &["init", "--bare", "--initial-branch=main", remote.to_str().unwrap()]);
        fs::write(Path::new(&root).join("file.txt"), "base\n").unwrap();
        stage(&root, &["file.txt"]).await;
        commit(&root, "initial").await;
        git(Path::new(&root), &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(Path::new(&root), &["push", "--set-upstream", "origin", "main"]);
        fs::write(Path::new(&root).join("file.txt"), "next\n").unwrap();
        stage(&root, &["file.txt"]).await;
        commit(&root, "next").await;
        assert_eq!(repository_status(&root).await.unwrap().ahead, 1);
        change_repository(&root, Operation::Push).await.unwrap();
        assert_eq!(repository_status(&root).await.unwrap().ahead, 0);
        let peer = temp.path().join("peer");
        git(temp.path(), &["clone", remote.to_str().unwrap(), peer.to_str().unwrap()]);
        fs::write(peer.join("from-peer.txt"), "remote\n").unwrap();
        git(&peer, &["add", "."]);
        git(&peer, &["-c", "user.name=Peer", "-c", "user.email=peer@example.test", "-c", "commit.gpgsign=false", "commit", "-m", "peer"]);
        git(&peer, &["push"]);
        change_repository(&root, Operation::Pull).await.unwrap();
        assert_eq!(fs::read_to_string(Path::new(&root).join("from-peer.txt")).unwrap(), "remote\n");
    }

    #[tokio::test]
    async fn rejects_invalid_roots_paths_empty_messages_and_binary_file_contents() {
        let (_temp, root) = init();
        assert!(repository_status("relative").await.is_err());
        for path in ["../outside", "/absolute", "", "a/../../outside", "a\\..\\b"] {
            assert!(repository_diff(&root, path, false).await.is_err());
        }
        assert!(change_repository(&root, Operation::Stage { paths: vec![] }).await.is_err());
        assert!(change_repository(&root, Operation::Commit { message: " \n".into() }).await.is_err());
        fs::write(Path::new(&root).join("binary"), b"a\0b").unwrap();
        assert_eq!(repository_diff(&root, "binary", false).await.unwrap(), "Binary file");
        assert!(change_repository(&root, Operation::Stage { paths: vec!["missing".into()] }).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn untracked_symlinks_are_not_followed_for_preview() {
        let (temp, root) = init();
        let outside = temp.path().join("outside");
        fs::write(&outside, "private").unwrap();
        std::os::unix::fs::symlink(outside, Path::new(&root).join("link")).unwrap();
        assert!(repository_diff(&root, "link", false).await.is_err());
    }

    #[tokio::test]
    async fn handlers_reject_cross_site_reads_and_writes_before_running_git() {
        use crate::server::tests::test_state;
        use axum::http::header;
        let state: Shared = std::sync::Arc::new(test_state(&["http://127.0.0.1:4317"]));
        let (_temp, root) = init();
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "127.0.0.1:4317".parse().unwrap());
        headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        let body = Bytes::from(json!({ "dir": root, "action": "commit", "message": "test" }).to_string());
        assert_eq!(action(State(state.clone()), headers.clone(), body.clone()).await.status(), StatusCode::FORBIDDEN);
        headers.insert(header::ORIGIN, "http://evil.example".parse().unwrap());
        assert_eq!(
            status(State(state.clone()), headers.clone(), Query(DirectoryQuery { dir: root.clone() })).await.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(action(State(state.clone()), headers.clone(), body.clone()).await.status(), StatusCode::FORBIDDEN);
        headers.insert(header::ORIGIN, "http://127.0.0.1:4317".parse().unwrap());
        assert_eq!(status(State(state.clone()), headers.clone(), Query(DirectoryQuery { dir: root })).await.status(), StatusCode::OK);
        let (_temp2, root2) = init();
        fs::write(Path::new(&root2).join("file.txt"), "stage via HTTP\n").unwrap();
        let valid = Bytes::from(json!({ "dir": root2, "action": "stage", "paths": ["file.txt"] }).to_string());
        assert_eq!(action(State(state.clone()), headers.clone(), valid).await.status(), StatusCode::OK);
        let invalid = Bytes::from(r#"{"dir":"/", "action":"execute", "command":"rm"}"#);
        assert_eq!(action(State(state), headers, invalid).await.status(), StatusCode::BAD_REQUEST);
    }
}
