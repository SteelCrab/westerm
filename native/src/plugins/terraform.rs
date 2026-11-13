//! The Terraform plugin's run launches. The server writes what a run does
//! into a private spec file under a random token; the session query carries
//! only that token, and the host types the runner (`westerm plugin-run
//! <token> <state folder>`) into the new shell. The runner reads the spec once, runs the
//! tool in the pane, and records how it ended. So the browser never sends a
//! command.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::{is_lower_hex, LaunchError};
use crate::quote::{quoted_line, ShellKind};
use crate::state_dir::private_file;

/// The plugin's id.
pub const ID: &str = "terraform";
/// The only session action: run what a spec says.
pub const RUN_ACTION: &str = "run";
/// The westerm subcommand the new shell runs.
pub const RUNNER_COMMAND: &str = "plugin-run";
/// The plugin's folder in the state folder.
const ROOT_DIR: &str = "terraform";
/// The folder of run files, in the plugin's folder.
const RUNS_DIR: &str = "runs";

/// A run token as the server makes it: 32 lowercase hex digits.
pub fn is_token(text: &str) -> bool {
    is_lower_hex(text, 32)
}

/// A run to start. The token is always checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub token: String,
}

/// What a run does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Init,
    Validate,
    Plan,
    Apply,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Validate => "validate",
            Self::Plan => "plan",
            Self::Apply => "apply",
        }
    }
}

/// The program a project runs with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Terraform,
    Tofu,
}

impl Tool {
    /// The program's name on the PATH.
    pub fn program(self) -> &'static str {
        match self {
            Self::Terraform => "terraform",
            Self::Tofu => "tofu",
        }
    }
}

/// What the runner does for a token. The server writes it; the runner reads it once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSpec {
    /// The project's id.
    pub project: String,
    pub action: Action,
    pub tool: Tool,
    /// The project's folder; the tool runs there.
    pub cwd: PathBuf,
    /// The words after the program, e.g. `["plan", "-out=/…/x.tfplan"]`.
    pub args: Vec<String>,
    /// The plan file of a plan or an apply.
    pub plan: Option<PathBuf>,
    /// Where `show -json` goes after a plan.
    pub json: Option<PathBuf>,
}

/// The runner started: its process and when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStarted {
    pub pid: u32,
    /// Unix milliseconds.
    pub started_at: u64,
}

/// The tool ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunDone {
    /// The tool's exit code; 128 + the signal when a signal ended it.
    pub exit: i32,
    /// Unix milliseconds.
    pub ended_at: u64,
    /// What `workspace show` said before a plan.
    pub workspace: Option<String>,
    /// The tool's version, read before a plan.
    pub version: Option<String>,
    /// Whether the plan's JSON was written.
    pub json_ok: bool,
}

/// The plugin's folder: `<state>/terraform`.
pub fn root_dir(state_dir: &Path) -> PathBuf {
    state_dir.join(ROOT_DIR)
}

fn run_file(state_dir: &Path, token: &str, kind: &str) -> PathBuf {
    root_dir(state_dir).join(RUNS_DIR).join(format!("{token}.{kind}.json"))
}

/// Where the spec of `token` waits.
pub fn spec_path(state_dir: &Path, token: &str) -> PathBuf {
    run_file(state_dir, token, "spec")
}

/// Where the runner of `token` says it started.
pub fn started_path(state_dir: &Path, token: &str) -> PathBuf {
    run_file(state_dir, token, "started")
}

/// Where the runner of `token` says how the tool ended.
pub fn done_path(state_dir: &Path, token: &str) -> PathBuf {
    run_file(state_dir, token, "done")
}

/// Makes `dir` and its missing parents; `dir` itself is for the user only.
pub fn private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn runs_dir(state_dir: &Path) -> io::Result<()> {
    private_dir(&root_dir(state_dir))?;
    private_dir(&root_dir(state_dir).join(RUNS_DIR))
}

/// Writes what the run of `token` does, readable by the user only. A token is used once.
pub fn write_spec(state_dir: &Path, token: &str, spec: &RunSpec) -> io::Result<()> {
    runs_dir(state_dir)?;
    let body = serde_json::to_vec(spec).map_err(io::Error::other)?;
    let mut file = private_file().write(true).create_new(true).open(spec_path(state_dir, token))?;
    file.write_all(&body)
}

/// The spec of `token`, which is deleted once read; None when it is gone or broken.
/// Only the runner calls it, and the runner is Unix only (like plugin shells).
#[cfg_attr(not(unix), allow(dead_code))]
pub fn take_spec(state_dir: &Path, token: &str) -> Option<RunSpec> {
    let path = spec_path(state_dir, token);
    let bytes = fs::read(&path).ok()?;
    let _ = fs::remove_file(&path);
    serde_json::from_slice(&bytes).ok()
}

/// Writes a run file whole: a reader sees the old file or the new one.
#[cfg_attr(not(unix), allow(dead_code))]
fn write_whole<T: Serialize>(state_dir: &Path, path: &Path, value: &T) -> io::Result<()> {
    runs_dir(state_dir)?;
    let temp = path.with_extension("tmp");
    let body = serde_json::to_vec(value).map_err(io::Error::other)?;
    let mut file = private_file().write(true).create(true).truncate(true).open(&temp)?;
    file.write_all(&body)?;
    drop(file);
    fs::rename(&temp, path)
}

fn read_whole<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

#[cfg_attr(not(unix), allow(dead_code))]
pub fn write_started(state_dir: &Path, token: &str, started: &RunStarted) -> io::Result<()> {
    write_whole(state_dir, &started_path(state_dir, token), started)
}

pub fn read_started(state_dir: &Path, token: &str) -> Option<RunStarted> {
    read_whole(&started_path(state_dir, token))
}

#[cfg_attr(not(unix), allow(dead_code))]
pub fn write_done(state_dir: &Path, token: &str, done: &RunDone) -> io::Result<()> {
    write_whole(state_dir, &done_path(state_dir, token), done)
}

pub fn read_done(state_dir: &Path, token: &str) -> Option<RunDone> {
    read_whole(&done_path(state_dir, token))
}

/// The line a new shell types for `launch`: westerm's runner (`exe`) with the
/// token and the state folder. The folder is on the line because a pane's
/// shell does not get westerm's own variables (`WESTERM_STATE_DIR`), so the
/// runner could look for the spec somewhere else. It runs through `env`, so
/// no alias, function, or builtin of the user's shell replaces it.
pub fn command_in(launch: &Launch, exe: &Path, state_dir: &Path, shell: ShellKind) -> Result<String, LaunchError> {
    let words = [
        "env".to_owned(),
        "--".to_owned(),
        exe.to_string_lossy().into_owned(),
        RUNNER_COMMAND.to_owned(),
        launch.token.clone(),
        state_dir.to_string_lossy().into_owned(),
    ];
    quoted_line(&words, shell).map_err(|_| LaunchError::ControlCharacter)
}

/// `command_in` with this westerm and its state folder.
pub fn command(launch: &Launch, shell: ShellKind) -> Result<String, LaunchError> {
    let exe = std::env::current_exe().map_err(|_| LaunchError::Expired)?;
    let state_dir = crate::state_dir::state_dir().map_err(|_| LaunchError::Expired)?;
    command_in(launch, &exe, &state_dir, shell)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::PluginLaunch;

    fn spec() -> RunSpec {
        RunSpec {
            project: "0123456789abcdef".into(),
            action: Action::Plan,
            tool: Tool::Terraform,
            cwd: "/w/envs/dev".into(),
            args: vec!["plan".into(), "-out=/s/terraform/plans/x.tfplan".into()],
            plan: Some("/s/terraform/plans/x.tfplan".into()),
            json: Some("/s/terraform/plans/x.json".into()),
        }
    }

    #[test]
    fn tokens_are_32_lowercase_hex() {
        assert!(is_token(&"a".repeat(32)));
        for bad in ["a".repeat(31), "A".repeat(32), format!("{};", "a".repeat(31)), String::new()] {
            assert!(!is_token(&bad), "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_spec_is_private_and_read_once() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let token = "0".repeat(32);
        write_spec(dir.path(), &token, &spec()).unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&spec_path(dir.path(), &token)), 0o600);
        assert_eq!(mode(&root_dir(dir.path())), 0o700);
        assert_eq!(mode(&root_dir(dir.path()).join("runs")), 0o700);
        assert_eq!(take_spec(dir.path(), &token), Some(spec()));
        assert_eq!(take_spec(dir.path(), &token), None);
        assert!(!spec_path(dir.path(), &token).exists());
    }

    #[test]
    fn a_spec_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let token = "1".repeat(32);
        write_spec(dir.path(), &token, &spec()).unwrap();
        assert!(write_spec(dir.path(), &token, &spec()).is_err());
    }

    #[test]
    fn a_broken_spec_reads_as_none_and_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let token = "2".repeat(32);
        std::fs::create_dir_all(spec_path(dir.path(), &token).parent().unwrap()).unwrap();
        std::fs::write(spec_path(dir.path(), &token), "{").unwrap();
        assert_eq!(take_spec(dir.path(), &token), None);
        assert!(!spec_path(dir.path(), &token).exists());
    }

    #[test]
    fn started_and_done_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let token = "3".repeat(32);
        assert_eq!(read_started(dir.path(), &token), None);
        assert_eq!(read_done(dir.path(), &token), None);
        let started = RunStarted { pid: 42, started_at: 1_000 };
        write_started(dir.path(), &token, &started).unwrap();
        assert_eq!(read_started(dir.path(), &token), Some(started));
        let done = RunDone { exit: 3, ended_at: 2_000, workspace: Some("dev".into()), version: Some("1.9.8".into()), json_ok: true };
        write_done(dir.path(), &token, &done).unwrap();
        assert_eq!(read_done(dir.path(), &token), Some(done));
        assert_eq!(started_path(dir.path(), &token), root_dir(dir.path()).join("runs").join(format!("{token}.started.json")));
        assert_eq!(done_path(dir.path(), &token), root_dir(dir.path()).join("runs").join(format!("{token}.done.json")));
    }

    #[test]
    fn the_runner_line_quotes_a_path_with_spaces() {
        let launch = Launch { token: "a".repeat(32) };
        // The state folder goes on the line: the pane's shell does not have westerm's own variables.
        assert_eq!(
            command_in(&launch, Path::new("/Apps/my west/westerm"), Path::new("/tmp/state dir"), ShellKind::Sh).as_deref(),
            Ok("'env' '--' '/Apps/my west/westerm' 'plugin-run' 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' '/tmp/state dir'")
        );
    }

    #[test]
    fn terraform_launches_need_a_token() {
        let token = "a".repeat(32);
        let launch = PluginLaunch::parse("terraform", "run", &token).unwrap();
        assert_eq!((launch.plugin(), launch.action(), launch.target()), ("terraform", "run", token.clone()));
        assert!(PluginLaunch::parse("terraform", "plan", &token).is_none());
        assert!(PluginLaunch::parse("terraform", "run", "x;rm -rf ~").is_none());
        assert!(crate::plugins::PLUGIN_IDS.contains(&"terraform"));
    }

    #[test]
    fn actions_and_tools_have_their_words() {
        for (action, word) in [(Action::Init, "init"), (Action::Validate, "validate"), (Action::Plan, "plan"), (Action::Apply, "apply")] {
            assert_eq!(action.as_str(), word);
            assert_eq!(serde_json::from_str::<Action>(&format!("\"{word}\"")).ok(), Some(action));
            assert_eq!(serde_json::to_string(&action).unwrap(), format!("\"{word}\""));
        }
        assert!(serde_json::from_str::<Action>("\"destroy\"").is_err());
        assert_eq!((Tool::Terraform.program(), Tool::Tofu.program()), ("terraform", "tofu"));
        assert_eq!(serde_json::to_string(&Tool::Tofu).unwrap(), "\"tofu\"");
    }
}
