//! `/api/docs/…`: the Markdown files of the docs pane. Every request names a
//! root folder and a path inside it; nothing outside the root is read or
//! written, also through `..` or a symlink. The page that calls these routes
//! already runs shells as the user, so any folder may be a root: the guards
//! keep other websites out, not the user.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{host_allowed, is_json, no_store, origin_allowed, read_allowed, AppState, Shared};
use crate::state_dir::home_dir;

/// Files larger than this are neither opened nor saved.
pub(super) const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// A JSON body escapes the text, so it may be several times the file's size.
pub(super) const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
/// The tree lists at most this many files, this many folders deep.
const MAX_FILES: usize = 2000;
const MAX_DEPTH: usize = 8;
/// Folders that are never listed, whatever .gitignore says.
const SKIPPED: [&str; 3] = [".git", "node_modules", "target"];

/// Saves take turns, so two saves based on one version cannot both win.
static SAVES: Mutex<()> = Mutex::new(());
/// Makes each temp file name unique within this process.
static TEMP_COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq)]
enum DocsError {
    BadRoot,
    BadPath,
    Missing,
    TooLarge,
    NotText,
    Exists,
    /// The file on disk is not the one the edit started from: its version, or None when it is gone.
    Conflict(Option<String>),
    Io,
}

impl IntoResponse for DocsError {
    fn into_response(self) -> Response {
        let status = match &self {
            DocsError::BadRoot | DocsError::BadPath => StatusCode::BAD_REQUEST,
            DocsError::Missing => StatusCode::NOT_FOUND,
            DocsError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            DocsError::NotText => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            DocsError::Exists | DocsError::Conflict(_) => StatusCode::CONFLICT,
            DocsError::Io => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let version = match self {
            DocsError::Conflict(version) => version,
            _ => None,
        };
        (status, no_store(Json(json!({ "version": version })))).into_response()
    }
}

fn io_error(error: std::io::Error) -> DocsError {
    if error.kind() == ErrorKind::NotFound { DocsError::Missing } else { DocsError::Io }
}

/// The root as an absolute, canonical folder.
fn canonical_root(root: &str) -> Result<PathBuf, DocsError> {
    let path = Path::new(root);
    if !path.is_absolute() {
        return Err(DocsError::BadRoot);
    }
    let canonical = path.canonicalize().map_err(|_| DocsError::BadRoot)?;
    if canonical.is_dir() { Ok(canonical) } else { Err(DocsError::BadRoot) }
}

/// A Windows path without the `\\?\` prefix that canonicalize adds.
fn strip_verbatim(text: &str) -> String {
    match text.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix(r"UNC\") {
            Some(share) => format!(r"\\{share}"),
            None => rest.to_owned(),
        },
        None => text.to_owned(),
    }
}

/// A path as the page shows and sends it back.
pub(super) fn shown_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) { strip_verbatim(&text) } else { text.into_owned() }
}

/// The root as shown, its Markdown files, and whether the list stopped
/// early. A folder that cannot be read is an error, not an empty tree.
fn list_root(root: &str) -> Result<(String, Vec<String>, bool), DocsError> {
    let root = canonical_root(root)?;
    fs::read_dir(&root).map_err(|_| DocsError::BadRoot)?;
    let (files, truncated) = list_markdown(&root);
    Ok((shown_path(&root), files, truncated))
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown"))
}

/// The real path of `rel` inside the canonical `root`. `existing`: the file
/// must exist (symlinks resolved); otherwise its folder must, and the file
/// goes there.
fn resolve(root: &Path, rel: &str, existing: bool) -> Result<PathBuf, DocsError> {
    let path = Path::new(rel);
    let plain = !rel.is_empty() && !rel.contains('\0') && path.components().all(|part| matches!(part, Component::Normal(_)));
    if !plain || !is_markdown(path) {
        return Err(DocsError::BadPath);
    }
    let joined = root.join(path);
    if existing {
        let real = joined.canonicalize().map_err(io_error)?;
        return if real.starts_with(root) && real.is_file() && is_markdown(&real) { Ok(real) } else { Err(DocsError::BadPath) };
    }
    let (Some(parent), Some(name)) = (joined.parent(), joined.file_name()) else {
        return Err(DocsError::BadPath);
    };
    let folder = parent.canonicalize().map_err(|_| DocsError::BadPath)?;
    if folder.starts_with(root) && folder.is_dir() { Ok(folder.join(name)) } else { Err(DocsError::BadPath) }
}

/// The Markdown files under `root`, relative with `/`, sorted; true when the list stopped at MAX_FILES.
fn list_markdown(root: &Path) -> (Vec<String>, bool) {
    let walk = ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_global(false)
        .require_git(false)
        .follow_links(false)
        .max_depth(Some(MAX_DEPTH))
        .filter_entry(|entry| !entry.file_name().to_str().is_some_and(|name| SKIPPED.contains(&name)))
        .build();
    let mut files = Vec::new();
    let mut truncated = false;
    for entry in walk.flatten() {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) || !is_markdown(entry.path()) {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else { continue };
        let parts: Option<Vec<&str>> = rel.components().map(|part| part.as_os_str().to_str()).collect();
        let Some(parts) = parts else { continue };
        if files.len() == MAX_FILES {
            truncated = true;
            break;
        }
        files.push(parts.join("/"));
    }
    files.sort();
    (files, truncated)
}

/// The first 16 bytes of the SHA-256 of a file's bytes, in hex.
fn version(bytes: &[u8]) -> String {
    Sha256::digest(bytes)[..16].iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The text and version of a Markdown file.
fn read_doc(root: &Path, rel: &str) -> Result<(String, String), DocsError> {
    let path = resolve(root, rel, true)?;
    if fs::metadata(&path).map_err(io_error)?.len() > MAX_FILE_BYTES {
        return Err(DocsError::TooLarge);
    }
    let bytes = fs::read(&path).map_err(io_error)?;
    let hash = version(&bytes);
    let text = String::from_utf8(bytes).map_err(|_| DocsError::NotText)?;
    Ok((text, hash))
}

/// Saves `text` when the file on disk is still the one at `base`; None
/// means the edit expects no file there. Writes a temp file and renames it
/// over the real path, so a crash never leaves half a file.
fn write_doc(root: &Path, rel: &str, text: &str, base: Option<&str>) -> Result<String, DocsError> {
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(DocsError::TooLarge);
    }
    let _turn = SAVES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (path, permissions) = match resolve(root, rel, true) {
        Ok(path) => {
            let current = version(&fs::read(&path).map_err(io_error)?);
            if base != Some(current.as_str()) {
                return Err(DocsError::Conflict(Some(current)));
            }
            let permissions = fs::metadata(&path).map_err(io_error)?.permissions();
            (path, Some(permissions))
        }
        Err(DocsError::Missing) if base.is_none() => (resolve(root, rel, false)?, None),
        Err(DocsError::Missing) => return Err(DocsError::Conflict(None)),
        Err(error) => return Err(error),
    };
    let name = path.file_name().and_then(|name| name.to_str()).ok_or(DocsError::BadPath)?;
    let count = TEMP_COUNT.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_file_name(format!(".{name}.westerm-{}-{count}.tmp", std::process::id()));
    let written = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        if let Some(permissions) = permissions {
            fs::set_permissions(&temp, permissions)?;
        }
        fs::rename(&temp, &path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written.map_err(|_| DocsError::Io)?;
    Ok(version(text.as_bytes()))
}

/// A new file; never replaces one.
fn create_doc(root: &Path, rel: &str, text: &str) -> Result<String, DocsError> {
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(DocsError::TooLarge);
    }
    let path = resolve(root, rel, false)?;
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|error| {
        if error.kind() == ErrorKind::AlreadyExists { DocsError::Exists } else { DocsError::Io }
    })?;
    file.write_all(text.as_bytes()).map_err(|_| DocsError::Io)?;
    Ok(version(text.as_bytes()))
}

/// A change from the dashboard's own page: its Origin, and JSON, which needs a preflight from elsewhere.
fn write_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    is_json(headers) && (state.trusted || (host_allowed(state, headers) && origin_allowed(state, headers)))
}

pub(super) fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/docs/tree", get(tree))
        .route("/api/docs/file", get(file).put(save).post(create).layer(DefaultBodyLimit::max(MAX_BODY_BYTES)))
        .route("/api/docs/cwd", get(cwd))
}

/// Runs file work off the async threads.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, DocsError> + Send + 'static) -> Result<T, DocsError> {
    tokio::task::spawn_blocking(work).await.unwrap_or(Err(DocsError::Io))
}

#[derive(Deserialize)]
struct TreeQuery {
    root: String,
}

async fn tree(State(state): State<Shared>, headers: HeaderMap, Query(query): Query<TreeQuery>) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let listed = blocking(move || {
        let (root, files, truncated) = list_root(&query.root)?;
        Ok(json!({ "root": root, "files": files, "truncated": truncated }))
    });
    match listed.await {
        Ok(body) => no_store(Json(body)),
        Err(error) => error.into_response(),
    }
}

#[derive(Deserialize)]
struct FileQuery {
    root: String,
    path: String,
}

async fn file(State(state): State<Shared>, headers: HeaderMap, Query(query): Query<FileQuery>) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match blocking(move || read_doc(&canonical_root(&query.root)?, &query.path)).await {
        Ok((content, version)) => no_store(Json(json!({ "content": content, "version": version }))),
        Err(error) => error.into_response(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveBody {
    root: String,
    path: String,
    content: String,
    base_version: Option<String>,
}

async fn save(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    if !write_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(body) = serde_json::from_slice::<SaveBody>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let saved = blocking(move || write_doc(&canonical_root(&body.root)?, &body.path, &body.content, body.base_version.as_deref()));
    match saved.await {
        Ok(version) => no_store(Json(json!({ "version": version }))),
        Err(error) => error.into_response(),
    }
}

#[derive(Deserialize)]
struct CreateBody {
    root: String,
    path: String,
    content: String,
}

async fn create(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    if !write_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(body) = serde_json::from_slice::<CreateBody>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match blocking(move || create_doc(&canonical_root(&body.root)?, &body.path, &body.content)).await {
        Ok(version) => (StatusCode::CREATED, no_store(Json(json!({ "version": version })))).into_response(),
        Err(error) => error.into_response(),
    }
}

#[derive(Deserialize)]
struct CwdQuery {
    pid: Option<u32>,
}

/// The current folder of a process (a shell the docs pane follows), or the home folder.
async fn cwd(State(state): State<Shared>, headers: HeaderMap, Query(query): Query<CwdQuery>) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let found = tokio::task::spawn_blocking(move || match query.pid {
        None => {
            let home = home_dir();
            Some(home.canonicalize().unwrap_or(home))
        }
        Some(pid) => crate::cwd::process_cwd(pid).filter(|path| path.is_dir()),
    });
    match found.await.ok().flatten() {
        Some(path) => no_store(Json(json!({ "path": shown_path(&path) }))),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use axum::http::{header, HeaderMap};

    use super::*;
    use crate::server::tests::test_state;

    /// A fresh folder, canonical (macOS puts temp folders behind /private).
    fn root() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap();
        (dir, path)
    }

    fn touch(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn resolve_refuses_escapes() {
        let (_dir, root) = root();
        touch(&root, "docs/a.md", "# a");
        for rel in ["", "../x.md", "docs/../../x.md", "/etc/x.md", "./docs/a.md", "a\0.md", "notes.txt", "docs"] {
            assert_eq!(resolve(&root, rel, false), Err(DocsError::BadPath), "{rel:?}");
        }
        assert_eq!(resolve(&root, "docs/a.md", true), Ok(root.join("docs/a.md")));
        assert_eq!(resolve(&root, "docs/b.md", true), Err(DocsError::Missing));
        assert_eq!(resolve(&root, "docs/b.md", false), Ok(root.join("docs/b.md")));
        assert_eq!(resolve(&root, "nowhere/b.md", false), Err(DocsError::BadPath), "missing parent");
    }

    #[cfg(unix)]
    #[test]
    fn resolve_refuses_symlinks_that_leave_the_root() {
        let (_outside_dir, outside) = root();
        touch(&outside, "x.md", "secret");
        let (_dir, root) = root();
        touch(&root, "real.md", "# real");
        std::os::unix::fs::symlink(outside.join("x.md"), root.join("link.md")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("away")).unwrap();
        std::os::unix::fs::symlink(root.join("real.md"), root.join("same.md")).unwrap();
        assert_eq!(resolve(&root, "link.md", true), Err(DocsError::BadPath));
        assert_eq!(resolve(&root, "away/x.md", true), Err(DocsError::BadPath));
        assert_eq!(resolve(&root, "away/new.md", false), Err(DocsError::BadPath));
        assert_eq!(resolve(&root, "same.md", true), Ok(root.join("real.md")));
    }

    #[test]
    fn canonical_root_needs_an_absolute_folder() {
        let (_dir, root) = root();
        touch(&root, "a.md", "");
        assert_eq!(canonical_root(root.to_str().unwrap()), Ok(root.clone()));
        assert_eq!(canonical_root("relative/folder"), Err(DocsError::BadRoot));
        assert_eq!(canonical_root(root.join("a.md").to_str().unwrap()), Err(DocsError::BadRoot));
        assert_eq!(canonical_root(root.join("gone").to_str().unwrap()), Err(DocsError::BadRoot));
    }

    #[test]
    fn list_follows_gitignore_and_skips_heavy_folders() {
        let (_dir, root) = root();
        for rel in ["a.md", "docs/b.MD", "docs/c.markdown", "node_modules/x/c.md", "target/d.md", ".git/e.md", ".github/f.md", "ignored/g.md", "notes.txt"] {
            touch(&root, rel, "");
        }
        touch(&root, ".gitignore", "ignored/\n");
        let (files, truncated) = list_markdown(&root);
        assert_eq!(files, [".github/f.md", "a.md", "docs/b.MD", "docs/c.markdown"]);
        assert!(!truncated);
    }

    #[test]
    fn list_stops_at_the_file_limit() {
        let (_dir, root) = root();
        for index in 0..=MAX_FILES {
            touch(&root, &format!("n/{index}.md"), "");
        }
        let (files, truncated) = list_markdown(&root);
        assert_eq!(files.len(), MAX_FILES);
        assert!(truncated);
    }

    #[test]
    fn version_is_a_short_content_hash() {
        assert_eq!(version(b"abc"), "ba7816bf8f01cfea414140de5dae2223");
        assert_eq!(version(b"abc").len(), 32);
    }

    #[test]
    fn read_checks_size_and_text() {
        let (_dir, root) = root();
        touch(&root, "a.md", "# hi");
        assert_eq!(read_doc(&root, "a.md"), Ok(("# hi".to_owned(), version(b"# hi"))));
        fs::write(root.join("bin.md"), [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(read_doc(&root, "bin.md"), Err(DocsError::NotText));
        fs::write(root.join("big.md"), vec![b'a'; MAX_FILE_BYTES as usize + 1]).unwrap();
        assert_eq!(read_doc(&root, "big.md"), Err(DocsError::TooLarge));
    }

    #[test]
    fn write_needs_the_version_it_was_based_on() {
        let (_dir, root) = root();
        touch(&root, "a.md", "one");
        let stale = version(b"zero");
        assert_eq!(write_doc(&root, "a.md", "two", Some(&stale)), Err(DocsError::Conflict(Some(version(b"one")))));
        assert_eq!(write_doc(&root, "a.md", "two", None), Err(DocsError::Conflict(Some(version(b"one")))));
        assert_eq!(write_doc(&root, "a.md", "two", Some(&version(b"one"))), Ok(version(b"two")));
        assert_eq!(fs::read_to_string(root.join("a.md")).unwrap(), "two");
        let leftovers: Vec<_> = fs::read_dir(&root).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(leftovers, ["a.md"], "no temp file stays");
    }

    #[test]
    fn write_after_a_delete_needs_no_version() {
        let (_dir, root) = root();
        assert_eq!(write_doc(&root, "gone.md", "x", Some(&version(b"old"))), Err(DocsError::Conflict(None)));
        assert_eq!(write_doc(&root, "gone.md", "x", None), Ok(version(b"x")));
        assert_eq!(write_doc(&root, "big.md", &"a".repeat(MAX_FILE_BYTES as usize + 1), None), Err(DocsError::TooLarge));
    }

    #[cfg(unix)]
    #[test]
    fn write_keeps_the_file_mode() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, root) = root();
        touch(&root, "a.md", "one");
        fs::set_permissions(root.join("a.md"), fs::Permissions::from_mode(0o640)).unwrap();
        write_doc(&root, "a.md", "two", Some(&version(b"one"))).unwrap();
        assert_eq!(fs::metadata(root.join("a.md")).unwrap().permissions().mode() & 0o777, 0o640);
    }

    #[test]
    fn create_refuses_an_existing_file() {
        let (_dir, root) = root();
        touch(&root, "docs/a.md", "");
        assert_eq!(create_doc(&root, "docs/a.md", "x"), Err(DocsError::Exists));
        assert_eq!(create_doc(&root, "docs/new.md", "# new\n"), Ok(version(b"# new\n")));
        assert_eq!(fs::read_to_string(root.join("docs/new.md")).unwrap(), "# new\n");
        assert_eq!(create_doc(&root, "missing/new.md", ""), Err(DocsError::BadPath));
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_root_is_an_error_not_an_empty_tree() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, root) = root();
        touch(&root, "a.md", "");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).unwrap();
        let readable = fs::read_dir(&root).is_ok();
        let listed = list_root(root.to_str().unwrap());
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        if readable {
            return; // root ignores permissions
        }
        assert_eq!(listed.map(|_| ()), Err(DocsError::BadRoot));
    }

    #[test]
    fn list_root_answers_the_folder_as_shown() {
        let (_dir, root) = root();
        touch(&root, "a.md", "");
        // The page sends the folder as it was shown: on Windows without `\\?\`,
        // which would keep the `/` as part of the name.
        let shown_root = shown_path(&root);
        let (shown, files, truncated) = list_root(&format!("{shown_root}/")).unwrap();
        assert_eq!(shown, shown_root);
        assert_eq!(files, ["a.md"]);
        assert!(!truncated);
    }

    #[test]
    fn windows_verbatim_prefixes_are_dropped() {
        assert_eq!(strip_verbatim(r"\\?\C:\Users\me"), r"C:\Users\me");
        assert_eq!(strip_verbatim(r"\\?\UNC\server\share\docs"), r"\\server\share\docs");
        assert_eq!(strip_verbatim("/Users/me"), "/Users/me");
    }

    #[test]
    fn saves_of_one_file_take_turns() {
        let (_dir, root) = root();
        touch(&root, "a.md", "zero");
        let base = version(b"zero");
        let root = std::sync::Arc::new(root);
        let workers: Vec<_> = (0..8)
            .map(|index| {
                let (root, base) = (root.clone(), base.clone());
                std::thread::spawn(move || write_doc(&root, "a.md", &format!("text {index}"), Some(&base)))
            })
            .collect();
        let results: Vec<_> = workers.into_iter().map(|worker| worker.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1, "{results:?}");
        let leftovers: Vec<_> = fs::read_dir(&*root).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(leftovers, ["a.md"]);
    }

    fn page_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "127.0.0.1:4317".parse().unwrap());
        headers.insert(header::ORIGIN, "http://127.0.0.1:4317".parse().unwrap());
        headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        headers
    }

    #[tokio::test]
    async fn handlers_answer_with_status_codes() {
        let state: Shared = std::sync::Arc::new(test_state(&["http://127.0.0.1:4317"]));
        let (_dir, root) = root();
        touch(&root, "a.md", "one");
        let at = || root.to_str().unwrap().to_owned();
        let status = |response: Response| response.status();
        let query = |path: &str| Query(FileQuery { root: at(), path: path.to_owned() });

        assert_eq!(status(tree(State(state.clone()), page_headers(), Query(TreeQuery { root: at() })).await), StatusCode::OK);
        let mut cross = page_headers();
        cross.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert_eq!(status(tree(State(state.clone()), cross, Query(TreeQuery { root: at() })).await), StatusCode::FORBIDDEN);
        assert_eq!(status(tree(State(state.clone()), page_headers(), Query(TreeQuery { root: "rel".into() })).await), StatusCode::BAD_REQUEST);
        assert_eq!(status(file(State(state.clone()), page_headers(), query("a.md")).await), StatusCode::OK);
        assert_eq!(status(file(State(state.clone()), page_headers(), query("../a.md")).await), StatusCode::BAD_REQUEST);
        assert_eq!(status(file(State(state.clone()), page_headers(), query("b.md")).await), StatusCode::NOT_FOUND);

        let body = |base: &str| Bytes::from(format!(r#"{{"root":{:?},"path":"a.md","content":"two","baseVersion":"{base}"}}"#, at()));
        let mut no_origin = page_headers();
        no_origin.remove(header::ORIGIN);
        assert_eq!(status(save(State(state.clone()), no_origin, body(&version(b"one"))).await), StatusCode::FORBIDDEN);
        assert_eq!(status(save(State(state.clone()), page_headers(), body("stale")).await), StatusCode::CONFLICT);
        assert_eq!(status(save(State(state.clone()), page_headers(), body(&version(b"one"))).await), StatusCode::OK);
        let new = Bytes::from(format!(r#"{{"root":{:?},"path":"n.md","content":"x"}}"#, at()));
        assert_eq!(status(create(State(state.clone()), page_headers(), new.clone()).await), StatusCode::CREATED);
        assert_eq!(status(create(State(state.clone()), page_headers(), new).await), StatusCode::CONFLICT);
        assert_eq!(status(cwd(State(state.clone()), page_headers(), Query(CwdQuery { pid: None })).await), StatusCode::OK);
    }

    #[test]
    fn requests_from_other_sites_are_refused() {
        let state = test_state(&["http://127.0.0.1:4317"]);
        let headers = |pairs: &[(header::HeaderName, &str)]| {
            let mut map = HeaderMap::new();
            for (name, value) in pairs {
                map.insert(name.clone(), value.parse().unwrap());
            }
            map
        };
        let host = (header::HOST, "127.0.0.1:4317");
        let ours = (header::ORIGIN, "http://127.0.0.1:4317");
        let json = (header::CONTENT_TYPE, "application/json");
        assert!(read_allowed(&state, &headers(std::slice::from_ref(&host))));
        assert!(!read_allowed(&state, &headers(&[(header::HOST, "evil.example:4317")])), "DNS rebinding");
        assert!(!read_allowed(&state, &headers(&[host.clone(), (header::ORIGIN, "http://evil.example")])));
        assert!(!read_allowed(&state, &headers(&[host.clone(), (header::HeaderName::from_static("sec-fetch-site"), "cross-site")])));
        assert!(write_allowed(&state, &headers(&[host.clone(), ours.clone(), json.clone()])));
        assert!(!write_allowed(&state, &headers(&[host.clone(), json.clone()])), "no Origin");
        assert!(!write_allowed(&state, &headers(&[host.clone(), ours.clone()])), "not JSON");
        assert!(!write_allowed(&state, &headers(&[(header::HOST, "evil.example:4317"), ours, json])));
    }
}
