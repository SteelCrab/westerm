//! Packages that are fetched but not yet installed, and the steps that put a
//! package in its place. Every step here is blocking: it runs curl, tar, or
//! file operations, so the routes call it from `spawn_blocking`.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::catalog::UpdateMode;
use super::manifest::{Manifest, Permissions};
use super::package::{
    MAX_PACKAGE_BYTES, PACKAGE_TIMEOUT, SourceUrl, download, latest_release, parse_source_url, read_folder, unpack,
};
use super::store::{self, ExternalInfo, ExternalRecord, Source, SourceKind};
use super::Sources;
use crate::server::no_store;
use crate::server::plugins::PluginState;
use crate::time::now_millis;
use crate::update::{CURRENT, is_newer, sha256_hex};

/// Packages waiting for the user, at most this many.
const MAX_PENDING: usize = 4;
/// A fetched package is forgotten after this long.
const PENDING_TTL_MS: u64 = 15 * 60 * 1000;

/// Why a request failed: an HTTP status and the JSON body to send with it.
#[derive(Debug)]
pub struct Failure {
    pub status: StatusCode,
    pub body: Value,
}

impl Failure {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, body: json!({ "error": message.into() }) }
    }

    /// The input or the package is not acceptable.
    pub fn bad(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    /// Something we need from the network failed.
    pub fn gateway(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    /// The package asks for something other than what the user confirmed.
    pub fn permissions_changed(now: &Permissions) -> Self {
        Self { status: StatusCode::CONFLICT, body: json!({ "error": "permissions changed", "permissions": now }) }
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.status, no_store(Json(self.body))).into_response()
    }
}

/// A package that was fetched and checked.
#[derive(Debug)]
pub struct Staged {
    pub token: String,
    /// `<state>/plugins/.staging/<token>`: removed when the package moves, or
    /// is dropped. A folder has none.
    pub dir: Option<std::path::PathBuf>,
    /// The folder with `plugin.json`: inside `dir`, or the plugin's own folder.
    pub root: std::path::PathBuf,
    pub manifest: Manifest,
    pub source: Source,
    pub sha256: Option<String>,
    /// The bytes of the unpacked files. A folder has no count.
    pub size: Option<u64>,
    pub created: u64,
    /// Made by an update that waits for a second confirmation.
    pub update: bool,
}

impl Staged {
    /// Deletes the staging folder.
    pub fn discard(&self) {
        if let Some(dir) = &self.dir {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

/// The fetched packages, by token. A package is kept for 15 minutes; the
/// fifth one pushes out the oldest.
#[derive(Default)]
pub struct Pending(Mutex<HashMap<String, Staged>>);

impl Pending {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Staged>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Drops the packages that waited too long.
    fn purge(map: &mut HashMap<String, Staged>, now: u64) {
        map.retain(|_, staged| {
            let keep = now.saturating_sub(staged.created) < PENDING_TTL_MS;
            if !keep {
                staged.discard();
            }
            keep
        });
    }

    pub fn insert(&self, staged: Staged, now: u64) {
        let mut map = self.lock();
        Self::purge(&mut map, now);
        while map.len() >= MAX_PENDING {
            let Some(oldest) = map.values().min_by_key(|staged| staged.created).map(|staged| staged.token.clone()) else { break };
            if let Some(old) = map.remove(&oldest) {
                old.discard();
            }
        }
        map.insert(staged.token.clone(), staged);
    }

    /// Reads a waiting package without taking it.
    pub fn with<R>(&self, token: &str, now: u64, read: impl FnOnce(&Staged) -> R) -> Option<R> {
        let mut map = self.lock();
        Self::purge(&mut map, now);
        map.get(token).map(read)
    }

    pub fn take(&self, token: &str, now: u64) -> Option<Staged> {
        let mut map = self.lock();
        Self::purge(&mut map, now);
        map.remove(token)
    }
}

/// 32 hex characters from the time, the process, and a counter.
fn new_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    hasher.finalize().iter().take(16).map(|byte| format!("{byte:02x}")).collect()
}

/// Whether the two grants are the same: the programs as a set, and the
/// two switches.
pub fn same_permissions(a: &Permissions, b: &Permissions) -> bool {
    fn programs(permissions: &Permissions) -> BTreeSet<&String> {
        permissions.run.iter().collect()
    }
    programs(a) == programs(b) && a.panes == b.panes && a.background == b.background
}

/// Whether the plugin is installed and its package can be read. A record
/// whose package is gone does not count: installing again replaces it.
pub fn readable(state: &PluginState, id: &str) -> bool {
    state.external_infos().iter().any(|info| info.id == id)
}

/// Downloads `url`, checks its SHA-256 when `expected_sha256` is given, and
/// unpacks it under `<state>/plugins/.staging/<token>`. Every failure leaves
/// nothing in the staging folder.
pub fn stage_package(sources: &Sources, state_dir: &Path, url: &str, expected_sha256: Option<&str>) -> Result<Staged, Failure> {
    let staging = store::staging_dir(state_dir);
    fs::create_dir_all(&staging).map_err(|e| Failure::internal(format!("cannot create {}: {e}", staging.display())))?;
    let token = new_token();
    let archive = staging.join(format!("{token}.tar.gz"));
    let dir = staging.join(&token);
    let result = download_and_unpack(sources, url, expected_sha256, &archive, &dir).map(|unpacked| Staged {
        token,
        dir: Some(dir.clone()),
        root: unpacked.root,
        manifest: unpacked.manifest,
        source: Source { kind: SourceKind::Url, url: Some(url.to_owned()), path: None },
        sha256: Some(unpacked.sha256),
        size: Some(unpacked.size),
        created: now_millis(),
        update: false,
    });
    let _ = fs::remove_file(&archive);
    if result.is_err() {
        let _ = fs::remove_dir_all(&dir);
    }
    result
}

fn download_and_unpack(
    sources: &Sources,
    url: &str,
    expected_sha256: Option<&str>,
    archive: &Path,
    dir: &Path,
) -> Result<super::package::Unpacked, Failure> {
    download(url, archive, MAX_PACKAGE_BYTES, PACKAGE_TIMEOUT, sources).map_err(Failure::gateway)?;
    if let Some(expected) = expected_sha256 {
        let actual = sha256_hex(archive).map_err(|e| Failure::internal(format!("cannot read the download: {e}")))?;
        if actual != expected {
            return Err(Failure::bad(format!("the package's sha256 {actual} is not the one in the plugin list ({expected})")));
        }
    }
    unpack(archive, dir, CURRENT).map_err(Failure::bad)
}

/// A package from a direct address. The routes pass only addresses that
/// `parse_source_url` accepted; tests pass `file://` addresses.
pub fn stage_from_url(sources: &Sources, state_dir: &Path, url: &str) -> Result<Staged, Failure> {
    stage_package(sources, state_dir, url, None)
}

/// The address to download for what the user typed: the `.tar.gz` itself, or
/// the latest release asset of a GitHub repository.
pub fn package_url(sources: &Sources, typed: &str) -> Result<String, Failure> {
    match parse_source_url(typed).map_err(Failure::bad)? {
        SourceUrl::Tarball(url) => Ok(url),
        SourceUrl::GitHub { owner, repo } => {
            let release = latest_release(sources, &owner, &repo).map_err(Failure::gateway)?;
            release.asset_url.ok_or_else(|| Failure::bad(format!("the latest release of {owner}/{repo} has no westerm-plugin.tar.gz file")))
        }
    }
}

/// A development folder, read in place. The path must be absolute; the
/// record keeps the canonical path, so a link or a `..` cannot move it later.
pub fn stage_folder(path: &str) -> Result<Staged, Failure> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(Failure::bad("the folder must be an absolute path"));
    }
    let canonical = path.canonicalize().map_err(|e| Failure::bad(format!("cannot open the folder: {e}")))?;
    let text = canonical.to_str().ok_or_else(|| Failure::bad("the folder path must be valid text"))?.to_owned();
    let manifest = read_folder(&canonical, CURRENT).map_err(Failure::bad)?;
    Ok(Staged {
        token: new_token(),
        dir: None,
        root: canonical,
        manifest,
        source: Source { kind: SourceKind::Folder, url: None, path: Some(text) },
        sha256: None,
        size: None,
        created: now_millis(),
        update: false,
    })
}

/// The new version of an installed plugin, fetched and checked. `None` when
/// the source has nothing newer. A folder is always read again.
pub fn stage_update(state: &PluginState, record: &ExternalRecord) -> Result<Option<Staged>, Failure> {
    let sources = &state.externals.sources;
    let mut staged = match record.source.kind {
        SourceKind::Folder => {
            let path = record.source.path.as_deref().ok_or_else(|| Failure::bad("the record has no folder"))?;
            stage_folder(path)?
        }
        SourceKind::Index => {
            let entries = state
                .externals
                .catalog
                .index
                .get(sources, now_millis())
                .ok_or_else(|| Failure::gateway("Could not load the plugin list"))?;
            let entry = entries
                .into_iter()
                .find(|entry| entry.id == record.id)
                .ok_or_else(|| Failure::gateway("the plugin is not in the plugin list any more"))?;
            if !is_newer(&entry.version, &record.version) {
                return Ok(None);
            }
            let mut staged = stage_package(sources, &state.state_dir, &entry.url, Some(&entry.sha256))?;
            staged.source = Source { kind: SourceKind::Index, url: Some(entry.url), path: None };
            staged
        }
        SourceKind::Url => {
            let Some(Ok(SourceUrl::GitHub { owner, repo })) = record.source.url.as_deref().map(parse_source_url) else {
                return Err(Failure::conflict("reinstall to update"));
            };
            let release = latest_release(sources, &owner, &repo).map_err(Failure::gateway)?;
            if !is_newer(&release.version, &record.version) {
                return Ok(None);
            }
            let asset = release
                .asset_url
                .ok_or_else(|| Failure::bad(format!("the latest release of {owner}/{repo} has no westerm-plugin.tar.gz file")))?;
            let mut staged = stage_package(sources, &state.state_dir, &asset, None)?;
            staged.source = record.source.clone();
            staged
        }
    };
    let refusal = if staged.manifest.id != record.id {
        Some(format!("the package is {}, not {}", staged.manifest.id, record.id))
    } else if record.source.kind != SourceKind::Folder && !is_newer(&staged.manifest.version, &record.version) {
        Some(format!("the package is version {}, which is not newer than {}", staged.manifest.version, record.version))
    } else {
        None
    };
    if let Some(message) = refusal {
        staged.discard();
        return Err(Failure::bad(message));
    }
    staged.update = true;
    Ok(Some(staged))
}

/// How a package becomes the installed plugin.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Commit {
    /// A new plugin, or one whose files are gone.
    Install,
    /// A new version of an installed plugin, or a reinstall over it (any
    /// version): its storage stays.
    Update,
}

/// Puts a package in its place and writes the record, with `granted` set to
/// what the package's `plugin.json` asks for (the caller has confirmed it).
/// A package moves to `<state>/plugins/<id>/<version>/`; a folder stays where
/// it is. If anything fails, nothing of the new package is left, and the
/// installed plugin, if any, is as it was. After an update the old version's
/// folder is deleted.
pub fn commit(state: &PluginState, staged: Staged, mode: Commit) -> Result<ExternalInfo, Failure> {
    let result = place(state, &staged, mode);
    staged.discard();
    result
}

fn place(state: &PluginState, staged: &Staged, mode: Commit) -> Result<ExternalInfo, Failure> {
    let _writing = state.externals.write_lock.lock().unwrap_or_else(PoisonError::into_inner);
    let id = &staged.manifest.id;
    let previous = state.external(id);
    match mode {
        Commit::Install if previous.is_some() && readable(state, id) => return Err(Failure::conflict("already installed")),
        Commit::Update if previous.is_none() => return Err(Failure::not_found("not installed")),
        _ => {}
    }
    let record = ExternalRecord {
        id: id.clone(),
        version: staged.manifest.version.clone(),
        source: staged.source.clone(),
        sha256: staged.sha256.clone(),
        granted: staged.manifest.permissions.clone(),
        installed_at: now_millis(),
    };
    let mut moved_to = None;
    let plugin_dir = store::plugin_dir(&state.state_dir, id);
    let plugin_dir_existed = plugin_dir.exists();
    // What is in the way of the new package (the same version installed, a
    // folder left by an install that was cut short, or a record that could
    // not be read) moves aside, so a failure can put it back. It sits next
    // to the staging folder, which a restart clears.
    let aside = store::staging_dir(&state.state_dir).join(format!("{}.old", staged.token));
    if record.source.kind != SourceKind::Folder {
        let target = store::package_dir(&state.state_dir, &record);
        let moved = fs::create_dir_all(&plugin_dir)
            .and_then(|()| match fs::rename(&target, &aside) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            })
            .and_then(|()| fs::rename(&staged.root, &target));
        if let Err(error) = moved {
            put_back(&aside, &target);
            undo(&plugin_dir, plugin_dir_existed, None);
            return Err(Failure::internal(format!("cannot move the package into place: {error}")));
        }
        moved_to = Some(target);
    }
    if let Err(error) = state.put_external(record.clone(), staged.manifest.clone()) {
        undo(&plugin_dir, plugin_dir_existed, moved_to.as_deref());
        if let Some(target) = &moved_to {
            put_back(&aside, target);
        }
        return Err(Failure::internal(format!("cannot save plugins.json: {error}")));
    }
    let _ = fs::remove_dir_all(&aside);
    if let Some(previous) = previous.filter(|previous| previous.source.kind != SourceKind::Folder) {
        let old = store::package_dir(&state.state_dir, &previous);
        if moved_to.as_deref() != Some(old.as_path()) {
            let _ = fs::remove_dir_all(old);
        }
    }
    Ok(ExternalInfo::new(&record, &staged.manifest))
}

/// Moves the folder set aside back to `target`, after a failed install. Without
/// one nothing happens.
fn put_back(aside: &Path, target: &Path) {
    if aside.exists() {
        let _ = fs::rename(aside, target);
    }
}

/// Removes what a failed install put on the disk: the moved package, and the
/// plugin's folder when this install made it.
fn undo(plugin_dir: &Path, plugin_dir_existed: bool, moved_to: Option<&Path>) {
    if let Some(target) = moved_to {
        let _ = fs::remove_dir_all(target);
    }
    if !plugin_dir_existed {
        let _ = fs::remove_dir_all(plugin_dir);
    }
}

/// Removes a plugin: its package versions and storage, then the record. A
/// folder source's own folder stays.
pub fn remove(state: &PluginState, id: &str) -> Result<(), Failure> {
    let _writing = state.externals.write_lock.lock().unwrap_or_else(PoisonError::into_inner);
    if state.external(id).is_none() {
        return Err(Failure::not_found("not installed"));
    }
    match fs::remove_dir_all(store::plugin_dir(&state.state_dir, id)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Failure::internal(format!("cannot delete the plugin's files: {error}"))),
    }
    state.remove_external(id).map_err(|e| Failure::internal(format!("cannot save plugins.json: {e}")))?;
    Ok(())
}

/// Whether a record may be checked or updated by a request without a new
/// address: only `.tar.gz` links must be installed again.
pub fn needs_reinstall(record: &ExternalRecord) -> bool {
    super::catalog::update_mode(&record.source) == UpdateMode::Reinstall
}
