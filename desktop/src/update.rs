//! Updates for the desktop app from GitHub Releases: find the newest release,
//! download this system's app, check it against its SHA-256 sum, and install
//! it. Downloads go through curl, as `westerm update` does.
//!
//! - macOS: westerm-macos-app.zip replaces the whole app, which opens again.
//! - Windows: westerm-windows-app-setup.exe runs after the app quits.
//! - Linux: the packages (.deb, .rpm) need root, so the release page opens.
//!
//! The page's update panel asks the app, not the dashboard: the app may show
//! a westerm started elsewhere (a terminal), whose version is not the app's.
//!
//! WESTERM_RELEASES_API points at another copy of the release data (tests).
//! WESTERM_NO_UPDATE_CHECK turns checks off, as it does for westerm.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::AppHandle;

use crate::state::shared;

const LATEST_API: &str = "https://api.github.com/repos/SteelCrab/westerm/releases/latest";
/// How long an answer from GitHub is used before asking again; the page's
/// "Check now" and an install always ask.
const RECHECK_AFTER: Duration = Duration::from_secs(60 * 60);
static LATEST: Mutex<Option<(Instant, Release)>> = Mutex::new(None);

fn disabled() -> bool {
    std::env::var_os("WESTERM_NO_UPDATE_CHECK").is_some()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The new app is in place; this one quits.
    Relaunching,
    /// The release page is open for the person to install by hand.
    Manual,
}

impl Outcome {
    /// src/appBridge.ts AppUpdateResult.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Relaunching => "relaunching",
            Self::Manual => "manual",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub page: String,
    assets: Vec<(String, String)>,
}

impl Release {
    fn asset(&self, name: &str) -> Result<&str, String> {
        self.assets
            .iter()
            .find(|(asset, _)| asset == name)
            .map(|(_, url)| url.as_str())
            .ok_or_else(|| format!("the release has no {name}"))
    }
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
        page: json["html_url"].as_str().unwrap_or("https://github.com/SteelCrab/westerm/releases/latest").to_owned(),
        assets,
    })
}

/// The release file of the app for `os` (std::env::consts::OS); None where
/// the app cannot install itself.
fn asset_for(os: &str) -> Option<&'static str> {
    match os {
        "macos" => Some("westerm-macos-app.zip"),
        "windows" => Some("westerm-windows-app-setup.exe"),
        _ => None,
    }
}

/// A command with no console window on Windows (the app has none).
fn quiet(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn curl(url: &str, output: Option<&Path>, timeout_seconds: u32, version: &str) -> Result<Vec<u8>, String> {
    let mut command = quiet("curl");
    command
        .args(["--fail", "--silent", "--show-error", "--location", "--max-time"])
        .arg(timeout_seconds.to_string())
        .args(["--header", "Accept: application/vnd.github+json", "--user-agent"])
        .arg(format!("westerm-app/{version}"));
    if let Some(output) = output {
        command.arg("--output").arg(output);
    }
    let result = command.arg(url).output().map_err(|e| format!("cannot run curl: {e}"))?;
    if !result.status.success() {
        return Err(format!("download failed: {url} ({})", String::from_utf8_lossy(&result.stderr).trim()));
    }
    Ok(result.stdout)
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

/// Checks `file` against a `.sha256` file's text ("<sum>  <name>").
fn verify(file: &Path, sums: &str) -> Result<(), String> {
    let expected = sums.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    if expected.len() != 64 || expected != sha256_hex(file)? {
        return Err("the download does not match its SHA-256 sum".into());
    }
    Ok(())
}

/// A folder that is removed with everything in it, unless kept.
// Linux opens the release page and downloads nothing; the tests use these.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
struct WorkDir {
    path: PathBuf,
    keep: bool,
}

#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
impl WorkDir {
    fn new_in(parent: &Path) -> Result<Self, String> {
        let path = parent.join(format!("{WORK_PREFIX}{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).map_err(|e| format!("cannot write {}: {e}", parent.display()))?;
        Ok(Self { path, keep: false })
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

const WORK_PREFIX: &str = ".westerm-update-";

/// Downloads `name` from `release` into `work` and checks its sum.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
fn download(release: &Release, name: &str, work: &Path, version: &str) -> Result<PathBuf, String> {
    let file = work.join(name);
    curl(release.asset(name)?, Some(&file), 900, version)?;
    let sums = curl(release.asset(&format!("{name}.sha256"))?, None, 60, version)?;
    verify(&file, &String::from_utf8_lossy(&sums))?;
    Ok(file)
}

fn version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// The newest release: the last hour's answer unless `refresh`.
fn latest(app: &AppHandle, refresh: bool) -> Result<Release, String> {
    if !refresh
        && let Some((asked, release)) = LATEST.lock().unwrap().as_ref()
        && asked.elapsed() < RECHECK_AFTER
    {
        return Ok(release.clone());
    }
    let api = std::env::var("WESTERM_RELEASES_API").unwrap_or_else(|_| LATEST_API.to_owned());
    let release = parse_release(&curl(&api, None, 20, &version(app))?)?;
    *LATEST.lock().unwrap() = Some((Instant::now(), release.clone()));
    Ok(release)
}

/// shared/protocol.ts UpdateStatus for this app.
fn status_of(current: &str, disabled: bool, latest: impl FnOnce() -> Result<Release, String>) -> Value {
    let mut body = json!({ "current": current, "channel": "app" });
    if disabled {
        body["disabled"] = true.into();
        return body;
    }
    match latest() {
        Ok(release) => {
            body["newer"] = is_newer(&release.version, current).into();
            body["latest"] = release.version.into();
            body["page"] = release.page.into();
        }
        Err(error) => body["error"] = error.into(),
    }
    body
}

/// Whether a newer app exists, for the page's update panel.
pub fn status(app: &AppHandle, refresh: bool) -> Value {
    status_of(&version(app), disabled(), || latest(app, refresh))
}

/// Installs the newest release when it is newer than this app.
pub fn install(app: &AppHandle) -> Result<Outcome, String> {
    let shared = shared(app);
    if shared.updating.swap(true, Ordering::SeqCst) {
        return Err("an update is already running".into());
    }
    let result = install_newest(app);
    shared.updating.store(false, Ordering::SeqCst);
    if let Err(error) = &result {
        shared.log.write(&format!("westerm-app: update failed: {error}"));
    }
    result
}

fn install_newest(app: &AppHandle) -> Result<Outcome, String> {
    if disabled() {
        return Err("updates are turned off (WESTERM_NO_UPDATE_CHECK)".into());
    }
    let current = version(app);
    let release = latest(app, true)?;
    if !is_newer(&release.version, &current) {
        return Err(format!("westerm {current} is up to date"));
    }
    shared(app).log.write(&format!("westerm-app: updating {current} → {}", release.version));
    match asset_for(std::env::consts::OS) {
        Some(name) => platform::install(app, &release, name, &current),
        None => {
            let page = release.page.parse().map_err(|e| format!("{e}"))?;
            crate::windows::open_external(app, &page);
            Ok(Outcome::Manual)
        }
    }
}

/// Removes what an earlier update left: the old app next to this one
/// (macOS), or the installer in the temporary folder (Windows).
pub fn clean_up() {
    #[cfg(target_os = "macos")]
    let folder = platform::bundle().ok().and_then(|bundle| bundle.parent().map(Path::to_path_buf));
    #[cfg(not(target_os = "macos"))]
    let folder = cfg!(windows).then(std::env::temp_dir);
    let Some(entries) = folder.and_then(|folder| fs::read_dir(folder).ok()) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(WORK_PREFIX) {
            // An installer that still runs keeps its folder; the next start tries again.
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// After the app stopped its server for an update that then failed: the
/// dashboard comes back, so the window is not left without one.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
fn restore(app: &AppHandle, result: Result<Outcome, String>) -> Result<Outcome, String> {
    if result.is_err() {
        crate::state::reconnect(app);
    }
    result
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    /// westerm.app around this executable (Contents/MacOS/westerm-app).
    pub fn bundle() -> Result<PathBuf, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = exe.canonicalize().unwrap_or(exe);
        exe.ancestors()
            .find(|path| path.extension().is_some_and(|ext| ext == "app"))
            .map(Path::to_path_buf)
            .ok_or_else(|| "this app is not inside a westerm.app bundle".into())
    }

    fn read_only(path: &Path) -> bool {
        use std::os::unix::ffi::OsStrExt;
        let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false };
        // SAFETY: statvfs writes into the zeroed struct it is given.
        let mut info: libc::statvfs = unsafe { std::mem::zeroed() };
        unsafe { libc::statvfs(path.as_ptr(), &mut info) == 0 && info.f_flag & libc::ST_RDONLY != 0 }
    }

    fn bundle_version(app: &Path) -> Option<String> {
        let output = Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", "Print :CFBundleShortVersionString"])
            .arg(app.join("Contents/Info.plist"))
            .output()
            .ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    pub fn install(app: &AppHandle, release: &Release, name: &str, current: &str) -> Result<Outcome, String> {
        let bundle = bundle()?;
        let folder = bundle.parent().ok_or("the app has no folder")?;
        // A downloaded app opened in place runs from a read-only copy, and one
        // opened from the disk image runs from a read-only volume.
        if bundle.to_string_lossy().contains("/AppTranslocation/") || read_only(&bundle) {
            return Err("move westerm.app to the Applications folder, then update".into());
        }
        // The download sits next to the app, so the swap is a rename on one volume.
        let mut work = WorkDir::new_in(folder)?;
        let archive = download(release, name, &work.path, current)?;
        let unpacked = Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(&archive)
            .arg(&work.path)
            .status()
            .map_err(|e| format!("cannot run ditto: {e}"))?;
        if !unpacked.success() {
            return Err("cannot unpack the download".into());
        }
        let new_app = work.path.join("westerm.app");
        if bundle_version(&new_app).as_deref() != Some(release.version.as_str()) {
            return Err("the download is not the expected version".into());
        }

        // Stop the dashboard this app started, so the new copy starts its own
        // instead of finding this one.
        shared(app).stop_server();
        restore(app, swap(&mut work, &new_app, &bundle))
    }

    /// Puts `new_app` where `bundle` is and opens it. The old copy moves into
    /// `work`, and comes back if the move fails.
    fn swap(work: &mut WorkDir, new_app: &Path, bundle: &Path) -> Result<Outcome, String> {
        let previous = work.path.join("westerm-previous.app");
        fs::rename(bundle, &previous).map_err(|e| format!("cannot move {}: {e}", bundle.display()))?;
        if let Err(error) = fs::rename(new_app, bundle) {
            let _ = fs::rename(&previous, bundle);
            return Err(format!("cannot put the new app in place: {error}"));
        }
        // This copy still runs from the old files; the new copy removes them.
        work.keep = true;
        Command::new("/usr/bin/open")
            .arg("-n")
            .arg(bundle)
            .spawn()
            .map_err(|e| format!("cannot open the new app: {e}"))?;
        Ok(Outcome::Relaunching)
    }
}

#[cfg(windows)]
mod platform {
    use super::*;

    pub fn install(app: &AppHandle, release: &Release, name: &str, current: &str) -> Result<Outcome, String> {
        let mut work = WorkDir::new_in(&std::env::temp_dir())?;
        let setup = download(release, name, &work.path, current)?;
        let westerm = crate::server::sidecar()?;
        // A running westerm.exe cannot be replaced. The server goes first: it
        // would start the host again for the panes that reconnect. Then the
        // PTY host saves every session and stops; the shells come back once
        // as "restored". The installer stops it again (windows/hooks.nsh).
        shared(app).stop_server();
        let _ = quiet(&westerm).arg("stop").status();
        let started = Command::new(&setup).spawn().map_err(|e| format!("cannot run the installer: {e}"));
        restore(app, started.map(|_| {
            // The installer still runs from the folder.
            work.keep = true;
            Outcome::Relaunching
        }))
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use super::*;

    pub fn install(_: &AppHandle, _: &Release, name: &str, _: &str) -> Result<Outcome, String> {
        Err(format!("cannot install {name} here"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("0.2.2", "0.2.1"));
        assert!(is_newer("v0.10.0", "0.9.9"), "numbers, not text");
        assert!(!is_newer("0.2.1", "0.2.1"));
        assert!(!is_newer("0.2.0", "0.2.1"));
        assert!(!is_newer("garbage", "0.2.1"));
        assert_eq!(parse_version("v1.2.3-rc.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
    }

    #[test]
    fn reads_the_latest_release() {
        let body = br#"{"tag_name":"v0.3.0","html_url":"https://github.com/SteelCrab/westerm/releases/tag/v0.3.0",
            "assets":[{"name":"westerm-macos-app.zip","browser_download_url":"https://example.com/a.zip"},
                      {"name":"westerm-macos-app.zip.sha256","browser_download_url":"https://example.com/a.zip.sha256"}]}"#;
        let release = parse_release(body).unwrap();
        assert_eq!(release.version, "0.3.0");
        assert_eq!(release.page, "https://github.com/SteelCrab/westerm/releases/tag/v0.3.0");
        assert_eq!(release.asset("westerm-macos-app.zip"), Ok("https://example.com/a.zip"));
        assert!(release.asset("westerm-windows-app-setup.exe").is_err());
        assert!(parse_release(br#"{"message":"Not Found"}"#).is_err());
    }

    #[test]
    fn the_update_panel_hears_about_this_app() {
        let release = || Ok(Release { version: "0.3.0".into(), page: "https://example.com/v0.3.0".into(), assets: vec![] });
        assert_eq!(
            status_of("0.2.1", false, release),
            json!({ "current": "0.2.1", "channel": "app", "newer": true, "latest": "0.3.0", "page": "https://example.com/v0.3.0" })
        );
        assert_eq!(status_of("0.3.0", false, release)["newer"], false);
        assert_eq!(
            status_of("0.2.1", false, || Err("download failed".into())),
            json!({ "current": "0.2.1", "channel": "app", "error": "download failed" })
        );
        let asked = std::cell::Cell::new(false);
        let off = status_of("0.2.1", true, || {
            asked.set(true);
            release()
        });
        assert_eq!(off, json!({ "current": "0.2.1", "channel": "app", "disabled": true }));
        assert!(!asked.get(), "turned off: GitHub is not asked");
    }

    #[test]
    fn each_system_has_its_app_file() {
        assert_eq!(asset_for("macos"), Some("westerm-macos-app.zip"));
        assert_eq!(asset_for("windows"), Some("westerm-windows-app-setup.exe"));
        assert_eq!(asset_for("linux"), None, "packages need root: the release page opens");
    }

    #[test]
    fn checksum_mismatch_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("westerm-macos-app.zip");
        fs::write(&file, b"westerm").unwrap();
        let sum = sha256_hex(&file).unwrap();
        assert!(verify(&file, &format!("{sum}  westerm-macos-app.zip\n")).is_ok());
        assert!(verify(&file, &format!("{}  westerm-macos-app.zip\n", sum.to_uppercase())).is_ok());
        assert!(verify(&file, &format!("{}  westerm-macos-app.zip\n", "0".repeat(64))).is_err());
        assert!(verify(&file, "").is_err());
        assert!(verify(&file, "not-a-sum").is_err());
    }

    #[test]
    fn work_folders_go_away_unless_kept() {
        let parent = tempfile::tempdir().unwrap();
        let path = {
            let work = WorkDir::new_in(parent.path()).unwrap();
            fs::write(work.path.join("file"), b"x").unwrap();
            work.path.clone()
        };
        assert!(!path.exists());
        let mut work = WorkDir::new_in(parent.path()).unwrap();
        work.keep = true;
        let kept = work.path.clone();
        drop(work);
        assert!(kept.exists());
    }

    #[cfg(unix)]
    #[test]
    fn downloads_are_checked_before_use() {
        let source = tempfile::tempdir().unwrap();
        let app = source.path().join("westerm-macos-app.zip");
        fs::write(&app, b"new app").unwrap();
        let sum = sha256_hex(&app).unwrap();
        let good = source.path().join("good.sha256");
        fs::write(&good, format!("{sum}  westerm-macos-app.zip\n")).unwrap();
        let bad = source.path().join("bad.sha256");
        fs::write(&bad, format!("{}  westerm-macos-app.zip\n", "0".repeat(64))).unwrap();
        let release = |sums: &Path| Release {
            version: "9.9.9".into(),
            page: String::new(),
            assets: vec![
                ("westerm-macos-app.zip".into(), format!("file://{}", app.display())),
                ("westerm-macos-app.zip.sha256".into(), format!("file://{}", sums.display())),
            ],
        };
        let work = tempfile::tempdir().unwrap();
        let file = download(&release(&good), "westerm-macos-app.zip", work.path(), "0.0.0").unwrap();
        assert_eq!(fs::read(file).unwrap(), b"new app");
        let error = download(&release(&bad), "westerm-macos-app.zip", work.path(), "0.0.0").unwrap_err();
        assert!(error.contains("SHA-256"), "{error}");
    }
}
