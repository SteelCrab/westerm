//! `/api/agents/worktrees`: the worktrees of a folder's repository, and the
//! open pull request of each one's branch. Git lists the worktrees; GitHub's
//! `gh` lists the pull requests. Without `gh`, or signed out, the worktrees
//! come without pull requests.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::process::Command;

use super::plugins::git::{utf8, GitError, Repository};
use super::{no_store, read_allowed, Shared};

/// Pull requests are read again when they are this old.
const PR_MAX_AGE: Duration = Duration::from_secs(30);
/// `gh` asks GitHub: it gets this long. A remote window waits 35 s for the whole answer.
const GH_TIMEOUT: Duration = Duration::from_secs(10);
/// `gh` lists this many open pull requests.
const MAX_PRS: &str = "200";
/// A title is cut to this many characters.
const MAX_TITLE_CHARS: usize = 200;

pub(super) fn routes() -> Router<Shared> {
    Router::new().route("/api/agents/worktrees", get(worktrees))
}

#[derive(Deserialize)]
struct DirectoryQuery {
    dir: String,
}

async fn worktrees(State(state): State<Shared>, headers: HeaderMap, Query(query): Query<DirectoryQuery>) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let listing = match list(&query.dir).await {
        Ok(listing) => listing,
        Err(error) => return error.into_response(),
    };
    let prs = pull_requests(&listing.root).await;
    no_store(Json(body(&listing, prs.as_ref())))
}

/// One worktree with a folder. `head` is the first 7 characters of its commit.
#[derive(Debug, PartialEq, Eq)]
struct Worktree {
    path: String,
    /// None on a detached HEAD.
    branch: Option<String>,
    head: String,
}

/// The output of `git worktree list --porcelain`. A bare repository has no
/// folder to work in, and a prunable worktree has lost its folder: both are left out.
fn parse_worktrees(text: &str) -> Vec<Worktree> {
    text.split("\n\n").filter_map(parse_worktree).collect()
}

/// One block of the list: `worktree <path>`, then a line for each of its facts.
fn parse_worktree(block: &str) -> Option<Worktree> {
    let (mut path, mut head, mut branch) = (None, None, None);
    for line in block.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "worktree" => path = Some(native_path(value)),
            "HEAD" => head = Some(value.chars().take(7).collect()),
            "branch" => branch = Some(value.strip_prefix("refs/heads/").unwrap_or(value).to_owned()),
            "bare" | "prunable" => return None,
            _ => {}
        }
    }
    Some(Worktree { path: path?, branch, head: head? })
}

/// A path the way this OS writes it: Git for Windows prints `C:/Users/p`, shells report `C:\Users\p`.
fn native_path(path: &str) -> String {
    Path::new(path).components().collect::<PathBuf>().to_string_lossy().into_owned()
}

/// `owner/repo` from the URL of a remote: `https://host/owner/repo.git` or `git@host:owner/repo.git`.
fn repo_name(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    // After the host: `scheme://host/<path>`, or `user@host:<path>`.
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/')?.1,
        None => url.split_once('@')?.1.split_once(':')?.1,
    };
    let mut parts = path.rsplit('/').filter(|part| !part.is_empty());
    let (repo, owner) = (parts.next()?, parts.next()?);
    Some(format!("{owner}/{repo}"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Checks {
    Passing,
    Failing,
    Running,
    /// The pull request has no checks.
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct PullRequest {
    number: u64,
    title: String,
    url: String,
    draft: bool,
    checks: Checks,
    /// The checks that failed.
    failed: usize,
}

/// Open pull requests by their branch.
type PullRequests = HashMap<String, PullRequest>;

/// The output of `gh pr list --json …`. A pull request from a fork is left
/// out: its branch name is the fork's, not this repository's.
fn parse_prs(json: &[u8]) -> Option<PullRequests> {
    let listed: Vec<GhPullRequest> = serde_json::from_slice(json).ok()?;
    let mut prs = PullRequests::new();
    // Only https links open in a browser pane.
    for pr in listed.into_iter().filter(|pr| !pr.is_cross_repository && pr.url.starts_with("https://")) {
        let checks = pr.status_check_rollup.unwrap_or_default();
        let failed = checks.iter().filter(|check| check.outcome() == Checks::Failing).count();
        let state = if failed > 0 {
            Checks::Failing
        } else if checks.iter().any(|check| check.outcome() == Checks::Running) {
            Checks::Running
        } else if checks.is_empty() {
            Checks::None
        } else {
            Checks::Passing
        };
        // gh lists the newest first: it stays when two share a branch.
        prs.entry(pr.head_ref_name).or_insert(PullRequest {
            number: pr.number,
            title: pr.title.chars().take(MAX_TITLE_CHARS).collect(),
            url: pr.url,
            draft: pr.is_draft,
            checks: state,
            failed,
        });
    }
    Some(prs)
}

/// One pull request as `gh pr list --json` writes it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPullRequest {
    number: u64,
    title: String,
    url: String,
    is_draft: bool,
    head_ref_name: String,
    is_cross_repository: bool,
    #[serde(default)]
    status_check_rollup: Option<Vec<GhCheck>>,
}

/// A check run has `status` and `conclusion`; a commit status has `state`.
#[derive(Deserialize)]
struct GhCheck {
    status: Option<String>,
    conclusion: Option<String>,
    state: Option<String>,
}

impl GhCheck {
    fn outcome(&self) -> Checks {
        if let Some(state) = self.state.as_deref() {
            return match state {
                "FAILURE" | "ERROR" => Checks::Failing,
                "PENDING" | "EXPECTED" => Checks::Running,
                _ => Checks::Passing,
            };
        }
        if self.status.as_deref() != Some("COMPLETED") {
            return Checks::Running;
        }
        match self.conclusion.as_deref() {
            Some("FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE") => Checks::Failing,
            _ => Checks::Passing,
        }
    }
}

/// gh in `path_var`, else in `extra`.
fn find_gh(path_var: Option<OsString>, extra: &[PathBuf]) -> Option<PathBuf> {
    let name = if cfg!(windows) { "gh.exe" } else { "gh" };
    path_var
        .iter()
        .flat_map(std::env::split_paths)
        .chain(extra.iter().cloned())
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Where installers put gh outside PATH. An app opened from the macOS Finder
/// has no Homebrew folder in its PATH.
fn gh_dirs() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        vec![PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")]
    } else if cfg!(windows) {
        let under = |var: &str, rest: &str| std::env::var_os(var).map(|base| PathBuf::from(base).join(rest));
        [under("ProgramFiles", "GitHub CLI"), under("LOCALAPPDATA", "Microsoft\\WinGet\\Links")].into_iter().flatten().collect()
    } else {
        vec![PathBuf::from("/usr/local/bin"), PathBuf::from("/usr/bin")]
    }
}

/// What a folder's repository has: its main worktree, its name on the
/// remote, the worktree the folder is in, and all worktrees.
#[derive(Debug)]
struct Listing {
    root: String,
    name: Option<String>,
    current: Option<String>,
    worktrees: Vec<Worktree>,
}

async fn list(dir: &str) -> Result<Listing, GitError> {
    let repo = Repository::open(dir).await?;
    let (listed, origin) =
        tokio::join!(repo.checked(&["worktree", "list", "--porcelain"]), repo.run(&["remote", "get-url", "origin"]));
    let worktrees = parse_worktrees(&utf8(listed?)?);
    // Git lists the main worktree first.
    let Some(root) = worktrees.first().map(|worktree| worktree.path.clone()) else {
        return Err(GitError(StatusCode::NOT_FOUND, "This repository has no worktree".to_owned()));
    };
    // Git and the OS may spell one folder in two ways (a symlink, `C:/` and `\\?\C:\`): compare canonical paths.
    let mut current = None;
    for worktree in &worktrees {
        if tokio::fs::canonicalize(&worktree.path).await.is_ok_and(|path| path == repo.root()) {
            current = Some(worktree.path.clone());
            break;
        }
    }
    let origin = origin?;
    let name = origin.status.success().then(|| repo_name(&String::from_utf8_lossy(&origin.stdout))).flatten();
    Ok(Listing { root, name, current, worktrees })
}

/// The response body. `prs` tells that GitHub answered: a worktree without a
/// pull request then has none, instead of an unknown one.
fn body(listing: &Listing, prs: Option<&PullRequests>) -> Value {
    let worktrees: Vec<Value> = listing
        .worktrees
        .iter()
        .map(|worktree| {
            let pr = worktree.branch.as_ref().and_then(|branch| prs?.get(branch));
            json!({ "path": worktree.path, "branch": worktree.branch, "head": worktree.head, "pr": pr })
        })
        .collect();
    json!({
        "root": listing.root,
        "name": listing.name,
        "current": listing.current,
        "prs": prs.is_some(),
        "worktrees": worktrees,
    })
}

/// The pull requests of repositories, by main worktree, with when they were read.
#[derive(Default)]
struct PrCache(HashMap<String, (Instant, Option<PullRequests>)>);

impl PrCache {
    /// What was read for `root` less than `PR_MAX_AGE` ago.
    fn get(&self, root: &str, now: Instant) -> Option<&Option<PullRequests>> {
        self.0.get(root).filter(|(at, _)| Self::fresh(*at, now)).map(|(_, prs)| prs)
    }

    fn fresh(at: Instant, now: Instant) -> bool {
        now.saturating_duration_since(at) < PR_MAX_AGE
    }

    /// Keeps `prs` for `root` and drops what is too old to be used again.
    fn put(&mut self, root: &str, now: Instant, prs: Option<PullRequests>) {
        self.0.retain(|_, (at, _)| Self::fresh(*at, now));
        self.0.insert(root.to_owned(), (now, prs));
    }
}

/// The lock is held while gh runs: folders of one repository that ask
/// together share one run.
static PR_CACHE: tokio::sync::Mutex<Option<PrCache>> = tokio::sync::Mutex::const_new(None);

/// The open pull requests of the repository at `root`; None when gh gives none.
async fn pull_requests(root: &str) -> Option<PullRequests> {
    let mut cache = PR_CACHE.lock().await;
    let cache = cache.get_or_insert_default();
    if let Some(kept) = cache.get(root, Instant::now()) {
        return kept.clone();
    }
    let prs = read_prs(root).await;
    cache.put(root, Instant::now(), prs.clone());
    prs
}

/// Asks gh. None when gh is not installed, is signed out, does not know the
/// remote, or takes too long.
async fn read_prs(root: &str) -> Option<PullRequests> {
    let gh = find_gh(std::env::var_os("PATH"), &gh_dirs())?;
    let mut command = Command::new(gh);
    command
        .current_dir(root)
        .args(["pr", "list", "--state", "open", "--limit", MAX_PRS])
        .args(["--json", "number,title,url,isDraft,headRefName,isCrossRepository,statusCheckRollup"])
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(GH_TIMEOUT, command.output()).await.ok()?.ok()?;
    if !output.status.success() {
        return None;
    }
    parse_prs(&output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn worktrees_come_with_branch_or_detached_head() {
        let text = "worktree /Users/p/westerm\nHEAD b61efac0123456789\nbranch refs/heads/main\n\n\
                    worktree /Users/p/wt/pr-54\nHEAD 389f7650123456789\nbranch refs/heads/dependabot/npm/markdown-it\n\n\
                    worktree /Users/p/wt/loose\nHEAD 5ae5484fffffffff\ndetached\n\n";
        // Windows writes these paths with `\`: native_path has its own test.
        assert_eq!(
            parse_worktrees(text),
            [
                Worktree { path: native_path("/Users/p/westerm"), branch: Some("main".into()), head: "b61efac".into() },
                Worktree {
                    path: native_path("/Users/p/wt/pr-54"),
                    branch: Some("dependabot/npm/markdown-it".into()),
                    head: "389f765".into()
                },
                Worktree { path: native_path("/Users/p/wt/loose"), branch: None, head: "5ae5484".into() },
            ]
        );
    }

    #[test]
    fn bare_and_prunable_worktrees_are_left_out() {
        let text = "worktree /srv/repo.git\nbare\n\n\
                    worktree /srv/gone\nHEAD 1111111aaaaaaa\nbranch refs/heads/gone\nprunable gitdir file points to non-existent location\n\n\
                    worktree /srv/kept\nHEAD 2222222bbbbbbb\nbranch refs/heads/kept\nlocked on a usb stick\n";
        assert_eq!(
            parse_worktrees(text),
            [Worktree { path: native_path("/srv/kept"), branch: Some("kept".into()), head: "2222222".into() }]
        );
        assert_eq!(parse_worktrees(""), []);
    }

    #[test]
    fn paths_are_written_the_way_the_os_writes_them() {
        #[cfg(not(windows))]
        assert_eq!(native_path("/Users/p/we sterm/"), "/Users/p/we sterm");
        #[cfg(windows)]
        assert_eq!(native_path("C:/Users/p/we sterm"), "C:\\Users\\p\\we sterm");
        #[cfg(windows)]
        assert_eq!(
            parse_worktrees("worktree C:/Users/p/westerm\nHEAD b61efac0123\nbranch refs/heads/main\n")[0].path,
            "C:\\Users\\p\\westerm"
        );
    }

    #[test]
    fn repo_name_is_the_owner_and_the_repository() {
        for url in [
            "https://github.com/SteelCrab/westerm.git",
            "https://github.com/SteelCrab/westerm",
            "https://user@github.com/SteelCrab/westerm/",
            "git@github.com:SteelCrab/westerm.git",
            "ssh://git@github.com/SteelCrab/westerm.git\n",
        ] {
            assert_eq!(repo_name(url).as_deref(), Some("SteelCrab/westerm"), "{url}");
        }
        assert_eq!(repo_name("https://gitlab.com/group/sub/app.git").as_deref(), Some("sub/app"));
        // A folder on this computer, and a remote without an owner, have no such name.
        for url in ["/Users/p/westerm.git", "../westerm", "C:\\repos\\westerm", "https://example.com/westerm.git", ""] {
            assert_eq!(repo_name(url), None, "{url}");
        }
    }

    fn pr(checks: &str) -> String {
        format!(
            r#"{{"number":7,"title":"t","url":"https://github.com/o/r/pull/7","isDraft":false,"headRefName":"b","isCrossRepository":false,"statusCheckRollup":{checks}}}"#
        )
    }

    fn checks_of(checks: &str) -> (Checks, usize) {
        let prs = parse_prs(format!("[{}]", pr(checks)).as_bytes()).unwrap();
        (prs["b"].checks, prs["b"].failed)
    }

    #[test]
    fn checks_fail_when_one_failed_and_run_while_one_is_not_done() {
        let run = |status: &str, conclusion: &str| {
            format!(r#"{{"__typename":"CheckRun","status":"{status}","conclusion":"{conclusion}"}}"#)
        };
        let context = |state: &str| format!(r#"{{"__typename":"StatusContext","state":"{state}"}}"#);
        let list = |items: &[String]| format!("[{}]", items.join(","));

        assert_eq!(checks_of("[]"), (Checks::None, 0));
        assert_eq!(checks_of("null"), (Checks::None, 0));
        assert_eq!(
            checks_of(&list(&[run("COMPLETED", "SUCCESS"), run("COMPLETED", "SKIPPED"), run("COMPLETED", "NEUTRAL"), context("SUCCESS")])),
            (Checks::Passing, 0)
        );
        assert_eq!(checks_of(&list(&[run("COMPLETED", "SUCCESS"), run("IN_PROGRESS", "")])), (Checks::Running, 0));
        assert_eq!(checks_of(&list(&[run("QUEUED", ""), context("SUCCESS")])), (Checks::Running, 0));
        assert_eq!(checks_of(&list(&[context("PENDING")])), (Checks::Running, 0));
        // A failure shows at once: the checks still running do not hide it.
        assert_eq!(
            checks_of(&list(&[run("COMPLETED", "FAILURE"), run("IN_PROGRESS", ""), run("COMPLETED", "TIMED_OUT"), context("ERROR")])),
            (Checks::Failing, 3)
        );
        assert_eq!(checks_of(&list(&[run("COMPLETED", "CANCELLED"), context("FAILURE")])), (Checks::Failing, 2));
    }

    #[test]
    fn pull_requests_are_listed_by_branch() {
        let json = r#"[
            {"number":54,"title":"chore(deps): bump markdown-it","url":"https://github.com/o/r/pull/54","isDraft":false,
             "headRefName":"dependabot/npm/markdown-it","isCrossRepository":false,"statusCheckRollup":[]},
            {"number":60,"title":"draft","url":"https://github.com/o/r/pull/60","isDraft":true,
             "headRefName":"feat/x","isCrossRepository":false},
            {"number":61,"title":"from a fork","url":"https://github.com/o/r/pull/61","isDraft":false,
             "headRefName":"main","isCrossRepository":true,"statusCheckRollup":[]},
            {"number":62,"title":"older, same branch","url":"https://github.com/o/r/pull/62","isDraft":false,
             "headRefName":"feat/x","isCrossRepository":false,"statusCheckRollup":[]},
            {"number":63,"title":"odd link","url":"javascript:alert(1)","isDraft":false,
             "headRefName":"odd","isCrossRepository":false,"statusCheckRollup":[]}
        ]"#;
        let prs = parse_prs(json.as_bytes()).unwrap();
        assert_eq!(
            prs["dependabot/npm/markdown-it"],
            PullRequest {
                number: 54,
                title: "chore(deps): bump markdown-it".into(),
                url: "https://github.com/o/r/pull/54".into(),
                draft: false,
                checks: Checks::None,
                failed: 0
            }
        );
        assert_eq!((prs["feat/x"].number, prs["feat/x"].draft), (60, true), "gh lists the newest first");
        assert!(!prs.contains_key("main"), "a fork's main is not this repository's main");
        assert!(!prs.contains_key("odd"), "only https links open in a browser pane");
        assert_eq!(prs.len(), 2);
    }

    #[test]
    fn a_long_title_is_cut_and_bad_output_is_no_answer() {
        let long = pr("[]").replace(r#""title":"t""#, &format!(r#""title":"{}""#, "가".repeat(MAX_TITLE_CHARS + 50)));
        let prs = parse_prs(format!("[{long}]").as_bytes()).unwrap();
        assert_eq!(prs["b"].title.chars().count(), MAX_TITLE_CHARS);
        assert_eq!(parse_prs(b"[]").map(|prs| prs.len()), Some(0));
        assert_eq!(parse_prs(b"gh: not logged in"), None);
        assert_eq!(parse_prs(br#"{"message":"no"}"#), None);
    }

    #[test]
    fn gh_is_found_in_path_then_in_the_extra_folders() {
        let temp = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) { "gh.exe" } else { "gh" };
        let (first, second, empty) = (temp.path().join("first"), temp.path().join("second"), temp.path().join("empty"));
        for dir in [&first, &second, &empty] {
            fs::create_dir(dir).unwrap();
        }
        fs::write(first.join(name), "").unwrap();
        fs::write(second.join(name), "").unwrap();
        let path = |dirs: &[&PathBuf]| Some(std::env::join_paths(dirs.iter().copied()).unwrap());
        assert_eq!(find_gh(path(&[&empty, &first]), std::slice::from_ref(&second)), Some(first.join(name)));
        assert_eq!(find_gh(path(&[&empty]), std::slice::from_ref(&second)), Some(second.join(name)), "Finder gives an app no Homebrew PATH");
        assert_eq!(find_gh(None, std::slice::from_ref(&empty)), None);
    }

    fn git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git").current_dir(root).args(args).output().unwrap();
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }

    /// A repository with one commit on `main`, and a worktree `linked` on branch `topic`.
    fn repo_with_worktree() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        fs::create_dir(&root).unwrap();
        git(&root, &["init", "--quiet", "--initial-branch=main"]);
        for (key, value) in [("user.name", "Worktree Test"), ("user.email", "worktree@example.test"), ("commit.gpgsign", "false")] {
            git(&root, &["config", key, value]);
        }
        git(&root, &["commit", "--quiet", "--allow-empty", "-m", "initial"]);
        let linked = temp.path().join("linked");
        git(&root, &["worktree", "add", "--quiet", "-b", "topic", linked.to_str().unwrap()]);
        (temp, root, linked)
    }

    fn same_folder(a: &str, b: &Path) -> bool {
        fs::canonicalize(a).unwrap() == fs::canonicalize(b).unwrap()
    }

    #[tokio::test]
    async fn a_folder_lists_the_worktrees_of_its_repository() {
        let (_temp, root, linked) = repo_with_worktree();
        fs::create_dir(linked.join("nested")).unwrap();
        let listing = list(linked.join("nested").to_str().unwrap()).await.unwrap();
        assert!(same_folder(&listing.root, &root), "the main worktree, also from a linked one");
        assert_eq!(listing.name, None, "no remote");
        assert_eq!(
            listing.worktrees.iter().map(|worktree| worktree.branch.as_deref()).collect::<Vec<_>>(),
            [Some("main"), Some("topic")]
        );
        assert!(same_folder(&listing.worktrees[1].path, &linked));
        assert_eq!(listing.worktrees[0].head.len(), 7);
        // The folder is in `linked`, and the answer names it as Git lists it.
        assert_eq!(listing.current.as_deref(), Some(listing.worktrees[1].path.as_str()));

        let from_main = list(root.to_str().unwrap()).await.unwrap();
        assert_eq!(from_main.root, listing.root, "one repository, one root");
        assert_eq!(from_main.current.as_deref(), Some(from_main.worktrees[0].path.as_str()));
    }

    #[tokio::test]
    async fn the_repository_is_named_after_its_origin() {
        let (_temp, root, _linked) = repo_with_worktree();
        git(&root, &["remote", "add", "origin", "git@github.com:SteelCrab/westerm.git"]);
        assert_eq!(list(root.to_str().unwrap()).await.unwrap().name.as_deref(), Some("SteelCrab/westerm"));
    }

    #[tokio::test]
    async fn a_folder_outside_git_has_no_worktrees() {
        let temp = tempfile::tempdir().unwrap();
        // A repository above the temp folder would answer for it.
        if std::process::Command::new("git").current_dir(temp.path()).args(["rev-parse", "--show-toplevel"]).output().unwrap().status.success() {
            return;
        }
        assert_eq!(list(temp.path().to_str().unwrap()).await.unwrap_err().0, StatusCode::NOT_FOUND);
        assert_eq!(list("relative/folder").await.unwrap_err().0, StatusCode::BAD_REQUEST);
    }

    fn listing() -> Listing {
        Listing {
            root: "/r".into(),
            name: Some("o/r".into()),
            current: Some("/wt/x".into()),
            worktrees: vec![
                Worktree { path: "/r".into(), branch: Some("main".into()), head: "aaaaaaa".into() },
                Worktree { path: "/wt/x".into(), branch: Some("feat/x".into()), head: "bbbbbbb".into() },
                Worktree { path: "/wt/loose".into(), branch: None, head: "ccccccc".into() },
            ],
        }
    }

    #[test]
    fn the_body_joins_pull_requests_to_worktrees_by_branch() {
        let prs = parse_prs(format!("[{}]", pr("[]").replace(r#""headRefName":"b""#, r#""headRefName":"feat/x""#)).as_bytes()).unwrap();
        assert_eq!(
            body(&listing(), Some(&prs)),
            json!({
                "root": "/r",
                "name": "o/r",
                "current": "/wt/x",
                "prs": true,
                "worktrees": [
                    { "path": "/r", "branch": "main", "head": "aaaaaaa", "pr": null },
                    { "path": "/wt/x", "branch": "feat/x", "head": "bbbbbbb", "pr": {
                        "number": 7, "title": "t", "url": "https://github.com/o/r/pull/7",
                        "draft": false, "checks": "none", "failed": 0,
                    } },
                    { "path": "/wt/loose", "branch": null, "head": "ccccccc", "pr": null },
                ],
            })
        );
    }

    #[test]
    fn without_gh_the_body_says_pull_requests_are_unknown() {
        let body = body(&listing(), None);
        assert_eq!(body["prs"], json!(false));
        assert_eq!(body["worktrees"][1]["pr"], Value::Null);
    }

    #[test]
    fn pull_requests_are_kept_for_thirty_seconds() {
        let mut cache = PrCache::default();
        let start = Instant::now();
        assert!(cache.get("/r", start).is_none());
        cache.put("/r", start, Some(PullRequests::new()));
        cache.put("/no-gh", start, None);
        assert_eq!(cache.get("/r", start + PR_MAX_AGE - Duration::from_secs(1)), Some(&Some(PullRequests::new())));
        assert_eq!(cache.get("/no-gh", start), Some(&None), "a failed read is not tried again at once");
        assert!(cache.get("/r", start + PR_MAX_AGE).is_none());
        assert!(cache.get("/other", start).is_none());
        // A later read drops the old lists: the cache does not grow with every repository ever opened.
        cache.put("/new", start + PR_MAX_AGE, None);
        assert_eq!(cache.0.keys().collect::<Vec<_>>(), ["/new"]);
    }
}
