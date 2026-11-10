//! Starting PTY processes: shells, ssh, terminal-browser, terminal-code.
//! macOS and Linux start everything through a login shell; Windows starts
//! programs directly (ConPTY) with PowerShell as the shell.

use std::path::PathBuf;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty};

use crate::protocol::{PtySize, SessionKind};
use crate::quote::ShellKind;

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
pub(crate) fn login_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
}

/// How a new shell reads quoted words; None for a shell westerm cannot quote for.
#[cfg(unix)]
pub fn shell_kind() -> Option<ShellKind> {
    ShellKind::from_path(&login_shell())
}

/// Windows shells (PowerShell, cmd) get no plugin panes.
#[cfg(windows)]
pub fn shell_kind() -> Option<ShellKind> {
    None
}

/// Every kind starts through a login shell so PATH matches the user's
/// terminal. URLs and ssh destinations are positional parameters, never part
/// of the script, so they cannot inject shell syntax.
#[cfg(unix)]
pub fn command_for(kind: SessionKind, url: Option<&str>, target: Option<&str>) -> Result<(String, Vec<String>), String> {
    let shell = login_shell();
    let args: Vec<String> = match kind {
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
    };
    Ok((shell, args))
}

/// PowerShell 7 when installed, else Windows PowerShell, which every
/// Windows 10 and later has.
#[cfg(windows)]
fn windows_shell() -> String {
    let has_pwsh = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("pwsh.exe").is_file()));
    if has_pwsh { "pwsh.exe" } else { "powershell.exe" }.into()
}

/// Characters that cmd.exe reads as syntax. Tools installed as .cmd shims
/// (npm, scoop) run through cmd.exe, so arguments must not contain them.
#[cfg(windows)]
const CMD_SYNTAX: &str = "\"&|<>^%!";

/// Programs start directly with an argument list; there is no script.
#[cfg(windows)]
pub fn command_for(kind: SessionKind, url: Option<&str>, target: Option<&str>) -> Result<(String, Vec<String>), String> {
    Ok(match kind {
        SessionKind::Shell => (windows_shell(), vec!["-NoLogo".into()]),
        SessionKind::Ssh => {
            let target = target.ok_or("ssh needs a destination")?;
            ("ssh".into(), vec!["--".into(), target.into()])
        }
        SessionKind::Browser => {
            let mut args: Vec<String> = vec!["open".into(), "--no-merge".into()];
            if let Some(target) = target {
                args.extend(["--ssh".into(), target.into()]);
            }
            if let Some(url) = url {
                if url.contains(|c| CMD_SYNTAX.contains(c)) {
                    return Err(format!("this URL cannot be passed safely on Windows; remove {CMD_SYNTAX}"));
                }
                args.push(url.into());
            }
            ("terminal-browser".into(), args)
        }
        SessionKind::Editor => ("tode".into(), vec![".".into()]),
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
    let pair = native_pty_system().openpty(pty_size(size)).map_err(|e| e.to_string())?;
    let mut command = CommandBuilder::new(file);
    command.args(args);
    command.env_clear();
    for (key, value) in terminal_env(std::env::vars()) {
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

    #[cfg(windows)]
    #[test]
    fn windows_programs_get_argument_lists() {
        let (program, args) = command_for(SessionKind::Ssh, None, Some("prod")).unwrap();
        assert_eq!(program, "ssh");
        assert_eq!(args, ["--", "prod"]);
        let (_, args) = command_for(SessionKind::Browser, Some("localhost:3000"), Some("prod")).unwrap();
        assert_eq!(args, ["open", "--no-merge", "--ssh", "prod", "localhost:3000"]);
        assert!(command_for(SessionKind::Browser, Some("example.com/?a=1&b=2"), None).is_err(), "cmd.exe syntax");
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
