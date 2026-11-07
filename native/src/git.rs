//! The git branch of a folder, read from the files in `.git` (never by running
//! `git`), so it is cheap enough to ask once a second for every shell.

use std::io::Read;
use std::path::{Path, PathBuf};

/// How many parent folders to look through for `.git`.
const MAX_LEVELS: usize = 64;
/// `HEAD` and a `.git` file are a few dozen bytes; read no more than this.
const MAX_FILE: u64 = 4096;
const MAX_BRANCH: usize = 255;
const SHORT_HASH: usize = 7;

/// The branch checked out in `dir` or its nearest parent that is a repository
/// or linked worktree. A detached HEAD gives the first 7 characters of the commit.
pub fn branch(dir: &Path) -> Option<String> {
    branch_within(dir, MAX_LEVELS)
}

fn branch_within(dir: &Path, levels: usize) -> Option<String> {
    let mut current = dir;
    for _ in 0..=levels {
        let dot_git = current.join(".git");
        if let Ok(meta) = std::fs::metadata(&dot_git) {
            // The nearest `.git` decides, even when its HEAD is unreadable.
            let git_dir = if meta.is_dir() { dot_git } else { gitdir_of(current, &dot_git)? };
            return parse_head(&read_small(&git_dir.join("HEAD"))?);
        }
        current = current.parent()?;
    }
    None
}

/// A linked worktree has a `.git` file: `gitdir: <path>`, maybe relative to its folder.
fn gitdir_of(folder: &Path, dot_git: &Path) -> Option<PathBuf> {
    let text = read_small(dot_git)?;
    let path = text.trim().strip_prefix("gitdir:")?.trim();
    (!path.is_empty()).then(|| folder.join(path))
}

fn read_small(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path).ok()?.take(MAX_FILE).read_to_end(&mut bytes).ok()?;
    String::from_utf8(bytes).ok()
}

fn parse_head(head: &str) -> Option<String> {
    let head = head.trim();
    if let Some(name) = head.strip_prefix("ref: refs/heads/") {
        let valid = !name.is_empty() && name.len() <= MAX_BRANCH && !name.chars().any(char::is_control);
        return valid.then(|| name.to_string());
    }
    // Detached: a SHA-1 (40) or SHA-256 (64) object id.
    let is_hash = matches!(head.len(), 40 | 64) && head.bytes().all(|b| b.is_ascii_hexdigit());
    is_hash.then(|| head[..SHORT_HASH].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn a_branch_from_head() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".git")).unwrap();
        fs::write(tmp.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let deep = tmp.path().join("a/b");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(branch(&deep).as_deref(), Some("main"));
    }

    #[test]
    fn a_worktree_follows_its_gitdir() {
        let tmp = tempfile::tempdir().unwrap();
        let admin = tmp.path().join("repo/.git/worktrees/wt");
        fs::create_dir_all(&admin).unwrap();
        fs::write(admin.join("HEAD"), "ref: refs/heads/feat/x\n").unwrap();
        let wt = tmp.path().join("wt");
        fs::create_dir_all(&wt).unwrap();

        fs::write(wt.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
        assert_eq!(branch(&wt).as_deref(), Some("feat/x"));

        fs::write(wt.join(".git"), "gitdir: ../repo/.git/worktrees/wt\n").unwrap();
        assert_eq!(branch(&wt).as_deref(), Some("feat/x"));
    }

    #[test]
    fn a_detached_head_shows_seven_characters() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".git")).unwrap();
        fs::write(tmp.path().join(".git/HEAD"), "0123456789abcdef0123456789abcdef01234567\n").unwrap();
        assert_eq!(branch(tmp.path()).as_deref(), Some("0123456"));
    }

    #[test]
    fn outside_a_repository_is_none() {
        // A parent of the temp folder may be a repository, so do not climb.
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(branch_within(tmp.path(), 0), None);
    }
}
