//! `plugin.json`: what a plugin says about itself. Unknown keys are ignored.
//! A wrong value is refused with a one-line reason: `plugin.json: <key> <reason>`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::plugins::external::{is_external_id, is_program_name};

/// Longest `name`, in characters.
const MAX_NAME: usize = 40;
/// Longest `description`, in characters.
const MAX_DESCRIPTION: usize = 200;
/// Most programs in `permissions.run`.
const MAX_RUN: usize = 8;
/// Languages read from `i18n`. Other keys are ignored.
const LANGUAGES: [&str; 4] = ["ko", "ja", "zh", "en"];

/// Shells, interpreters, and wrappers that run other programs. This stops
/// mistakes; it is not a security boundary.
const FORBIDDEN: [&str; 47] = [
    "sh", "bash", "zsh", "fish", "dash", "ksh", "csh", "tcsh", "nu", "xonsh", "elvish", "pwsh", "powershell", "cmd", "wsl",
    "cscript", "wscript", "mshta", "env", "command", "builtin", "alias", "fc", "eval", "exec", "source", "sudo", "doas", "su",
    "runas", "xargs", "nohup", "nice", "time", "timeout", "watch", "script", "python", "python3", "node", "deno", "bun",
    "ruby", "perl", "php", "lua", "osascript",
];

/// What a plugin may do. Each is a separate grant.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Permissions {
    /// Program names the plugin may run, with any arguments.
    pub run: Vec<String>,
    /// Open terminal and browser panes.
    pub panes: bool,
    /// Keep running while the panel is closed.
    pub background: bool,
}

impl Permissions {
    /// What `next` has and `self` does not.
    pub fn added(&self, next: &Permissions) -> Permissions {
        Permissions {
            run: next.run.iter().filter(|name| !self.run.contains(name)).cloned().collect(),
            panes: next.panes && !self.panes,
            background: next.background && !self.background,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.run.is_empty() && !self.panes && !self.background
    }
}

/// A translated `name` and `description`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalText {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// An SVG in the package; none means the puzzle icon.
    pub icon: Option<String>,
    /// The HTML page of the panel.
    pub panel: String,
    /// The lowest westerm version, as `>=x.y.z`.
    pub westerm: Option<String>,
    pub permissions: Permissions,
    pub i18n: BTreeMap<String, LocalText>,
}

/// Whether `run` may not list this program: a shell, an interpreter, or a
/// wrapper. Case and a `.exe` ending do not matter, and neither does a
/// version (`python3.12` is `python`, `ksh93` is `ksh`).
pub fn forbidden_program(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let base = lower.strip_suffix(".exe").unwrap_or(&lower);
    let bare = base.trim_end_matches(|c: char| c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    let listed = |text: &str| FORBIDDEN.contains(&text);
    listed(base) || listed(bare)
}

/// Checks `bytes` as a `plugin.json`. `current` is the running westerm version.
pub fn parse(bytes: &[u8], current: &str) -> Result<Manifest, String> {
    let root: Value = serde_json::from_slice(bytes).map_err(|e| format!("plugin.json: not valid JSON ({e})"))?;
    let root = root.as_object().ok_or("plugin.json: must be a JSON object")?;

    let id = text(root, "id")?.ok_or("plugin.json: id is missing")?;
    if !is_external_id(&id) {
        return Err("plugin.json: id must look like author.name (a-z, 0-9, -)".into());
    }
    let version = text(root, "version")?.ok_or("plugin.json: version is missing")?;
    if triple(&version).is_none() {
        return Err("plugin.json: version must look like 1.2.3".into());
    }
    let name = limited(root, "name", MAX_NAME)?.ok_or("plugin.json: name is missing")?;
    if name.is_empty() {
        return Err("plugin.json: name must not be empty".into());
    }
    let description = limited(root, "description", MAX_DESCRIPTION)?.unwrap_or_default();
    let panel = text(root, "panel")?.ok_or("plugin.json: panel is missing")?;
    check_path("panel", &panel, ".html")?;
    let icon = text(root, "icon")?;
    if let Some(icon) = &icon {
        check_path("icon", icon, ".svg")?;
    }
    let westerm = text(root, "westerm")?;
    if let Some(range) = &westerm {
        check_westerm(range, current)?;
    }
    Ok(Manifest {
        id,
        name,
        version,
        description,
        icon,
        panel,
        westerm,
        permissions: permissions(root.get("permissions"))?,
        i18n: i18n(root.get("i18n"))?,
    })
}

/// Reads and checks `plugin.json` in `root`. The panel and the icon must be
/// plain files inside `root`.
pub fn read(root: &Path, current: &str) -> Result<Manifest, String> {
    let bytes = std::fs::read(root.join("plugin.json")).map_err(|e| format!("plugin.json: cannot read it ({e})"))?;
    let manifest = parse(&bytes, current)?;
    let real_root = root.canonicalize().map_err(|e| format!("plugin.json: cannot open the folder ({e})"))?;
    plain_file(&real_root, root, "panel", &manifest.panel)?;
    if let Some(icon) = &manifest.icon {
        plain_file(&real_root, root, "icon", icon)?;
    }
    Ok(manifest)
}

/// `key` is a regular file (not a link) under `root`.
fn plain_file(real_root: &Path, root: &Path, key: &str, relative: &str) -> Result<(), String> {
    let path = root.join(relative);
    let is_file = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file());
    let inside = path.canonicalize().is_ok_and(|real| real.starts_with(real_root));
    if is_file && inside {
        Ok(())
    } else {
        Err(format!("plugin.json: {key} {relative} is not a file in the package"))
    }
}

/// A string value, or `None` when the key is absent.
fn text(root: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match root.get(key) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("plugin.json: {key} must be a string")),
    }
}

/// A string of at most `max` characters (not bytes).
fn limited(root: &Map<String, Value>, key: &str, max: usize) -> Result<Option<String>, String> {
    let value = text(root, key)?;
    match &value {
        Some(value) if value.chars().count() > max => Err(format!("plugin.json: {key} must be at most {max} characters")),
        _ => Ok(value),
    }
}

/// `x.y.z` of digits only, with no `v` and no suffix.
fn triple(text: &str) -> Option<(u64, u64, u64)> {
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    text.split('.').all(digits).then(|| crate::update::parse_version(text)).flatten()
}

fn check_westerm(range: &str, current: &str) -> Result<(), String> {
    let minimum = range
        .strip_prefix(">=")
        .and_then(triple)
        .ok_or("plugin.json: westerm must look like >=1.2.3")?;
    match crate::update::parse_version(current) {
        Some(now) if now < minimum => Err(format!("plugin.json: westerm {range} is needed, this is {current}")),
        _ => Ok(()),
    }
}

/// A relative path in the package, the only form the asset route accepts:
/// no empty part, no part that starts with `.` (so no `.`, `..`, or dot file
/// such as `.env` or `.git/config` of a folder source), and no `\`, NUL, or
/// `:`. So no absolute path, no Windows drive, and no alternate data stream.
pub fn is_package_path(path: &str) -> bool {
    !path.contains(['\\', '\0', ':']) && path.split('/').all(|part| !part.is_empty() && !part.starts_with('.'))
}

/// `path` is a package path (see `is_package_path`) that ends with `extension`.
fn check_path(key: &str, path: &str, extension: &str) -> Result<(), String> {
    if !is_package_path(path) {
        return Err(format!("plugin.json: {key} must be a relative path inside the package"));
    }
    if !path.to_ascii_lowercase().ends_with(extension) {
        return Err(format!("plugin.json: {key} must end with {extension}"));
    }
    Ok(())
}

fn permissions(value: Option<&Value>) -> Result<Permissions, String> {
    let Some(value) = value else { return Ok(Permissions::default()) };
    let map = value.as_object().ok_or("plugin.json: permissions must be an object")?;
    let flag = |key: &str| match map.get(key) {
        None => Ok(false),
        Some(Value::Bool(on)) => Ok(*on),
        Some(_) => Err(format!("plugin.json: permissions.{key} must be true or false")),
    };
    Ok(Permissions { run: programs(map.get("run"))?, panes: flag("panes")?, background: flag("background")? })
}

fn programs(value: Option<&Value>) -> Result<Vec<String>, String> {
    let Some(value) = value else { return Ok(Vec::new()) };
    let list = value.as_array().ok_or("plugin.json: permissions.run must be a list of program names")?;
    let mut names: Vec<String> = Vec::new();
    for item in list {
        let name = item.as_str().ok_or("plugin.json: permissions.run must be a list of program names")?;
        if !is_program_name(name) {
            return Err(format!("plugin.json: permissions.run has {name:?}, which is not a plain program name"));
        }
        if forbidden_program(name) {
            return Err(format!("plugin.json: permissions.run may not list {name}, a shell or interpreter"));
        }
        if !names.iter().any(|known| known == name) {
            names.push(name.to_owned());
        }
    }
    if names.len() > MAX_RUN {
        return Err(format!("plugin.json: permissions.run lists more than {MAX_RUN} programs"));
    }
    Ok(names)
}

fn i18n(value: Option<&Value>) -> Result<BTreeMap<String, LocalText>, String> {
    let Some(value) = value else { return Ok(BTreeMap::new()) };
    let map = value.as_object().ok_or("plugin.json: i18n must be an object")?;
    let mut out = BTreeMap::new();
    for language in LANGUAGES {
        let Some(entry) = map.get(language) else { continue };
        let entry = entry.as_object().ok_or_else(|| format!("plugin.json: i18n.{language} must be an object"))?;
        let name = limited(entry, "name", MAX_NAME).map_err(|e| e.replacen("name", &format!("i18n.{language}.name"), 1))?;
        let description = limited(entry, "description", MAX_DESCRIPTION)
            .map_err(|e| e.replacen("description", &format!("i18n.{language}.description"), 1))?;
        out.insert(language.to_owned(), LocalText { name, description });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const GOOD: &str = r#"{"id":"k8s-tools.kubernetes","name":"Kubernetes","version":"0.3.1","description":"See pods.",
      "icon":"icon.svg","panel":"panel.html","westerm":">=0.4.0","permissions":{"run":["kubectl"],"panes":true,"background":true},
      "i18n":{"ko":{"name":"쿠버네티스"},"xx":{"name":"ignored"}},"extra":1}"#;

    /// GOOD with one top-level key replaced.
    fn with(key: &str, value: Value) -> Vec<u8> {
        let mut root: Value = serde_json::from_str(GOOD).unwrap();
        root[key] = value;
        root.to_string().into_bytes()
    }

    #[test]
    fn a_good_manifest_parses_and_ignores_unknown_keys() {
        let m = parse(GOOD.as_bytes(), "0.4.0").unwrap();
        assert_eq!((m.id.as_str(), m.version.as_str()), ("k8s-tools.kubernetes", "0.3.1"));
        assert_eq!(m.permissions, Permissions { run: vec!["kubectl".into()], panes: true, background: true });
        assert_eq!(m.i18n.keys().collect::<Vec<_>>(), vec!["ko"]);
        assert_eq!(parse(&with("permissions", json!({})), "0.4.0").unwrap().permissions, Permissions::default());
    }

    #[test]
    fn bad_values_are_refused_with_a_reason() {
        for (key, value) in [
            ("id", json!("docker")),
            ("id", json!("A.b")),
            ("version", json!("1.0")),
            ("version", json!("v1.0.0")),
            ("version", json!("1.0.0-rc.1")),
            ("westerm", json!("^0.4.0")),
            ("westerm", json!(">=9.0.0")),
            ("westerm", json!(">=v0.4.0")),
            ("name", json!("x".repeat(41))),
            ("name", json!("")),
            ("description", json!("x".repeat(201))),
            ("panel", json!("../panel.html")),
            ("panel", json!("/panel.html")),
            ("panel", json!("panel.js")),
            ("icon", json!("icon.png")),
            ("permissions", json!({"panes":"yes"})),
            ("permissions", json!({"run":["/usr/bin/kubectl"]})),
            ("permissions", json!({"run":["a", "b", "c", "d", "e", "f", "g", "h", "i"]})),
            ("permissions", json!({"run":["bash"]})),
            ("i18n", json!({"ko":{"name":"x".repeat(41)}})),
            ("i18n", json!({"ja":{"description":"x".repeat(201)}})),
        ] {
            let error = parse(&with(key, value.clone()), "0.4.0").unwrap_err();
            assert!(error.starts_with("plugin.json"), "{key}={value}: {error}");
        }
        assert!(parse(&with("name", json!("가".repeat(40))), "0.4.0").is_ok(), "characters, not bytes");
        assert!(parse(b"[]", "0.4.0").is_err());
        assert!(parse(b"{", "0.4.0").is_err());
    }

    #[test]
    fn path_fields_refuse_what_the_asset_route_refuses() {
        for bad in ["a//panel.html", "./panel.html", "a/./panel.html", "a/../panel.html", "..", "a\\panel.html", "a\0panel.html",
            "C:panel.html", "a:b/panel.html", "/abs/panel.html", "/panel.html", "panel.html/", ""]
        {
            assert!(parse(&with("panel", json!(bad)), "0.4.0").is_err(), "panel {bad:?}");
            let icon = bad.replace(".html", ".svg");
            assert!(parse(&with("icon", json!(icon)), "0.4.0").is_err(), "icon {icon:?}");
        }
        let m = parse(&with("panel", json!("ui/index.html")), "0.4.0").unwrap();
        assert_eq!(m.panel, "ui/index.html");
        assert!(parse(&with("icon", json!("img/icon.svg")), "0.4.0").is_ok());
    }

    #[test]
    fn run_names_are_joined_and_the_westerm_range_is_checked() {
        let m = parse(&with("permissions", json!({"run":["git", "git", "kubectl"]})), "0.4.0").unwrap();
        assert_eq!(m.permissions.run, vec!["git", "kubectl"]);
        assert!(parse(&with("westerm", json!(">=0.4.0")), "0.4.0").is_ok());
        assert!(parse(&with("westerm", json!(">=0.4.1")), "0.4.0").is_err());
        let mut root: Value = serde_json::from_str(GOOD).unwrap();
        root.as_object_mut().unwrap().remove("westerm");
        assert!(parse(root.to_string().as_bytes(), "0.0.1").is_ok(), "no key, no check");
    }

    #[test]
    fn shells_and_interpreters_are_refused_in_any_case_and_version() {
        for name in ["Bash", "ZSH", "python3", "Python3.12", "node18", "ksh93", "powershell.exe", "cmd.exe", "lua5.4", "sudo", "env"] {
            assert!(forbidden_program(name), "{name}");
        }
        for name in ["kubectl", "git", "docker", "helm", "gh", "k9s", "aws", "terraform-1.5"] {
            assert!(!forbidden_program(name), "{name}");
        }
    }

    #[test]
    fn added_permissions_are_what_the_update_adds() {
        let old = Permissions { run: vec!["kubectl".into()], panes: true, background: false };
        let new = Permissions { run: vec!["kubectl".into(), "helm".into()], panes: false, background: true };
        assert_eq!(old.added(&new), Permissions { run: vec!["helm".into()], panes: false, background: true });
        assert!(old.added(&old).is_empty());
    }

    #[test]
    fn read_needs_the_panel_and_icon_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("plugin.json"), GOOD).unwrap();
        assert!(read(root, "0.4.0").unwrap_err().contains("panel"));
        std::fs::write(root.join("panel.html"), "<p>hi</p>").unwrap();
        assert!(read(root, "0.4.0").unwrap_err().contains("icon"));
        std::fs::write(root.join("icon.svg"), "<svg/>").unwrap();
        assert_eq!(read(root, "0.4.0").unwrap().id, "k8s-tools.kubernetes");
        assert!(read(root, "0.3.0").is_err(), "the version check runs too");
        assert!(read(&root.join("missing"), "0.4.0").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn read_refuses_links() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("pkg");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("plugin.json"), GOOD).unwrap();
        std::fs::write(root.join("panel.html"), "<p>hi</p>").unwrap();
        std::fs::write(dir.path().join("outside.svg"), "<svg/>").unwrap();
        std::os::unix::fs::symlink(dir.path().join("outside.svg"), root.join("icon.svg")).unwrap();
        assert!(read(&root, "0.4.0").unwrap_err().contains("icon"));
        // A panel in a linked folder leaves the package too.
        std::fs::remove_file(root.join("icon.svg")).unwrap();
        std::fs::write(root.join("icon.svg"), "<svg/>").unwrap();
        std::fs::create_dir(dir.path().join("elsewhere")).unwrap();
        std::fs::write(dir.path().join("elsewhere/page.html"), "x").unwrap();
        std::os::unix::fs::symlink(dir.path().join("elsewhere"), root.join("ui")).unwrap();
        let manifest = String::from_utf8(with("panel", json!("ui/page.html"))).unwrap();
        std::fs::write(root.join("plugin.json"), manifest).unwrap();
        assert!(read(&root, "0.4.0").unwrap_err().contains("panel"));
    }
}
