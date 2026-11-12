//! What `plugins.json` says about external plugins, and where their files
//! live. A record is what the user confirmed at install: the server checks
//! permissions against `granted`, never against a later `plugin.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::manifest::{LocalText, Manifest, Permissions};
use crate::plugins::external::is_external_id;
use crate::plugins::{DEFAULT_ON, PLUGIN_IDS};

/// Where a plugin came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// The verified list.
    Index,
    /// A GitHub repository or a `.tar.gz` link.
    Url,
    /// A local folder, used in place.
    Folder,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub kind: SourceKind,
    /// The address, for `index` and `url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The folder, for `folder`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// One installed external plugin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRecord {
    pub id: String,
    pub version: String,
    pub source: Source,
    /// The SHA-256 of the package, in lowercase hex. A folder has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// What the user confirmed.
    #[serde(default)]
    pub granted: Permissions,
    /// When it was installed, updated, or reloaded, in milliseconds. The
    /// page restarts a plugin's instance when this changes.
    #[serde(default)]
    pub installed_at: u64,
}

/// The shape of `plugins.json`. `installed` is built from the other parts:
/// the built-in ids that are on, then the id of every record. `off` lists the
/// default-on plugins (`DEFAULT_ON`) the user turned off: a file written
/// before such a plugin existed does not name it, and it stays on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PluginsFile {
    pub installed: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub off: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub external: Vec<ExternalRecord>,
}

/// No file yet: only the default-on plugins.
impl Default for PluginsFile {
    fn default() -> Self {
        Self::new(DEFAULT_ON, Vec::new())
    }
}

impl PluginsFile {
    /// `builtin` are the known built-in ids that are on; `external` are valid records.
    pub fn new(builtin: &[&str], external: Vec<ExternalRecord>) -> Self {
        let installed = builtin.iter().map(|id| (*id).to_owned()).chain(external.iter().map(|record| record.id.clone())).collect();
        let off = DEFAULT_ON.iter().filter(|id| !builtin.contains(id)).map(|id| (*id).to_owned()).collect();
        Self { installed, off, external }
    }

    /// The built-in ids that are on.
    pub fn builtin(&self) -> Vec<&'static str> {
        PLUGIN_IDS.iter().copied().filter(|id| self.installed.iter().any(|on| on == id)).collect()
    }
}

/// What the page needs to list one external plugin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalInfo {
    pub id: String,
    pub version: String,
    pub name: String,
    pub description: String,
    pub i18n: BTreeMap<String, LocalText>,
    pub icon: Option<String>,
    pub panel: String,
    pub source: Source,
    pub granted: Permissions,
    pub installed_at: u64,
}

impl ExternalInfo {
    pub fn new(record: &ExternalRecord, manifest: &Manifest) -> Self {
        Self {
            id: record.id.clone(),
            version: record.version.clone(),
            name: manifest.name.clone(),
            description: manifest.description.clone(),
            i18n: manifest.i18n.clone(),
            icon: manifest.icon.clone(),
            panel: manifest.panel.clone(),
            source: record.source.clone(),
            granted: record.granted.clone(),
            installed_at: record.installed_at,
        }
    }
}

/// `x.y.z` of digits only. A version is a folder name, so nothing else may pass.
pub fn plain_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// A record the server can use: an external id and a plain version, because
/// both become folder names.
fn usable(record: &ExternalRecord) -> bool {
    is_external_id(&record.id) && plain_version(&record.version)
}

/// Reads a `plugins.json`. A broken file reads as empty. A record that does
/// not parse, has a bad id or version, or repeats an id is dropped. Built-in
/// ids are kept if this build knows them; an external id is installed when
/// it has a record.
pub fn parse_plugins_file(bytes: &[u8]) -> PluginsFile {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Raw {
        installed: Vec<String>,
        off: Vec<String>,
        external: Vec<Value>,
    }
    let Ok(raw) = serde_json::from_slice::<Raw>(bytes) else { return PluginsFile::default() };
    let mut external: Vec<ExternalRecord> = Vec::new();
    for value in raw.external {
        let Ok(record) = serde_json::from_value::<ExternalRecord>(value) else { continue };
        if usable(&record) && !external.iter().any(|known| known.id == record.id) {
            external.push(record);
        }
    }
    // A default-on plugin is on unless the file says the user turned it off.
    let on = |id: &&str| raw.installed.iter().any(|on| on == id) || (DEFAULT_ON.contains(id) && !raw.off.iter().any(|off| off == id));
    let builtin: Vec<&str> = PLUGIN_IDS.iter().copied().filter(on).collect();
    PluginsFile::new(&builtin, external)
}

/// `<state>/plugins`.
fn plugins_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("plugins")
}

/// Everything the app keeps for one plugin: its package versions and its storage.
pub fn plugin_dir(state_dir: &Path, id: &str) -> PathBuf {
    plugins_dir(state_dir).join(id)
}

/// The folder with the record's `plugin.json`: the folder itself for a folder
/// source, else `<state>/plugins/<id>/<version>`.
pub fn package_dir(state_dir: &Path, record: &ExternalRecord) -> PathBuf {
    match (&record.source.kind, &record.source.path) {
        (SourceKind::Folder, Some(path)) => PathBuf::from(path),
        _ => plugin_dir(state_dir, &record.id).join(&record.version),
    }
}

/// The plugin's storage file (`storage` in the panel API). The storage route
/// of a later change reads it.
#[allow(dead_code)]
pub fn data_path(state_dir: &Path, id: &str) -> PathBuf {
    plugin_dir(state_dir, id).join("data.json")
}

/// Where a package is unpacked and checked before it moves to its folder.
pub fn staging_dir(state_dir: &Path) -> PathBuf {
    plugins_dir(state_dir).join(".staging")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    fn k8s(extra: serde_json::Value) -> serde_json::Value {
        let mut record = json!({
            "id": "k8s-tools.kubernetes", "version": "0.3.1",
            "source": {"kind": "index", "url": "https://example.test/k8s.tar.gz"},
            "sha256": "ab12", "granted": {"run": ["kubectl"], "panes": true, "background": true},
            "installedAt": 1_794_139_521_000u64
        });
        if let (Some(record), Some(extra)) = (record.as_object_mut(), extra.as_object()) {
            record.extend(extra.clone());
        }
        record
    }

    #[test]
    fn old_plugins_files_still_read() {
        // A file from before the Git plugin does not name it: Git stays on.
        let file = parse_plugins_file(br#"{"installed":["docker"]}"#);
        assert_eq!(file.installed, vec!["git", "docker"]);
        assert!(file.external.is_empty());
        assert_eq!(parse_plugins_file(b"{not json").installed, vec!["git"]);
        assert_eq!(parse_plugins_file(b"").installed, vec!["git"]);
        assert_eq!(parse_plugins_file(br#"{"installed":"docker"}"#).installed, vec!["git"]);
        assert_eq!(parse_plugins_file(br#"{"installed":["docker","nope","docker"]}"#).installed, vec!["git", "docker"]);
    }

    #[test]
    fn a_default_on_plugin_is_on_until_the_file_says_off() {
        assert_eq!(PluginsFile::default().installed, vec!["git"], "no file yet");
        let off = parse_plugins_file(br#"{"installed":["docker"],"off":["git"]}"#);
        assert_eq!(off.installed, vec!["docker"]);
        assert_eq!(off.off, vec!["git"]);
        assert_eq!(parse_plugins_file(serde_json::to_string(&off).unwrap().as_bytes()), off, "off survives a write and a read");
        // `installed` wins over `off`, and `off` means nothing for other plugins.
        assert_eq!(parse_plugins_file(br#"{"installed":["git"],"off":["git","docker"]}"#), PluginsFile::new(&["git"], Vec::new()));
        assert!(PluginsFile::new(&["git", "docker"], Vec::new()).off.is_empty());
    }

    #[test]
    fn external_ids_need_a_record() {
        let body = json!({"installed": ["k8s-tools.kubernetes", "a.b", "docker"], "external": [k8s(json!({}))]});
        let file = parse_plugins_file(body.to_string().as_bytes());
        assert_eq!(file.installed, vec!["git", "docker", "k8s-tools.kubernetes"], "built-in first, then external in record order");
        assert_eq!(file.external.len(), 1);
        assert_eq!(file.external[0].installed_at, 1_794_139_521_000);
        assert_eq!(file.external[0].granted.run, vec!["kubectl"]);
    }

    #[test]
    fn unusable_records_are_dropped() {
        let body = json!({"installed": [], "external": [
            k8s(json!({})),
            k8s(json!({})),                           // same id twice: the first wins
            k8s(json!({"id": "docker"})),             // a built-in id
            k8s(json!({"id": "../evil.x"})),          // a path in the id
            k8s(json!({"id": "a.b", "version": "../../x"})), // a path in the version
            k8s(json!({"id": "a.c", "version": "1.0"})),
            k8s(json!({"id": "a.d", "source": {"kind": "ftp"}})),
            json!("text"),
        ]});
        let file = parse_plugins_file(body.to_string().as_bytes());
        assert_eq!(file.installed, vec!["git", "k8s-tools.kubernetes"]);
        assert_eq!(file.external.len(), 1);
    }

    #[test]
    fn a_record_without_a_time_or_with_null_source_fields_reads() {
        let body = json!({"external": [{
            "id": "a.b", "version": "1.0.0", "source": {"kind": "folder", "url": null, "path": "/work/plugin"},
            "sha256": null, "granted": {}
        }]});
        let file = parse_plugins_file(body.to_string().as_bytes());
        let record = &file.external[0];
        assert_eq!((record.source.kind, record.source.url.as_deref(), record.source.path.as_deref()), (SourceKind::Folder, None, Some("/work/plugin")));
        assert_eq!((record.sha256.as_deref(), record.installed_at), (None, 0));
        assert!(record.granted.is_empty());
    }

    #[test]
    fn absent_optional_fields_are_left_out_when_written() {
        let record = ExternalRecord {
            id: "a.b".into(),
            version: "1.0.0".into(),
            source: Source { kind: SourceKind::Folder, url: None, path: Some("/work/plugin".into()) },
            sha256: None,
            granted: Permissions::default(),
            installed_at: 7,
        };
        assert_eq!(
            serde_json::to_string(&record).unwrap(),
            r#"{"id":"a.b","version":"1.0.0","source":{"kind":"folder","path":"/work/plugin"},"granted":{"run":[],"panes":false,"background":false},"installedAt":7}"#
        );
        let file = PluginsFile::new(&["git", "docker"], Vec::new());
        assert_eq!(serde_json::to_string(&file).unwrap(), r#"{"installed":["git","docker"]}"#, "the old shape when nothing is external or off");
        let with = PluginsFile::new(&[], vec![record]);
        assert_eq!(parse_plugins_file(serde_json::to_string(&with).unwrap().as_bytes()), with, "it reads back");
    }

    #[test]
    fn folders_follow_the_record() {
        let state = Path::new("/state");
        let mut record = parse_plugins_file(json!({"external": [k8s(json!({}))]}).to_string().as_bytes()).external.remove(0);
        assert_eq!(package_dir(state, &record), state.join("plugins").join("k8s-tools.kubernetes").join("0.3.1"));
        record.source = Source { kind: SourceKind::Folder, url: None, path: Some("/work/k8s".into()) };
        assert_eq!(package_dir(state, &record), Path::new("/work/k8s"), "a folder source is used in place");
        assert_eq!(data_path(state, "k8s-tools.kubernetes"), state.join("plugins").join("k8s-tools.kubernetes").join("data.json"));
        assert_eq!(staging_dir(state), state.join("plugins").join(".staging"));
    }
}
