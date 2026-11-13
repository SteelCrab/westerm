//! The Terraform plugin's settings: `<state>/terraform/settings.json`.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::plugins::terraform::{private_dir, root_dir, Tool};
use crate::state_dir::private_file;

/// What the user set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Ask the public registry for newer module versions.
    pub check_versions: bool,
    /// The tool the user chose for a project, by its folder.
    pub tools: HashMap<PathBuf, Tool>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { check_versions: true, tools: HashMap::new() }
    }
}

fn path(state_dir: &Path) -> PathBuf {
    root_dir(state_dir).join("settings.json")
}

/// The saved settings; a missing or broken file reads as the defaults.
pub fn load(state_dir: &Path) -> Settings {
    fs::read(path(state_dir)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Writes the settings whole, readable by the user only.
pub fn save(state_dir: &Path, settings: &Settings) -> io::Result<()> {
    private_dir(&root_dir(state_dir))?;
    let (path, temp) = (path(state_dir), root_dir(state_dir).join("settings.json.tmp"));
    let body = serde_json::to_vec(settings).map_err(io::Error::other)?;
    let mut file = private_file().write(true).create(true).truncate(true).open(&temp)?;
    file.write_all(&body)?;
    drop(file);
    fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn broken_settings_read_as_default() {
        let dir = tempfile::tempdir().unwrap();
        let default = load(dir.path());
        assert!(default.check_versions);
        assert!(default.tools.is_empty());
        fs::create_dir_all(dir.path().join("terraform")).unwrap();
        for broken in ["{", "[]", r#"{"checkVersions":"yes"}"#] {
            fs::write(dir.path().join("terraform/settings.json"), broken).unwrap();
            assert_eq!(load(dir.path()), Settings::default(), "{broken}");
        }
    }

    #[test]
    fn settings_round_trip_in_a_private_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings { check_versions: false, ..Settings::default() };
        settings.tools.insert("/w/envs/dev".into(), Tool::Tofu);
        save(dir.path(), &settings).unwrap();
        assert_eq!(load(dir.path()), settings);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join("terraform/settings.json")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // A file with only one of the fields keeps the default of the other.
        fs::write(dir.path().join("terraform/settings.json"), r#"{"tools":{"/a":"terraform"}}"#).unwrap();
        assert!(load(dir.path()).check_versions);
    }
}
