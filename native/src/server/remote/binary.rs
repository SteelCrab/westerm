//! The Linux westerm a server gets: the release of this exact version, or,
//! for a source build, a file the developer names. It is downloaded here and
//! sent over SSH, so the server needs no internet access.

use std::path::{Path, PathBuf};

use super::Failure;
use crate::protocol::RemoteFailure;
use crate::remote::remote_dir;
use crate::update::{self, Channel};

/// The release build for a server's `uname -m`.
pub fn linux_triple(arch: &str) -> Option<&'static str> {
    match arch {
        "x86_64" | "amd64" => Some("x86_64-unknown-linux-musl"),
        "aarch64" | "arm64" => Some("aarch64-unknown-linux-musl"),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum InstallPlan {
    UpToDate,
    Install,
    /// The server has a newer release than this dashboard; never downgrade it.
    RemoteNewer,
}

/// What to do with the server's westerm, from its version line and ours.
pub fn install_plan(remote: Option<&str>, local: &str) -> InstallPlan {
    let Some(remote) = remote else { return InstallPlan::Install };
    if remote == local {
        return InstallPlan::UpToDate;
    }
    let version = remote.strip_prefix("westerm ").and_then(|rest| rest.split_whitespace().next());
    if version.is_some_and(|version| update::is_newer(version, update::CURRENT)) {
        InstallPlan::RemoteNewer
    } else {
        InstallPlan::Install
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BinarySource {
    /// A file the developer named with WESTERM_REMOTE_BINARY_DIR.
    Given(PathBuf),
    Cached(PathBuf),
    /// Download the release into this cache path.
    Download(PathBuf),
    /// A source build: its version has no release with this code.
    DevBuild,
}

pub fn binary_source(env_dir: Option<&Path>, channel: Channel, cache: &Path, triple: &str) -> BinarySource {
    if let Some(dir) = env_dir {
        return BinarySource::Given(dir.join(format!("westerm-{triple}")));
    }
    if channel == Channel::Source {
        return BinarySource::DevBuild;
    }
    if cache.is_file() { BinarySource::Cached(cache.to_path_buf()) } else { BinarySource::Download(cache.to_path_buf()) }
}

/// A local path to the Linux westerm for `triple`, downloading the release
/// into `<state>/remote/bin/<version>/<triple>/westerm` when needed.
pub async fn obtain(state: &Path, triple: &str) -> Result<PathBuf, Failure> {
    let dir = remote_dir(state).map_err(|e| Failure::new(RemoteFailure::Setup, e.to_string()))?;
    let cache = dir.join("bin").join(update::CURRENT).join(triple).join("westerm");
    let env_dir = std::env::var_os("WESTERM_REMOTE_BINARY_DIR").map(PathBuf::from);
    match binary_source(env_dir.as_deref(), update::channel(), &cache, triple) {
        BinarySource::Given(path) if path.is_file() => Ok(path),
        BinarySource::Given(path) => Err(Failure::new(RemoteFailure::DevBinary, format!("{} is missing", path.display()))),
        BinarySource::DevBuild => Err(Failure::new(
            RemoteFailure::DevBinary,
            format!("set WESTERM_REMOTE_BINARY_DIR to a folder with westerm-{triple}"),
        )),
        BinarySource::Cached(path) => Ok(path),
        BinarySource::Download(path) => {
            let (triple, dest) = (triple.to_owned(), path.clone());
            tokio::task::spawn_blocking(move || update::download_release_binary(update::CURRENT, &triple, &dest))
                .await
                .map_err(|e| Failure::new(RemoteFailure::Download, e.to_string()))?
                .map_err(|e| Failure::new(RemoteFailure::Download, e))?;
            Ok(path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::Channel;

    #[test]
    fn maps_linux_architectures() {
        assert_eq!(linux_triple("x86_64"), Some("x86_64-unknown-linux-musl"));
        assert_eq!(linux_triple("amd64"), Some("x86_64-unknown-linux-musl"));
        assert_eq!(linux_triple("aarch64"), Some("aarch64-unknown-linux-musl"));
        assert_eq!(linux_triple("arm64"), Some("aarch64-unknown-linux-musl"));
        assert_eq!(linux_triple("armv7l"), None);
        assert_eq!(linux_triple("riscv64"), None);
    }

    #[test]
    fn never_downgrades() {
        let local = crate::version_line();
        assert_eq!(install_plan(Some(&local), &local), InstallPlan::UpToDate);
        assert_eq!(install_plan(Some("westerm 99.0.0 (host rs-1)"), &local), InstallPlan::RemoteNewer);
        assert_eq!(install_plan(Some("westerm 0.0.1 (host rs-1)"), &local), InstallPlan::Install);
        assert_eq!(install_plan(None, &local), InstallPlan::Install);
        let same_version_other_build = format!("westerm {} (host rs-other)", crate::update::CURRENT);
        assert_eq!(install_plan(Some(&same_version_other_build), &local), InstallPlan::Install);
    }

    #[test]
    fn picks_where_the_binary_comes_from() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("bin/0.2.0/x86_64-unknown-linux-musl/westerm");
        let triple = "x86_64-unknown-linux-musl";
        assert_eq!(
            binary_source(Some(Path::new("/opt/builds")), Channel::Source, &cache, triple),
            BinarySource::Given(PathBuf::from("/opt/builds/westerm-x86_64-unknown-linux-musl"))
        );
        assert_eq!(binary_source(None, Channel::Source, &cache, triple), BinarySource::DevBuild);
        assert_eq!(binary_source(None, Channel::Binary, &cache, triple), BinarySource::Download(cache.clone()));
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        std::fs::write(&cache, b"elf").unwrap();
        assert_eq!(binary_source(None, Channel::App, &cache, triple), BinarySource::Cached(cache.clone()));
    }
}
