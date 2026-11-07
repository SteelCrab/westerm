//! The dashboard the windows show: a westerm that already serves on the port
//! (one started in a terminal, so both share the same sessions), or one this
//! app starts from the westerm executable installed next to it.
//!
//! Quitting stops only a server the app started. The PTY host runs in its
//! own session, so shells survive as they do with the CLI.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 4317;
/// How long a new server may take to print its address.
const START_TIMEOUT: Duration = Duration::from_secs(30);

/// `PORT` from the environment, as the CLI reads it, or 4317.
pub fn preferred_port() -> u16 {
    std::env::var("PORT").ok().and_then(|port| port.parse().ok()).unwrap_or(DEFAULT_PORT)
}

/// The westerm executable that ships with the app, next to the app's own.
pub fn sidecar() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the app: {e}"))?;
    let path = exe.with_file_name(format!("westerm{}", std::env::consts::EXE_SUFFIX));
    if path.is_file() { Ok(path) } else { Err(format!("{} is missing", path.display())) }
}

/// Whether a westerm answers on `port`: /api/terminal-config returns its
/// terminal font settings. Any other program on the port does not count.
pub fn probe(port: u16) -> bool {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_secs(1)) else { return false };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let request = format!("GET /api/terminal-config HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = Vec::new();
    let _ = stream.take(256 * 1024).read_to_end(&mut response);
    is_westerm_answer(&response)
}

/// A 200 answer whose JSON body has `fontFamilies`.
fn is_westerm_answer(response: &[u8]) -> bool {
    let text = String::from_utf8_lossy(response);
    let Some((head, body)) = text.split_once("\r\n\r\n") else { return false };
    if head.split(' ').nth(1) != Some("200") {
        return false;
    }
    // A chunked body has size lines around the JSON; take the object itself.
    let (Some(start), Some(end)) = (body.find('{'), body.rfind('}')) else { return false };
    serde_json::from_str::<serde_json::Value>(&body[start..=end])
        .is_ok_and(|json| json.get("fontFamilies").is_some())
}

/// The port in the line a server prints when it is ready:
/// "> westerm: http://127.0.0.1:4317".
pub fn ready_port(line: &str) -> Option<u16> {
    line.trim_end().strip_prefix("> westerm: http://127.0.0.1:")?.parse().ok()
}

/// A UTF-8 locale name for a system language tag ("ko-KR" → "ko_KR.UTF-8").
fn locale_name(tag: &str) -> Option<String> {
    let mut parts = tag.split(['-', '_']);
    let language = parts.next().filter(|part| (2..=3).contains(&part.len()))?.to_ascii_lowercase();
    // Skip a script ("Hans") and take the region ("CN").
    let region = parts.find(|part| part.len() == 2)?.to_ascii_uppercase();
    Some(format!("{language}_{region}.UTF-8"))
}

/// The LANG to give the server when the app has none.
///
/// Apps opened from Finder get no LANG. Shells then fall back to the C locale
/// and break Korean, Japanese, and Chinese text. A locale must exist on the
/// system; `exists` checks that.
fn fallback_lang(tag: Option<&str>, exists: impl Fn(&str) -> bool) -> String {
    tag.and_then(locale_name).filter(|name| exists(name)).unwrap_or_else(|| "en_US.UTF-8".to_owned())
}

/// Sets LANG on `command` when the app's environment has no locale at all.
fn set_locale(command: &mut Command) {
    if cfg!(windows) || ["LANG", "LC_ALL", "LC_CTYPE"].iter().any(|key| std::env::var_os(key).is_some()) {
        return;
    }
    let tag = sys_locale::get_locale();
    let lang = if cfg!(target_os = "macos") {
        fallback_lang(tag.as_deref(), |name| Path::new("/usr/share/locale").join(name).exists())
    } else {
        // glibc keeps compiled locales elsewhere; C.UTF-8 is always there.
        "C.UTF-8".to_owned()
    };
    command.env("LANG", lang);
}

/// The log of servers this app starts.
pub struct Log {
    pub path: PathBuf,
    file: Mutex<Option<File>>,
}

impl Log {
    pub fn open(path: PathBuf) -> Self {
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        Self { path, file: Mutex::new(file) }
    }

    pub fn write(&self, line: &str) {
        if let Some(file) = self.file.lock().unwrap().as_mut() {
            let _ = writeln!(file, "{line}");
        }
    }
}

/// A server this app started.
pub struct Server {
    child: Arc<Mutex<Option<Child>>>,
}

impl Server {
    /// Runs `westerm serve --port <port>` and waits for its ready line. Port 0
    /// asks for any free port. Fails when it exits first (the port is taken).
    /// `on_exit` runs once if the server ends on its own later.
    pub fn start(
        exe: &Path,
        port: u16,
        log: Arc<Log>,
        on_exit: impl FnOnce(ExitStatus) + Send + 'static,
    ) -> Result<(Self, u16), String> {
        let mut command = Command::new(exe);
        command
            .args(["serve", "--port", &port.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        set_locale(&mut command);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // The app has no console; without this the server opens one.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        log.write(&format!("westerm-app: starting {} serve --port {port}", exe.display()));
        let mut child = command.spawn().map_err(|e| format!("could not run {}: {e}", exe.display()))?;

        let (ready, port_rx) = mpsc::channel();
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let out_log = Arc::clone(&log);
        std::thread::spawn(move || {
            let mut ready = Some(ready);
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                out_log.write(&line);
                if let Some(port) = ready_port(&line)
                    && let Some(ready) = ready.take()
                {
                    let _ = ready.send(port);
                }
            }
        });
        let err_log = Arc::clone(&log);
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                err_log.write(&line);
            }
        });

        let child = Arc::new(Mutex::new(Some(child)));
        let server = Self { child: Arc::clone(&child) };
        match port_rx.recv_timeout(START_TIMEOUT) {
            Ok(port) => {
                watch(child, log, on_exit);
                Ok((server, port))
            }
            Err(_) => {
                server.stop();
                Err(format!("westerm did not start on port {port}"))
            }
        }
    }

    /// Stops the server (not the PTY host) and waits for it.
    pub fn stop(&self) {
        let Some(mut child) = self.child.lock().unwrap().take() else { return };
        if cfg!(windows) {
            // No signal to ask with; the server keeps nothing to save.
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        terminate(&child);
        for _ in 0..30 {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Asks the server to end with SIGTERM.
fn terminate(child: &Child) {
    #[cfg(unix)]
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        // SAFETY: kill only sends a signal to the server this app started.
        unsafe { libc::kill(pid, libc::SIGTERM) };
    }
    #[cfg(not(unix))]
    let _ = child;
}

/// Reports a server that ends by itself; `stop` takes the child first, so a
/// stopped server is not reported.
fn watch(child: Arc<Mutex<Option<Child>>>, log: Arc<Log>, on_exit: impl FnOnce(ExitStatus) + Send + 'static) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let mut guard = child.lock().unwrap();
            let Some(running) = guard.as_mut() else { return };
            if let Ok(Some(status)) = running.try_wait() {
                guard.take();
                drop(guard);
                log.write(&format!("westerm-app: the server stopped ({status})"));
                on_exit(status);
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ready_line_gives_the_port() {
        assert_eq!(ready_port("> westerm: http://127.0.0.1:4317\n"), Some(4317));
        assert_eq!(ready_port("> westerm: http://127.0.0.1:52011"), Some(52011));
        assert_eq!(ready_port("> westerm: http://127.0.0.1:"), None);
        assert_eq!(ready_port("> pty host 1 started (pid 4)"), None);
        assert_eq!(ready_port("westerm: http://127.0.0.1:4317"), None, "an error line, not the ready line");
    }

    #[test]
    fn probe_rejects_non_westerm_answers() {
        let ok = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"fontFamilies\":[]}";
        assert!(is_westerm_answer(ok));
        let chunked = b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n13\r\n{\"fontFamilies\":[]}\r\n0\r\n\r\n";
        assert!(is_westerm_answer(chunked));
        assert!(!is_westerm_answer(b"HTTP/1.1 200 OK\r\n\r\n<!doctype html><p>{}</p>"), "another web server");
        assert!(!is_westerm_answer(b"HTTP/1.1 404 Not Found\r\n\r\n{\"fontFamilies\":[]}"));
        assert!(!is_westerm_answer(b"SSH-2.0-OpenSSH_9.6\r\n"));
        assert!(!is_westerm_answer(b""));
    }

    #[test]
    fn probe_finds_nothing_on_a_closed_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(!probe(port));
    }

    #[test]
    fn a_system_language_becomes_a_utf8_locale() {
        assert_eq!(locale_name("ko-KR").as_deref(), Some("ko_KR.UTF-8"));
        assert_eq!(locale_name("zh-Hans-CN").as_deref(), Some("zh_CN.UTF-8"));
        assert_eq!(locale_name("en_US").as_deref(), Some("en_US.UTF-8"));
        assert_eq!(locale_name("ja"), None, "no region");
        assert_eq!(fallback_lang(Some("ko-KR"), |_| true), "ko_KR.UTF-8");
        assert_eq!(fallback_lang(Some("ko-KR"), |_| false), "en_US.UTF-8", "a locale the system lacks");
        assert_eq!(fallback_lang(None, |_| true), "en_US.UTF-8");
    }

    #[cfg(unix)]
    #[test]
    fn a_server_that_exits_first_does_not_start() {
        let folder = tempfile::tempdir().unwrap();
        let log = Arc::new(Log::open(folder.path().join("westerm.log")));
        let result = Server::start(Path::new("/usr/bin/false"), 0, Arc::clone(&log), |_| {});
        assert!(result.is_err());
        let written = std::fs::read_to_string(&log.path).unwrap();
        assert!(written.contains("starting /usr/bin/false serve --port 0"), "{written}");
    }
}
