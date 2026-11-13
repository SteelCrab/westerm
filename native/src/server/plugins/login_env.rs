//! The user's login-shell environment, read once and shared by every plugin.
//! A desktop app or a service manager starts westerm with a short PATH and
//! without the tool settings (`KUBECONFIG`, `DOCKER_HOST`, …) that the user's
//! shell sets. The whole environment is kept; each user picks its keys.

#[cfg(any(feature = "plugin-db", feature = "plugin-terraform"))]
use std::fs;
use std::future::Future;
#[cfg(any(feature = "plugin-db", feature = "plugin-terraform"))]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Stdio;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use tokio::process::Command;
use tokio::sync::Mutex;

/// The login shell gets this long to print its environment.
#[cfg(unix)]
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// After the login shell exits, its last output gets this long to arrive.
#[cfg(unix)]
const PROBE_GRACE: Duration = Duration::from_millis(200);

/// The login shell's variables, in the order it printed them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct LoginEnv {
    pub(super) vars: Vec<(String, String)>,
}

/// A variable name as a shell writes it: `[A-Za-z_][A-Za-z0-9_]*`.
#[cfg(any(unix, test))]
fn is_name(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The variables in `env` output. Lines that do not start with a name and
/// `=` (a greeting, the rest of a multi-line value) are skipped; for a name
/// that appears twice the last value wins, in the place of the first.
#[cfg(any(unix, test))]
pub(super) fn parse_login_env(output: &str) -> Vec<(String, String)> {
    let mut vars: Vec<(String, String)> = Vec::new();
    for line in output.lines() {
        let Some((key, value)) = line.trim_end_matches('\r').split_once('=') else { continue };
        if !is_name(key) {
            continue;
        }
        match vars.iter_mut().find(|(known, _)| known == key) {
            Some(entry) => entry.1 = value.to_owned(),
            None => vars.push((key.to_owned(), value.to_owned())),
        }
    }
    vars
}

/// The server's own environment; values that are not UTF-8 are left out.
fn server_env() -> LoginEnv {
    LoginEnv { vars: std::env::vars_os().filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?))).collect() }
}

/// Runs the user's login shell once to print its environment. A failure or a
/// timeout gives the server's own environment. Output without `PATH` keeps
/// the server's own PATH, so an odd shell does not hide every program.
#[cfg(unix)]
async fn probe_login_env(shell: &str, timeout: Duration) -> LoginEnv {
    let Some(stdout) = run_probe(shell, timeout).await else { return server_env() };
    let mut vars = parse_login_env(&stdout);
    if !vars.iter().any(|(key, _)| key == "PATH")
        && let Ok(path) = std::env::var("PATH")
    {
        vars.push(("PATH".to_owned(), path));
    }
    LoginEnv { vars }
}

/// Runs `shell -i -l -c env` in a session of its own and returns what it
/// printed, or `None` when it did not start, failed, or ran past `timeout`.
///
/// A new session has no controlling terminal, so an interactive shell cannot
/// take over the terminal that runs westerm. The probe ends when the shell
/// exits, not when stdout closes: a background job from an rc file may hold
/// the pipe open. Its whole process group is killed at the end.
#[cfg(unix)]
async fn run_probe(shell: &str, timeout: Duration) -> Option<String> {
    let mut command = Command::new(shell);
    command.args(["-i", "-l", "-c", "env"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    // SAFETY: setsid is async-signal-safe and touches no memory of the parent.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = command.spawn().ok()?;
    // After setsid the child leads its own process group: the group id is its pid.
    let group = libc::pid_t::try_from(child.id()?).ok()?;
    let mut stdout = child.stdout.take()?;
    let mut buffer = Vec::new();
    let status = {
        let wait = child.wait();
        let read = read_into(&mut stdout, &mut buffer);
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(wait, read, deadline);
        let mut closed = false;
        loop {
            tokio::select! {
                status = &mut wait => break status.ok(),
                () = &mut read, if !closed => closed = true,
                () = &mut deadline => break None,
            }
        }
    };
    if status.is_some_and(|status| status.success()) {
        let _ = tokio::time::timeout(PROBE_GRACE, read_into(&mut stdout, &mut buffer)).await;
    }
    // SAFETY: killpg only sends a signal to the probe's own process group.
    unsafe {
        libc::killpg(group, libc::SIGKILL);
    }
    if status.is_none() {
        let _ = child.wait().await;
    }
    status.filter(|status| status.success()).map(|_| String::from_utf8_lossy(&buffer).into_owned())
}

/// Appends `stdout` to `buffer` until it closes. Each chunk lands in
/// `buffer` as soon as it is read, so cancelling this future loses nothing.
#[cfg(unix)]
async fn read_into(stdout: &mut tokio::process::ChildStdout, buffer: &mut Vec<u8>) {
    use tokio::io::AsyncReadExt;
    let mut chunk = [0_u8; 8192];
    while let Ok(read @ 1..) = stdout.read(&mut chunk).await {
        buffer.extend_from_slice(&chunk[..read]);
    }
}

/// The login shell's environment (the server's own on Windows, where a
/// desktop app gets the user's environment).
pub(super) async fn load() -> LoginEnv {
    #[cfg(unix)]
    return probe_login_env(&crate::spawn::login_shell(), PROBE_TIMEOUT).await;
    #[cfg(not(unix))]
    server_env()
}

/// The login environment, read once and kept until `reset`. Every plugin
/// shares it, so Docker's Retry or a new install reads it again for all.
#[derive(Default)]
pub(super) struct LoginEnvCache {
    env: Mutex<Option<LoginEnv>>,
}

impl LoginEnvCache {
    /// The remembered environment; `load` runs only when there is none.
    pub(super) async fn get_or_load<F, Fut>(&self, load: F) -> LoginEnv
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = LoginEnv>,
    {
        let mut slot = self.env.lock().await;
        match &*slot {
            Some(env) => env.clone(),
            None => slot.insert(load().await).clone(),
        }
    }

    /// Forgets the environment; the next read asks the login shell again.
    pub(super) async fn reset(&self) {
        *self.env.lock().await = None;
    }
}

/// `name` on the PATH of `env`: an executable file (`.exe` on Windows).
#[cfg(any(feature = "plugin-db", feature = "plugin-terraform"))]
pub fn find_program(env: &LoginEnv, name: &str) -> Option<PathBuf> {
    let path = env.vars.iter().rev().find(|(key, _)| key == "PATH").map(|(_, value)| value)?;
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() };
    std::env::split_paths(path).map(|dir| dir.join(&file)).find(|candidate| is_executable(candidate))
}

#[cfg(all(unix, any(feature = "plugin-db", feature = "plugin-terraform")))]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(all(not(unix), any(feature = "plugin-db", feature = "plugin-terraform")))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_login_environment_keeps_every_variable_and_skips_noise() {
        let out = "Welcome!\nPATH=/a\nKUBECONFIG=/k\nMULTI=line one\nline two=x\n1BAD=y\nPATH=/opt/homebrew/bin:/a\r\n";
        assert_eq!(
            parse_login_env(out),
            vec![("PATH".into(), "/opt/homebrew/bin:/a".into()), ("KUBECONFIG".into(), "/k".into()), ("MULTI".into(), "line one".into())]
        );
    }

    #[test]
    fn names_follow_the_shell_rule() {
        for key in ["PATH", "_", "a_1", "Docker_HOST"] {
            assert!(is_name(key), "{key}");
        }
        for key in ["", "1A", "A-B", "A B", "Ä", "A.B"] {
            assert!(!is_name(key), "{key}");
        }
    }

    #[tokio::test]
    async fn the_cache_remembers_until_reset() {
        let cache = LoginEnvCache::default();
        let first = LoginEnv { vars: vec![("PATH".into(), "/one".into())] };
        let second = LoginEnv { vars: vec![("PATH".into(), "/two".into())] };
        let loads = std::sync::atomic::AtomicUsize::new(0);
        let load = |env: &LoginEnv| {
            loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let env = env.clone();
            async move { env }
        };
        assert_eq!(cache.get_or_load(|| load(&first)).await, first);
        assert_eq!(cache.get_or_load(|| load(&second)).await, first, "remembered");
        cache.reset().await;
        assert_eq!(cache.get_or_load(|| load(&second)).await, second, "read again after a reset");
        assert_eq!(loads.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[cfg(unix)]
    mod probe {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        use std::path::Path;

        const READ: Duration = Duration::from_secs(5);

        /// A folder with an executable `name` that runs `body`.
        fn script(dir: &Path, name: &str, body: &str) {
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        /// The PATH value in `env`, if any.
        fn path_of(env: &LoginEnv) -> Option<String> {
            env.vars.iter().find(|(key, _)| key == "PATH").map(|(_, value)| value.clone())
        }

        /// True while a process with this pid exists and is not a zombie
        /// waiting for its parent; polls for up to 3 s for it to go.
        async fn gone(pid: libc::pid_t) -> bool {
            for _ in 0..30 {
                // SAFETY: signal 0 only checks that the process exists.
                if unsafe { libc::kill(pid, 0) } != 0 {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            false
        }

        #[tokio::test]
        async fn the_login_shell_environment_is_read() {
            let dir = tempfile::tempdir().unwrap();
            script(dir.path(), "shell", "echo Welcome\necho \"args: $*\" >&2\necho PATH=/opt/fake/bin\necho SECRET=x\necho DOCKER_HOST=unix:///c.sock");
            let shell = dir.path().join("shell");
            let env = probe_login_env(shell.to_str().unwrap(), READ).await;
            assert_eq!(
                env.vars,
                vec![
                    ("PATH".to_owned(), "/opt/fake/bin".to_owned()),
                    ("SECRET".to_owned(), "x".to_owned()),
                    ("DOCKER_HOST".to_owned(), "unix:///c.sock".to_owned())
                ]
            );
        }

        #[tokio::test]
        async fn a_hung_login_shell_falls_back() {
            let dir = tempfile::tempdir().unwrap();
            script(dir.path(), "shell", "exec sleep 5");
            let shell = dir.path().join("shell");
            let env = probe_login_env(shell.to_str().unwrap(), Duration::from_millis(200)).await;
            assert_eq!(env, server_env());
            assert_eq!(path_of(&env), std::env::var("PATH").ok());
        }

        #[tokio::test]
        async fn a_background_job_holding_stdout_does_not_stall_the_probe() {
            let dir = tempfile::tempdir().unwrap();
            let bg = dir.path().join("bg");
            // `sleep 30 &` keeps the stdout pipe open after the shell exits.
            script(dir.path(), "shell", &format!("echo PATH=/opt/fake/bin\nsleep 30 &\necho $! > '{}'", bg.display()));
            let shell = dir.path().join("shell");
            let started = std::time::Instant::now();
            let env = probe_login_env(shell.to_str().unwrap(), READ).await;
            assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
            assert_eq!(env.vars, vec![("PATH".to_owned(), "/opt/fake/bin".to_owned())]);
            // The probe's process group is killed, so the job does not outlive it.
            let pid: libc::pid_t = std::fs::read_to_string(&bg).unwrap().trim().parse().unwrap();
            assert!(gone(pid).await, "the background job {pid} still runs");
        }

        #[tokio::test]
        async fn the_probe_runs_in_its_own_session_without_a_terminal() {
            let dir = tempfile::tempdir().unwrap();
            let out = dir.path().join("out");
            // A session leader's process group id is its own pid. `/dev/tty`
            // opens only for a process with a controlling terminal.
            let body = format!(
                "pgid=$(ps -o pgid= -p $$ | tr -d ' ')\nif (exec 3</dev/tty) 2>/dev/null; then tty=yes; else tty=no; fi\necho \"$$ $pgid $tty\" > '{}'\necho PATH=/opt/fake/bin",
                out.display()
            );
            script(dir.path(), "shell", &body);
            let shell = dir.path().join("shell");
            probe_login_env(shell.to_str().unwrap(), READ).await;
            let line = std::fs::read_to_string(&out).unwrap();
            let fields: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 3, "{line}");
            assert_eq!(fields[0], fields[1], "the probe leads its own process group: {line}");
            assert_eq!(fields[2], "no", "the probe has no controlling terminal: {line}");
        }

        #[tokio::test]
        async fn a_probe_without_path_keeps_the_server_path() {
            let dir = tempfile::tempdir().unwrap();
            script(dir.path(), "shell", "echo DOCKER_HOST=unix:///c.sock");
            let shell = dir.path().join("shell");
            let env = probe_login_env(shell.to_str().unwrap(), READ).await;
            assert_eq!(path_of(&env), std::env::var("PATH").ok());
            assert!(env.vars.contains(&("DOCKER_HOST".to_owned(), "unix:///c.sock".to_owned())));
        }

        #[tokio::test]
        async fn a_missing_login_shell_falls_back() {
            let env = probe_login_env("/nonexistent/shell", READ).await;
            assert_eq!(env, server_env());
        }
    }
}
