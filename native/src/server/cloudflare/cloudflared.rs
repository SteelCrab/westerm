//! Finding and talking to cloudflared: where it is installed, the arguments
//! of a Quick Tunnel, and its metrics server, which tells the tunnel's
//! address (`/quicktunnel`) and whether Cloudflare's edge is connected (`/ready`).

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::state_dir::home_dir;

/// A path to use instead of searching, for tests and unusual installs.
pub const ENV_OVERRIDE: &str = "WESTERM_CLOUDFLARED";

/// cloudflared from `override_path` only when it is set; else the first
/// one in `path_var`, then in `extra`.
pub fn find(override_path: Option<OsString>, path_var: Option<OsString>, extra: &[PathBuf]) -> Option<PathBuf> {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let name = if cfg!(windows) { "cloudflared.exe" } else { "cloudflared" };
    path_var
        .iter()
        .flat_map(std::env::split_paths)
        .chain(extra.iter().cloned())
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Where installers put cloudflared outside PATH. An app opened from the
/// macOS Finder has no Homebrew folder in its PATH.
pub fn common_dirs() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        vec![PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")]
    } else if cfg!(windows) {
        let under = |var: &str, rest: &str| std::env::var_os(var).map(|base| PathBuf::from(base).join(rest));
        [
            under("ProgramFiles", "cloudflared"),
            under("ProgramFiles(x86)", "cloudflared"),
            under("LOCALAPPDATA", "Microsoft\\WinGet\\Links"),
        ]
        .into_iter()
        .flatten()
        .collect()
    } else {
        vec![PathBuf::from("/usr/local/bin"), PathBuf::from("/usr/bin"), home_dir().join(".local/bin")]
    }
}

/// A Quick Tunnel to `127.0.0.1:origin_port`, its metrics on
/// `127.0.0.1:metrics_port`. The empty `config` keeps a user's
/// ~/.cloudflared/config.yml out: its ingress rules would send requests elsewhere.
pub fn args(metrics_port: u16, origin_port: u16, config: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["tunnel", "--no-autoupdate", "--config"].into_iter().map(OsString::from).collect();
    args.push(config.as_os_str().to_owned());
    args.push("--metrics".into());
    args.push(format!("127.0.0.1:{metrics_port}").into());
    args.push("--url".into());
    args.push(format!("http://127.0.0.1:{origin_port}").into());
    args
}

pub const EMPTY_CONFIG: &str = "# westerm: an empty config, so ~/.cloudflared/config.yml does not apply\n";

pub fn write_empty_config(state_dir: &Path) -> io::Result<PathBuf> {
    let path = state_dir.join("cloudflared.yml");
    std::fs::write(&path, EMPTY_CONFIG)?;
    Ok(path)
}

/// The tunnel's host name from `/quicktunnel`: `{"hostname":"<name>.trycloudflare.com"}`.
pub fn parse_hostname(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let name = value.get("hostname")?.as_str()?;
    let name = name.strip_prefix("https://").unwrap_or(name);
    let valid = !name.is_empty()
        && name.len() <= 253
        && name.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-'));
    valid.then(|| name.to_owned())
}

async fn metrics_get(metrics_port: u16, path: &str) -> Option<(u16, Vec<u8>)> {
    let stream = tokio::net::TcpStream::connect(("127.0.0.1", metrics_port)).await.ok()?;
    crate::server::http1::exchange(stream, "GET", path, None).await
}

/// The tunnel's host name, once cloudflared has it. Its metrics server opens
/// only after Cloudflare handed out the name.
pub async fn hostname(metrics_port: u16) -> Option<String> {
    let (status, body) = metrics_get(metrics_port, "/quicktunnel").await?;
    if status == 200 { parse_hostname(&body) } else { None }
}

/// Some(true) while cloudflared has a connection to Cloudflare's edge,
/// Some(false) while it has none; None when its metrics do not answer.
pub async fn ready(metrics_port: u16) -> Option<bool> {
    match metrics_get(metrics_port, "/ready").await?.0 {
        200 => Some(true),
        503 => Some(false),
        _ => None,
    }
}

/// A port on 127.0.0.1 that was free a moment ago, for cloudflared's metrics.
pub fn free_port() -> io::Result<u16> {
    Ok(std::net::TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    const NAME: &str = if cfg!(windows) { "cloudflared.exe" } else { "cloudflared" };

    fn dir_with_binary() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(NAME), b"").unwrap();
        dir
    }

    #[test]
    fn find_prefers_the_override() {
        let installed = dir_with_binary();
        let path = Some(OsString::from(installed.path()));
        let other = dir_with_binary();
        let chosen = other.path().join(NAME);
        assert_eq!(find(Some(chosen.clone().into()), path.clone(), &[]), Some(chosen));
        let missing = other.path().join("nothing-here");
        assert_eq!(find(Some(missing.into()), path, &[]), None, "a wrong override is not replaced by PATH");
    }

    #[test]
    fn find_searches_path_then_extra_dirs() {
        let empty = tempfile::tempdir().unwrap();
        let installed = dir_with_binary();
        let path = std::env::join_paths([empty.path(), installed.path()]).unwrap();
        assert_eq!(find(None, Some(path), &[]), Some(installed.path().join(NAME)));

        let extra = dir_with_binary();
        let only_empty = Some(OsString::from(empty.path()));
        assert_eq!(find(None, only_empty.clone(), &[extra.path().to_path_buf()]), Some(extra.path().join(NAME)));
        assert_eq!(find(None, only_empty, &[empty.path().to_path_buf()]), None);
        assert_eq!(find(None, None, &[]), None);
    }

    #[test]
    fn args_use_an_empty_config() {
        let args = args(20241, 51000, Path::new("/s/cloudflared.yml"));
        let expected: Vec<OsString> = [
            "tunnel",
            "--no-autoupdate",
            "--config",
            "/s/cloudflared.yml",
            "--metrics",
            "127.0.0.1:20241",
            "--url",
            "http://127.0.0.1:51000",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        assert_eq!(args, expected);
        assert!(!args.iter().any(|arg| arg == "--hello-world" || arg == "--allowed-mail"));
    }

    #[test]
    fn the_empty_config_lands_in_the_state_folder() {
        let state = tempfile::tempdir().unwrap();
        let path = write_empty_config(state.path()).unwrap();
        assert_eq!(path, state.path().join("cloudflared.yml"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), EMPTY_CONFIG);
    }

    #[test]
    fn parse_hostname_cases() {
        assert_eq!(parse_hostname(br#"{"hostname":"abc-def.trycloudflare.com"}"#).as_deref(), Some("abc-def.trycloudflare.com"));
        assert_eq!(parse_hostname(br#"{"hostname":"https://abc.trycloudflare.com"}"#).as_deref(), Some("abc.trycloudflare.com"));
        assert_eq!(parse_hostname(br#"{"hostname":""}"#), None);
        assert_eq!(parse_hostname(br#"{"hostname":"a b"}"#), None);
        assert_eq!(parse_hostname(br#"{"hostname":"Evil.Example"}"#), None);
        assert_eq!(parse_hostname(b"not json"), None);
    }

    #[test]
    fn free_port_is_a_real_port() {
        assert_ne!(free_port().unwrap(), 0);
    }
}
