//! The local channel between the dashboard and the PTY host.
//!
//! - Unix: a socket file (0600) in the private state directory.
//! - Windows: a named pipe. Its name is random, and the host writes it with
//!   its process id to `host.pipe` in the state directory, which only the
//!   user can read. A client checks that the pipe's server is that process,
//!   so a pipe that someone else creates under an old name is refused.

#[cfg(unix)]
pub use unix::*;
#[cfg(windows)]
pub use windows::*;

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use tokio::net::{UnixListener, UnixStream};

    pub type Stream = UnixStream;
    pub type Listener = UnixListener;

    fn socket(dir: &Path) -> PathBuf {
        dir.join("host.sock")
    }

    pub async fn connect(dir: &Path) -> io::Result<Stream> {
        let stream = UnixStream::connect(socket(dir)).await?;
        tune(&stream);
        Ok(stream)
    }

    /// Socket buffers of a few MiB, for both ends of a dashboard connection.
    ///
    /// macOS gives a Unix socket 8 KiB, so a pixel update of several MiB is
    /// written 8 KiB at a time, and after each write tungstenite moves the
    /// rest of its buffer forward (`out_buffer.drain`): quadratic copying
    /// that cost the host a whole CPU core. The kernel caps the size at
    /// kern.ipc.maxsockbuf; a refused size keeps the default.
    pub fn tune(stream: &Stream) {
        use std::os::fd::AsRawFd;
        let size: libc::c_int = 4 << 20;
        for option in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
            // SAFETY: setsockopt on an open socket with a c_int value.
            unsafe {
                libc::setsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    option,
                    (&raw const size).cast(),
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                );
            }
        }
    }

    /// True once a stopped host has removed its endpoint.
    pub fn is_gone(dir: &Path) -> bool {
        !socket(dir).exists()
    }

    pub fn describe(dir: &Path) -> String {
        socket(dir).display().to_string()
    }

    /// Listens as the host, which holds the host lock, so a socket file left
    /// here is stale.
    pub fn bind(dir: &Path) -> io::Result<Listener> {
        bind_socket(&socket(dir))
    }

    /// Listens on the socket file `path` (0600). The socket is made under a
    /// temporary name and renamed into place: bind() creates the file before
    /// listen() makes it accept, and a client that connects in between would
    /// be refused. A file already at `path` is replaced.
    pub fn bind_socket(path: &Path) -> io::Result<UnixListener> {
        let mut temp = path.as_os_str().to_owned();
        temp.push(format!(".{}", std::process::id()));
        let temp = PathBuf::from(temp);
        let _ = fs::remove_file(&temp);
        let listener = UnixListener::bind(&temp)?;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
        fs::rename(&temp, path)?;
        Ok(listener)
    }

    pub fn unbind(dir: &Path) {
        let _ = fs::remove_file(socket(dir));
    }

    #[cfg(test)]
    mod tests {
        use std::os::fd::AsRawFd;

        fn buffer(stream: &super::Stream, option: libc::c_int) -> libc::c_int {
            let mut value: libc::c_int = 0;
            let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
            let result = unsafe {
                libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, option, (&raw mut value).cast(), &mut len)
            };
            assert_eq!(result, 0);
            value
        }

        #[tokio::test]
        async fn both_ends_have_room_for_a_whole_pixel_update() {
            let dir = tempfile::tempdir().unwrap();
            let listener = super::bind(dir.path()).unwrap();
            let client = super::connect(dir.path()).await.unwrap();
            let (server, _) = listener.accept().await.unwrap();
            super::tune(&server);
            for stream in [&client, &server] {
                for option in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
                    assert!(buffer(stream, option) >= 1 << 20, "option {option}: {}", buffer(stream, option));
                }
            }
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::fs;
    use std::hash::{BuildHasher, Hasher};
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions};

    pub type Stream = NamedPipeClient;

    const PIPE_PREFIX: &str = r"\\.\pipe\westerm-host-";
    const ERROR_PIPE_BUSY: i32 = 231;

    fn record(dir: &Path) -> PathBuf {
        dir.join("host.pipe")
    }

    fn invalid(message: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
    }

    /// The pipe name and host process id from `host.pipe`.
    fn read_record(dir: &Path) -> io::Result<(String, u32)> {
        let text = fs::read_to_string(record(dir))?;
        let mut lines = text.lines();
        let name = lines.next().filter(|name| name.starts_with(PIPE_PREFIX)).ok_or_else(|| invalid("bad host.pipe"))?;
        let pid = lines.next().and_then(|pid| pid.trim().parse().ok()).ok_or_else(|| invalid("bad host.pipe"))?;
        Ok((name.to_owned(), pid))
    }

    pub async fn connect(dir: &Path) -> io::Result<Stream> {
        let (name, pid) = read_record(dir)?;
        let mut attempts = 0;
        let client = loop {
            match ClientOptions::new().open(&name) {
                Ok(client) => break client,
                // Every instance is taken for a moment; the host makes a new one per client.
                Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) && attempts < 50 => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(error) => return Err(error),
            }
        };
        let mut server = 0u32;
        // SAFETY: the handle is a connected pipe that `client` owns; the call
        // only writes the server's process id.
        let ok = unsafe {
            windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId(client.as_raw_handle() as _, &mut server)
        };
        if ok == 0 || server != pid {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the pipe does not belong to the westerm host"));
        }
        Ok(client)
    }

    /// True once a stopped host has removed its endpoint.
    pub fn is_gone(dir: &Path) -> bool {
        !record(dir).exists()
    }

    pub fn describe(dir: &Path) -> String {
        read_record(dir).map(|(name, _)| name).unwrap_or_else(|_| record(dir).display().to_string())
    }

    /// 128 random bits as hex. RandomState keys come from the OS random source.
    fn random_hex() -> String {
        (0..2)
            .map(|_| {
                let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
                hasher.write_u32(std::process::id());
                format!("{:016x}", hasher.finish())
            })
            .collect()
    }

    fn instance(name: &str) -> io::Result<NamedPipeServer> {
        ServerOptions::new().reject_remote_clients(true).create(name)
    }

    pub struct Listener {
        name: String,
        next: NamedPipeServer,
    }

    /// Listens as the host on a new pipe and records its name for clients.
    pub fn bind(dir: &Path) -> io::Result<Listener> {
        let name = format!("{PIPE_PREFIX}{}", random_hex());
        // The first instance fails if the name already exists, so nobody can
        // have set up the pipe ahead of the host.
        let first = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name)?;
        let temp = dir.join("host.pipe.tmp");
        fs::write(&temp, format!("{name}\n{}\n", std::process::id()))?;
        fs::rename(&temp, record(dir))?;
        Ok(Listener { name, next: first })
    }

    pub fn unbind(dir: &Path) {
        let _ = fs::remove_file(record(dir));
    }

    impl axum::serve::Listener for Listener {
        type Io = NamedPipeServer;
        type Addr = String;

        async fn accept(&mut self) -> (Self::Io, Self::Addr) {
            loop {
                if let Err(error) = self.next.connect().await {
                    eprintln!("pipe connect failed: {error}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if let Ok(fresh) = instance(&self.name) {
                        self.next = fresh;
                    }
                    continue;
                }
                // Open the next instance before handing this one out, so the
                // pipe name never disappears while the host runs.
                match instance(&self.name) {
                    Ok(fresh) => return (std::mem::replace(&mut self.next, fresh), self.name.clone()),
                    Err(error) => {
                        eprintln!("pipe instance failed: {error}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }

        fn local_addr(&self) -> io::Result<Self::Addr> {
            Ok(self.name.clone())
        }
    }
}
