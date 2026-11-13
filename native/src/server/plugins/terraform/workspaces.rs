//! Workspaces: the list and the switch. Both run the tool, and the list can
//! reach the backend, so neither runs unless the user asks.

use std::path::Path;
use std::time::Duration;

use super::capture;
use crate::server::plugins::login_env::LoginEnv;

/// How long `workspace list` and `workspace select` may take.
pub const WORKSPACE_TIMEOUT: Duration = Duration::from_secs(15);

/// The names in what `workspace list` prints: one a line, the current one after `*`.
pub fn parse_list(output: &str) -> Vec<String> {
    output.lines().map(|line| line.trim().trim_start_matches('*').trim()).filter(|name| !name.is_empty()).map(str::to_owned).collect()
}

/// The project's workspaces, as its tool lists them. This can reach the backend.
/// Err: the tool's error line.
pub async fn list(program: &Path, cwd: &Path, env: &LoginEnv, timeout: Duration) -> Result<Vec<String>, String> {
    capture(program, &["workspace", "list"], cwd, env, timeout).await.map(|output| parse_list(&output))
}

/// Switches the project to the workspace `name`.
pub async fn select(program: &Path, cwd: &Path, env: &LoginEnv, name: &str, timeout: Duration) -> Result<(), String> {
    capture(program, &["workspace", "select", name], cwd, env, timeout).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{write_tool, Env};
    use super::*;
    use crate::server::plugins::login_env::LoginEnv;

    fn login(env: &Env) -> LoginEnv {
        LoginEnv { vars: vec![("PATH".to_owned(), format!("{}:/usr/bin:/bin", env.root.join("bin").display()))] }
    }

    #[test]
    fn the_list_is_parsed() {
        assert_eq!(parse_list("  default\n* dev\n  prod-eu\n\n"), ["default", "dev", "prod-eu"]);
        assert_eq!(parse_list("* default\n"), ["default"]);
        assert!(parse_list("").is_empty());
    }

    #[tokio::test]
    async fn a_list_runs_the_tool_in_the_project() {
        let env = Env::new(true, &[]).await;
        write_tool(&env.root, "terraform", "#!/bin/sh\npwd > \"$(dirname \"$0\")/pwd.txt\"\nprintf '  default\\n* dev\\n'\n");
        let cwd = env.root.join("w/envs/dev");
        let listed = list(&env.root.join("bin/terraform"), &cwd, &login(&env), WORKSPACE_TIMEOUT).await;
        assert_eq!(listed, Ok(vec!["default".to_owned(), "dev".to_owned()]));
        assert_eq!(std::fs::read_to_string(env.root.join("bin/pwd.txt")).unwrap().trim(), cwd.to_string_lossy());
        assert_eq!(WORKSPACE_TIMEOUT, Duration::from_secs(15));
    }

    #[tokio::test]
    async fn a_failing_list_gives_its_error_line() {
        let env = Env::new(true, &[]).await;
        write_tool(&env.root, "terraform", "#!/bin/sh\nprintf '\\n\\033[31m╷\\033[0m\\n\\033[31m│\\033[0m \\033[1mError: No valid credential sources found\\033[0m\\n│ more\\n' >&2\nexit 1\n");
        let listed = list(&env.root.join("bin/terraform"), &env.root.join("w/envs/dev"), &login(&env), WORKSPACE_TIMEOUT).await;
        assert_eq!(listed, Err("Error: No valid credential sources found".to_owned()));
    }

    #[tokio::test]
    async fn a_slow_list_times_out() {
        let env = Env::new(true, &[]).await;
        write_tool(&env.root, "terraform", "#!/bin/sh\nsleep 5\n");
        let started = std::time::Instant::now();
        let listed = list(&env.root.join("bin/terraform"), &env.root.join("w/envs/dev"), &login(&env), Duration::from_millis(100)).await;
        assert_eq!(listed, Err("Timed out".to_owned()));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn select_passes_the_name_as_one_word() {
        let env = Env::new(true, &["terraform"]).await;
        let done = select(&env.root.join("bin/terraform"), &env.root.join("w/envs/dev"), &login(&env), "prod eu", WORKSPACE_TIMEOUT).await;
        assert_eq!(done, Ok(()));
        assert_eq!(env.calls(), ["workspace select prod eu"]);
    }
}
