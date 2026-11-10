//! `run`: a program the user granted, started with an argument array (no
//! shell), within a time limit, with each output stream cut at a size.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::server::plugins::login_env::LoginEnv;

/// What a finished program left behind. `code` is `None` when a signal ended it.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct RunOutput {
    pub(super) code: Option<i32>,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum RunError {
    /// The program is not on the PATH.
    NotFound,
    /// The program ran past the time limit and was killed.
    TimedOut,
    /// It could not start, or waiting for it failed.
    Other(String),
}

/// After the program exits, output still in its pipes gets this long to
/// arrive. A background job it left may hold the pipes open; the answer does
/// not wait for that job.
const EXIT_GRACE: Duration = Duration::from_millis(200);

/// Login-shell variables that describe the probe shell itself, not the
/// user's settings: `run` does not pass them on.
const SHELL_OWN: [&str; 4] = ["PWD", "OLDPWD", "SHLVL", "_"];

/// Runs `program args…` in `cwd`. The environment is the server's own with
/// every login-shell variable on top (the login value wins), except the
/// probe shell's own place (`SHELL_OWN`). stdout and stderr each keep their
/// first `cap` bytes; the rest is read and dropped, so a chatty program does
/// not block on a full pipe.
///
/// The answer comes when the program exits (plus `EXIT_GRACE` for its last
/// output), not when its pipes close. On Unix the program leads a process
/// group of its own, which is killed when the run ends: after the exit, on a
/// timeout, and when the request goes away. So a background job it started
/// does not outlive it. A program still running after `timeout` is killed.
pub(super) async fn run_capped(program: &str, args: &[String], cwd: &Path, env: &LoginEnv, timeout: Duration, cap: usize) -> Result<RunOutput, RunError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .envs(env.vars.iter().filter(|(key, _)| !SHELL_OWN.contains(&key.as_str())).map(|(key, value)| (key, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flash from a GUI app
    // SAFETY: setsid is async-signal-safe and touches no memory of the
    // parent. A new session also has no controlling terminal.
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => RunError::NotFound,
        _ => RunError::Other(error.to_string()),
    })?;
    // After setsid the child leads its own process group: the group id is its
    // pid. Declared after `child`, so it is dropped (and kills) first.
    #[cfg(unix)]
    let _group = child.id().and_then(|pid| libc::pid_t::try_from(pid).ok()).map(ProcessGroup);
    let (mut stdout, mut stderr) = (child.stdout.take(), child.stderr.take());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = {
        let wait = child.wait();
        let read_out = read_capped(stdout.as_mut(), &mut out, cap);
        let read_err = read_capped(stderr.as_mut(), &mut err, cap);
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(wait, read_out, read_err, deadline);
        let (mut out_closed, mut err_closed) = (false, false);
        loop {
            tokio::select! {
                status = &mut wait => break Some(status),
                () = &mut read_out, if !out_closed => out_closed = true,
                () = &mut read_err, if !err_closed => err_closed = true,
                () = &mut deadline => break None,
            }
        }
    };
    // On a timeout the group is killed on return.
    let status = status.ok_or(RunError::TimedOut)?.map_err(|error| RunError::Other(error.to_string()))?;
    let rest = async { tokio::join!(read_capped(stdout.as_mut(), &mut out, cap), read_capped(stderr.as_mut(), &mut err, cap)) };
    let _ = tokio::time::timeout(EXIT_GRACE, rest).await;
    Ok(RunOutput {
        code: status.code(),
        stdout: String::from_utf8_lossy(&out).into_owned(),
        stderr: String::from_utf8_lossy(&err).into_owned(),
    })
}

/// The process group of a running program; dropping it kills the group.
#[cfg(unix)]
struct ProcessGroup(libc::pid_t);

#[cfg(unix)]
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        // SAFETY: killpg only sends a signal to the run's own process group,
        // which setsid made with the program as its leader.
        unsafe {
            libc::killpg(self.0, libc::SIGKILL);
        }
    }
}

/// Appends `reader` to `kept` until it closes, keeping at most `cap` bytes
/// and dropping the rest. Each chunk lands in `kept` as soon as it is read,
/// so cancelling this future loses nothing.
async fn read_capped(reader: Option<&mut (impl AsyncRead + Unpin)>, kept: &mut Vec<u8>, cap: usize) {
    let Some(reader) = reader else { return };
    let mut chunk = [0_u8; 8192];
    while let Ok(read @ 1..) = reader.read(&mut chunk).await {
        let room = cap.saturating_sub(kept.len());
        kept.extend_from_slice(&chunk[..read.min(room)]);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const LIMIT: Duration = Duration::from_secs(5);
    const CAP: usize = 1_048_576;

    /// A folder on a temp PATH with an executable `name` that runs `body`.
    fn fake(dir: &Path, name: &str, body: &str) {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A login environment whose PATH is `dir` plus the system tools.
    fn login(dir: &Path, extra: &[(&str, &str)]) -> LoginEnv {
        let mut vars = vec![("PATH".to_owned(), format!("{}:/bin:/usr/bin", dir.display()))];
        vars.extend(extra.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())));
        LoginEnv { vars }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[tokio::test]
    async fn run_passes_arguments_as_an_array() {
        let dir = tempfile::tempdir().unwrap();
        fake(dir.path(), "kubectl", "for arg in \"$@\"; do printf '%s\\n' \"$arg\"; done");
        let args = strings(&["a b", "$(x)", ";", "'"]);
        let output = run_capped("kubectl", &args, dir.path(), &login(dir.path(), &[]), LIMIT, CAP).await.unwrap();
        assert_eq!(output, RunOutput { code: Some(0), stdout: "a b\n$(x)\n;\n'\n".into(), stderr: String::new() });
    }

    #[tokio::test]
    async fn run_gets_the_login_environment() {
        let dir = tempfile::tempdir().unwrap();
        fake(dir.path(), "kubectl", "printf '%s' \"$KUBECONFIG\"");
        let output = run_capped("kubectl", &[], dir.path(), &login(dir.path(), &[("KUBECONFIG", "/k")]), LIMIT, CAP).await.unwrap();
        assert_eq!(output.stdout, "/k");
        // HOME is the server's; the login value beats it.
        fake(dir.path(), "home", "printf '%s' \"$HOME\"");
        let output = run_capped("home", &[], dir.path(), &login(dir.path(), &[]), LIMIT, CAP).await.unwrap();
        assert_eq!(Some(output.stdout), std::env::var("HOME").ok(), "the server's environment is inherited");
        let output = run_capped("home", &[], dir.path(), &login(dir.path(), &[("HOME", "/login-home")]), LIMIT, CAP).await.unwrap();
        assert_eq!(output.stdout, "/login-home");
    }

    #[tokio::test]
    async fn run_reports_the_exit_code_stderr_and_cwd() {
        let dir = tempfile::tempdir().unwrap();
        fake(dir.path(), "helm", "pwd -P\necho oops >&2\nexit 3");
        let output = run_capped("helm", &[], dir.path(), &login(dir.path(), &[]), LIMIT, CAP).await.unwrap();
        assert_eq!(output.code, Some(3));
        assert_eq!(output.stdout.trim_end(), dir.path().canonicalize().unwrap().display().to_string());
        assert_eq!(output.stderr, "oops\n");
    }

    #[tokio::test]
    async fn a_missing_program_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_capped("kubectl", &[], dir.path(), &login(dir.path(), &[]), LIMIT, CAP).await;
        assert_eq!(result, Err(RunError::NotFound));
    }

    #[tokio::test]
    async fn a_hung_program_times_out() {
        let dir = tempfile::tempdir().unwrap();
        fake(dir.path(), "kubectl", "sleep 5");
        let started = std::time::Instant::now();
        let result = run_capped("kubectl", &[], dir.path(), &login(dir.path(), &[]), Duration::from_millis(200), CAP).await;
        assert_eq!(result, Err(RunError::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(1), "took {:?}", started.elapsed());
    }

    #[tokio::test]
    async fn output_is_cut_at_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        // 2 MiB of `y` on stdout, then 10 bytes on stderr: the program must
        // not block on a full pipe after the first MiB.
        fake(dir.path(), "kubectl", "head -c 2097152 /dev/zero | tr '\\0' 'y'\nprintf '0123456789' >&2");
        let output = run_capped("kubectl", &[], dir.path(), &login(dir.path(), &[]), LIMIT, CAP).await.unwrap();
        assert_eq!(output.stdout.len(), 1_048_576);
        assert!(output.stdout.bytes().all(|byte| byte == b'y'));
        assert_eq!(output.stderr, "0123456789");
        assert_eq!(output.code, Some(0));
    }

    /// Whether a process with this pid is still there. A killed process
    /// whose parent is gone is reaped by init soon after, so this polls.
    async fn gone(pid: libc::pid_t) -> bool {
        for _ in 0..50 {
            // SAFETY: signal 0 only checks that the process exists.
            if unsafe { libc::kill(pid, 0) } == -1 {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    fn read_pid(path: &Path) -> libc::pid_t {
        std::fs::read_to_string(path).unwrap().trim().parse().unwrap()
    }

    #[tokio::test]
    async fn a_background_job_holding_stdout_does_not_hold_the_answer() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("sleep.pid");
        fake(dir.path(), "kubectl", &format!("sleep 30 &\necho $! > '{}'\necho done\necho warn >&2", pid_file.display()));
        let started = std::time::Instant::now();
        let output = run_capped("kubectl", &[], dir.path(), &login(dir.path(), &[]), LIMIT, CAP).await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
        assert_eq!(output, RunOutput { code: Some(0), stdout: "done\n".into(), stderr: "warn\n".into() });
        assert!(gone(read_pid(&pid_file)).await, "the background sleep is killed with its group");
    }

    #[tokio::test]
    async fn a_timeout_kills_the_programs_children_too() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("sleep.pid");
        fake(dir.path(), "kubectl", &format!("sleep 30 &\necho $! > '{}'\nsleep 30", pid_file.display()));
        let started = std::time::Instant::now();
        let result = run_capped("kubectl", &[], dir.path(), &login(dir.path(), &[]), Duration::from_millis(300), CAP).await;
        assert_eq!(result, Err(RunError::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(1), "took {:?}", started.elapsed());
        assert!(gone(read_pid(&pid_file)).await, "the grandchild is killed");
    }

    #[tokio::test]
    async fn a_dropped_run_kills_its_group() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("sleep.pid");
        fake(dir.path(), "kubectl", &format!("sleep 30 &\necho $! > '{}'\nsleep 30", pid_file.display()));
        let env = login(dir.path(), &[]);
        let mut run = Box::pin(run_capped("kubectl", &[], dir.path(), &env, LIMIT, CAP));
        // Runs until the background sleep has written its pid.
        let pid = loop {
            tokio::select! {
                result = &mut run => panic!("the run ended: {result:?}"),
                () = tokio::time::sleep(Duration::from_millis(20)) => {
                    if let Some(pid) = std::fs::read_to_string(&pid_file).ok().and_then(|text| text.trim().parse().ok()) {
                        break pid;
                    }
                }
            }
        };
        // The page went away: the request's future is dropped mid-run.
        drop(run);
        assert!(gone(pid).await, "the grandchild is killed when the run is dropped");
    }

    #[tokio::test]
    async fn the_login_shells_own_place_is_not_passed_on() {
        let dir = tempfile::tempdir().unwrap();
        fake(dir.path(), "kubectl", "printf '%s|%s|%s|%s' \"$PWD\" \"$OLDPWD\" \"$SHLVL\" \"$_\"");
        let extra = [("PWD", "/login-pwd"), ("OLDPWD", "/login-old"), ("SHLVL", "7"), ("_", "/usr/bin/env"), ("KUBECONFIG", "/k")];
        let output = run_capped("kubectl", &[], dir.path(), &login(dir.path(), &extra), LIMIT, CAP).await.unwrap();
        for value in ["/login-pwd", "/login-old", "|7|", "/usr/bin/env"] {
            assert!(!output.stdout.contains(value), "{value} in {:?}", output.stdout);
        }
    }
}
