//! Updates from GitHub Releases: `westerm update` and the dashboard's update
//! button (/api/update).
//!
//! Downloads go through the system's curl and tar, which macOS, Linux, and
//! Windows 10 or later all have, so the binary carries no TLS library. Each
//! archive is checked against the SHA-256 sum published with it, and the new
//! executable must run and report the expected version before it replaces
//! this one.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const TARGET: &str = env!("WESTERM_TARGET");
const LATEST_API: &str = "https://api.github.com/repos/SteelCrab/westerm/releases/latest";

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    /// The release page, which has the notes.
    pub page: String,
    /// (file name, download URL)
    assets: Vec<(String, String)>,
}

impl Release {
    fn asset(&self, name: &str) -> Option<&str> {
        self.assets.iter().find(|(asset, _)| asset == name).map(|(_, url)| url.as_str())
    }
}

/// How this copy of westerm is updated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    /// An installed executable: replaced in place.
    Binary,
    /// Inside westerm.app: the app replaces its whole bundle.
    MacApp,
    /// Built from source (target/release): update with git pull and npm run build.
    Source,
}

/// This executable, with links resolved, so an update replaces the real file.
pub fn executable() -> Result<PathBuf, String> {
    let path = std::env::current_exe().map_err(|e| format!("cannot find this executable: {e}"))?;
    path.canonicalize().map_err(|e| format!("cannot resolve {}: {e}", path.display()))
}

pub fn channel() -> Channel {
    let Ok(path) = executable() else { return Channel::Binary };
    channel_of(&path)
}

fn channel_of(path: &Path) -> Channel {
    if path.to_string_lossy().contains(".app/Contents/MacOS/") {
        return Channel::MacApp;
    }
    // target/release, target/debug, or target/<triple>/release
    let parts: Vec<String> = path.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
    let built = |name: &str| name == "release" || name == "debug";
    let from_source = parts.windows(2).any(|w| w[0] == "target" && built(&w[1]))
        || parts.windows(3).any(|w| w[0] == "target" && built(&w[2]));
    if from_source { Channel::Source } else { Channel::Binary }
}

/// "1.2.3" or "v1.2.3-rc.1" as (1, 2, 3).
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

/// The release archive for this build. Linux uses the static (musl)
/// archives; Windows on Arm runs the x64 build.
fn asset_name() -> Option<String> {
    asset_for(TARGET)
}

fn asset_for(target: &str) -> Option<String> {
    let archive = if target.ends_with("-apple-darwin") {
        target
    } else if target.contains("-linux-") {
        if target.starts_with("x86_64") {
            "x86_64-unknown-linux-musl"
        } else if target.starts_with("aarch64") {
            "aarch64-unknown-linux-musl"
        } else {
            return None;
        }
    } else if target.contains("-windows-") {
        "x86_64-pc-windows-msvc"
    } else {
        return None;
    };
    let extension = if archive.contains("windows") { "zip" } else { "tar.gz" };
    Some(format!("westerm-{archive}.{extension}"))
}

fn curl(url: &str, output: Option<&Path>, timeout_seconds: u32) -> Result<Vec<u8>, String> {
    let mut command = Command::new("curl");
    command
        .args(["--fail", "--silent", "--show-error", "--location", "--max-time"])
        .arg(timeout_seconds.to_string())
        .args(["--header", "Accept: application/vnd.github+json", "--user-agent"])
        .arg(format!("westerm/{CURRENT}"));
    if let Some(output) = output {
        command.arg("--output").arg(output);
    }
    let result = command.arg(url).output().map_err(|e| format!("cannot run curl: {e}"))?;
    if !result.status.success() {
        return Err(format!("download failed: {url} ({})", String::from_utf8_lossy(&result.stderr).trim()));
    }
    Ok(result.stdout)
}

/// The newest published release. WESTERM_RELEASES_API points elsewhere
/// (a mirror, or a file:// fixture in tests).
pub fn latest() -> Result<Release, String> {
    let api = std::env::var("WESTERM_RELEASES_API").unwrap_or_else(|_| LATEST_API.to_owned());
    parse_release(&curl(&api, None, 20)?)
}

fn parse_release(body: &[u8]) -> Result<Release, String> {
    let json: serde_json::Value = serde_json::from_slice(body).map_err(|e| format!("unexpected release data: {e}"))?;
    let tag = json["tag_name"].as_str().ok_or("the release has no tag")?;
    parse_version(tag).ok_or_else(|| format!("unexpected release version {tag}"))?;
    let assets = json["assets"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|asset| {
                    Some((asset["name"].as_str()?.to_owned(), asset["browser_download_url"].as_str()?.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Release {
        version: tag.trim_start_matches('v').to_owned(),
        page: json["html_url"].as_str().unwrap_or_default().to_owned(),
        assets,
    })
}

fn sha256_hex(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// A temporary folder that is removed with everything in it.
struct WorkDir(PathBuf);

impl WorkDir {
    fn new() -> Result<Self, String> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let path = std::env::temp_dir().join(format!("westerm-update-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
        Ok(Self(path))
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Downloads `release`, checks it, and puts it in place of the executable at `exe`.
pub fn install(release: &Release, exe: &Path) -> Result<(), String> {
    match channel_of(exe) {
        Channel::MacApp => return Err("this westerm is inside westerm.app; update the app from its menu".into()),
        Channel::Source => return Err("this westerm was built from source; update it with git pull and npm run build".into()),
        Channel::Binary => {}
    }
    let name = asset_name().ok_or("there is no release build for this system")?;
    let url = release.asset(&name).ok_or_else(|| format!("the release has no {name}"))?;
    let sums = release.asset(&format!("{name}.sha256")).ok_or_else(|| format!("the release has no {name}.sha256"))?;

    let work = WorkDir::new()?;
    let archive = work.0.join(&name);
    curl(url, Some(&archive), 600)?;
    let expected = String::from_utf8_lossy(&curl(sums, None, 60)?)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if expected.is_empty() || expected != sha256_hex(&archive)? {
        return Err(format!("{name} does not match its SHA-256 sum"));
    }

    // tar reads .tar.gz everywhere, and .zip with the bsdtar of macOS and Windows.
    let unpacked = Command::new("tar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&work.0)
        .status()
        .map_err(|e| format!("cannot run tar: {e}"))?;
    if !unpacked.success() {
        return Err(format!("cannot unpack {name}"));
    }
    let folder = name.trim_end_matches(".tar.gz").trim_end_matches(".zip");
    let binary = work.0.join(folder).join(if cfg!(windows) { "westerm.exe" } else { "westerm" });
    if !binary.is_file() {
        return Err(format!("{name} has no westerm executable"));
    }
    // Refuse a download that does not run here or is not the version it claims.
    let reported = Command::new(&binary).arg("--version").output().map_err(|e| format!("the new westerm does not run: {e}"))?;
    if !String::from_utf8_lossy(&reported.stdout).starts_with(&format!("westerm {} ", release.version)) {
        return Err(format!("the downloaded westerm is not version {}", release.version));
    }
    replace(exe, &binary)
}

fn cannot_write(exe: &Path, error: std::io::Error) -> String {
    let folder = exe.parent().unwrap_or(exe).display();
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        format!("cannot write {folder}; run the install script again, or update as the folder's owner")
    } else {
        format!("cannot replace {}: {error}", exe.display())
    }
}

/// A rename swaps the file at once; a running westerm keeps the old one.
#[cfg(unix)]
fn replace(exe: &Path, new: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let staged = exe.with_file_name(format!(".westerm-update-{}", std::process::id()));
    fs::copy(new, &staged).map_err(|e| cannot_write(exe, e))?;
    let renamed = fs::set_permissions(&staged, fs::Permissions::from_mode(0o755)).and_then(|()| fs::rename(&staged, exe));
    renamed.map_err(|e| {
        let _ = fs::remove_file(&staged);
        cannot_write(exe, e)
    })
}

/// A running .exe cannot be overwritten, but it can be renamed. The old copy
/// is deleted the next time westerm starts.
#[cfg(windows)]
fn replace(exe: &Path, new: &Path) -> Result<(), String> {
    let old = old_copy(exe);
    let _ = fs::remove_file(&old);
    fs::rename(exe, &old).map_err(|e| cannot_write(exe, e))?;
    if let Err(error) = fs::copy(new, exe) {
        let _ = fs::rename(&old, exe);
        return Err(cannot_write(exe, error));
    }
    Ok(())
}

#[cfg(windows)]
fn old_copy(exe: &Path) -> PathBuf {
    let name = exe.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    exe.with_file_name(format!("{name}.old"))
}

/// Deletes what an earlier update on Windows had to leave behind.
pub fn clean_up() {
    #[cfg(windows)]
    if let Ok(exe) = executable() {
        let _ = fs::remove_file(old_copy(&exe));
    }
}

/// `westerm update [--check]`
pub fn run_cli(check_only: bool) -> Result<(), String> {
    println!("westerm {CURRENT}");
    let release = latest()?;
    if !is_newer(&release.version, CURRENT) {
        println!("westerm is up to date");
        return Ok(());
    }
    println!("new version {} — {}", release.version, release.page);
    if check_only {
        return Ok(());
    }
    install(&release, &executable()?)?;
    println!("updated to {}. Restart westerm to use it; open shells come back once as \"restored\".", release.version);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("v0.10.0", "0.9.9"), "numbers, not text");
        assert!(is_newer("1.0.0", "0.99.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("garbage", "0.1.0"));
        assert_eq!(parse_version("v1.2.3-rc.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
    }

    #[test]
    fn each_build_finds_its_archive() {
        assert_eq!(asset_for("aarch64-apple-darwin").as_deref(), Some("westerm-aarch64-apple-darwin.tar.gz"));
        assert_eq!(asset_for("x86_64-unknown-linux-gnu").as_deref(), Some("westerm-x86_64-unknown-linux-musl.tar.gz"));
        assert_eq!(asset_for("aarch64-unknown-linux-musl").as_deref(), Some("westerm-aarch64-unknown-linux-musl.tar.gz"));
        assert_eq!(asset_for("aarch64-pc-windows-msvc").as_deref(), Some("westerm-x86_64-pc-windows-msvc.zip"));
        assert_eq!(asset_for("riscv64gc-unknown-freebsd"), None);
        assert!(asset_name().is_some(), "this build has a release archive");
    }

    #[test]
    fn copies_are_told_apart_by_where_they_live() {
        assert_eq!(channel_of(Path::new("/Applications/westerm.app/Contents/MacOS/westerm")), Channel::MacApp);
        assert_eq!(channel_of(Path::new("/home/me/westerm/native/target/release/westerm")), Channel::Source);
        assert_eq!(channel_of(Path::new("/w/native/target/x86_64-pc-windows-msvc/release/westerm.exe")), Channel::Source);
        assert_eq!(channel_of(Path::new("/home/me/.local/bin/westerm")), Channel::Binary);
    }

    #[test]
    fn reads_the_github_release_shape() {
        let body = br#"{"tag_name":"v0.2.0","html_url":"https://github.com/SteelCrab/westerm/releases/tag/v0.2.0",
            "assets":[{"name":"westerm-x86_64-pc-windows-msvc.zip","browser_download_url":"https://example.com/w.zip"}]}"#;
        let release = parse_release(body).unwrap();
        assert_eq!(release.version, "0.2.0");
        assert_eq!(release.asset("westerm-x86_64-pc-windows-msvc.zip"), Some("https://example.com/w.zip"));
        assert!(parse_release(br#"{"message":"Not Found"}"#).is_err());
    }
}
