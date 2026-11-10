//! Plugin packages: where they come from, downloading them, and unpacking a
//! `.tar.gz` after checking every entry.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::read::MultiGzDecoder;
use serde_json::Value;

use super::manifest::{self, Manifest};
use super::Sources;
use crate::update::{parse_version, sha256_hex, CURRENT};

/// Largest download of a package.
pub const MAX_PACKAGE_BYTES: u64 = 5 * 1024 * 1024;
/// Largest total of the files in an unpacked package.
pub const MAX_UNPACKED_BYTES: u64 = 10 * 1024 * 1024;
/// Most files and folders in a package.
pub const MAX_ENTRIES: usize = 300;
/// Largest tar stream inside the gzip, so a small archive cannot inflate
/// without end.
pub const MAX_TAR_BYTES: u64 = 11 * 1024 * 1024;
/// Seconds for a package download.
pub const PACKAGE_TIMEOUT: u32 = 60;
/// Seconds for the plugin list and the GitHub API.
pub const META_TIMEOUT: u32 = 20;
/// Largest plugin list or GitHub API answer.
pub const MAX_META_BYTES: u64 = 1024 * 1024;

/// The release asset a GitHub repository publishes its package as.
const RELEASE_ASSET: &str = "westerm-plugin.tar.gz";
/// The error when the tar stream passes `MAX_TAR_BYTES`.
const TOO_BIG: &str = "the package unpacks to more than 11 MiB";
const MIB: u64 = 1024 * 1024;

/// What the user typed as the address of a package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceUrl {
    /// `https://github.com/<owner>/<repo>`: the latest release's package.
    GitHub { owner: String, repo: String },
    /// An `https://` address of a `.tar.gz` file.
    Tarball(String),
}

/// The latest release of a GitHub repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The tag without its leading `v`.
    pub version: String,
    /// The `westerm-plugin.tar.gz` asset, if the release has one.
    pub asset_url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    /// A link, device, FIFO, or another tar type, by its type byte.
    Other(u8),
}

/// One entry of a tar archive, as the archive names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedEntry {
    /// The name after pax `path` and GNU long names, not yet checked.
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
}

/// A package unpacked into a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unpacked {
    /// The folder with `plugin.json`: the target folder, or the single
    /// folder at the top of the archive.
    pub root: PathBuf,
    pub manifest: Manifest,
    /// The total bytes of the unpacked files.
    pub size: u64,
    /// The SHA-256 of the archive, in lowercase hex.
    pub sha256: String,
}

/// Reads a package address: a GitHub repository or a `.tar.gz` file, both
/// over `https://`.
pub fn parse_source_url(text: &str) -> Result<SourceUrl, String> {
    let refused = || "the address must be https://github.com/<owner>/<repo> or an https:// link to a .tar.gz file".to_owned();
    if text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(refused());
    }
    let rest = text.strip_prefix("https://").ok_or_else(refused)?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if host.is_empty() {
        return Err(refused());
    }
    if host.eq_ignore_ascii_case("github.com")
        && let Some((owner, repo)) = github_repo(path)
    {
        return Ok(SourceUrl::GitHub { owner, repo });
    }
    let file = path.split(['?', '#']).next().unwrap_or_default();
    if file.ends_with(".tar.gz") {
        return Ok(SourceUrl::Tarball(text.to_owned()));
    }
    Err(refused())
}

/// `<owner>/<repo>`, allowing a trailing `/` and `.git`.
fn github_repo(path: &str) -> Option<(String, String)> {
    let path = path.strip_suffix('/').unwrap_or(path);
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, repo) = path.split_once('/')?;
    (is_repo_name(owner) && is_repo_name(repo)).then(|| (owner.to_owned(), repo.to_owned()))
}

/// A GitHub owner or repository name: `[A-Za-z0-9_.-]+`, but not `.` or `..`.
fn is_repo_name(name: &str) -> bool {
    !matches!(name, "" | "." | "..") && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// A curl command for `url` with a time and size limit. Only `https://`
/// (and `file://` when `sources` allows it), also across redirects.
fn curl(url: &str, max_bytes: u64, timeout_secs: u32, sources: &Sources) -> Result<Command, String> {
    let local = sources.allow_file && url.starts_with("file://");
    if !url.starts_with("https://") && !local {
        return Err(format!("only https:// downloads are allowed: {url}"));
    }
    let protocols = if sources.allow_file { "=https,file" } else { "=https" };
    let mut command = Command::new("curl");
    command
        .args(["--fail", "--silent", "--show-error", "--location", "--proto", protocols, "--proto-redir", protocols])
        .arg("--max-time")
        .arg(timeout_secs.to_string())
        .arg("--max-filesize")
        .arg(max_bytes.to_string())
        .arg("--user-agent")
        .arg(format!("westerm/{CURRENT}"));
    Ok(command)
}

/// Runs a curl command and returns what it printed.
fn run_curl(mut command: Command, url: &str) -> Result<Vec<u8>, String> {
    let output = command.output().map_err(|e| format!("cannot run curl: {e}"))?;
    if !output.status.success() {
        return Err(format!("download failed: {url} ({})", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(output.stdout)
}

/// Downloads `url` to `dest`, at most `max_bytes` in `timeout_secs`. A
/// failed download leaves no file at `dest`.
pub fn download(url: &str, dest: &Path, max_bytes: u64, timeout_secs: u32, sources: &Sources) -> Result<(), String> {
    let result = curl(url, max_bytes, timeout_secs, sources).and_then(|mut command| {
        command.arg("--output").arg(dest).arg(url);
        run_curl(command, url)?;
        // curl cannot always tell the size before the transfer.
        let size = fs::metadata(dest).map_err(|e| format!("cannot read the download: {e}"))?.len();
        if size > max_bytes {
            return Err(format!("the download is larger than {max_bytes} bytes"));
        }
        Ok(())
    });
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result
}

/// A small JSON document: the plugin list or a GitHub API answer.
pub fn fetch_meta(url: &str, sources: &Sources) -> Result<Vec<u8>, String> {
    let mut command = curl(url, MAX_META_BYTES, META_TIMEOUT, sources)?;
    command.args(["--header", "Accept: application/vnd.github+json"]).arg(url);
    let body = run_curl(command, url)?;
    if body.len() as u64 > MAX_META_BYTES {
        return Err(format!("the answer from {url} is larger than {} MiB", MAX_META_BYTES / MIB));
    }
    Ok(body)
}

/// The latest release of `owner/repo` and its package asset.
pub fn latest_release(sources: &Sources, owner: &str, repo: &str) -> Result<Release, String> {
    if !is_repo_name(owner) || !is_repo_name(repo) {
        return Err(format!("{owner}/{repo} is not a GitHub repository"));
    }
    let url = format!("{}/repos/{owner}/{repo}/releases/latest", sources.github_api.trim_end_matches('/'));
    let body = fetch_meta(&url, sources).map_err(|e| format!("cannot read the latest release of {owner}/{repo}: {e}"))?;
    let json: Value = serde_json::from_slice(&body).map_err(|e| format!("unexpected release data: {e}"))?;
    let tag = json["tag_name"].as_str().ok_or_else(|| format!("the latest release of {owner}/{repo} has no tag"))?;
    let version = plain_version(tag).ok_or_else(|| {
        format!("the latest release of {owner}/{repo} has the tag {tag}, which is not a plain version like v1.2.3")
    })?;
    let asset_url = json["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|asset| asset["name"] == RELEASE_ASSET))
        .and_then(|asset| asset["browser_download_url"].as_str())
        .map(str::to_owned);
    Ok(Release { version, asset_url })
}

/// `x.y.z` from a tag `x.y.z` or `vx.y.z`, in the form a manifest version
/// has. A pre-release or build suffix is not a plain version.
fn plain_version(tag: &str) -> Option<String> {
    let core = tag.strip_prefix('v').unwrap_or(tag);
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if !core.split('.').all(digits) {
        return None;
    }
    let (major, minor, patch) = parse_version(core)?;
    Some(format!("{major}.{minor}.{patch}"))
}

/// Lists a `.tar.gz` the way tar reads it, without writing anything. Names
/// come from pax `path` records and GNU long names when the archive has
/// them. Fails on a damaged archive, data after the end, or a tar stream
/// larger than `MAX_TAR_BYTES`.
pub fn list_archive(archive: &Path) -> Result<Vec<ListedEntry>, String> {
    let file = fs::File::open(archive).map_err(|e| format!("cannot open the package: {e}"))?;
    // gzip and tar read every gzip member, one after another.
    let mut tar = Capped { inner: MultiGzDecoder::new(io::BufReader::new(file)), left: MAX_TAR_BYTES };
    walk(&mut tar)
}

/// Stops with `TOO_BIG` once more than `left` bytes come out.
struct Capped<R> {
    inner: R,
    left: u64,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.left = self.left.checked_sub(read as u64).ok_or_else(|| io::Error::other(TOO_BIG))?;
        Ok(read)
    }
}

fn read_error(error: io::Error) -> String {
    if error.to_string() == TOO_BIG {
        TOO_BIG.to_owned()
    } else {
        format!("the package is not a valid .tar.gz ({error})")
    }
}

/// What a pax `x` header or a GNU `L` header says about the next entry.
#[derive(Default)]
struct Extended {
    path: Option<String>,
    size: Option<u64>,
}

fn walk(tar: &mut impl Read) -> Result<Vec<ListedEntry>, String> {
    let mut entries = Vec::new();
    let mut next: Option<Extended> = None;
    let mut block = [0u8; 512];
    while read_block(tar, &mut block)? {
        if block.iter().all(|&b| b == 0) {
            rest_is_zero(tar)?;
            break;
        }
        check_sum(&block)?;
        let kind = block[156];
        let header_size = octal(&block[124..136])?;
        if matches!(kind, b'x' | b'g' | b'L') {
            // Real archives never stack these; tar programs disagree on what it means.
            if next.is_some() {
                return Err("the package has two extended headers for one entry".into());
            }
            let data = read_data(tar, header_size)?;
            match kind {
                b'x' => next = Some(pax_records(&data, false)?),
                b'g' => {
                    pax_records(&data, true)?;
                }
                _ => {
                    let name = utf8(data.split(|&b| b == 0).next().unwrap_or_default())?;
                    next = Some(Extended { path: Some(name), size: None });
                }
            }
            continue;
        }
        let extended = next.take().unwrap_or_default();
        let size = extended.size.unwrap_or(header_size);
        let path = match extended.path {
            Some(path) => path,
            None => ustar_name(&block)?,
        };
        skip(tar, size)?;
        let kind = match kind {
            b'0' | 0 => EntryKind::File,
            b'5' => EntryKind::Dir,
            other => EntryKind::Other(other),
        };
        entries.push(ListedEntry { path, kind, size });
    }
    if next.is_some() {
        return Err("the package ends after an extended header".into());
    }
    Ok(entries)
}

/// Fills `block`; `false` at the end of the stream.
fn read_block(tar: &mut impl Read, block: &mut [u8; 512]) -> Result<bool, String> {
    let mut filled = 0;
    while filled < block.len() {
        match tar.read(&mut block[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err("the package ends inside a header".into()),
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(read_error(error)),
        }
    }
    Ok(true)
}

/// After the end-of-archive block only zero padding may follow, so tar
/// cannot find entries this listing did not see.
fn rest_is_zero(tar: &mut impl Read) -> Result<(), String> {
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match tar.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) if buffer[..read].iter().any(|&b| b != 0) => {
                return Err("the package has data after the end of the archive".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(read_error(error)),
        }
    }
}

/// The data of an entry and its padding to 512 bytes.
fn padded(size: u64) -> Result<u64, String> {
    if size > MAX_TAR_BYTES {
        return Err(TOO_BIG.into());
    }
    Ok(size.next_multiple_of(512))
}

fn read_data(tar: &mut impl Read, size: u64) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    let read = tar.take(padded(size)?).read_to_end(&mut data).map_err(read_error)?;
    if (read as u64) < padded(size)? {
        return Err("the package ends inside an entry".into());
    }
    data.truncate(size as usize);
    Ok(data)
}

fn skip(tar: &mut impl Read, size: u64) -> Result<(), String> {
    let padded = padded(size)?;
    let skipped = io::copy(&mut tar.take(padded), &mut io::sink()).map_err(read_error)?;
    if skipped < padded {
        return Err("the package ends inside an entry".into());
    }
    Ok(())
}

/// The header checksum, summed as unsigned or (old tars) signed bytes. tar
/// programs resync differently after a bad header, so it is refused.
fn check_sum(block: &[u8; 512]) -> Result<(), String> {
    let stored = octal(&block[148..156])?;
    let field = 148..156;
    let unsigned: u64 = block.iter().enumerate().map(|(i, &b)| if field.contains(&i) { 32 } else { u64::from(b) }).sum();
    let signed: i64 = block.iter().enumerate().map(|(i, &b)| if field.contains(&i) { 32 } else { i64::from(b as i8) }).sum();
    if stored == unsigned || i64::try_from(stored) == Ok(signed) {
        Ok(())
    } else {
        Err("the package has a damaged tar header".into())
    }
}

/// An octal number field: digits between optional spaces, ended by NUL or a
/// space. Base-256 numbers only appear for sizes far over the limits.
fn octal(field: &[u8]) -> Result<u64, String> {
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        return Err(TOO_BIG.into());
    }
    let text = field.split(|&b| b == 0).next().unwrap_or_default();
    let text = std::str::from_utf8(text).map_err(|_| "the package has a damaged tar header")?.trim_matches(' ');
    if text.is_empty() {
        return Ok(0);
    }
    if !text.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        return Err("the package has a damaged tar header".into());
    }
    u64::from_str_radix(text, 8).map_err(|_| "the package has a damaged tar header".into())
}

/// `prefix/name` of a POSIX ustar header, or `name` of a GNU or old header.
/// libarchive reads the prefix of any magic that starts with `ustar` except
/// the exact GNU `ustar  \0`, while GNU tar reads it only after `ustar\0`.
/// Other `ustar` magics are refused, so both read the same name.
fn ustar_name(block: &[u8; 512]) -> Result<String, String> {
    let c_text = |bytes: &[u8]| utf8(bytes.split(|&b| b == 0).next().unwrap_or_default());
    let name = c_text(&block[..100])?;
    let magic = &block[257..265];
    let posix = &magic[..6] == b"ustar\0";
    if &magic[..5] == b"ustar" && !posix && magic != b"ustar  \0" {
        return Err("the package has a tar header with an unusual magic".into());
    }
    if !posix {
        return Ok(name);
    }
    let prefix = c_text(&block[345..500])?;
    // star archives keep times in the last bytes of the prefix field, and
    // GNU tar reads them that way; a longer prefix would read differently.
    if prefix.len() > 130 {
        return Err("the package has a tar header with an unusual name prefix".into());
    }
    Ok(if prefix.is_empty() { name } else { format!("{prefix}/{name}") })
}

fn utf8(bytes: &[u8]) -> Result<String, String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| "the package has a file name that is not UTF-8".into())
}

/// The records of a pax header: `<length> <key>=<value>\n`. `path` and
/// `size` change the next entry. A global (`g`) header may not set them;
/// GNU tar would apply them to every entry and libarchive to none. Sparse
/// files are refused: their real names and data layout live in these records.
fn pax_records(data: &[u8], global: bool) -> Result<Extended, String> {
    let damaged = || "the package has a damaged pax header".to_owned();
    let mut extended = Extended::default();
    let mut rest = data;
    while !rest.is_empty() {
        let space = rest.iter().position(|&b| b == b' ').ok_or_else(damaged)?;
        let digits = &rest[..space];
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return Err(damaged());
        }
        let length: usize = std::str::from_utf8(digits).ok().and_then(|text| text.parse().ok()).ok_or_else(damaged)?;
        if length <= space + 1 || length > rest.len() || rest[length - 1] != b'\n' {
            return Err(damaged());
        }
        let record = &rest[space + 1..length - 1];
        let equals = record.iter().position(|&b| b == b'=').ok_or_else(damaged)?;
        let (key, value) = (&record[..equals], &record[equals + 1..]);
        match key {
            b"path" | b"size" if global => return Err("the package has a global pax header that renames entries".into()),
            b"path" => extended.path = Some(utf8(value)?),
            b"size" => {
                let text = std::str::from_utf8(value).map_err(|_| damaged())?;
                if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(damaged());
                }
                extended.size = Some(text.parse().map_err(|_| TOO_BIG.to_owned())?);
            }
            _ if key.starts_with(b"GNU.sparse.") || key == b"SCHILY.realsize" || key == b"SUN.holesdata" => {
                return Err("the package has a sparse file".into());
            }
            _ => {}
        }
        rest = &rest[length..];
    }
    Ok(extended)
}

/// Checks a listing against the package rules: plain files and folders
/// only, safe relative names, at most `MAX_ENTRIES` files and folders
/// (counting the folders tar makes for nested names), and at most
/// `MAX_UNPACKED_BYTES` of file data.
fn check_listing(entries: &[ListedEntry]) -> Result<(), String> {
    let mut nodes = BTreeSet::new();
    let mut total: u64 = 0;
    for entry in entries {
        match entry.kind {
            EntryKind::File => total = total.saturating_add(entry.size),
            EntryKind::Dir if entry.size > 0 => return Err(format!("the package has a folder with data: {:?}", entry.path)),
            EntryKind::Dir => {}
            EntryKind::Other(kind) => {
                return Err(format!(
                    "the package may only have files and folders: {:?} has tar type {:?}",
                    entry.path,
                    char::from(kind)
                ));
            }
        }
        let mut path = safe_path(entry)?;
        while !path.is_empty() {
            nodes.insert(path.clone());
            path.truncate(path.rfind('/').unwrap_or(0));
        }
        if nodes.len() > MAX_ENTRIES {
            return Err(format!("the package has more than {MAX_ENTRIES} files and folders"));
        }
        if total > MAX_UNPACKED_BYTES {
            return Err(format!("the package is larger than {} MiB unpacked", MAX_UNPACKED_BYTES / MIB));
        }
    }
    Ok(())
}

/// The name of an entry without `.` and empty parts. Refused: an empty or
/// absolute name, a `..` part, a `\`, `:` (a drive or stream on Windows), or
/// a control character, and a file whose name ends with `/`.
fn safe_path(entry: &ListedEntry) -> Result<String, String> {
    let raw = entry.path.as_str();
    let parts: Vec<&str> = raw.split('/').filter(|part| !matches!(*part, "" | ".")).collect();
    let unsafe_name = raw.is_empty()
        || raw.starts_with('/')
        || raw.chars().any(|c| matches!(c, '\\' | ':') || c.is_control())
        || parts.contains(&"..")
        || (entry.kind == EntryKind::File && (parts.is_empty() || raw.ends_with('/')));
    if unsafe_name {
        return Err(format!("the package has an unsafe name: {raw:?}"));
    }
    Ok(parts.join("/"))
}

/// Unpacks `archive` into `into`, which must be missing or empty. The
/// listing is checked before tar runs and the written files after, and
/// `plugin.json` is read from the package root. On any failure `into` is
/// removed.
pub fn unpack(archive: &Path, into: &Path, current: &str) -> Result<Unpacked, String> {
    check_listing(&list_archive(archive)?)?;
    match fs::read_dir(into).map(|mut items| items.next().is_some()) {
        Ok(false) => {}
        Ok(true) => return Err(format!("{} is not empty", into.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot use {}: {error}", into.display())),
    }
    let result = extract(archive, into, current);
    if result.is_err() {
        open_up(into);
        let _ = fs::remove_dir_all(into);
    }
    result
}

fn extract(archive: &Path, into: &Path, current: &str) -> Result<Unpacked, String> {
    let sha256 = sha256_hex(archive).map_err(|e| format!("cannot read the package: {e}"))?;
    fs::create_dir_all(into).map_err(|e| format!("cannot create {}: {e}", into.display()))?;
    let output = Command::new("tar")
        .args(["--no-same-owner", "--no-same-permissions", "-xzf"])
        .arg(archive)
        .arg("-C")
        .arg(into)
        .output()
        .map_err(|e| format!("cannot run tar: {e}"))?;
    if !output.status.success() {
        return Err(format!("cannot unpack the package ({})", String::from_utf8_lossy(&output.stderr).trim()));
    }
    open_up(into);
    let size = check_tree(into)?;
    let root = package_root(into)?;
    let manifest = manifest::read(&root, current)?;
    Ok(Unpacked { root, manifest, size, sha256 })
}

/// Gives the folders tar wrote mode 755 and the files 644, so a mode from
/// the archive (a folder no one may enter, a setuid file) cannot stop the
/// checks, the reads, or a later removal. Links and other entries are left
/// alone; `check_tree` refuses them.
fn open_up(into: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut folders = vec![into.to_path_buf()];
        while let Some(folder) = folders.pop() {
            let _ = fs::set_permissions(&folder, fs::Permissions::from_mode(0o755));
            let Ok(items) = fs::read_dir(&folder) else { continue };
            for path in items.flatten().map(|item| item.path()) {
                match fs::symlink_metadata(&path) {
                    Ok(meta) if meta.is_dir() => folders.push(path),
                    Ok(meta) if meta.is_file() => {
                        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o644));
                    }
                    _ => {}
                }
            }
        }
    }
    #[cfg(not(unix))]
    let _ = into;
}

/// Walks what tar wrote, without following links: plain files and folders
/// only, within the same limits as the listing. tar may have written fewer
/// files than listed (macOS tar turns `._*` files into attributes), so the
/// two are not compared one to one. Returns the total file bytes.
fn check_tree(into: &Path) -> Result<u64, String> {
    let mut count = 0;
    let mut total: u64 = 0;
    let mut folders = vec![into.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let items = fs::read_dir(&folder).map_err(|e| format!("cannot read {}: {e}", folder.display()))?;
        for item in items {
            let path = item.map_err(|e| format!("cannot read {}: {e}", folder.display()))?.path();
            let meta = fs::symlink_metadata(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            count += 1;
            if meta.is_dir() {
                folders.push(path);
            } else if meta.is_file() {
                total += meta.len();
            } else {
                return Err(format!("the package may only have files and folders: {}", path.display()));
            }
            if count > MAX_ENTRIES {
                return Err(format!("the package has more than {MAX_ENTRIES} files and folders"));
            }
            if total > MAX_UNPACKED_BYTES {
                return Err(format!("the package is larger than {} MiB unpacked", MAX_UNPACKED_BYTES / MIB));
            }
        }
    }
    Ok(total)
}

/// The single folder at the top of the package, or `into` itself. AppleDouble
/// `._*` files that macOS tar adds beside the folder do not count.
fn package_root(into: &Path) -> Result<PathBuf, String> {
    let items = fs::read_dir(into).map_err(|e| format!("cannot read {}: {e}", into.display()))?;
    let mut top = Vec::new();
    for item in items {
        let item = item.map_err(|e| format!("cannot read {}: {e}", into.display()))?;
        if !item.file_name().to_string_lossy().starts_with("._") {
            top.push(item.path());
        }
    }
    Ok(match top.as_slice() {
        [only] if fs::symlink_metadata(only).is_ok_and(|meta| meta.is_dir()) => only.clone(),
        _ => into.to_path_buf(),
    })
}

/// Reads a local development folder in place; it is never copied. Unlike a
/// package it has no size, count, or link limits: it is the developer's own
/// folder. Reads from it stay inside it later, where the asset route
/// resolves each path and refuses anything outside the folder.
pub fn read_folder(path: &Path, current: &str) -> Result<Manifest, String> {
    if !path.is_absolute() {
        return Err("the folder must be an absolute path".into());
    }
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    manifest::read(path, current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flate2::write::GzEncoder;
    use flate2::Compression;

    const CURRENT: &str = "0.4.0";
    const PLUGIN_JSON: &str = r#"{"id":"k8s-tools.kubernetes","name":"Kubernetes","version":"0.3.1","panel":"panel.html"}"#;
    const MIB: usize = 1024 * 1024;
    /// An AppleDouble file with no entries, like the `._*` files macOS tar adds.
    const APPLE_DOUBLE: [u8; 26] = [0, 5, 22, 7, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    /// One tar header and its data, written by `tar_bytes`.
    #[derive(Clone, Debug)]
    struct Entry {
        name: String,
        kind: u8,
        data: Vec<u8>,
        link: String,
        mode: &'static str,
        /// The 8 bytes of magic and version at offset 257.
        magic: &'static [u8; 8],
        /// The ustar name prefix at offset 345.
        prefix: &'static str,
    }

    fn entry(name: &str, kind: u8, data: &[u8], link: &str) -> Entry {
        let mode = if kind == b'5' { "0000755" } else { "0000644" };
        Entry { name: name.into(), kind, data: data.to_vec(), link: link.into(), mode, magic: b"ustar\x0000", prefix: "" }
    }
    fn file(name: &str) -> Entry {
        entry(name, b'0', b"", "")
    }
    fn file_with(name: &str, data: &[u8]) -> Entry {
        entry(name, b'0', data, "")
    }
    fn dir(name: &str) -> Entry {
        entry(name, b'5', b"", "")
    }
    fn link(name: &str, target: &str) -> Entry {
        entry(name, b'2', b"", target)
    }
    fn hardlink(name: &str, target: &str) -> Entry {
        entry(name, b'1', b"", target)
    }
    fn typed(name: &str, kind: u8) -> Entry {
        entry(name, kind, b"", "")
    }
    /// A pax header (`x` for the next entry, `g` for all) with these records.
    fn pax(kind: u8, records: &[(&str, &str)]) -> Entry {
        let mut data = Vec::new();
        for (key, value) in records {
            let body = format!(" {key}={value}\n");
            let mut length = body.len() + 1;
            while (length.to_string().len() + body.len()) != length {
                length += 1;
            }
            data.extend_from_slice(format!("{length}{body}").as_bytes());
        }
        entry("PaxHeader/entry", kind, &data, "")
    }
    /// A GNU `L` header that names the next entry.
    fn long_name(name: &str) -> Entry {
        entry("././@LongLink", b'L', format!("{name}\0").as_bytes(), "")
    }

    fn header(entry: &Entry) -> [u8; 512] {
        assert!(entry.name.len() <= 100 && entry.link.len() <= 100, "{entry:?}");
        let mut block = [0u8; 512];
        let mut put = |at: usize, bytes: &[u8]| block[at..at + bytes.len()].copy_from_slice(bytes);
        put(0, entry.name.as_bytes());
        put(100, entry.mode.as_bytes());
        put(108, b"0000000\0");
        put(116, b"0000000\0");
        put(124, format!("{:011o}\0", entry.data.len()).as_bytes());
        put(136, b"00000000000\0");
        put(148, b"        ");
        put(156, &[entry.kind]);
        put(157, entry.link.as_bytes());
        put(257, entry.magic);
        put(345, entry.prefix.as_bytes());
        let sum: u32 = block.iter().map(|&b| u32::from(b)).sum();
        block[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        block
    }

    /// Whether the checks before tar refuse `archive`.
    fn refused_before_tar(archive: &Path) -> bool {
        list_archive(archive).and_then(|listed| check_listing(&listed)).is_err()
    }

    fn tar_bytes(entries: &[Entry]) -> Vec<u8> {
        let mut out = Vec::new();
        for entry in entries {
            out.extend_from_slice(&header(entry));
            out.extend_from_slice(&entry.data);
            out.resize(out.len().next_multiple_of(512), 0);
        }
        out.resize(out.len() + 1024, 0);
        out
    }

    /// A temporary folder for archives and unpacked packages.
    struct Scratch(tempfile::TempDir);

    impl Scratch {
        fn new() -> Self {
            Self(tempfile::tempdir().unwrap())
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.path().join(name)
        }
        fn gz(&self, entries: &[Entry]) -> PathBuf {
            self.gz_bytes(&tar_bytes(entries))
        }
        fn gz_bytes(&self, tar: &[u8]) -> PathBuf {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = self.path(&format!("archive-{}.tar.gz", NEXT.fetch_add(1, Ordering::Relaxed)));
            let mut encoder = GzEncoder::new(fs::File::create(&path).unwrap(), Compression::fast());
            encoder.write_all(tar).unwrap();
            encoder.finish().unwrap();
            path
        }
    }

    fn package() -> Vec<Entry> {
        vec![file_with("plugin.json", PLUGIN_JSON.as_bytes()), file_with("panel.html", b"<p>hi</p>")]
    }

    #[cfg(unix)]
    fn file_url(path: &Path) -> String {
        format!("file://{}", path.display())
    }

    /// The system tar packs `names` from `folder`.
    #[cfg(unix)]
    fn system_tar(archive: &Path, folder: &Path, names: &[&str]) {
        let packed = std::process::Command::new("tar").arg("-czf").arg(archive).arg("-C").arg(folder).args(names).status();
        assert!(packed.unwrap().success());
    }

    #[test]
    fn source_urls_are_https_github_or_tarballs() {
        assert_eq!(
            parse_source_url("https://github.com/k8s-tools/westerm-k8s").unwrap(),
            SourceUrl::GitHub { owner: "k8s-tools".into(), repo: "westerm-k8s".into() }
        );
        assert_eq!(
            parse_source_url("https://github.com/o/r.git/").unwrap(),
            SourceUrl::GitHub { owner: "o".into(), repo: "r".into() }
        );
        assert!(matches!(parse_source_url("https://github.com/o/r/"), Ok(SourceUrl::GitHub { .. })));
        assert_eq!(
            parse_source_url("https://example.com/p/westerm-plugin.tar.gz").unwrap(),
            SourceUrl::Tarball("https://example.com/p/westerm-plugin.tar.gz".into())
        );
        assert!(matches!(
            parse_source_url("https://github.com/o/r/releases/download/v1.0.0/westerm-plugin.tar.gz"),
            Ok(SourceUrl::Tarball(_))
        ));
        for bad in [
            "http://example.com/p.tar.gz",
            "file:///tmp/p.tar.gz",
            "https://example.com/p.zip",
            "https://github.com/o",
            "https://github.com/o/r/tree/main",
            "https://github.com/o/r$x",
            "https://github.com/../r",
            "https://example.com/p.tar.gz x",
            "https:///p.tar.gz",
            "",
        ] {
            assert!(parse_source_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn bad_entries_are_refused_before_extracting() {
        let scratch = Scratch::new();
        let into = scratch.path("out");
        let long = "a/".repeat(60);
        for bad in [
            file("../x"),
            file("/etc/x"),
            file("a/../../x"),
            file(r"a\b"),
            file("a:b"),
            file("a\u{1}b"),
            file("dir-as-file/"),
            link("a", "/etc"),
            hardlink("a", "b"),
            typed("dev", b'3'),
            typed("fifo", b'6'),
            typed("sparse", b'S'),
            entry("dir-with-data", b'5', b"x", ""),
            // bsdtar applies the prefix of any "ustar" magic, GNU tar only of
            // "ustar\0"; only the forms both read the same way pass.
            Entry { magic: b"ustarX00", ..file("plugin.json") },
            Entry { magic: b"ustar X\0", ..file("plugin.json") },
            Entry { magic: b"ustarX00", prefix: "../../escape", ..file("x") },
            Entry { magic: b"ustar X\0", prefix: "/tmp/x", ..file("x") },
            Entry { prefix: "../../escape", ..file("x") },
            Entry { prefix: "/tmp/x", ..file("x") },
        ] {
            let archive = scratch.gz(&[package(), vec![bad.clone()]].concat());
            assert!(refused_before_tar(&archive), "{bad:?}");
            assert!(unpack(&archive, &into, CURRENT).is_err(), "{bad:?}");
            assert!(!into.exists(), "nothing extracted for {bad:?}");
        }
        for bad in [
            vec![pax(b'x', &[("path", "../x")]), file("x")],
            vec![pax(b'x', &[("GNU.sparse.name", "x")]), file("x")],
            vec![pax(b'g', &[("path", "x")]), file("x")],
            vec![long_name(&format!("{long}../x")), file("x")],
            vec![long_name("x"), pax(b'x', &[("path", "y")]), file("x")],
            vec![pax(b'x', &[("path", "x")])],
        ] {
            let archive = scratch.gz(&[package(), bad.clone()].concat());
            assert!(refused_before_tar(&archive), "{bad:?}");
            assert!(unpack(&archive, &into, CURRENT).is_err(), "{bad:?}");
            assert!(!into.exists(), "nothing extracted for {bad:?}");
        }
    }

    #[test]
    fn broken_archives_are_refused() {
        let scratch = Scratch::new();
        let mut bad_sum = tar_bytes(&package());
        bad_sum[0] = b'q';
        let mut after_end = tar_bytes(&package());
        after_end.extend_from_slice(&tar_bytes(&[link("evil", "/etc")]));
        let not_gzip = scratch.path("plain.tar.gz");
        fs::write(&not_gzip, tar_bytes(&package())).unwrap();
        for archive in [scratch.gz_bytes(&bad_sum), scratch.gz_bytes(&after_end), not_gzip] {
            assert!(list_archive(&archive).is_err(), "{}", archive.display());
        }
        // A second gzip member is read the way gzip and tar read it.
        let two_members = scratch.path("two.tar.gz");
        let mut first = tar_bytes(&package());
        first.truncate(first.len() - 1024);
        let second = scratch.gz(&[link("evil", "/etc")]);
        fs::write(&two_members, [fs::read(scratch.gz_bytes(&first)).unwrap(), fs::read(second).unwrap()].concat()).unwrap();
        let listed = list_archive(&two_members).unwrap();
        assert_eq!(listed.last().map(|entry| (entry.path.as_str(), entry.kind)), Some(("evil", EntryKind::Other(b'2'))));
        assert!(unpack(&two_members, &scratch.path("out"), CURRENT).is_err());
        assert!(!scratch.path("out").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_plain_package_unpacks() {
        let scratch = Scratch::new();
        let stage = scratch.path("stage");
        fs::create_dir_all(stage.join("js")).unwrap();
        let manifest = PLUGIN_JSON.replace(r#""panel""#, r#""icon":"icon.svg","panel""#);
        fs::write(stage.join("plugin.json"), manifest).unwrap();
        fs::write(stage.join("panel.html"), "<p>hi</p>").unwrap();
        fs::write(stage.join("icon.svg"), "<svg/>").unwrap();
        fs::write(stage.join("js/app.js"), "1").unwrap();
        for names in [&["plugin.json", "panel.html", "icon.svg", "js"][..], &["."][..]] {
            let archive = scratch.path("plain.tar.gz");
            system_tar(&archive, &stage, names);
            let into = scratch.path("plain");
            let unpacked = unpack(&archive, &into, CURRENT).unwrap();
            assert_eq!(unpacked.root, into);
            assert_eq!(unpacked.manifest.id, "k8s-tools.kubernetes");
            assert_eq!(unpacked.manifest.icon.as_deref(), Some("icon.svg"));
            assert_eq!(unpacked.sha256, crate::update::sha256_hex(&archive).unwrap());
            assert!(into.join("js/app.js").is_file());
            assert!(unpacked.size >= (PLUGIN_JSON.len() + 9 + 6 + 1) as u64);
            fs::remove_dir_all(&into).unwrap();
        }
    }

    #[test]
    fn a_single_top_folder_is_the_package() {
        let scratch = Scratch::new();
        let into = scratch.path("out");
        let archive = scratch.gz(&[
            file_with("._pkg", &APPLE_DOUBLE),
            file_with("pkg/plugin.json", PLUGIN_JSON.as_bytes()),
            file_with("pkg/panel.html", b"<p>hi</p>"),
        ]);
        let unpacked = unpack(&archive, &into, CURRENT).unwrap();
        assert_eq!(unpacked.root, into.join("pkg"));
        assert_eq!(unpacked.manifest.id, "k8s-tools.kubernetes");

        // A folder with a file beside it is not the package.
        let into = scratch.path("beside");
        let archive = scratch.gz(&[file_with("README", b"x"), file_with("pkg/plugin.json", PLUGIN_JSON.as_bytes())]);
        assert!(unpack(&archive, &into, CURRENT).is_err());
        assert!(!into.exists(), "a failed unpack leaves nothing behind");
    }

    #[test]
    fn modes_from_the_archive_do_not_lock_the_package() {
        let scratch = Scratch::new();
        let locked = |name: &str| Entry { mode: "0000000", ..dir(name) };
        let into = scratch.path("locked");
        let archive = scratch.gz(&[
            locked("pkg/"),
            file_with("pkg/plugin.json", PLUGIN_JSON.as_bytes()),
            file_with("pkg/panel.html", b"<p>hi</p>"),
            Entry { mode: "0004000", ..file_with("pkg/run", b"x") },
        ]);
        let unpacked = unpack(&archive, &into, CURRENT).unwrap();
        assert_eq!(unpacked.manifest.id, "k8s-tools.kubernetes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(into.join("pkg/run")).unwrap().permissions().mode() & 0o7777, 0o644);
        }

        // A failed unpack still removes a folder it could not enter.
        let into = scratch.path("failed");
        let archive = scratch.gz(&[locked("pkg/"), locked("pkg/deep/"), file_with("pkg/deep/x", b"x"), file("README")]);
        assert!(unpack(&archive, &into, CURRENT).is_err());
        assert!(!into.exists());
    }

    #[test]
    fn an_unpack_never_writes_into_a_full_folder() {
        let scratch = Scratch::new();
        let into = scratch.path("full");
        fs::create_dir_all(&into).unwrap();
        fs::write(into.join("keep"), "mine").unwrap();
        assert!(unpack(&scratch.gz(&package()), &into, CURRENT).is_err());
        assert_eq!(fs::read_to_string(into.join("keep")).unwrap(), "mine");
    }

    #[test]
    fn size_and_count_limits() {
        let scratch = Scratch::new();
        let into = scratch.path("out");
        let fixed = (PLUGIN_JSON.len() + "<p>hi</p>".len()) as u64;

        let mut many = package();
        many.extend((0..MAX_ENTRIES - 1).map(|i| file(&format!("f{i}"))));
        let archive = scratch.gz(&many);
        assert!(refused_before_tar(&archive));
        let error = unpack(&archive, &into, CURRENT).unwrap_err();
        assert!(error.contains("more than 300"), "{error}");
        assert!(!into.exists());

        // Folders that tar would make count too.
        let mut nested = package();
        nested.extend((0..150).map(|i| file(&format!("d{i}/f"))));
        let archive = scratch.gz(&nested);
        assert!(refused_before_tar(&archive));
        assert!(unpack(&archive, &into, CURRENT).unwrap_err().contains("more than 300"));

        let too_big = vec![b'x'; (MAX_UNPACKED_BYTES - fixed + 1) as usize];
        let archive = scratch.gz(&[package(), vec![file_with("big", &too_big)]].concat());
        assert!(refused_before_tar(&archive));
        let error = unpack(&archive, &into, CURRENT).unwrap_err();
        assert!(error.contains("larger than 10 MiB"), "{error}");
        assert!(!into.exists());

        let mut bomb = tar_bytes(&package());
        bomb.resize(12 * MIB, 0);
        let archive = scratch.gz_bytes(&bomb);
        assert!(refused_before_tar(&archive));
        let error = list_archive(&archive).unwrap_err();
        assert!(error.contains("more than 11 MiB"), "{error}");

        let mut full = package();
        full.extend((0..MAX_ENTRIES - 3).map(|i| file(&format!("f{i}"))));
        full.push(file_with("big", &vec![b'x'; (MAX_UNPACKED_BYTES - fixed) as usize]));
        assert_eq!(full.len(), MAX_ENTRIES);
        let unpacked = unpack(&scratch.gz(&full), &into, CURRENT).unwrap();
        assert_eq!(unpacked.size, MAX_UNPACKED_BYTES);
        assert_eq!(unpacked.root, into);
    }

    /// GNU magic keeps times where ustar keeps the prefix, so its prefix
    /// bytes are not part of the name; `ustar\0` with any version uses them.
    #[test]
    fn accepted_tar_magics_read_like_tar() {
        let scratch = Scratch::new();
        let archive = scratch.gz(&[
            Entry { magic: b"ustar  \0", prefix: "ignored", ..file_with("plugin.json", PLUGIN_JSON.as_bytes()) },
            Entry { magic: b"ustar\0xx", prefix: "sub", ..file_with("panel.html", b"<p>hi</p>") },
        ]);
        let listed = list_archive(&archive).unwrap();
        let paths: Vec<&str> = listed.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, ["plugin.json", "sub/panel.html"]);
        assert!(!refused_before_tar(&archive));
        #[cfg(unix)]
        {
            let listing = std::process::Command::new("tar").arg("-tzf").arg(&archive).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&listing.stdout).lines().collect::<Vec<_>>(), paths, "the system tar agrees");
        }
    }

    /// The headers macOS tar (bsdtar) and GNU tar write: pax `x` records with
    /// xattrs, `._*` AppleDouble files, a `g` header like `git archive`'s, and
    /// a GNU `L` long name.
    #[test]
    fn macos_and_gnu_tar_archives_install() {
        let scratch = Scratch::new();
        let into = scratch.path("out");
        let long = format!("pkg/{}.js", "a".repeat(120));
        let archive = scratch.gz(&[
            pax(b'g', &[("comment", "0123456789abcdef")]),
            dir("pkg/"),
            pax(b'x', &[("path", "pkg/plugin.json"), ("LIBARCHIVE.xattr.com.apple.provenance", "AQAAAAAAAAA=")]),
            file_with("pkg/plugin.js", PLUGIN_JSON.as_bytes()),
            file_with("pkg/._panel.html", &APPLE_DOUBLE),
            file_with("pkg/panel.html", b"<p>hi</p>"),
            long_name(&long),
            file_with(&long[..100], b"console.log(1)"),
        ]);
        let listed = list_archive(&archive).unwrap();
        assert!(!refused_before_tar(&archive));
        let paths: Vec<&str> = listed.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, ["pkg/", "pkg/plugin.json", "pkg/._panel.html", "pkg/panel.html", long.as_str()]);
        assert_eq!(listed[0].kind, EntryKind::Dir);
        let size = PLUGIN_JSON.len() as u64;
        let plugin_json = ListedEntry { path: "pkg/plugin.json".into(), kind: EntryKind::File, size };
        assert_eq!(listed[1], plugin_json);

        let unpacked = unpack(&archive, &into, CURRENT).unwrap();
        assert_eq!(unpacked.root, into.join("pkg"));
        assert_eq!(unpacked.manifest.id, "k8s-tools.kubernetes");
        assert!(into.join(&long).is_file());
        assert!(!into.join("pkg/plugin.js").exists());
    }

    /// What tar on this computer writes for a folder with extended
    /// attributes and a long file name.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_system_tar_of_a_folder_with_xattrs_installs() {
        let scratch = Scratch::new();
        let pkg = scratch.path("stage/pkg");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("plugin.json"), PLUGIN_JSON).unwrap();
        fs::write(pkg.join("panel.html"), "<p>hi</p>").unwrap();
        let long = format!("{}.js", "b".repeat(120));
        fs::write(pkg.join(&long), "1").unwrap();
        let mut xattr = std::process::Command::new("xattr");
        let tagged = xattr.args(["-w", "com.westerm.test", "yes"]).arg(pkg.join("panel.html")).status();
        assert!(tagged.unwrap().success());
        let archive = scratch.path("mac.tar.gz");
        system_tar(&archive, &scratch.path("stage"), &["pkg"]);
        let unpacked = unpack(&archive, &scratch.path("out"), CURRENT).unwrap();
        assert_eq!(unpacked.root, scratch.path("out/pkg"));
        assert!(unpacked.root.join(&long).is_file());
    }

    #[test]
    fn a_folder_is_read_in_place() {
        let scratch = Scratch::new();
        let folder = scratch.path("dev");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("plugin.json"), PLUGIN_JSON).unwrap();
        fs::write(folder.join("panel.html"), "<p>hi</p>").unwrap();
        assert_eq!(read_folder(&folder, CURRENT).unwrap().id, "k8s-tools.kubernetes");
        assert!(read_folder(Path::new("dev"), CURRENT).is_err(), "a relative path");
        assert!(read_folder(&scratch.path("missing"), CURRENT).is_err());
        assert!(read_folder(&folder.join("plugin.json"), CURRENT).is_err(), "a file");
    }

    #[cfg(unix)]
    #[test]
    fn downloads_keep_to_their_limit() {
        let scratch = Scratch::new();
        let local = Sources { allow_file: true, ..Sources::default() };
        let source = scratch.path("source.tar.gz");
        let dest = scratch.path("dest.tar.gz");

        fs::write(&source, vec![0u8; MAX_PACKAGE_BYTES as usize + 1]).unwrap();
        assert!(download(&file_url(&source), &dest, MAX_PACKAGE_BYTES, PACKAGE_TIMEOUT, &local).is_err());
        assert!(!dest.exists(), "a refused download leaves no file");

        fs::write(&source, vec![0u8; MAX_PACKAGE_BYTES as usize]).unwrap();
        download(&file_url(&source), &dest, MAX_PACKAGE_BYTES, PACKAGE_TIMEOUT, &local).unwrap();
        assert_eq!(fs::metadata(&dest).unwrap().len(), MAX_PACKAGE_BYTES);
        fs::remove_file(&dest).unwrap();

        assert!(download(&file_url(&source), &dest, MAX_PACKAGE_BYTES, PACKAGE_TIMEOUT, &Sources::default()).is_err());
        assert!(download("http://127.0.0.1:9/p.tar.gz", &dest, MAX_PACKAGE_BYTES, PACKAGE_TIMEOUT, &local).is_err());
        assert!(!dest.exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_latest_release_asset_is_found() {
        let scratch = Scratch::new();
        let api = scratch.path("api");
        let release = |repo: &str, body: &str| {
            let folder = api.join(format!("repos/o/{repo}/releases"));
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("latest"), body).unwrap();
        };
        release(
            "r",
            r#"{"tag_name":"v0.4.0","assets":[{"name":"other.zip","browser_download_url":"file:///x/other.zip"},
                {"name":"westerm-plugin.tar.gz","browser_download_url":"file:///x/westerm-plugin.tar.gz"}]}"#,
        );
        release("bare", r#"{"tag_name":"1.2.3","assets":[{"name":"other.zip","browser_download_url":"file:///x/o.zip"}]}"#);
        release("odd", r#"{"tag_name":"latest","assets":[]}"#);
        let tags =
            [("v123", "v1.2.3"), ("beta", "v1.2.3-beta"), ("build", "1.2.3+build"), ("vv", "vv1.2.3"), ("four", "1.2.3.4")];
        for (repo, tag) in tags {
            release(repo, &format!(r#"{{"tag_name":"{tag}","assets":[]}}"#));
        }
        let sources = Sources { github_api: file_url(&api), allow_file: true, ..Sources::default() };

        assert_eq!(
            latest_release(&sources, "o", "r").unwrap(),
            Release { version: "0.4.0".into(), asset_url: Some("file:///x/westerm-plugin.tar.gz".into()) }
        );
        assert_eq!(latest_release(&sources, "o", "bare").unwrap(), Release { version: "1.2.3".into(), asset_url: None });
        assert!(latest_release(&sources, "o", "odd").is_err());
        assert_eq!(latest_release(&sources, "o", "v123").unwrap().version, "1.2.3");
        for repo in ["beta", "build", "vv", "four"] {
            let error = latest_release(&sources, "o", repo).unwrap_err();
            assert!(error.contains("not a plain version"), "{repo}: {error}");
        }
        assert!(latest_release(&sources, "o", "missing").is_err());
        assert!(latest_release(&sources, "o", "..").is_err());
    }
}
