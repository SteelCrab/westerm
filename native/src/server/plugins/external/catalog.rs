//! The plugin list (the "index") and the check for newer versions. The index
//! is a JSON array that westerm downloads at most once an hour. A newer
//! version is looked for in the index (index plugins) or in the latest
//! GitHub release (plugins installed from a repository address).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::manifest::Permissions;
use super::package::{fetch_meta, latest_release, parse_source_url, SourceUrl};
use super::store::{plain_version, Source, SourceKind};
use super::Sources;
use crate::plugins::external::{is_external_id, is_program_name};
use crate::server::plugins::PluginState;
use crate::server::Shared;
use crate::time::now_millis;
use crate::update::is_newer;

/// How long an index (and an answer about one plugin) is remembered.
const TTL_MS: u64 = 60 * 60 * 1000;
/// The first update check runs this long after the server starts.
const FIRST_CHECK: Duration = Duration::from_secs(10 * 60);
/// Then it runs this often.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// One plugin of the verified list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The repository, for people to read.
    #[serde(default)]
    pub repo: String,
    pub version: String,
    /// The `.tar.gz` package.
    pub url: String,
    /// The SHA-256 of the package, in lowercase hex.
    pub sha256: String,
    #[serde(default)]
    pub permissions: Permissions,
}

impl IndexEntry {
    /// The entry in its normal form, or `None` when it cannot be used: a bad
    /// id, version, address, sum, or program name.
    fn checked(mut self) -> Option<Self> {
        self.sha256.make_ascii_lowercase();
        let usable = is_external_id(&self.id)
            && !self.name.is_empty()
            && plain_version(&self.version)
            && !self.url.is_empty()
            && !self.url.chars().any(|c| c.is_whitespace() || c.is_control())
            && self.sha256.len() == 64
            && self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            && self.permissions.run.iter().all(|name| is_program_name(name));
        usable.then_some(self)
    }
}

/// Reads an index: a JSON array. An entry that does not fit is left out, and
/// so is a second entry with the same id. Anything but an array is an error.
pub fn parse_index(bytes: &[u8]) -> Option<Vec<IndexEntry>> {
    let Value::Array(items) = serde_json::from_slice(bytes).ok()? else { return None };
    let mut entries: Vec<IndexEntry> = Vec::new();
    for item in items {
        let Some(entry) = serde_json::from_value::<IndexEntry>(item).ok().and_then(IndexEntry::checked) else { continue };
        if !entries.iter().any(|known| known.id == entry.id) {
            entries.push(entry);
        }
    }
    Some(entries)
}

/// The index, kept for an hour after a good download.
#[derive(Default)]
pub struct IndexCache {
    cached: Mutex<Option<(u64, Vec<IndexEntry>)>>,
}

impl IndexCache {
    /// The index as of `now` (milliseconds). Downloads it when the kept one is
    /// an hour old or there is none. A failed download is not kept: `None`,
    /// and the next call tries again. This waits for the network.
    pub fn get(&self, sources: &Sources, now: u64) -> Option<Vec<IndexEntry>> {
        let mut cached = self.cached.lock().ok()?;
        if let Some((at, entries)) = cached.as_ref()
            && now.saturating_sub(*at) < TTL_MS
        {
            return Some(entries.clone());
        }
        let entries = fetch_meta(&sources.index_url, sources).ok().and_then(|body| parse_index(&body))?;
        *cached = Some((now, entries.clone()));
        Some(entries)
    }

    /// Forgets the kept index, so the next `get` downloads it again.
    #[cfg(all(test, unix))]
    pub fn clear(&self) {
        if let Ok(mut cached) = self.cached.lock() {
            *cached = None;
        }
    }
}

/// The newest version found for one plugin, and when it was looked for.
struct Checked {
    at: u64,
    version: Option<String>,
}

/// The index and what the last checks found.
#[derive(Default)]
pub struct Catalog {
    pub index: IndexCache,
    latest: Mutex<HashMap<String, Checked>>,
}

impl Catalog {
    /// The newest version found for the plugin, when it was looked for.
    pub fn latest(&self, id: &str) -> Option<String> {
        self.latest.lock().ok()?.get(id)?.version.clone()
    }

    fn fresh(&self, id: &str, now: u64) -> bool {
        self.latest.lock().is_ok_and(|latest| latest.get(id).is_some_and(|checked| now.saturating_sub(checked.at) < TTL_MS))
    }

    /// Notes that the plugin was looked for. A failed look (`None`) keeps the
    /// version found before.
    fn remember(&self, id: &str, now: u64, version: Option<String>) {
        let Ok(mut latest) = self.latest.lock() else { return };
        let checked = latest.entry(id.to_owned()).or_insert(Checked { at: now, version: None });
        checked.at = now;
        if version.is_some() {
            checked.version = version;
        }
    }
}

/// How a plugin gets a new version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateMode {
    /// The server looks for a newer version.
    Check,
    /// A `.tar.gz` link: the user installs it again.
    Reinstall,
    /// A local folder: read it again.
    Reload,
}

pub fn update_mode(source: &Source) -> UpdateMode {
    match source.kind {
        SourceKind::Index => UpdateMode::Check,
        SourceKind::Folder => UpdateMode::Reload,
        SourceKind::Url => match source.url.as_deref().map(parse_source_url) {
            Some(Ok(SourceUrl::GitHub { .. })) => UpdateMode::Check,
            _ => UpdateMode::Reinstall,
        },
    }
}

/// Looks for a newer version of every plugin that can be checked and was not
/// looked for in the last hour. Returns how many were looked for: no plugin
/// from the index or from GitHub means no request at all. This waits for the
/// network.
pub fn check_updates(state: &PluginState, now: u64) -> usize {
    let sources = &state.externals.sources;
    let catalog = &state.externals.catalog;
    let mut looked_for = 0;
    for info in state.external_infos() {
        if update_mode(&info.source) != UpdateMode::Check || catalog.fresh(&info.id, now) {
            continue;
        }
        looked_for += 1;
        let latest = match info.source.kind {
            SourceKind::Index => {
                // A failed download is not noted, so opening the list again tries again.
                let Some(entries) = catalog.index.get(sources, now) else { continue };
                entries.into_iter().find(|entry| entry.id == info.id).map(|entry| entry.version)
            }
            _ => match info.source.url.as_deref().map(parse_source_url) {
                Some(Ok(SourceUrl::GitHub { owner, repo })) => latest_release(sources, &owner, &repo).ok().map(|release| release.version),
                _ => None,
            },
        };
        catalog.remember(&info.id, now, latest);
    }
    looked_for
}

/// The version to offer for an installed plugin, when it is newer.
pub fn available_update(state: &PluginState, id: &str, installed: &str) -> Option<String> {
    state.externals.catalog.latest(id).filter(|latest| is_newer(latest, installed))
}

/// Checks for updates 10 minutes after the server starts and then every 24
/// hours. A server without index or GitHub plugins makes no request.
pub fn spawn_daily_check(state: Shared) {
    tokio::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            let state = state.clone();
            let _ = tokio::task::spawn_blocking(move || check_updates(&state.plugins, now_millis())).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::server::plugins::external::manifest;
    use crate::server::plugins::external::store::ExternalRecord;
    use serde_json::json;
    use std::path::Path;

    const SUM: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn entry(id: &str, version: &str) -> Value {
        json!({"id": id, "name": "Kubernetes", "description": "See pods.", "repo": "https://github.com/k8s-tools/westerm-k8s",
               "version": version, "url": "https://example.test/k8s.tar.gz", "sha256": SUM, "permissions": {"run": ["kubectl"]}})
    }

    fn sources(dir: &Path) -> Sources {
        Sources { index_url: format!("file://{}/index.json", dir.display()), github_api: format!("file://{}/api", dir.display()), allow_file: true }
    }

    fn write_index(dir: &Path, entries: &[Value]) {
        std::fs::write(dir.join("index.json"), serde_json::to_vec(entries).unwrap()).unwrap();
    }

    /// A plugin of this kind in `state`, with a package folder under it.
    fn install(state: &PluginState, dir: &Path, source: Source, version: &str) {
        let package = dir.join("pkg").join(version);
        std::fs::create_dir_all(&package).unwrap();
        let manifest_json = json!({"id": "k8s-tools.kubernetes", "name": "Kubernetes", "version": version, "panel": "panel.html"});
        std::fs::write(package.join("plugin.json"), manifest_json.to_string()).unwrap();
        std::fs::write(package.join("panel.html"), "<p>panel</p>").unwrap();
        let manifest = manifest::read(&package, "0.4.0").unwrap();
        let source = if source.kind == SourceKind::Folder { Source { path: Some(package.display().to_string()), ..source } } else { source };
        let record = ExternalRecord {
            id: "k8s-tools.kubernetes".into(),
            version: version.into(),
            source,
            sha256: None,
            granted: Permissions::default(),
            installed_at: 1,
        };
        state.put_external(record, manifest).unwrap();
        // A non-folder record keeps its package under the state folder.
        if let Some(path) = state.external("k8s-tools.kubernetes").filter(|r| r.source.kind != SourceKind::Folder) {
            let target = super::super::store::package_dir(&state.state_dir, &path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            let _ = std::fs::remove_dir_all(&target);
            std::fs::rename(&package, &target).unwrap();
            state.put_external(path, manifest::read(&target, "0.4.0").unwrap()).unwrap();
        }
    }

    fn index_source() -> Source {
        Source { kind: SourceKind::Index, url: Some("https://example.test/k8s.tar.gz".into()), path: None }
    }

    #[test]
    fn the_index_is_kept_for_an_hour() {
        let dir = tempfile::tempdir().unwrap();
        let sources = sources(dir.path());
        let cache = IndexCache::default();
        assert!(cache.get(&sources, 1_000).is_none(), "no file, no index");
        write_index(dir.path(), &[entry("k8s-tools.kubernetes", "0.3.1")]);
        assert_eq!(cache.get(&sources, 1_000).unwrap()[0].version, "0.3.1", "a failure was not kept");
        write_index(dir.path(), &[entry("k8s-tools.kubernetes", "0.4.0")]);
        assert_eq!(cache.get(&sources, 1_000 + 3_599_999).unwrap()[0].version, "0.3.1", "still the kept one");
        assert_eq!(cache.get(&sources, 1_000 + 3_600_000).unwrap()[0].version, "0.4.0", "an hour later");
        std::fs::write(dir.path().join("index.json"), "{not json").unwrap();
        assert!(cache.get(&sources, 1_000 + 3_600_000 * 3).is_none(), "a broken file is a failure");
    }

    #[test]
    fn the_index_drops_entries_that_do_not_fit() {
        let mut bad_sum = entry("a.sum", "1.0.0");
        bad_sum["sha256"] = json!("abc");
        let mut upper = entry("a.upper", "1.0.0");
        upper["sha256"] = json!(SUM.to_uppercase());
        let mut bad_run = entry("a.run", "1.0.0");
        bad_run["permissions"] = json!({"run": ["../kubectl"]});
        let mut no_name = entry("a.name", "1.0.0");
        no_name["name"] = json!("");
        let body = json!([
            entry("k8s-tools.kubernetes", "0.3.1"),
            entry("k8s-tools.kubernetes", "9.9.9"), // the same id twice: the first wins
            entry("docker", "1.0.0"),               // not an external id
            entry("a.version", "1.0"),
            entry("a.suffix", "1.0.0-rc.1"),
            bad_sum,
            upper,
            bad_run,
            no_name,
            json!("text"),
            json!({"id": "a.partial"}),
        ]);
        let entries = parse_index(body.to_string().as_bytes()).unwrap();
        let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, vec!["k8s-tools.kubernetes", "a.upper"]);
        assert_eq!(entries[0].version, "0.3.1");
        assert_eq!(entries[1].sha256, SUM, "the sum is lowercased");
        assert!(parse_index(br#"{"plugins":[]}"#).is_none(), "the index is an array");
        assert!(parse_index(b"[]").unwrap().is_empty());
    }

    #[test]
    fn the_daily_check_asks_nothing_without_updatable_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = PluginState::load(dir.path());
        state.externals.sources = sources(dir.path());
        assert_eq!(check_updates(&state, 5), 0, "nothing installed");
        install(&state, dir.path(), Source { kind: SourceKind::Folder, url: None, path: None }, "0.3.1");
        assert_eq!(check_updates(&state, 5), 0, "a folder is reloaded by hand");
        install(&state, dir.path(), Source { kind: SourceKind::Url, url: Some("https://example.test/k8s.tar.gz".into()), path: None }, "0.3.1");
        assert_eq!(check_updates(&state, 5), 0, "a .tar.gz link is installed again by hand");
        assert!(!dir.path().join("index.json").exists());
    }

    #[test]
    fn an_index_plugin_is_looked_for_once_an_hour() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = PluginState::load(dir.path());
        state.externals.sources = sources(dir.path());
        install(&state, dir.path(), index_source(), "0.3.1");

        assert_eq!(check_updates(&state, 10), 1);
        assert_eq!(available_update(&state, "k8s-tools.kubernetes", "0.3.1"), None, "no index yet");
        write_index(dir.path(), &[entry("k8s-tools.kubernetes", "0.4.0")]);
        assert_eq!(check_updates(&state, 11), 1, "a failed look is looked for again at once");
        assert_eq!(available_update(&state, "k8s-tools.kubernetes", "0.3.1").as_deref(), Some("0.4.0"));
        assert_eq!(available_update(&state, "k8s-tools.kubernetes", "0.4.0"), None, "not newer than what is installed");
        assert_eq!(check_updates(&state, 12), 0, "remembered");
        assert_eq!(check_updates(&state, 11 + TTL_MS), 1, "an hour later");
    }

    #[test]
    fn a_github_plugin_is_looked_for_by_its_latest_tag() {
        let dir = tempfile::tempdir().unwrap();
        let api = dir.path().join("api/repos/k8s-tools/westerm-k8s/releases");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::write(api.join("latest"), json!({"tag_name": "v0.5.0", "assets": []}).to_string()).unwrap();
        let mut state = PluginState::load(dir.path());
        state.externals.sources = sources(dir.path());
        let github = Source { kind: SourceKind::Url, url: Some("https://github.com/k8s-tools/westerm-k8s".into()), path: None };
        install(&state, dir.path(), github.clone(), "0.3.1");
        assert_eq!(update_mode(&github), UpdateMode::Check);
        assert_eq!(check_updates(&state, 10), 1);
        assert_eq!(available_update(&state, "k8s-tools.kubernetes", "0.3.1").as_deref(), Some("0.5.0"));
        assert!(!dir.path().join("index.json").exists(), "the index is not downloaded for a GitHub plugin");
    }

    #[test]
    fn update_modes_follow_the_source() {
        let url = |text: &str| Source { kind: SourceKind::Url, url: Some(text.into()), path: None };
        assert_eq!(update_mode(&index_source()), UpdateMode::Check);
        assert_eq!(update_mode(&url("https://github.com/o/r")), UpdateMode::Check);
        assert_eq!(update_mode(&url("https://example.test/k8s.tar.gz")), UpdateMode::Reinstall);
        assert_eq!(update_mode(&Source { kind: SourceKind::Folder, url: None, path: Some("/work/k8s".into()) }), UpdateMode::Reload);
    }
}
