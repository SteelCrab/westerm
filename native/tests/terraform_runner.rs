//! The Terraform plugin's runner (`westerm plugin-run <token>`), end to end
//! with a fake `terraform` on the PATH. macOS and Linux only, like plugin
//! shells.
#![cfg(all(unix, feature = "plugin-terraform"))]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_westerm");

/// What the fake does: the words the runner asks before and after a plan,
/// then one behavior for each action.
const FAKE: &str = r#"#!/bin/sh
case "$1 $2" in
  "workspace show") echo dev; exit 0 ;;
  "version -json") echo '{"terraform_version":"1.9.8"}'; exit 0 ;;
  "show -json") echo '{"format_version":"1.2"}'; exit 0 ;;
esac
if [ -n "$FAKE_PWD" ]; then pwd > "$FAKE_PWD"; fi
case "$1" in
  validate) exit 3 ;;
  plan) echo "Plan: 1 to add"; exit "${FAKE_PLAN_EXIT:-0}" ;;
  init) kill -INT $$; sleep 1; exit 0 ;;
  apply) trap '' INT; sleep 1; echo ended > "$FAKE_MARKER"; exit 0 ;;
esac
exit 9
"#;

struct Env {
    root: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let env = Self { root };
        fs::create_dir_all(env.bin()).unwrap();
        fs::create_dir_all(env.state().join("terraform/runs")).unwrap();
        fs::create_dir_all(env.project()).unwrap();
        let fake = env.bin().join("terraform");
        fs::write(&fake, FAKE).unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
        env
    }

    fn bin(&self) -> PathBuf {
        self.root.path().join("bin")
    }

    fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }

    /// A folder with spaces and Korean in its name.
    fn project(&self) -> PathBuf {
        self.root.path().join("my infra").join("dev 환경")
    }

    fn run_file(&self, token: &str, kind: &str) -> PathBuf {
        self.state().join("terraform/runs").join(format!("{token}.{kind}.json"))
    }

    fn plan_file(&self) -> PathBuf {
        self.state().join("terraform/plans/x.tfplan")
    }

    fn json_file(&self) -> PathBuf {
        self.state().join("terraform/plans/x.json")
    }

    /// Writes the spec of `token` as the server does.
    fn spec(&self, token: &str, action: &str, args: &[&str], with_plan: bool) {
        fs::create_dir_all(self.plan_file().parent().unwrap()).unwrap();
        let path = |file: PathBuf| if with_plan { serde_json::json!(file) } else { serde_json::Value::Null };
        let spec = serde_json::json!({
            "project": "0123456789abcdef",
            "action": action,
            "tool": "terraform",
            "cwd": self.project(),
            "args": args,
            "plan": path(self.plan_file()),
            "json": if action == "plan" { path(self.json_file()) } else { serde_json::Value::Null },
        });
        fs::write(self.run_file(token, "spec"), spec.to_string()).unwrap();
    }

    fn command(&self, token: &str) -> Command {
        let mut command = Command::new(BIN);
        // The state folder is an argument. The variable names another folder,
        // as in a pane's shell, which does not have westerm's own variables.
        command
            .args(["plugin-run", token])
            .arg(self.state())
            .env("WESTERM_STATE_DIR", self.root.path().join("another state"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin().display()))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn run(&self, token: &str) -> Output {
        self.command(token).output().unwrap()
    }

    fn done(&self, token: &str) -> serde_json::Value {
        serde_json::from_slice(&fs::read(self.run_file(token, "done")).expect("a done file")).unwrap()
    }
}

fn token(digit: char) -> String {
    digit.to_string().repeat(32)
}

fn wait(child: &mut Child, limit: Duration) -> std::process::ExitStatus {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(start.elapsed() < limit, "the runner did not end in {limit:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_exit_code_passes_through() {
    let env = Env::new();
    let token = token('1');
    env.spec(&token, "validate", &["validate"], false);
    let output = env.run(&token);
    assert_eq!(output.status.code(), Some(3));
    let done = env.done(&token);
    assert_eq!(done["exit"], 3);
    assert_eq!(done["json_ok"], false);
    assert_eq!(done["workspace"], serde_json::Value::Null, "only a plan asks for the workspace");
    assert!(done["ended_at"].as_u64().unwrap() > 0);
    let started: serde_json::Value = serde_json::from_slice(&fs::read(env.run_file(&token, "started")).unwrap()).unwrap();
    assert!(started["pid"].as_u64().unwrap() > 0);
    assert!(!env.run_file(&token, "spec").exists(), "the spec is read once");
    assert!(String::from_utf8_lossy(&output.stdout).contains("$ terraform validate"));
}

#[test]
fn a_plan_writes_the_json_and_what_it_saw() {
    let env = Env::new();
    let token = token('2');
    let out = format!("-out={}", env.plan_file().display());
    env.spec(&token, "plan", &["plan", &out], true);
    let output = env.run(&token);
    assert_eq!(output.status.code(), Some(0));
    let done = env.done(&token);
    assert_eq!((done["exit"].as_i64(), done["json_ok"].as_bool()), (Some(0), Some(true)));
    assert_eq!(done["workspace"], "dev");
    assert_eq!(done["version"], "1.9.8");
    assert_eq!(fs::read_to_string(env.json_file()).unwrap().trim(), r#"{"format_version":"1.2"}"#);
    assert_eq!(fs::metadata(env.json_file()).unwrap().permissions().mode() & 0o777, 0o600);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("$ terraform plan -out=…/x.tfplan"), "{stdout}");
    assert!(stdout.contains("Plan: 1 to add"), "the tool's own output shows: {stdout}");
    assert!(!stdout.contains("format_version"), "the JSON is not shown: {stdout}");
    assert!(!stdout.contains("1.9.8"), "the version is not shown: {stdout}");
}

#[test]
fn a_failed_plan_writes_no_json() {
    let env = Env::new();
    let token = token('3');
    env.spec(&token, "plan", &["plan", "-out=x"], true);
    let output = env.command(&token).env("FAKE_PLAN_EXIT", "1").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let done = env.done(&token);
    assert_eq!((done["exit"].as_i64(), done["json_ok"].as_bool()), (Some(1), Some(false)));
    assert!(!env.json_file().exists());
}

#[test]
fn an_expired_token_runs_nothing() {
    let env = Env::new();
    let token = token('4');
    let output = env.run(&token);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("This run expired"));
    assert!(!env.run_file(&token, "started").exists());
    assert!(!env.run_file(&token, "done").exists());
    // A word that is not a token reads no file at all.
    let odd = env.run("../../x");
    assert_eq!(odd.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&odd.stderr).contains("This run expired"));
}

#[test]
fn the_state_folder_must_be_given() {
    let env = Env::new();
    let token = token('9');
    env.spec(&token, "validate", &["validate"], false);
    let output = Command::new(BIN).args(["plugin-run", &token]).env("WESTERM_STATE_DIR", env.state()).stdin(Stdio::null()).output().unwrap();
    assert_ne!(output.status.code(), Some(3), "the tool did not run");
    assert!(env.run_file(&token, "spec").exists(), "the spec was not read");
}

#[test]
fn a_run_works_in_a_folder_with_spaces() {
    let env = Env::new();
    let token = token('5');
    env.spec(&token, "plan", &["plan"], false);
    let seen = env.root.path().join("pwd.txt");
    let output = env.command(&token).env("FAKE_PWD", &seen).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let pwd = fs::read_to_string(&seen).unwrap();
    assert_eq!(fs::canonicalize(pwd.trim()).unwrap(), fs::canonicalize(env.project()).unwrap());
}

#[test]
fn a_tool_ended_by_ctrl_c_reports_128_plus_the_signal() {
    // The fake sends itself SIGINT. It ends only when the runner did not hand
    // it an ignored SIGINT.
    let env = Env::new();
    let token = token('6');
    env.spec(&token, "init", &["init"], false);
    let output = env.run(&token);
    assert_eq!(output.status.code(), Some(130));
    assert_eq!(env.done(&token)["exit"], 130);
}

#[test]
fn ctrl_c_does_not_end_the_runner_before_the_tool() {
    let env = Env::new();
    let token = token('7');
    env.spec(&token, "apply", &["apply", "x.tfplan"], false);
    let marker = env.root.path().join("ended.txt");
    let mut child = env.command(&token).env("FAKE_MARKER", &marker).spawn().unwrap();
    // The started file says the runner is past its first step, where it takes over Ctrl+C.
    let begun = Instant::now();
    while !env.run_file(&token, "started").exists() {
        assert!(begun.elapsed() < Duration::from_secs(10), "the runner did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    // SAFETY: the pid is our own child, still running.
    assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) }, 0);
    std::thread::sleep(Duration::from_millis(300));
    assert!(child.try_wait().unwrap().is_none(), "the runner waits for the tool");
    assert!(!marker.exists());
    let status = wait(&mut child, Duration::from_secs(10));
    assert_eq!(status.code(), Some(0));
    assert!(marker.exists(), "the tool ended before the runner did");
    assert_eq!(env.done(&token)["exit"], 0);
}

#[test]
fn a_missing_tool_is_reported_and_recorded() {
    let env = Env::new();
    let token = token('8');
    env.spec(&token, "validate", &["validate"], false);
    fs::remove_file(env.bin().join("terraform")).unwrap();
    let output = env.run(&token);
    assert_eq!(output.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&output.stderr).contains("terraform"));
    assert_eq!(env.done(&token)["exit"], 127);
}

