//! Files the dashboard reads for the browser: Ghostty's font settings,
//! ~/.ssh/config hosts, and the saved layout.

use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::protocol::normalize_ssh_target;
use crate::state_dir::home_dir;

#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalConfig {
    /// Ghostty font-family values in fallback order; empty when unset.
    pub font_families: Vec<String>,
    /// Ghostty font-size in points; null when unset.
    pub font_size: Option<f64>,
}

fn unquote(value: &str) -> &str {
    let value = value.trim();
    value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value)
}

/// Repeated font-family lines build a fallback list; an empty value clears
/// it; the first font-family in a file replaces inherited ones.
pub fn parse_ghostty_font(text: &str, base: TerminalConfig) -> TerminalConfig {
    let mut config = base;
    let mut families_set = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        if key.starts_with('#') {
            continue;
        }
        let value = unquote(value);
        match key {
            "font-family" => {
                if !families_set {
                    config.font_families.clear();
                    families_set = true;
                }
                if value.is_empty() {
                    config.font_families.clear();
                } else if !config.font_families.iter().any(|f| f == value) {
                    config.font_families.push(value.to_owned());
                }
            }
            "font-size" => {
                if let Ok(size) = value.parse::<f64>() {
                    if (6.0..=72.0).contains(&size) {
                        config.font_size = Some(size);
                    }
                }
            }
            _ => {}
        }
    }
    config
}

pub fn read_terminal_config() -> TerminalConfig {
    let home = home_dir();
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    let paths = [
        xdg.join("ghostty").join("config"),
        home.join("Library/Application Support/com.mitchellh.ghostty/config"),
    ];
    paths.iter().fold(TerminalConfig::default(), |config, path| match fs::read_to_string(path) {
        Ok(text) => parse_ghostty_font(&text, config),
        Err(_) => config,
    })
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SshHost {
    /// The Host alias, which `ssh <alias>` accepts.
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
}

/// Concrete Host entries; patterns (`*`, `?`, `!`) are skipped. Returns the
/// Include patterns so the caller can follow them.
pub fn parse_ssh_config(text: &str) -> (Vec<SshHost>, Vec<String>) {
    let mut hosts: Vec<SshHost> = Vec::new();
    let mut includes = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let Some((key, rest)) = line.split_once(|c: char| c.is_whitespace() || c == '=') else { continue };
        let values: Vec<String> = rest
            .trim_start_matches(|c: char| c.is_whitespace() || c == '=')
            .split_whitespace()
            .map(|v| v.trim_matches('"').to_owned())
            .collect();
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                current.clear();
                for alias in values {
                    if alias.contains(['*', '?', '!']) || normalize_ssh_target(&alias).as_deref() != Some(alias.as_str()) {
                        continue;
                    }
                    current.push(hosts.len());
                    hosts.push(SshHost { alias, hostname: None, user: None });
                }
            }
            "match" => current.clear(),
            "include" => includes.extend(values),
            "hostname" => {
                for &i in &current {
                    hosts[i].hostname.get_or_insert_with(|| values.first().cloned().unwrap_or_default());
                }
            }
            "user" => {
                for &i in &current {
                    hosts[i].user.get_or_insert_with(|| values.first().cloned().unwrap_or_default());
                }
            }
            _ => {}
        }
    }
    (hosts, includes)
}

/// Expands "~/" and one "*" in the file name; enough for `Include config.d/*`.
fn expand_include(pattern: &str, ssh_dir: &Path) -> Vec<PathBuf> {
    let path = match pattern.strip_prefix("~/") {
        Some(rest) => home_dir().join(rest),
        None if Path::new(pattern).is_absolute() => PathBuf::from(pattern),
        None => ssh_dir.join(pattern),
    };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let Some((prefix, suffix)) = name.split_once('*') else { return vec![path] };
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut found: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with(prefix) && n.ends_with(suffix))
                .map(|n| dir.join(n))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

/// Hosts from ~/.ssh/config and its includes; the first definition wins.
pub fn read_ssh_hosts(ssh_dir: &Path) -> Vec<SshHost> {
    fn visit(file: &Path, ssh_dir: &Path, depth: usize, out: &mut Vec<SshHost>) {
        if depth > 4 {
            return;
        }
        let Ok(text) = fs::read_to_string(file) else { return };
        let (hosts, includes) = parse_ssh_config(&text);
        for host in hosts {
            if !out.iter().any(|known| known.alias == host.alias) {
                out.push(host);
            }
        }
        for pattern in includes {
            for included in expand_include(&pattern, ssh_dir) {
                visit(&included, ssh_dir, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    visit(&ssh_dir.join("config"), ssh_dir, 0, &mut out);
    out
}

pub const WORKSPACE_MAX_BYTES: usize = 256 * 1024;

pub fn workspace_path(state_dir: &Path) -> PathBuf {
    state_dir.join("workspace.json")
}

/// The browser validates the layout fully on load; the server keeps it
/// private, bounded, and well-formed.
pub fn write_workspace(state_dir: &Path, body: &[u8]) -> bool {
    if body.len() > WORKSPACE_MAX_BYTES {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else { return false };
    if value.get("version").and_then(serde_json::Value::as_u64) != Some(1)
        || !value.get("tabs").is_some_and(serde_json::Value::is_array)
        || !value.get("sessions").is_some_and(serde_json::Value::is_array)
    {
        return false;
    }
    let path = workspace_path(state_dir);
    let temp = path.with_extension("json.tmp");
    let written = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)
        .and_then(|mut file| file.write_all(&serde_json::to_vec(&value).unwrap_or_default()))
        .and_then(|()| fs::rename(&temp, &path));
    written.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ghostty_fonts_build_a_fallback_list() {
        let config = parse_ghostty_font(
            "font-family = D2Coding\nfont-family = \"Symbols Nerd Font Mono\"\nfont-family-bold = D2Coding\nfont-size = 15\n# font-family = X\n",
            TerminalConfig::default(),
        );
        assert_eq!(config.font_families, ["D2Coding", "Symbols Nerd Font Mono"]);
        assert_eq!(config.font_size, Some(15.0));
        let reset = parse_ghostty_font("font-family = A\nfont-family =\nfont-family = B\nfont-size = huge", TerminalConfig::default());
        assert_eq!(reset, TerminalConfig { font_families: vec!["B".into()], font_size: None });
    }

    #[test]
    fn ssh_config_yields_concrete_hosts() {
        let (hosts, includes) = parse_ssh_config(
            "Host *\n  ServerAliveInterval 30\nHost prod staging  # two\n  HostName 10.0.0.5\n  User deploy\nHost *.internal !bastion\n  User nobody\nMatch host foo\n  User ignored\nHost \"quoted\"\nInclude config.d/*\n",
        );
        let names: Vec<_> = hosts.iter().map(|h| (h.alias.as_str(), h.hostname.as_deref(), h.user.as_deref())).collect();
        assert_eq!(names, [("prod", Some("10.0.0.5"), Some("deploy")), ("staging", Some("10.0.0.5"), Some("deploy")), ("quoted", None, None)]);
        assert_eq!(includes, ["config.d/*"]);
    }

    #[test]
    fn includes_are_followed_first_definition_wins() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("config.d")).unwrap();
        fs::write(dir.path().join("config"), "Host a\n  HostName first\nInclude config.d/*\n").unwrap();
        fs::write(dir.path().join("config.d/work"), "Host a\n  HostName second\nHost b\n").unwrap();
        let hosts = read_ssh_hosts(dir.path());
        let names: Vec<_> = hosts.iter().map(|h| (h.alias.as_str(), h.hostname.as_deref())).collect();
        assert_eq!(names, [("a", Some("first")), ("b", None)]);
        assert!(read_ssh_hosts(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn workspace_saves_are_bounded_and_well_formed() {
        let dir = tempfile::tempdir().unwrap();
        assert!(write_workspace(dir.path(), br#"{"version":1,"tabs":[],"sessions":[]}"#));
        assert!(!write_workspace(dir.path(), b"not json"));
        assert!(!write_workspace(dir.path(), br#"{"version":2,"tabs":[],"sessions":[]}"#));
        assert!(!write_workspace(dir.path(), &vec![b' '; WORKSPACE_MAX_BYTES + 1]));
    }
}
