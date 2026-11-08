//! Starting PTY processes: shells, ssh, terminal-browser, terminal-code.
//! macOS and Linux start everything through a login shell. Windows starts
//! PowerShell and ssh directly (ConPTY), and terminal-browser and
//! terminal-code through a login shell in WSL: they have Linux builds only.

use std::path::PathBuf;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty};

use crate::protocol::{PtySize, SessionKind};

/// Variables that make programs think they run inside another terminal.
/// terminal-browser reads them to pick a pane strategy and frame transport.
const FOREIGN_TERMINAL_VARS: &[&str] = &[
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERM_SESSION_ID",
    "TMUX",
    "TMUX_PANE",
    "STY",
    "ZELLIJ",
    "ZELLIJ_SESSION_NAME",
    "KITTY_PID",
    "KITTY_WINDOW_ID",
    "KITTY_LISTEN_ON",
    "KITTY_PUBLIC_KEY",
    "ITERM_SESSION_ID",
    "ITERM_PROFILE",
    "LC_TERMINAL",
    "LC_TERMINAL_VERSION",
    "WEZTERM_PANE",
    "WEZTERM_EXECUTABLE",
    "WEZTERM_UNIX_SOCKET",
    "ALACRITTY_SOCKET",
    "ALACRITTY_WINDOW_ID",
    "VSCODE_INJECTION",
    "PIXEL_TTY",
    "PIXEL_EMBED",
    "TERMINAL_BROWSER_INTEROP_TARGET",
    "WESTERM_HOST_BUILD",
    "WESTERM_STATE_DIR",
    "WESTERM_RESTARTING",
];
const FOREIGN_TERMINAL_PREFIXES: &[&str] = &["GHOSTTY_", "KITTY_", "ITERM_", "WEZTERM_", "VSCODE_"];

/// terminal-browser and terminal-code draw with the pixel library. In a
/// terminal it does not know, such as westerm, pixel sends the whole picture
/// for every change, even a blinking caret. westerm draws pixel's patches
/// correctly, so it asks for them: only the changed parts are sent. A value
/// set before westerm starts or in a shell startup file, or terminal-browser's
/// own `render.presenter` setting, wins.
const PIXEL_PRESENTER: (&str, &str) = ("TERMINAL_BROWSER_PRESENT", "patched");

pub fn terminal_env(base: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = base
        .into_iter()
        .filter(|(key, _)| {
            !FOREIGN_TERMINAL_VARS.contains(&key.as_str())
                && !FOREIGN_TERMINAL_PREFIXES.iter().any(|prefix| key.starts_with(prefix))
                && !["TERM", "COLORTERM"].contains(&key.as_str())
        })
        .collect();
    env.push(("TERM".into(), "xterm-256color".into()));
    env.push(("COLORTERM".into(), "truecolor".into()));
    env.push(("TERM_PROGRAM".into(), "westerm".into()));
    let (presenter, patched) = PIXEL_PRESENTER;
    if !env.iter().any(|(key, _)| key == presenter) {
        env.push((presenter.into(), patched.into()));
    }
    env
}

/// What to run in a new PTY.
#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub kind: SessionKind,
    pub url: Option<String>,
    pub target: Option<String>,
    pub cwd: PathBuf,
}

#[cfg(unix)]
fn login_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
}

/// Every kind starts through a login shell so PATH matches the user's
/// terminal.
#[cfg(unix)]
pub fn command_for(kind: SessionKind, url: Option<&str>, target: Option<&str>) -> Result<(String, Vec<String>), String> {
    Ok((login_shell(), login_shell_args(kind, url, target)?))
}

/// The arguments of a login shell that runs `kind`. URLs and ssh
/// destinations are positional parameters, never part of the script, so they
/// cannot inject shell syntax.
pub fn login_shell_args(kind: SessionKind, url: Option<&str>, target: Option<&str>) -> Result<Vec<String>, String> {
    Ok(match kind {
        SessionKind::Shell => vec!["-l".into()],
        SessionKind::Ssh => {
            let target = target.ok_or("ssh needs a destination")?;
            vec!["-l".into(), "-c".into(), r#"exec ssh -- "$1""#.into(), "ssh".into(), target.into()]
        }
        SessionKind::Browser => {
            // With a target, pages load through that ssh host, so
            // "localhost:3000" is the remote machine's dev server.
            let mut script = vec!["exec terminal-browser open --no-merge".to_owned()];
            let mut params = Vec::new();
            if let Some(target) = target {
                script.push(r#"--ssh "$1""#.into());
                params.push(target.to_owned());
            }
            if let Some(url) = url {
                script.push(format!(r#""${}""#, params.len() + 1));
                params.push(url.to_owned());
            }
            let mut args = vec!["-l".into(), "-c".into(), script.join(" "), "terminal-browser".into()];
            args.extend(params);
            args
        }
        // `tode <folder>` without --split opens VS Code in this pane.
        SessionKind::Editor => vec!["-l".into(), "-c".into(), "exec tode .".into(), "tode".into()],
    })
}

/// Run by /bin/sh in WSL: starts the user's login shell with the arguments
/// that follow, so PATH matches a WSL terminal (installers put tode and
/// terminal-browser in ~/.local/bin).
#[cfg(any(windows, test))]
const WSL_LOGIN_SHELL: &str = r#"shell=$(getent passwd "$(id -un)" | cut -d: -f7); exec "${shell:-/bin/sh}" "$@""#;

/// `wsl.exe` arguments that run a login shell with `login_args`. `--exec`
/// passes every argument as it is; without it WSL joins them into a command
/// line for the default shell, which would read URLs as syntax.
#[cfg(any(windows, test))]
pub fn wsl_args(login_args: Vec<String>) -> Vec<String> {
    let mut args: Vec<String> = ["--exec", "/bin/sh", "-c", WSL_LOGIN_SHELL, "sh"].map(String::from).into();
    args.extend(login_args);
    args
}

/// Variables from terminal_env() that programs in WSL read. WSL passes a
/// Windows variable to Linux only when WSLENV names it.
#[cfg(any(windows, test))]
const WSL_SHARED: &[&str] = &["TERM", "COLORTERM", "TERM_PROGRAM", "TERMINAL_BROWSER_PRESENT"];

/// `env` with WSL_SHARED added to WSLENV. The user's own entries stay.
#[cfg(any(windows, test))]
pub fn wsl_env(mut env: Vec<(String, String)>) -> Vec<(String, String)> {
    let index = env.iter().position(|(key, _)| key.eq_ignore_ascii_case("WSLENV"));
    let current = index.map(|i| env[i].1.clone()).unwrap_or_default();
    let mut entries: Vec<String> = current.split(':').filter(|entry| !entry.is_empty()).map(String::from).collect();
    for name in WSL_SHARED {
        // An entry is NAME or NAME/flags.
        if !entries.iter().any(|entry| entry.split('/').next() == Some(*name)) {
            entries.push((*name).into());
        }
    }
    let value = entries.join(":");
    match index {
        Some(i) => env[i].1 = value,
        None => env.push(("WSLENV".into(), value)),
    }
    env
}

#[cfg(windows)]
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// PowerShell 7 when installed, else Windows PowerShell, which every
/// Windows 10 and later has.
#[cfg(windows)]
fn windows_shell() -> String {
    if on_path("pwsh.exe") { "pwsh.exe" } else { "powershell.exe" }.into()
}

/// terminal-browser and terminal-code have Linux builds only.
#[cfg(windows)]
fn runs_in_wsl(kind: SessionKind) -> bool {
    matches!(kind, SessionKind::Browser | SessionKind::Editor)
}

#[cfg(windows)]
const WSL_MISSING: &str =
    "terminal-browser and terminal-code run in WSL on Windows. Install WSL in PowerShell with `wsl --install`, then restart this pane.";

/// PowerShell and ssh start directly with an argument list. wsl.exe is a
/// real executable, not a cmd.exe shim, so URLs need no filtering.
#[cfg(windows)]
pub fn command_for(kind: SessionKind, url: Option<&str>, target: Option<&str>) -> Result<(String, Vec<String>), String> {
    Ok(match kind {
        SessionKind::Shell => (windows_shell(), vec!["-NoLogo".into()]),
        SessionKind::Ssh => {
            let target = target.ok_or("ssh needs a destination")?;
            ("ssh".into(), vec!["--".into(), target.into()])
        }
        SessionKind::Browser | SessionKind::Editor => ("wsl.exe".into(), wsl_args(login_shell_args(kind, url, target)?)),
    })
}

pub fn pty_size(size: PtySize) -> portable_pty::PtySize {
    // portable-pty writes these into ws_xpixel/ws_ypixel: the whole content
    // area in pixels, which is what Kitty clients expect.
    portable_pty::PtySize { rows: size.rows, cols: size.cols, pixel_width: size.width, pixel_height: size.height }
}

pub struct Pty {
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    pub pid: u32,
}

pub fn spawn(spec: &LaunchSpec, size: PtySize) -> Result<Pty, String> {
    let (file, args) = command_for(spec.kind, spec.url.as_deref(), spec.target.as_deref())?;
    let env = terminal_env(std::env::vars());
    #[cfg(windows)]
    let env = if runs_in_wsl(spec.kind) {
        if !on_path("wsl.exe") {
            return Err(WSL_MISSING.into());
        }
        wsl_env(env)
    } else {
        env
    };
    let pair = native_pty_system().openpty(pty_size(size)).map_err(|e| e.to_string())?;
    let mut command = CommandBuilder::new(file);
    command.args(args);
    command.env_clear();
    for (key, value) in env {
        command.env(key, value);
    }
    command.cwd(&spec.cwd);
    let child = pair.slave.spawn_command(command).map_err(|e| e.to_string())?;
    let pid = child.process_id().unwrap_or(0);
    Ok(Pty { master: pair.master, child, pid })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn destinations_are_arguments_not_script() {
        let (_, args) = command_for(SessionKind::Ssh, None, Some("prod")).unwrap();
        assert_eq!(args[1..], ["-c", r#"exec ssh -- "$1""#, "ssh", "prod"]);
        assert!(command_for(SessionKind::Ssh, None, None).is_err());
        let (_, args) = command_for(SessionKind::Browser, Some("localhost:3000"), Some("prod")).unwrap();
        assert_eq!(args[2..], [r#"exec terminal-browser open --no-merge --ssh "$1" "$2""#, "terminal-browser", "prod", "localhost:3000"]);
        let (_, args) = command_for(SessionKind::Browser, Some("example.com; rm -rf ~"), None).unwrap();
        assert_eq!(args.last().unwrap(), "example.com; rm -rf ~");
    }

    #[test]
    fn login_shell_scripts_take_destinations_as_parameters() {
        assert_eq!(login_shell_args(SessionKind::Shell, None, None).unwrap(), ["-l"]);
        let args = login_shell_args(SessionKind::Editor, None, None).unwrap();
        assert_eq!(args, ["-l", "-c", "exec tode .", "tode"]);
        let args = login_shell_args(SessionKind::Browser, Some("example.com/?a=1&b=2"), None).unwrap();
        assert_eq!(args, ["-l", "-c", r#"exec terminal-browser open --no-merge "$1""#, "terminal-browser", "example.com/?a=1&b=2"]);
    }

    #[test]
    fn wsl_runs_the_login_shell_without_parsing_its_arguments() {
        let login = login_shell_args(SessionKind::Browser, Some("localhost:3000"), Some("prod")).unwrap();
        let args = wsl_args(login.clone());
        assert_eq!(args[..5], ["--exec", "/bin/sh", "-c", WSL_LOGIN_SHELL, "sh"]);
        assert_eq!(args[5..], login[..], "the login shell arguments follow unchanged");
        assert!(!WSL_LOGIN_SHELL.contains("localhost"), "destinations never enter the script");
    }

    #[test]
    fn wsl_receives_the_variables_westerm_sets() {
        let env = wsl_env(terminal_env([("PATH".to_owned(), "C:\\Windows".to_owned())]));
        let wslenv: Vec<&str> = env.iter().filter(|(k, _)| k == "WSLENV").map(|(_, v)| v.as_str()).collect();
        assert_eq!(wslenv, ["TERM:COLORTERM:TERM_PROGRAM:TERMINAL_BROWSER_PRESENT"]);
    }

    #[test]
    fn wsl_keeps_the_users_shared_variables() {
        let base = [("WslEnv", "USERPROFILE/p:TERM_PROGRAM"), ("PATH", "C:\\Windows")].map(|(k, v)| (k.to_owned(), v.to_owned()));
        let env = wsl_env(base.to_vec());
        let wslenv: Vec<(&str, &str)> =
            env.iter().filter(|(k, _)| k.eq_ignore_ascii_case("WSLENV")).map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(wslenv, [("WslEnv", "USERPROFILE/p:TERM_PROGRAM:TERM:COLORTERM:TERMINAL_BROWSER_PRESENT")]);
    }

    #[cfg(windows)]
    #[test]
    fn windows_programs_get_argument_lists() {
        let (program, args) = command_for(SessionKind::Ssh, None, Some("prod")).unwrap();
        assert_eq!(program, "ssh");
        assert_eq!(args, ["--", "prod"]);
        let (program, args) = command_for(SessionKind::Browser, Some("example.com/?a=1&b=2"), Some("prod")).unwrap();
        assert_eq!(program, "wsl.exe");
        assert_eq!(args.last().unwrap(), "example.com/?a=1&b=2", "wsl.exe is not a cmd.exe shim");
        let (program, _) = command_for(SessionKind::Editor, None, None).unwrap();
        assert_eq!(program, "wsl.exe");
        let (shell, _) = command_for(SessionKind::Shell, None, None).unwrap();
        assert!(shell.ends_with(".exe"));
    }

    #[test]
    fn foreign_terminal_variables_do_not_leak() {
        let base = [("PATH", "/bin"), ("TERM", "xterm-ghostty"), ("TERM_PROGRAM", "ghostty"), ("GHOSTTY_RESOURCES_DIR", "/x"), ("TMUX", "/t")]
            .map(|(k, v)| (k.to_owned(), v.to_owned()));
        let env = terminal_env(base);
        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["PATH", "TERM", "COLORTERM", "TERM_PROGRAM", "TERMINAL_BROWSER_PRESENT"]);
        assert!(env.contains(&("TERM_PROGRAM".into(), "westerm".into())));
    }

    #[test]
    fn pixel_apps_are_asked_for_patches() {
        let env = terminal_env([("PATH".to_owned(), "/bin".to_owned())]);
        assert!(env.contains(&("TERMINAL_BROWSER_PRESENT".into(), "patched".into())));
    }

    #[test]
    fn a_presenter_set_before_westerm_stays() {
        let base = [("TERMINAL_BROWSER_PRESENT", "full"), ("PATH", "/bin")].map(|(k, v)| (k.to_owned(), v.to_owned()));
        let env = terminal_env(base);
        let presenters: Vec<&str> =
            env.iter().filter(|(k, _)| k == "TERMINAL_BROWSER_PRESENT").map(|(_, v)| v.as_str()).collect();
        assert_eq!(presenters, ["full"]);
    }
}
