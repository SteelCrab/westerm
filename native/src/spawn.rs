//! Starting PTY processes: login shells, ssh, terminal-browser, terminal-code.

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
];
const FOREIGN_TERMINAL_PREFIXES: &[&str] = &["GHOSTTY_", "KITTY_", "ITERM_", "WEZTERM_", "VSCODE_"];

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

fn login_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
}

/// Every kind starts through a login shell so PATH matches the user's
/// terminal. URLs and ssh destinations are positional parameters, never part
/// of the script, so they cannot inject shell syntax.
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
    fn foreign_terminal_variables_do_not_leak() {
        let base = [("PATH", "/bin"), ("TERM", "xterm-ghostty"), ("TERM_PROGRAM", "ghostty"), ("GHOSTTY_RESOURCES_DIR", "/x"), ("TMUX", "/t")]
            .map(|(k, v)| (k.to_owned(), v.to_owned()));
        let env = terminal_env(base);
        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["PATH", "TERM", "COLORTERM", "TERM_PROGRAM"]);
        assert!(env.contains(&("TERM_PROGRAM".into(), "westerm".into())));
    }
}
