//! The Terraform plugin's runner with the real tool: init, plan, and apply
//! of a resource that needs no cloud (`terraform_data`). It runs only with
//! `WESTERM_TEST_TERRAFORM=1` and `terraform` or `tofu` on the PATH; without
//! them the test passes without doing anything.
#![cfg(all(unix, feature = "plugin-terraform"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_westerm");

/// The first of `terraform` and `tofu` on the PATH.
fn tool() -> Option<&'static str> {
    ["terraform", "tofu"].into_iter().find(|tool| Command::new(tool).arg("version").output().is_ok_and(|output| output.status.success()))
}

/// A project, its tool, and where westerm keeps its plan.
struct Run<'a> {
    state: &'a Path,
    project: &'a Path,
    tool: &'a str,
    plan: &'a Path,
}

impl Run<'_> {
    /// Writes the spec of `token` as the server does, and runs the runner for it.
    /// Gives the runner's exit code and what it recorded.
    fn start(&self, token: &str, action: &str, args: &[&str]) -> (i32, serde_json::Value) {
        let runs = self.state.join("terraform/runs");
        fs::create_dir_all(&runs).unwrap();
        let spec = serde_json::json!({
            "project": "0123456789abcdef",
            "action": action,
            "tool": self.tool,
            "cwd": self.project,
            "args": args,
            "plan": self.plan,
            "json": if action == "plan" { serde_json::json!(self.plan.with_extension("json")) } else { serde_json::Value::Null },
        });
        fs::write(runs.join(format!("{token}.spec.json")), spec.to_string()).unwrap();
        let output = Command::new(BIN).args(["plugin-run", token]).arg(self.state).output().unwrap();
        let done = serde_json::from_slice(&fs::read(runs.join(format!("{token}.done.json"))).expect("a done file")).unwrap();
        (output.status.code().unwrap_or(-1), done)
    }
}

#[test]
fn a_real_plan_is_saved_read_and_applied() {
    if std::env::var_os("WESTERM_TEST_TERRAFORM").is_none() {
        return;
    }
    let Some(tool) = tool() else { return };
    let root = tempfile::tempdir().unwrap();
    let (state, project): (PathBuf, PathBuf) = (root.path().join("state"), root.path().join("my infra"));
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(state.join("terraform/plans")).unwrap();
    fs::write(project.join("main.tf"), "resource \"terraform_data\" \"a\" {\n  input = \"x\"\n}\n\noutput \"hidden\" {\n  value     = \"not-shown\"\n  sensitive = true\n}\n").unwrap();
    let init = Command::new(tool).arg("init").arg("-input=false").current_dir(&project).output().unwrap();
    assert!(init.status.success(), "{}", String::from_utf8_lossy(&init.stderr));

    let plan = state.join("terraform/plans/x.tfplan");
    let out = format!("-out={}", plan.display());
    let run = Run { state: &state, project: &project, tool, plan: &plan };
    let (code, done) = run.start(&"1".repeat(32), "plan", &["plan", &out]);
    assert_eq!(code, 0);
    assert_eq!((done["exit"].as_i64(), done["json_ok"].as_bool()), (Some(0), Some(true)));
    assert_eq!(done["workspace"], "default");
    assert!(done["version"].as_str().is_some_and(|version| version.split('.').count() >= 3), "{}", done["version"]);
    assert!(plan.is_file());

    // The JSON has the shape the server reduces.
    let json: serde_json::Value = serde_json::from_slice(&fs::read(plan.with_extension("json")).unwrap()).unwrap();
    assert!(json["format_version"].is_string());
    let change = &json["resource_changes"][0];
    assert_eq!((change["address"].as_str(), change["mode"].as_str(), change["type"].as_str(), change["name"].as_str()), (Some("terraform_data.a"), Some("managed"), Some("terraform_data"), Some("a")));
    assert_eq!(change["change"]["actions"], serde_json::json!(["create"]));
    assert_eq!(change["change"]["after"]["input"], "x");
    assert_eq!(change["change"]["after_unknown"]["id"], true);
    assert_eq!(json["output_changes"]["hidden"]["actions"], serde_json::json!(["create"]));

    let (code, done) = run.start(&"2".repeat(32), "apply", &["apply", &plan.display().to_string()]);
    assert_eq!(code, 0);
    assert_eq!(done["exit"], 0);
    assert!(project.join("terraform.tfstate").is_file(), "the apply wrote the state");

    // The same plan again: the tool refuses a plan that no longer matches the state.
    let (code, _) = run.start(&"3".repeat(32), "apply", &["apply", &plan.display().to_string()]);
    assert_ne!(code, 0, "a stale plan is refused by the tool itself");
}
