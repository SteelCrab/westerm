//! The Terraform plugin's runner: `westerm plugin-run <token> <state folder>`,
//! typed into a new shell by the host. It reads the run's spec once, runs the tool in the
//! same terminal, and records how the tool ended, so the server knows the run
//! is over and whether it worked. Around a plan it also records the workspace
//! and the version, and writes the plan as JSON: only the same program in the
//! same environment can read a plan file.

/// Runs the spec of `token` and gives the tool's exit code.
#[cfg(not(unix))]
pub fn run(_state_dir: &std::path::Path, _token: &str) -> i32 {
    eprintln!("Runs are not supported on this system.");
    1
}

#[cfg(unix)]
pub use unix::run;

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::io::{Read, Write};
    use std::os::unix::process::ExitStatusExt;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use crate::plugins::terraform::{is_token, root_dir, take_spec, write_done, write_started, Action, RunDone, RunSpec, RunStarted};
    use crate::state_dir::private_file;
    use crate::time::now_millis;

    /// How long `workspace show` and `version -json` may take.
    const ASK_TIMEOUT: Duration = Duration::from_secs(10);
    /// How long `show -json` may take.
    const JSON_TIMEOUT: Duration = Duration::from_secs(60);
    /// What the shell gives for a program that is not there.
    const NOT_FOUND: i32 = 127;

    /// Does nothing: with a handler, Ctrl+C does not end the runner, and the
    /// tool it starts gets the default again (an ignored signal would stay
    /// ignored in the tool).
    extern "C" fn on_sigint(_: libc::c_int) {}

    /// Runs the spec of `token` and gives the tool's exit code.
    pub fn run(state_dir: &Path, token: &str) -> i32 {
        // First of all: once the started file is there, Ctrl+C no longer ends the runner.
        // SAFETY: the handler does nothing, which is safe in a signal handler.
        unsafe { libc::signal(libc::SIGINT, on_sigint as extern "C" fn(libc::c_int) as libc::sighandler_t) };
        let Some(spec) = Some(token).filter(|token| is_token(token)).and_then(|token| take_spec(state_dir, token)) else {
            eprintln!("This run expired. Start it again from the Terraform panel.");
            return 1;
        };
        let _ = write_started(state_dir, token, &RunStarted { pid: std::process::id(), started_at: now_millis() });

        let program = spec.tool.program();
        let planning = spec.action == Action::Plan;
        let workspace = planning.then(|| ask(&spec, &["workspace", "show"])).flatten().map(|text| text.trim().to_owned()).filter(|text| !text.is_empty());
        let version = planning.then(|| ask(&spec, &["version", "-json"])).flatten().and_then(|text| version_of(&text));

        println!("\x1b[2m$ {program} {}\x1b[0m", shown_args(&spec.args, state_dir));
        let _ = std::io::stdout().flush();
        let exit = match Command::new(program).args(&spec.args).current_dir(&spec.cwd).status() {
            Ok(status) => status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
            Err(error) => {
                eprintln!("westerm: {program}: {error}");
                NOT_FOUND
            }
        };
        let json_ok = planning && exit == 0 && write_json(&spec);
        let _ = write_done(state_dir, token, &RunDone { exit, ended_at: now_millis(), workspace, version, json_ok });
        exit
    }

    /// The arguments as the pane shows them: a path in westerm's own folder is
    /// cut to its file name.
    fn shown_args(args: &[String], state_dir: &Path) -> String {
        let root = root_dir(state_dir).to_string_lossy().into_owned();
        let shown = |arg: &String| match (arg.find(&root), arg.rfind('/')) {
            (Some(start), Some(slash)) if slash >= start => format!("{}…{}", &arg[..start], &arg[slash..]),
            _ => arg.clone(),
        };
        args.iter().map(shown).collect::<Vec<_>>().join(" ")
    }

    /// The `terraform_version` of `version -json`.
    fn version_of(text: &str) -> Option<String> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        Some(value.get("terraform_version")?.as_str()?.to_owned())
    }

    /// What the tool prints for `args`, unseen; None when it fails or is slow.
    fn ask(spec: &RunSpec, args: &[&str]) -> Option<String> {
        let mut child = Command::new(spec.tool.program())
            .args(args)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut stdout = child.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).ok().map(|_| text)
        });
        let ok = ended_ok(&mut child, ASK_TIMEOUT);
        // A killed tool closes its output, so the reader ends.
        let text = reader.join().ok().flatten();
        text.filter(|_| ok)
    }

    /// Writes `show -json` of the plan to the spec's JSON file, unseen.
    fn write_json(spec: &RunSpec) -> bool {
        let (Some(plan), Some(json)) = (&spec.plan, &spec.json) else { return false };
        let Ok(file) = private_file().write(true).create_new(true).open(json) else { return false };
        let child = Command::new(spec.tool.program())
            .args(["show", "-json"])
            .arg(plan)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::from(file))
            .stderr(Stdio::null())
            .spawn();
        let ok = child.is_ok_and(|mut child| ended_ok(&mut child, JSON_TIMEOUT));
        if !ok {
            let _ = fs::remove_file(json);
        }
        ok
    }

    /// Waits for `child` up to `limit`, then kills it. True when it ended with 0.
    fn ended_ok(child: &mut Child, limit: Duration) -> bool {
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status.success(),
                Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn paths_in_the_state_folder_are_cut_to_their_file_name() {
            let state = Path::new("/home/a/.local/state/westerm");
            let args = ["plan".to_owned(), "-out=/home/a/.local/state/westerm/terraform/plans/0123/abcd.tfplan".to_owned(), "-target=module.eks".to_owned()];
            assert_eq!(shown_args(&args, state), "plan -out=…/abcd.tfplan -target=module.eks");
            let apply = ["apply".to_owned(), "/home/a/.local/state/westerm/terraform/plans/0123/abcd.tfplan".to_owned()];
            assert_eq!(shown_args(&apply, state), "apply …/abcd.tfplan");
            let other = ["plan".to_owned(), "-out=/tmp/x.tfplan".to_owned()];
            assert_eq!(shown_args(&other, state), "plan -out=/tmp/x.tfplan");
        }

        #[test]
        fn the_version_is_read_from_the_json() {
            assert_eq!(version_of(r#"{"terraform_version":"1.9.8","platform":"darwin_arm64"}"#).as_deref(), Some("1.9.8"));
            assert_eq!(version_of("Terraform v1.9.8"), None);
            assert_eq!(version_of(r#"{"terraform_version":3}"#), None);
        }
    }
}
